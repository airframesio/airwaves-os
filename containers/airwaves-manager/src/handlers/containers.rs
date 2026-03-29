use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;

use crate::ports::DockerPort;
use crate::{AppError, AppState};

pub async fn list(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::domain::ContainerInfo>>, AppError> {
    let containers = state.docker.list_containers().await?;
    Ok(Json(containers))
}

pub async fn start(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.docker.start_container(&id).await?;
    Ok(Json(serde_json::json!({"status": "started", "id": id})))
}

pub async fn stop(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.docker.stop_container(&id).await?;
    Ok(Json(serde_json::json!({"status": "stopped", "id": id})))
}

pub async fn restart(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.docker.restart_container(&id).await?;
    Ok(Json(serde_json::json!({"status": "restarted", "id": id})))
}

#[derive(Deserialize)]
pub struct LogsQuery {
    #[serde(default = "default_tail")]
    tail: usize,
}

fn default_tail() -> usize {
    100
}

pub async fn logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<LogsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let logs = state.docker.get_logs(&id, query.tail).await?;
    Ok(Json(serde_json::json!({"id": id, "logs": logs})))
}
