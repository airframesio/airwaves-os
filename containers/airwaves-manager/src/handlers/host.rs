use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::ports::{ConfigPort, HostPort};
use crate::{AppError, AppState};

#[derive(Deserialize)]
pub struct SetHostnameRequest {
    pub hostname: String,
}

/// Change the system hostname (persistent) and keep config.json in sync.
pub async fn set_hostname(
    State(state): State<AppState>,
    Json(req): Json<SetHostnameRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let hostname = req.hostname.trim().to_lowercase();
    state.host.set_hostname(&hostname).await?;

    // Mirror the new hostname into the stored config so the UI stays consistent.
    if let Ok(mut config) = state.config.read_config().await {
        config.device.hostname = hostname.clone();
        let _ = state.config.write_config(&config).await;
    }

    Ok(Json(serde_json::json!({
        "status": "ok",
        "hostname": hostname,
    })))
}

/// Reboot the host (fires after the response is sent).
pub async fn reboot(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    state.host.reboot().await?;
    Ok(Json(serde_json::json!({"status": "rebooting"})))
}

/// Power off the host (fires after the response is sent).
pub async fn shutdown(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    state.host.shutdown().await?;
    Ok(Json(serde_json::json!({"status": "shutting_down"})))
}

#[derive(Deserialize)]
pub struct RestartServiceRequest {
    pub service: String,
}

/// Restart an allowlisted host service.
pub async fn restart_service(
    State(state): State<AppState>,
    Json(req): Json<RestartServiceRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.host.restart_service(&req.service).await?;
    Ok(Json(serde_json::json!({"status": "restarted", "service": req.service})))
}

#[derive(Deserialize)]
pub struct SetTimezoneRequest {
    pub timezone: String,
}

/// Set the system timezone.
pub async fn set_timezone(
    State(state): State<AppState>,
    Json(req): Json<SetTimezoneRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.host.set_timezone(&req.timezone).await?;
    Ok(Json(serde_json::json!({"status": "ok", "timezone": req.timezone})))
}
