use axum::extract::State;
use axum::Json;

use crate::ports::HardwarePort;
use crate::{AppError, AppState};

pub async fn list_devices(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::domain::UsbDevice>>, AppError> {
    let devices = state.hardware.list_usb_devices()?;
    Ok(Json(devices))
}

pub async fn list_sdr(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::domain::SdrDevice>>, AppError> {
    let devices = state.hardware.list_sdr_devices()?;
    Ok(Json(devices))
}
