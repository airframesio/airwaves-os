use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::ports::UpdatePort;
use crate::{AppError, AppState};

/// Cached update status (does not hit the network unless nothing is cached).
pub async fn get_status(
    State(state): State<AppState>,
) -> Result<Json<crate::domain::UpdateStatus>, AppError> {
    Ok(Json(state.updater.status_cached().await))
}

/// Force a fresh check against the remote manifest.
pub async fn check(
    State(state): State<AppState>,
) -> Result<Json<crate::domain::UpdateStatus>, AppError> {
    Ok(Json(state.updater.check().await))
}

#[derive(Deserialize)]
pub struct ApplyRequest {
    /// Components to update: manager, gateway, compose, catalog,
    /// os_packages, os_major — or ["all"].
    pub components: Vec<String>,
}

pub async fn apply(
    State(state): State<AppState>,
    Json(req): Json<ApplyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if req.components.is_empty() {
        return Err(AppError::BadRequest(
            "No components specified".to_string(),
        ));
    }
    state.updater.apply(req.components).await?;
    Ok(Json(serde_json::json!({"status": "started"})))
}

pub async fn progress(
    State(state): State<AppState>,
) -> Result<Json<crate::domain::UpdateProgress>, AppError> {
    Ok(Json(state.updater.progress().await))
}
