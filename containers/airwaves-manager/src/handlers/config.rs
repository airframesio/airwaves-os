use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// Sentinel written into exported backups in place of secret values. On
/// restore, fields carrying this sentinel are merged back from the current
/// config so an exported backup can be re-imported without losing secrets.
pub const REDACTED: &str = "__REDACTED__";

/// Whether a JSON key names a secret (API keys, feeder sharing keys, WiFi
/// PSKs/passwords, tokens) rather than benign data like `hostname` or `id`.
fn is_sensitive_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    if k.contains("public") {
        return false;
    }
    k == "key"
        || k == "psk"
        || k == "apikey"
        || k.ends_with("_key")
        || k.ends_with("-key")
        || k.contains("password")
        || k.contains("passphrase")
        || k.contains("secret")
        || k.contains("token")
}

fn redact_secrets(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if is_sensitive_key(k) && !v.is_null() {
                    *v = Value::String(REDACTED.to_string());
                } else {
                    redact_secrets(v);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                redact_secrets(item);
            }
        }
        _ => {}
    }
}

/// Replace `__REDACTED__` sentinels in an uploaded config with the values
/// currently stored on the device; sentinels with no current counterpart are
/// dropped rather than persisted.
fn restore_redacted(uploaded: &mut Value, current: &Value) {
    match uploaded {
        Value::Object(map) => {
            let mut orphaned = Vec::new();
            for (k, v) in map.iter_mut() {
                if v.as_str() == Some(REDACTED) {
                    match current.get(k) {
                        Some(cur) => *v = cur.clone(),
                        None => orphaned.push(k.clone()),
                    }
                } else {
                    restore_redacted(v, current.get(k).unwrap_or(&Value::Null));
                }
            }
            for k in orphaned {
                map.remove(&k);
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter_mut().enumerate() {
                restore_redacted(item, current.get(i).unwrap_or(&Value::Null));
            }
        }
        _ => {}
    }
}

/// Full system backup - exports config + catalog + feed configs
#[derive(Serialize, Deserialize)]
pub struct SystemBackup {
    pub version: String,
    pub timestamp: String,
    pub config: Value,
    pub catalog: Value,
}

pub async fn export_backup(
    State(state): State<AppState>,
) -> Result<Json<SystemBackup>, AppError> {
    let config = state.config.read_config().await?;
    let mut config_value = serde_json::to_value(&config)?;
    redact_secrets(&mut config_value);

    let catalog = tokio::fs::read_to_string("/etc/airwaves/catalog.json")
        .await
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Array(vec![]));

    Ok(Json(SystemBackup {
        version: "1".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        config: config_value,
        catalog,
    }))
}

pub async fn import_backup(
    State(state): State<AppState>,
    Json(backup): Json<SystemBackup>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Merge redacted secrets back from the current config, then validate
    // against the typed config structure before persisting anything.
    let current = state.config.read_config().await?;
    let current_value = serde_json::to_value(&current)?;

    let mut uploaded = backup.config;
    restore_redacted(&mut uploaded, &current_value);

    let config: AirwavesConfig = serde_json::from_value(uploaded)
        .map_err(|e| AppError::BadRequest(format!("Invalid config in backup: {}", e)))?;

    // Restore config
    state.config.write_config(&config).await?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_secrets_but_not_benign_fields() {
        let mut value = json!({
            "device": {"id": "aw-123", "name": "Attic", "hostname": "airwaves-attic"},
            "aggregators": {
                "fr24": {"id": "fr24", "host": "feed.fr24.com", "api_key": "abc123"},
                "adsbx": {"id": "adsbx", "api_key": null}
            },
            "apps": {
                "installed": [{"id": "readsb", "env": {"FEEDER_KEY": "k1", "LAT": "37.0", "PUBLIC_KEY": "pk"}}]
            }
        });
        redact_secrets(&mut value);
        assert_eq!(value["device"]["hostname"], "airwaves-attic");
        assert_eq!(value["device"]["id"], "aw-123");
        assert_eq!(value["aggregators"]["fr24"]["api_key"], REDACTED);
        assert_eq!(value["aggregators"]["fr24"]["host"], "feed.fr24.com");
        assert!(value["aggregators"]["adsbx"]["api_key"].is_null());
        assert_eq!(value["apps"]["installed"][0]["env"]["FEEDER_KEY"], REDACTED);
        assert_eq!(value["apps"]["installed"][0]["env"]["LAT"], "37.0");
        assert_eq!(value["apps"]["installed"][0]["env"]["PUBLIC_KEY"], "pk");
    }

    #[test]
    fn restore_merges_sentinels_from_current_config() {
        let current = json!({
            "aggregators": {"fr24": {"id": "fr24", "api_key": "abc123"}}
        });
        let mut uploaded = json!({
            "aggregators": {
                "fr24": {"id": "fr24", "api_key": REDACTED},
                "new": {"id": "new", "api_key": REDACTED}
            }
        });
        restore_redacted(&mut uploaded, &current);
        assert_eq!(uploaded["aggregators"]["fr24"]["api_key"], "abc123");
        assert!(uploaded["aggregators"]["new"].get("api_key").is_none());
    }
}
