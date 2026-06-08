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

/// Force-refresh the system at its current release (repair drift): sync current
/// host repair files, re-pull the images already pinned in compose, and
/// force-recreate containers. Does not perform a version upgrade.
pub async fn refresh(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.updater.refresh().await?;
    Ok(Json(serde_json::json!({"status": "started"})))
}

#[derive(Deserialize)]
pub struct SetChannelRequest {
    pub channel: String,
}

/// Switch the update channel (stable | beta | dev) and re-check immediately
/// against the new channel's manifest.
pub async fn set_channel(
    State(state): State<AppState>,
    Json(req): Json<SetChannelRequest>,
) -> Result<Json<crate::domain::UpdateStatus>, AppError> {
    state.updater.set_channel(&req.channel)?;
    Ok(Json(state.updater.check().await))
}
