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
