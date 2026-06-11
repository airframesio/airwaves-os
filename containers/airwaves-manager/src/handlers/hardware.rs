use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::{AirwavesConfig, SdrDevice};
use crate::ports::{ConfigPort, HardwarePort};
use crate::{AppError, AppState};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct SdrDeviceOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    serial: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSdrDeviceRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    serial: Option<String>,
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn sdr_device_overrides(
    config: &AirwavesConfig,
) -> std::collections::HashMap<String, SdrDeviceOverride> {
    config
        .hardware
        .get("sdr_devices")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

fn apply_sdr_overrides(devices: &mut [SdrDevice], config: &AirwavesConfig) {
    let overrides = sdr_device_overrides(config);
    for device in devices {
        if let Some(override_config) = overrides.get(&device.id) {
            if let Some(name) = override_config.name.as_ref() {
                device.name = name.clone();
                device.configured_name = Some(name.clone());
            }
            if let Some(serial) = override_config.serial.as_ref() {
                device.configured_serial = Some(serial.clone());
            }
        }
    }
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .expect("value was normalized to object")
}

fn set_or_remove_string(object: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        object.insert(key.to_string(), Value::String(value));
    } else {
        object.remove(key);
    }
}

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
        apply_sdr_overrides(&mut devices, &config);
    }
    Ok(Json(devices))
}

pub async fn update_sdr(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    Json(request): Json<UpdateSdrDeviceRequest>,
) -> Result<Json<SdrDevice>, AppError> {
    let physical_devices = state.hardware.list_sdr_devices()?;
    if !physical_devices.iter().any(|device| device.id == device_id) {
        return Err(AppError::NotFound(format!(
            "SDR device {device_id} is not currently connected"
        )));
    }

    let mut config = state.config.read_config().await?;
    let name = clean_optional(request.name);
    let serial = clean_optional(request.serial);

    let hardware = ensure_object(&mut config.hardware);
    let sdr_devices = hardware
        .entry("sdr_devices".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let sdr_devices = ensure_object(sdr_devices);
    let entry = sdr_devices
        .entry(device_id.clone())
        .or_insert_with(|| Value::Object(Map::new()));
    let entry = ensure_object(entry);

    set_or_remove_string(entry, "name", name);
    set_or_remove_string(entry, "serial", serial);

    if entry.is_empty() {
        sdr_devices.remove(&device_id);
    }

    state.config.write_config(&config).await?;

    let mut devices = state.hardware.list_sdr_devices()?;
    crate::sdr::annotate_sdr_devices(&mut devices, &config);
    apply_sdr_overrides(&mut devices, &config);

    let device = devices
        .into_iter()
        .find(|device| device.id == device_id)
        .ok_or_else(|| AppError::NotFound(format!("SDR device {device_id} disappeared")))?;

    let _ = state.events_tx.send(crate::ws::Event::SdrDeviceChanged {
        action: "updated".to_string(),
        device_id,
    });

    Ok(Json(device))
}
