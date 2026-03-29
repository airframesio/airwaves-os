use axum::extract::State;
use axum::Json;

use crate::domain::AirwavesConfig;
use crate::ports::ConfigPort;
use crate::{AppError, AppState};

pub async fn get_config(
    State(state): State<AppState>,
) -> Result<Json<AirwavesConfig>, AppError> {
    let config = state.config.read_config().await?;
    Ok(Json(config))
}

pub async fn update_config(
    State(state): State<AppState>,
    Json(config): Json<AirwavesConfig>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.config.write_config(&config).await?;
    Ok(Json(serde_json::json!({"status": "updated"})))
}
