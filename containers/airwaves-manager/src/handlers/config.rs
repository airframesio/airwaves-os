use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

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

/// Full system backup - exports config + catalog + feed configs
#[derive(Serialize, Deserialize)]
pub struct SystemBackup {
    pub version: String,
    pub timestamp: String,
    pub config: AirwavesConfig,
    pub catalog: serde_json::Value,
}

pub async fn export_backup(
    State(state): State<AppState>,
) -> Result<Json<SystemBackup>, AppError> {
    let config = state.config.read_config().await?;

    let catalog = tokio::fs::read_to_string("/etc/airwaves/catalog.json")
        .await
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Array(vec![]));

    Ok(Json(SystemBackup {
        version: "1".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        config,
        catalog,
    }))
}

pub async fn import_backup(
    State(state): State<AppState>,
    Json(backup): Json<SystemBackup>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Restore config
    state.config.write_config(&backup.config).await?;

    // Restore catalog if present
    if backup.catalog.is_array() {
        let catalog_str = serde_json::to_string_pretty(&backup.catalog)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        tokio::fs::write("/etc/airwaves/catalog.json", catalog_str)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to restore catalog: {}", e)))?;
    }

    Ok(Json(serde_json::json!({
        "status": "restored",
        "timestamp": backup.timestamp,
    })))
}
