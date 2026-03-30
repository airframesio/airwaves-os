use axum::extract::State;
use axum::Json;

use crate::domain::{DecodedMessage, ForwardingConfig, ForwardingMode, ForwardingStats};
use crate::ports::ConfigPort;
use crate::ws::Event;
use crate::{AppError, AppState};

/// Receive messages from remote nodes (ingest endpoint)
/// Called by secondary nodes forwarding their decoded data
pub async fn ingest_messages(
    State(state): State<AppState>,
    Json(messages): Json<Vec<DecodedMessage>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let count = messages.len();

    // Update stats
    {
        let mut stats = state.forwarding_stats.lock().unwrap();
        stats.messages_received += count as u64;
        stats.last_received = Some(chrono::Utc::now().to_rfc3339());
    }

    // Broadcast received messages to local WebSocket clients
    for msg in &messages {
        let _ = state.events_tx.send(Event::MessageReceived {
            source_node: msg.source_node.clone(),
            decoder: msg.decoder.clone(),
            message_type: msg.message_type.clone(),
        });
    }

    // Store in message buffer for the UI
    {
        let mut buffer = state.message_buffer.lock().unwrap();
        for msg in messages {
            buffer.push_back(msg);
            if buffer.len() > 1000 {
                buffer.pop_front();
            }
        }
    }

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "count": count,
    })))
}

/// Get recent messages (from local decoders + forwarded from peers)
pub async fn get_messages(
    State(state): State<AppState>,
) -> Result<Json<Vec<DecodedMessage>>, AppError> {
    let buffer = state.message_buffer.lock().unwrap();
    let messages: Vec<DecodedMessage> = buffer.iter().cloned().collect();
    Ok(Json(messages))
}

/// Get forwarding configuration
pub async fn get_forwarding_config(
    State(state): State<AppState>,
) -> Result<Json<ForwardingConfig>, AppError> {
    let config = state.config.read_config().await?;
    let fwd: ForwardingConfig = config
        .apps
        .get("forwarding")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(ForwardingConfig {
            enabled: false,
            target_ip: String::new(),
            target_port: 8080,
            mode: ForwardingMode::Disabled,
            decoders: vec![],
        });
    Ok(Json(fwd))
}

/// Update forwarding configuration
pub async fn set_forwarding_config(
    State(state): State<AppState>,
    Json(fwd): Json<ForwardingConfig>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut config = state.config.read_config().await?;

    if let Some(obj) = config.apps.as_object_mut() {
        obj.insert(
            "forwarding".to_string(),
            serde_json::to_value(&fwd).unwrap(),
        );
    } else {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "forwarding".to_string(),
            serde_json::to_value(&fwd).unwrap(),
        );
        config.apps = serde_json::Value::Object(obj);
    }

    state.config.write_config(&config).await?;

    // Notify the forwarding service to reconfigure
    let _ = state.events_tx.send(Event::ForwardingConfigChanged);

    Ok(Json(serde_json::json!({"status": "updated"})))
}

/// Get forwarding stats
pub async fn get_forwarding_stats(
    State(state): State<AppState>,
) -> Result<Json<ForwardingStats>, AppError> {
    let stats = state.forwarding_stats.lock().unwrap().clone();
    Ok(Json(stats))
}

/// Generate simulated messages for testing the forwarding pipeline.
/// POST /api/v1/messages/simulate - injects fake decoded messages as if
/// they came from a secondary node, populating the message buffer and
/// triggering WebSocket events.
pub async fn simulate_messages(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    use rand::Rng;
    let mut rng = rand::rng();

    let decoders = vec![
        ("readsb", "adsb", "1090 MHz"),
        ("acarsdec", "acars", "131.550 MHz"),
        ("dumpvdl2", "vdl2", "136.975 MHz"),
        ("ais-catcher", "ais", "162.025 MHz"),
    ];

    let callsigns = ["UAL123", "DAL456", "AAL789", "SWA321", "JBU567", "VSFIN", "GBHKP"];
    let node_names = ["airwaves-remote-1", "airwaves-attic", "airwaves-rooftop"];

    let mut messages = Vec::new();
    for _ in 0..5 {
        let (decoder, msg_type, freq) = decoders[rng.random_range(0..decoders.len())];
        let callsign = callsigns[rng.random_range(0..callsigns.len())];
        let node = node_names[rng.random_range(0..node_names.len())];

        let raw = match msg_type {
            "adsb" => format!("DF:17 CA:5 ICAO:{:06X} TC:11 Alt:{} Lat:{:.4} Lon:{:.4}",
                rng.random_range(0xA00000u32..0xAFFFFFu32),
                rng.random_range(1000..41000),
                37.0 + rng.random_range(0.0f64..1.0),
                -122.0 - rng.random_range(0.0f64..1.0)),
            "acars" => format!("ACARS mode:2 label:H1 blk:{} ack:! tail:{} msg:{}",
                rng.random_range(1..9), callsign,
                ["POSRPT", "WXRPT", "OOOI", "FREETEXT"][rng.random_range(0..4)]),
            "vdl2" => format!("VDL2 freq:{} src:{} dst:ATLTWXA msg:CPDLC WILCO",
                freq, callsign),
            "ais" => format!("AIS type:1 MMSI:{:09} nav:0 speed:{:.1} course:{:.1} lat:{:.4} lon:{:.4}",
                rng.random_range(200000000u32..799999999u32),
                rng.random_range(0.0f64..25.0),
                rng.random_range(0.0f64..360.0),
                37.0 + rng.random_range(0.0f64..1.0),
                -122.0 - rng.random_range(0.0f64..1.0)),
            _ => "unknown".to_string(),
        };

        messages.push(DecodedMessage {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            source_node: node.to_string(),
            decoder: decoder.to_string(),
            message_type: msg_type.to_string(),
            frequency: Some(freq.to_string()),
            signal_level: Some(-(rng.random_range(5.0f64..35.0))),
            raw,
            metadata: serde_json::Value::Null,
        });
    }

    let count = messages.len();

    // Inject into the message buffer (same path as real ingest)
    {
        let mut stats = state.forwarding_stats.lock().unwrap();
        stats.messages_received += count as u64;
        stats.last_received = Some(chrono::Utc::now().to_rfc3339());
    }

    for msg in &messages {
        let _ = state.events_tx.send(crate::ws::Event::MessageReceived {
            source_node: msg.source_node.clone(),
            decoder: msg.decoder.clone(),
            message_type: msg.message_type.clone(),
        });
    }

    {
        let mut buffer = state.message_buffer.lock().unwrap();
        for msg in messages {
            buffer.push_back(msg);
            if buffer.len() > 1000 {
                buffer.pop_front();
            }
        }
    }

    Ok(Json(serde_json::json!({
        "status": "simulated",
        "count": count,
    })))
}
