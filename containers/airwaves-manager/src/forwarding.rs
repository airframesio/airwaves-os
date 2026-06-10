//! Background forwarding service.
//!
//! Reads decoder container logs, parses decoded messages, and forwards
//! them to the configured primary node. Also stores them in the local
//! message buffer for the UI.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::adapters::{ConfigAdapter, DockerAdapter};
use crate::domain::{DecodedMessage, ForwardingConfig, ForwardingMode, ForwardingStats};
use crate::ports::{ConfigPort, DockerPort};

/// Known decoder container prefixes and their message type
const DECODER_PREFIXES: &[(&str, &str)] = &[
    ("airwaves-acarsdec", "acars"),
    ("airwaves-dumpvdl2", "vdl2"),
    ("airwaves-dumphfdl", "hfdl"),
    ("airwaves-vdlm2dec", "vdl2"),
    ("airwaves-ais-catcher", "ais"),
    ("airwaves-rtl-433", "ism"),
    ("airwaves-satdump", "satellite"),
];

fn looks_like_service_log(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with('[')
        || lower.contains("[webapp]")
        || lower.contains("[error]")
        || lower.contains("[warning]")
        || lower.contains(" table plugin")
        || lower.contains("collectd")
        || lower.contains("starting ")
        || lower.contains("started ")
        || lower.contains("listening ")
        || lower.contains("connected to ")
        || lower.contains("connection ")
}

fn looks_like_decoded_message(msg_type: &str, line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.len() < 5 || looks_like_service_log(trimmed) {
        return false;
    }

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return match msg_type {
            "acars" => {
                json.get("label").is_some()
                    || json.get("msg_text").is_some()
                    || json.get("tail").is_some()
                    || json.get("mode").is_some()
            }
            "vdl2" => {
                json.get("vdl2").is_some()
                    || json.get("avlc").is_some()
                    || json.get("acars").is_some()
                    || json.get("app").is_some()
            }
            "hfdl" => json.get("hfdl").is_some() || json.get("lpdu").is_some(),
            "ais" => json.get("mmsi").is_some() || json.get("msgtype").is_some(),
            "ism" => json.get("model").is_some() && json.get("time").is_some(),
            "satellite" => json.is_object(),
            _ => false,
        };
    }

    let lower = trimmed.to_ascii_lowercase();
    match msg_type {
        "acars" => {
            lower.starts_with("acars ")
                || lower.contains("acars mode")
                || (lower.contains(" label:") && lower.contains(" block"))
        }
        "vdl2" => {
            lower.starts_with("vdl2 ")
                || lower.contains("avlc")
                || lower.contains("xid")
                || lower.contains("cpdlc")
        }
        "hfdl" => lower.starts_with("hfdl ") || lower.contains("lpdu") || lower.contains("spdu"),
        "ais" => trimmed.starts_with("!AIVDM") || trimmed.starts_with("!AIVDO"),
        "ism" => trimmed.starts_with('{') && lower.contains("\"model\""),
        "satellite" => trimmed.starts_with('{'),
        _ => false,
    }
}

fn decoded_message_from_log(
    hostname: &str,
    container_name: &str,
    msg_type: &str,
    line: &str,
) -> Option<DecodedMessage> {
    let trimmed = line.trim();
    if !looks_like_decoded_message(msg_type, trimmed) {
        return None;
    }

    let metadata = serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .unwrap_or(serde_json::Value::Null);

    Some(DecodedMessage {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        source_node: hostname.to_string(),
        decoder: container_name.trim_start_matches("airwaves-").to_string(),
        message_type: msg_type.to_string(),
        frequency: None,
        signal_level: None,
        raw: trimmed.to_string(),
        metadata,
    })
}

