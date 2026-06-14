//! Background forwarding service.
//!
//! Reads decoder container logs, parses decoded messages, and stores them in
//! the local message buffer for the UI on every node. When forwarding is
//! enabled, it additionally forwards them to a configured primary node.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::adapters::{ConfigAdapter, DockerAdapter};
use crate::domain::{DecodedMessage, ForwardingConfig, ForwardingMode, ForwardingStats};
use crate::ports::{ConfigPort, DockerPort};
use crate::ws::Event;

/// UDP port the manager listens on for decoded messages pushed by local
/// decoders. Decoders are configured with an output pointing at
/// `airwaves-manager:<this port>` in the acars_router JSON dialect.
pub const MESSAGE_INGEST_PORT: u16 = 5555;

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

/// Whether collected messages should also be forwarded to a primary node.
/// This gates ONLY forwarding — local collection into the buffer always runs,
/// so the Live Messages page works on a standalone device.
fn should_forward_to_primary(fwd: &ForwardingConfig) -> bool {
    fwd.enabled && fwd.mode != ForwardingMode::Disabled && !fwd.target_ip.is_empty()
}

fn looks_like_service_log(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("[webapp]")
        || lower.contains("[error]")
        || lower.contains("[warning]")
        || lower.contains("namespace")
        || lower.contains("=>")
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
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Forwarding service disabled: failed to create HTTP client: {}", e);
                return;
            }
        };

        let hostname = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());
        let mut seen_log_lines: VecDeque<String> = VecDeque::new();
        let mut seen_log_set: HashSet<String> = HashSet::new();

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

            // Collect local decoder messages into the buffer on every node, so
            // the Live Messages page works on a standalone device. Forwarding to
            // a primary is a separate, optional step gated below.
            let forwarding_active = should_forward_to_primary(&fwd);

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
                        let line_key = format!("{}:{}", container.name, line.trim());
                        if !seen_log_set.insert(line_key.clone()) {
                            continue;
                        }
                        seen_log_lines.push_back(line_key);
                        while seen_log_lines.len() > 2000 {
                            if let Some(oldest) = seen_log_lines.pop_front() {
                                seen_log_set.remove(&oldest);
                            }
                        }

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
                let mut buffer = message_buffer.lock().unwrap_or_else(|e| e.into_inner());
                for msg in &messages {
                    buffer.push_back(msg.clone());
                    if buffer.len() > 1000 {
                        buffer.pop_front();
                    }
                }
            }

            // Forward to a primary node only when forwarding is enabled.
            if forwarding_active {
                let url = format!(
                    "http://{}:{}/api/v1/messages/ingest",
                    fwd.target_ip, fwd.target_port
                );

                match client.post(&url).json(&messages).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let mut s = stats.lock().unwrap_or_else(|e| e.into_inner());
                        s.messages_forwarded += messages.len() as u64;
                        s.last_forwarded = Some(chrono::Utc::now().to_rfc3339());
                    }
                    Ok(resp) => {
                        tracing::warn!("Forward failed: HTTP {}", resp.status());
                        let mut s = stats.lock().unwrap_or_else(|e| e.into_inner());
                        s.messages_failed += messages.len() as u64;
                    }
                    Err(e) => {
                        tracing::warn!("Forward failed: {}", e);
                        let mut s = stats.lock().unwrap_or_else(|e| e.into_inner());
                        s.messages_failed += messages.len() as u64;
                    }
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

/// Background UDP listener that ingests decoded messages pushed by local
/// decoders (acars_router JSON dialect), parses them, and stores them in the
/// shared message buffer that the Live Messages page reads. This is the
/// network-ingest path: decoders emit over the network rather than to stdout,
/// so log-tailing alone cannot see them.
pub fn spawn_message_ingest(
    buffer: Arc<Mutex<VecDeque<DecodedMessage>>>,
    stats: Arc<Mutex<ForwardingStats>>,
    events_tx: broadcast::Sender<Event>,
) {
    tokio::spawn(async move {
        let bind = format!("0.0.0.0:{MESSAGE_INGEST_PORT}");
        let sock = match tokio::net::UdpSocket::bind(&bind).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Message ingest: failed to bind {bind}: {e}");
                return;
            }
        };
        tracing::info!("Message ingest listening on udp/{MESSAGE_INGEST_PORT}");
        let hostname = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());
        let mut buf = vec![0u8; 65535];
        loop {
            let n = match sock.recv_from(&mut buf).await {
                Ok((n, _)) => n,
                Err(e) => {
                    tracing::warn!("Message ingest recv error: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
            };
            let Ok(text) = std::str::from_utf8(&buf[..n]) else {
                continue;
            };
            // A datagram may contain one JSON object or several newline-separated.
            for line in text.lines() {
                let Some(msg) = parse_ingested_message(line, &hostname) else {
                    continue;
                };
                let _ = events_tx.send(Event::MessageReceived {
                    source_node: msg.source_node.clone(),
                    decoder: msg.decoder.clone(),
                    message_type: msg.message_type.clone(),
                });
                {
                    let mut s = stats.lock().unwrap_or_else(|e| e.into_inner());
                    s.messages_received += 1;
                    s.last_received = Some(chrono::Utc::now().to_rfc3339());
                }
                {
                    let mut b = buffer.lock().unwrap_or_else(|e| e.into_inner());
                    b.push_back(msg);
                    if b.len() > 1000 {
                        b.pop_front();
                    }
                }
            }
        }
    });
}

