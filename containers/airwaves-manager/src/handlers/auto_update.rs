use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::auto_update::{self, AutoUpdateSettings, AutoUpdateStatus};
use crate::ports::ConfigPort;
use crate::{AppError, AppState};

pub async fn get_auto_update(
    State(state): State<AppState>,
) -> Result<Json<AutoUpdateStatus>, AppError> {
    Ok(Json(auto_update::status(&state).await?))
}

#[derive(Deserialize)]
pub struct SetAutoUpdateRequest {
    pub enabled: Option<bool>,
    pub check_interval_hours: Option<u32>,
}

/// Change the setting (partial update). Stored in config.preferences.
pub async fn set_auto_update(
    State(state): State<AppState>,
    Json(req): Json<SetAutoUpdateRequest>,
) -> Result<Json<AutoUpdateStatus>, AppError> {
    let config = state.config.read_config().await?;
    let current = auto_update::settings_from_config(&config);
    let next = AutoUpdateSettings {
        enabled: req.enabled.unwrap_or(current.enabled),
        check_interval_hours: req
            .check_interval_hours
            .unwrap_or(current.check_interval_hours),
    }
    .clamped();
    auto_update::save_settings(&state, &next).await?;
    Ok(Json(auto_update::status(&state).await?))
}

/// Check every installed app now (in the background) and return at once.
pub async fn run_auto_update(
    State(state): State<AppState>,
) -> Result<Json<AutoUpdateStatus>, AppError> {
    let bg = state.clone();
    tokio::spawn(async move {
        auto_update::run_once(&bg).await;
    });
    // Give the run a moment to take its lock so `running` reads true.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    Ok(Json(auto_update::status(&state).await?))
}
