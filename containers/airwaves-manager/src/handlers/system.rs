use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::ports::{ConfigPort, DockerPort, HardwarePort, SystemPort};
use crate::{AppError, AppState};

pub async fn get_info(
    State(state): State<AppState>,
) -> Result<Json<crate::domain::SystemInfo>, AppError> {
    let mut info = state.system.get_info()?;
    if let Ok(config) = state.config.read_config().await {
        info.device_id = config.device.id;
    }
    Ok(Json(info))
}

pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<crate::domain::SystemStats>, AppError> {
    let stats = state.system.get_stats()?;
    Ok(Json(stats))
}

/// Aggregated health/status endpoint - single call for dashboard overview
#[derive(Serialize)]
pub struct SystemOverview {
    pub info: crate::domain::SystemInfo,
    pub stats: crate::domain::SystemStats,
    pub containers: ContainerSummary,
    pub hardware: HardwareSummary,
}

#[derive(Serialize)]
pub struct ContainerSummary {
    pub total: usize,
    pub running: usize,
    pub stopped: usize,
    pub airwaves_apps: usize,
}

#[derive(Serialize)]
pub struct HardwareSummary {
    pub sdr_devices: usize,
    pub usb_devices: usize,
}

pub async fn get_overview(
    State(state): State<AppState>,
) -> Result<Json<SystemOverview>, AppError> {
    let mut info = state.system.get_info()?;
    if let Ok(config) = state.config.read_config().await {
        info.device_id = config.device.id;
    }
    let stats = state.system.get_stats()?;

    let containers = state.docker.list_containers().await.unwrap_or_default();
    let running = containers.iter().filter(|c| c.state == "running").count();
    let airwaves_apps = containers
        .iter()
        .filter(|c| c.name.starts_with("airwaves-") && c.name != "airwaves-gateway" && c.name != "airwaves-manager")
        .count();

    let sdr_devices = state.hardware.list_sdr_devices().unwrap_or_default().len();
    let usb_devices = state.hardware.list_usb_devices().unwrap_or_default().len();

    Ok(Json(SystemOverview {
        info,
        stats,
        containers: ContainerSummary {
            total: containers.len(),
            running,
            stopped: containers.len() - running,
            airwaves_apps,
        },
        hardware: HardwareSummary {
            sdr_devices,
            usb_devices,
        },
    }))
}