/// Spawn the forwarding background service
pub fn spawn_forwarding_service(
    docker: Arc<DockerAdapter>,
    config: Arc<ConfigAdapter>,
    stats: Arc<Mutex<ForwardingStats>>,
    message_buffer: Arc<Mutex<VecDeque<DecodedMessage>>>,
) {
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        let hostname = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());

        loop {
            // Read forwarding config
            let fwd_config = match config.read_config().await {
                Ok(cfg) => cfg
                    .apps
                    .get("forwarding")
                    .and_then(|v| serde_json::from_value::<ForwardingConfig>(v.clone()).ok()),
                Err(_) => None,
            };

            let fwd = fwd_config.unwrap_or(ForwardingConfig {
                enabled: false,
                target_ip: String::new(),
                target_port: 8080,
                mode: ForwardingMode::Disabled,
                decoders: vec![],
            });

            if !fwd.enabled || fwd.mode == ForwardingMode::Disabled || fwd.target_ip.is_empty() {
                // Forwarding disabled, sleep and check again
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                continue;
            }

            // Get running decoder containers
            let containers = match docker.list_containers().await {
                Ok(c) => c,
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let decoder_containers: Vec<_> = containers
                .iter()
                .filter(|c| {
                    c.state == "running"
                        && DECODER_PREFIXES
                            .iter()
                            .any(|(prefix, _)| c.name.starts_with(prefix))
                })
                .filter(|c| {
                    // Filter by configured decoders if selective mode
                    if fwd.mode == ForwardingMode::Selective {
                        let short_name = c.name.trim_start_matches("airwaves-");
                        fwd.decoders.iter().any(|d| d == short_name)
                    } else {
                        true
                    }
                })
                .collect();

            // Collect recent messages from each decoder
            let mut messages = Vec::new();
            for container in &decoder_containers {
                let msg_type = DECODER_PREFIXES
                    .iter()
                    .find(|(prefix, _)| container.name.starts_with(prefix))
                    .map(|(_, t)| *t)
                    .unwrap_or("unknown");

                // Get last 10 log lines
                use crate::ports::DockerPort;
                if let Ok(logs) = docker.get_logs(&container.name, 10).await {
                    for line in logs.lines() {
                        if let Some(message) =
                            decoded_message_from_log(&hostname, &container.name, msg_type, line)
                        {
                            messages.push(message);
                        }
                    }
                }
            }

            if messages.is_empty() {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            // Store locally
            {
                let mut buffer = message_buffer.lock().unwrap();
                for msg in &messages {
                    buffer.push_back(msg.clone());
                    if buffer.len() > 1000 {
                        buffer.pop_front();
                    }
                }
            }

            // Forward to primary node
            let url = format!(
                "http://{}:{}/api/v1/messages/ingest",
                fwd.target_ip, fwd.target_port
            );

            match client.post(&url).json(&messages).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let mut s = stats.lock().unwrap();
                    s.messages_forwarded += messages.len() as u64;
                    s.last_forwarded = Some(chrono::Utc::now().to_rfc3339());
                }
                Ok(resp) => {
                    tracing::warn!("Forward failed: HTTP {}", resp.status());
                    let mut s = stats.lock().unwrap();
                    s.messages_failed += messages.len() as u64;
                }
                Err(e) => {
                    tracing::warn!("Forward failed: {}", e);
                    let mut s = stats.lock().unwrap();
                    s.messages_failed += messages.len() as u64;
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::looks_like_decoded_message;

    #[test]
    fn rejects_service_logs() {
        assert!(!looks_like_decoded_message(
            "acars",
            "[2026-06-10 00:58:32.574][webapp] [00:58:32] [32mnamespace"
        ));
        assert!(!looks_like_decoded_message(
            "adsb",
            "[collectd] 2026/06/10 02:40:23 [error] table plugin"
        ));
        assert!(!looks_like_decoded_message("vdl2", "Starting dumpvdl2"));
    }

    #[test]
    fn accepts_decoded_message_shapes() {
        assert!(looks_like_decoded_message(
            "acars",
            "ACARS mode:2 label:H1 block_id:1 tail:N123AB msg:POSRPT"
        ));
        assert!(looks_like_decoded_message(
            "acars",
            r#"{"label":"H1","msg_text":"POSRPT","tail":"N123AB"}"#
        ));
        assert!(looks_like_decoded_message(
            "ais",
            "!AIVDM,1,1,,A,15Muq?002>G?svP00<:O?vN60<0,0*5C"
        ));
    }
}