/// Parse one JSON line from the ingest socket into a DecodedMessage. Handles the
/// acars_router dialects: flat acarsdec ACARS, nested `vdl2`/`hfdl` objects, and
/// AIS-Catcher AIS. Returns None for anything unrecognized.
fn parse_ingested_message(line: &str, hostname: &str) -> Option<DecodedMessage> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(line).ok()?;

    let (decoder, msg_type): (&str, &str) = if json.get("vdl2").is_some() {
        ("dumpvdl2", "vdl2")
    } else if json.get("hfdl").is_some() {
        ("dumphfdl", "hfdl")
    } else if json.get("mmsi").is_some()
        || json.get("class").and_then(|c| c.as_str()) == Some("AIS")
    {
        ("ais-catcher", "ais")
    } else if json.get("label").is_some()
        || json.get("text").is_some()
        || json.get("tail").is_some()
        || json.get("mode").is_some()
    {
        ("acarsdec", "acars")
    } else {
        return None;
    };

    let frequency = ingest_frequency(&json, msg_type);
    let signal_level = ingest_level(&json, msg_type);
    let timestamp = ingest_timestamp(&json);
    let raw = ingest_raw_text(&json, msg_type, line);

    Some(DecodedMessage {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp,
        source_node: hostname.to_string(),
        decoder: decoder.to_string(),
        message_type: msg_type.to_string(),
        frequency,
        signal_level,
        raw,
        metadata: json,
    })
}

fn ingest_frequency(json: &serde_json::Value, msg_type: &str) -> Option<String> {
    let mhz = match msg_type {
        // dumpvdl2 / dumphfdl report frequency in Hz, nested under their object.
        "vdl2" => json.pointer("/vdl2/freq").and_then(serde_json::Value::as_f64)? / 1e6,
        "hfdl" => json.pointer("/hfdl/freq").and_then(serde_json::Value::as_f64)? / 1e6,
        // acarsdec reports MHz already.
        "acars" => json.get("freq").and_then(serde_json::Value::as_f64)?,
        _ => return None,
    };
    Some(format!("{mhz:.3} MHz"))
}

fn ingest_level(json: &serde_json::Value, msg_type: &str) -> Option<f64> {
    match msg_type {
        "vdl2" => json.pointer("/vdl2/sig_level").and_then(serde_json::Value::as_f64),
        "hfdl" => json.pointer("/hfdl/sig_level").and_then(serde_json::Value::as_f64),
        _ => json
            .get("level")
            .or_else(|| json.get("sig_level"))
            .and_then(serde_json::Value::as_f64),
    }
}

fn ingest_timestamp(json: &serde_json::Value) -> String {
    if let Some(ts) = json.get("timestamp").and_then(serde_json::Value::as_f64) {
        let secs = ts.trunc() as i64;
        let nanos = (ts.fract() * 1e9) as u32;
        if let Some(dt) = chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos) {
            return dt.to_rfc3339();
        }
    }
    chrono::Utc::now().to_rfc3339()
}

fn js_str(v: Option<&serde_json::Value>) -> &str {
    v.and_then(serde_json::Value::as_str).unwrap_or("")
}

