use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use bollard::container::LogsOptions;
use futures::{SinkExt, StreamExt};

use crate::AppState;

/// WebSocket handler for broadcast events (stats, container changes)
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_events_socket(socket, state))
}

async fn handle_events_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.events_tx.subscribe();

    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

/// WebSocket handler for streaming container logs in real-time
pub async fn ws_logs_handler(
    ws: WebSocketUpgrade,
    Path(container_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_logs_socket(socket, container_id, state))
}

async fn handle_logs_socket(socket: WebSocket, container_id: String, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    let docker = state.docker.clone();
    let id = container_id.clone();

    // Stream logs from Docker
    let send_task = tokio::spawn(async move {
        let opts = LogsOptions::<String> {
            follow: true,
            stdout: true,
            stderr: true,
            tail: "50".to_string(),
            ..Default::default()
        };

        let mut stream = docker.stream_logs(&id, opts).await;

        while let Some(msg) = stream.next().await {
            match msg {
                Ok(line) => {
                    let payload = serde_json::json!({
                        "type": "log",
                        "container": container_id,
                        "line": line.trim_end(),
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    });
                    if let Ok(json) = serde_json::to_string(&payload) {
                        if sender.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}
