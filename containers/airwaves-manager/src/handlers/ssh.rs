use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::adapters::host::SshStatus;
use crate::{AppError, AppState};

/// Whether the host's SSH server is installed, enabled at boot and running.
pub async fn get_ssh(State(state): State<AppState>) -> Json<SshStatus> {
    Json(state.host.ssh_status().await)
}

#[derive(Deserialize)]
pub struct SetSshRequest {
    pub enabled: bool,
}

/// Enable+start or disable+stop the host's SSH server. Returns the state
/// read back from systemd afterwards, so the UI shows what actually happened.
pub async fn set_ssh(
    State(state): State<AppState>,
    Json(req): Json<SetSshRequest>,
) -> Result<Json<SshStatus>, AppError> {
    if !state.host.ssh_status().await.available {
        return Err(AppError::BadRequest(
            "This system has no SSH server (ssh.service not found)".into(),
        ));
    }
    tracing::info!(
        "SSH {} via API",
        if req.enabled { "enabled" } else { "disabled" }
    );
    Ok(Json(state.host.set_ssh(req.enabled).await?))
}
