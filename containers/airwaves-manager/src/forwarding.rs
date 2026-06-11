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
    ("airwaves-readsb", "adsb"),
    ("airwaves-acarsdec", "acars"),
    ("airwaves-dumpvdl2", "vdl2"),
    ("airwaves-dumphfdl", "hfdl"),
    ("airwaves-vdlm2dec", "vdl2"),
    ("airwaves-ais-catcher", "ais"),
    ("airwaves-rtl-airband", "airband"),
    ("airwaves-rtl-433", "ism"),
    ("airwaves-satdump", "satellite"),
];

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
                        let trimmed = line.trim();
                        if trimmed.is_empty() || trimmed.len() < 5 {
                            continue;
                        }
                        messages.push(DecodedMessage {
                            id: uuid::Uuid::new_v4().to_string(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            source_node: hostname.clone(),
                            decoder: container.name.trim_start_matches("airwaves-").to_string(),
                            message_type: msg_type.to_string(),
                            frequency: None,
                            signal_level: None,
                            raw: trimmed.to_string(),
                            metadata: serde_json::Value::Null,
                        });
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

            // Forward to primary node
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

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}
