use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::domain::InstallProgress;
use crate::{AppError, AppState};

const INSTALL_DIR: &str = "/etc/airwaves/install";

/// GET /api/v1/system/disks — candidate internal disks to install onto.
pub async fn get_disks(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let raw = state.host.list_install_disks().await?;
    let disks: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| AppError::Internal(format!("disk list parse failed: {e}")))?;
    Ok(Json(disks))
}

#[derive(Deserialize)]
pub struct InstallRequest {
    pub device: String,
}

/// POST /api/v1/system/install — install the live system onto `device`.
/// Destructive; the device must be one the disk listing offered.
pub async fn start_install(
    State(state): State<AppState>,
    Json(req): Json<InstallRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Defense in depth: only allow a device the host actually offered as a
    // candidate target (host.start_install also validates the format).
    let raw = state.host.list_install_disks().await?;
    let offered = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .map(|a| {
            a.iter()
                .any(|d| d.get("device").and_then(|x| x.as_str()) == Some(req.device.as_str()))
        })
        .unwrap_or(false);
    if !offered {
        return Err(AppError::BadRequest(format!(
            "{} is not an available install target",
            req.device
        )));
    }

    // Seed a queued status so the UI reflects the request immediately.
    let _ = std::fs::create_dir_all(INSTALL_DIR);
    let queued = InstallProgress {
        state: "running".to_string(),
        phase: "queued".to_string(),
        percent: 0,
        target: Some(req.device.clone()),
        log: vec!["Install requested".to_string()],
        reboot_required: false,
        error: None,
        started_at: Some(chrono::Utc::now().to_rfc3339()),
        finished_at: None,
    };
    let _ = std::fs::write(
        format!("{INSTALL_DIR}/status.json"),
        serde_json::to_string_pretty(&queued).unwrap_or_default(),
    );

    state.host.start_install(&req.device).await?;
    Ok(Json(serde_json::json!({ "status": "started", "device": req.device })))
}

/// GET /api/v1/system/install/progress — read the installer's status.json.
pub async fn get_progress() -> Result<Json<InstallProgress>, AppError> {
    let progress = std::fs::read_to_string(format!("{INSTALL_DIR}/status.json"))
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_else(|| InstallProgress {
            state: "idle".to_string(),
            ..Default::default()
        });
    Ok(Json(progress))
}
