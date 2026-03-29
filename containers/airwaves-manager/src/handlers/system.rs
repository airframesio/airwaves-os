use axum::extract::State;
use axum::Json;

use crate::ports::SystemPort;
use crate::{AppError, AppState};

pub async fn get_info(
    State(state): State<AppState>,
) -> Result<Json<crate::domain::SystemInfo>, AppError> {
    let info = state.system.get_info()?;
    Ok(Json(info))
}

pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<crate::domain::SystemStats>, AppError> {
    let stats = state.system.get_stats()?;
    Ok(Json(stats))
}