fn ingest_raw_text(json: &serde_json::Value, msg_type: &str, line: &str) -> String {
    let s = js_str;
    let summary = match msg_type {
        "acars" => {
            let label = s(json.get("label"));
            let tail = s(json.get("tail"));
            let flight = s(json.get("flight"));
            let text = s(json.get("text"));
            let mut parts = vec!["ACARS".to_string()];
            if !label.is_empty() {
                parts.push(format!("label:{label}"));
            }
            if !tail.is_empty() {
                parts.push(format!("tail:{tail}"));
            }
            if !flight.is_empty() {
                parts.push(format!("flight:{flight}"));
            }
            if !text.is_empty() {
                parts.push(text.to_string());
            }
            parts.join(" ")
        }
        "vdl2" => {
            let text = s(json.pointer("/vdl2/avlc/acars/msg_text"));
            if text.is_empty() {
                "VDL2 frame".to_string()
            } else {
                format!("VDL2 ACARS {text}")
            }
        }
        "ais" => {
            let mmsi = json
                .get("mmsi")
                .map(|v| v.to_string())
                .unwrap_or_default();
            format!("AIS MMSI:{mmsi}")
        }
        _ => String::new(),
    };
    let summary = summary.trim().to_string();
    // Fall back to the compact JSON when no human-readable text was extracted,
    // so the row is never blank. Full structured data is kept in metadata.
    let out = if summary.is_empty() || summary == "ACARS" {
        line.to_string()
    } else {
        summary
    };
    out.chars().take(500).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        looks_like_decoded_message, parse_ingested_message, should_forward_to_primary,
    };
    use crate::domain::{ForwardingConfig, ForwardingMode};

    fn fwd(enabled: bool, mode: ForwardingMode, target: &str) -> ForwardingConfig {
        ForwardingConfig {
            enabled,
            target_ip: target.to_string(),
            target_port: 8080,
            mode,
            decoders: vec![],
        }
    }

    #[test]
    fn standalone_node_collects_but_does_not_forward() {
        // The default standalone config must NOT forward — but local collection
        // (which is no longer gated on this) keeps running so Live Messages works.
        assert!(!should_forward_to_primary(&fwd(
            false,
            ForwardingMode::Disabled,
            ""
        )));
        // Enabled but no target, or mode disabled: still no forwarding.
        assert!(!should_forward_to_primary(&fwd(true, ForwardingMode::All, "")));
        assert!(!should_forward_to_primary(&fwd(
            true,
            ForwardingMode::Disabled,
            "10.0.0.2"
        )));
        // Fully configured: forward.
        assert!(should_forward_to_primary(&fwd(
            true,
            ForwardingMode::All,
            "10.0.0.2"
        )));
    }

    #[test]
    fn ingest_parses_acarsdec_json() {
        let line = r#"{"timestamp":1623948123.5,"station_id":"airwaves-acarsdec","freq":131.55,"level":-23.1,"mode":"2","label":"H1","tail":"N123AB","flight":"UA0123","text":"POSRPT KSFO"}"#;
        let m = parse_ingested_message(line, "airwaves-first").expect("acars parses");
        assert_eq!(m.decoder, "acarsdec");
        assert_eq!(m.message_type, "acars");
        assert_eq!(m.frequency.as_deref(), Some("131.550 MHz"));
        assert_eq!(m.signal_level, Some(-23.1));
        assert!(m.raw.contains("N123AB") && m.raw.contains("POSRPT"));
        assert!(m.timestamp.starts_with("2021-")); // epoch 1623948123 -> 2021
    }

    #[test]
    fn ingest_parses_vdl2_json_with_hz_frequency() {
        let line = r#"{"vdl2":{"freq":136975000,"sig_level":-30.2,"avlc":{"acars":{"msg_text":"CPDLC WILCO"}}}}"#;
        let m = parse_ingested_message(line, "airwaves-first").expect("vdl2 parses");
        assert_eq!(m.decoder, "dumpvdl2");
        assert_eq!(m.message_type, "vdl2");
        assert_eq!(m.frequency.as_deref(), Some("136.975 MHz"));
        assert_eq!(m.signal_level, Some(-30.2));
        assert!(m.raw.contains("CPDLC"));
    }

    #[test]
    fn ingest_parses_ais_json() {
        let line = r#"{"class":"AIS","type":1,"mmsi":507319468,"lat":37.5,"lon":-122.4,"speed":20.1}"#;
        let m = parse_ingested_message(line, "airwaves-first").expect("ais parses");
        assert_eq!(m.decoder, "ais-catcher");
        assert_eq!(m.message_type, "ais");
        assert!(m.raw.contains("507319468"));
    }

    #[test]
    fn ingest_rejects_garbage_and_non_messages() {
        assert!(parse_ingested_message("not json", "h").is_none());
        assert!(parse_ingested_message("", "h").is_none());
        assert!(parse_ingested_message(r#"{"unrelated":true}"#, "h").is_none());
    }

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
        assert!(looks_like_decoded_message(
            "acars",
            "[2026-06-10 02:59:46] ACARS mode:2 label:H1 block_id:1 tail:N123AB msg:POSRPT"
        ));
    }
}
