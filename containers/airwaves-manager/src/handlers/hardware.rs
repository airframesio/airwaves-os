use axum::extract::State;
use axum::Json;

use crate::ports::{ConfigPort, HardwarePort};
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
    let mut devices = state.hardware.list_sdr_devices()?;
    if let Ok(config) = state.config.read_config().await {
        crate::sdr::annotate_sdr_devices(&mut devices, &config);
    }
    Ok(Json(devices))
}
