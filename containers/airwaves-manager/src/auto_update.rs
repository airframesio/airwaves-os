//! Automatic updates for installed apps.
//!
//! When `config.preferences.auto_update_apps.enabled` is true, the manager
//! checks every installed app once per `check_interval_hours` (default 24):
//!
//! - An app that is not pinned to a version moves to the catalog's current
//!   image when its recorded image differs (catalog images are pinned to
//!   version tags, so this is how most updates arrive).
//! - Otherwise the recorded image is pulled again, and the app is recreated
//!   only when the tag now points at a different image than the container's.
//!
//! Recreation always goes through `handlers::apps::reapply_app`, the same path
//! a config edit uses, so recorded env, SDR assignment and per-app preparation
//! are kept. The setting lives in preferences so it is part of backups.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

use crate::handlers::apps::{load_catalog, reapply_app, recorded_app_ids, recorded_app_image};
use crate::ports::ConfigPort;
use crate::{AppError, AppState};

pub const DEFAULT_INTERVAL_HOURS: u32 = 24;
const MIN_INTERVAL_HOURS: u32 = 1;
const MAX_INTERVAL_HOURS: u32 = 24 * 7;
/// How often the background task wakes to see whether a run is due.
const TICK: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutoUpdateSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval")]
    pub check_interval_hours: u32,
}

fn default_interval() -> u32 {
    DEFAULT_INTERVAL_HOURS
}

impl Default for AutoUpdateSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            check_interval_hours: DEFAULT_INTERVAL_HOURS,
        }
    }
}

impl AutoUpdateSettings {
    pub fn clamped(mut self) -> Self {
        self.check_interval_hours = self
            .check_interval_hours
            .clamp(MIN_INTERVAL_HOURS, MAX_INTERVAL_HOURS);
        self
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AppUpdateResult {
    pub app_id: String,
    /// "updated", "up_to_date", "skipped" or "failed".
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AutoUpdateRun {
    /// RFC 3339 UTC.
    pub started_at: String,
    pub finished_at: String,
    pub results: Vec<AppUpdateResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoUpdateStatus {
    #[serde(flatten)]
    pub settings: AutoUpdateSettings,
    pub running: bool,
    pub last_run: Option<AutoUpdateRun>,
}

fn last_run() -> &'static Mutex<Option<AutoUpdateRun>> {
    static LAST: OnceLock<Mutex<Option<AutoUpdateRun>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
}

fn run_lock() -> &'static AsyncMutex<()> {
    static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| AsyncMutex::new(()))
}

pub fn settings_from_config(config: &crate::domain::AirwavesConfig) -> AutoUpdateSettings {
    config
        .preferences
        .get("auto_update_apps")
        .and_then(|v| serde_json::from_value::<AutoUpdateSettings>(v.clone()).ok())
        .unwrap_or_default()
        .clamped()
}

pub async fn save_settings(
    state: &AppState,
    settings: &AutoUpdateSettings,
) -> Result<(), AppError> {
    let mut config = state.config.read_config().await?;
    if !config.preferences.is_object() {
        config.preferences = serde_json::json!({});
    }
    config.preferences["auto_update_apps"] = serde_json::to_value(settings)
        .map_err(|e| AppError::Internal(format!("serialize auto-update settings: {e}")))?;
    state.config.write_config(&config).await
}

pub async fn status(state: &AppState) -> Result<AutoUpdateStatus, AppError> {
    let config = state.config.read_config().await?;
    Ok(AutoUpdateStatus {
        settings: settings_from_config(&config),
        running: run_lock().try_lock().is_err(),
        last_run: last_run().lock().ok().and_then(|g| g.clone()),
    })
}

/// The tag of an image reference ("latest" when none), ignoring a registry port.
fn image_tag(image: &str) -> Option<String> {
    if image.contains('@') {
        return None;
    }
    let last = image.rsplit('/').next().unwrap_or(image);
    Some(
        last.split_once(':')
            .map(|(_, t)| t)
            .unwrap_or("latest")
            .to_string(),
    )
}

/// What to do for one app, decided from its record and the catalog.
#[derive(Debug, PartialEq)]
enum Plan {
    /// Move to the catalog's image (reapply with no tag override).
    MoveToCatalog,
    /// Pull the recorded image and recreate if it changed, keeping `tag`.
    Refresh { tag: Option<String> },
}

fn plan_for(recorded_image: &str, pinned: bool, catalog_image: &str) -> Plan {
    if !pinned && recorded_image != catalog_image {
        Plan::MoveToCatalog
    } else {
        Plan::Refresh {
            tag: if pinned {
                image_tag(recorded_image)
            } else {
                None
            },
        }
    }
}

async fn update_one(state: &AppState, app_id: &str) -> AppUpdateResult {
    let mut result = AppUpdateResult {
        app_id: app_id.to_string(),
        ..Default::default()
    };
    let fail = |mut r: AppUpdateResult, e: AppError| {
        r.outcome = "failed".into();
        r.error = Some(e.to_string());
        r
    };

    let config = match state.config.read_config().await {
        Ok(c) => c,
        Err(e) => return fail(result, e),
    };
    let Some((recorded_image, pinned)) = recorded_app_image(&config, app_id) else {
        result.outcome = "skipped".into();
        return result;
    };
    let catalog = load_catalog().await;
    let Some(catalog_app) = catalog.iter().find(|a| a.id == app_id) else {
        // No longer in the catalog: nothing to compare against.
        result.outcome = "skipped".into();
        return result;
    };
    result.from_image = Some(recorded_image.clone());

    match plan_for(&recorded_image, pinned, &catalog_app.image) {
        Plan::MoveToCatalog => match reapply_app(state, app_id, Default::default(), None).await {
            Ok(_) => {
                result.outcome = "updated".into();
                result.to_image = Some(catalog_app.image.clone());
            }
            Err(e) => return fail(result, e),
        },
        Plan::Refresh { tag } => {
            let pulled = match state.docker.pull_image(&recorded_image).await {
                Ok(id) => id,
                Err(e) => return fail(result, e),
            };
            let running = state
                .docker
                .container_image_id(&format!("airwaves-{app_id}"))
                .await;
            if running.as_deref() == Some(pulled.as_str()) {
                result.outcome = "up_to_date".into();
            } else {
                match reapply_app(state, app_id, Default::default(), tag).await {
                    Ok(_) => {
                        result.outcome = "updated".into();
                        result.to_image = Some(recorded_image.clone());
                    }
                    Err(e) => return fail(result, e),
                }
            }
        }
    }
    result
}

/// Check every installed app once. Returns None if a run is already going.
pub async fn run_once(state: &AppState) -> Option<AutoUpdateRun> {
    let _guard = run_lock().try_lock().ok()?;
    let started_at = chrono::Utc::now().to_rfc3339();
    let ids = match state.config.read_config().await {
        Ok(config) => recorded_app_ids(&config),
        Err(e) => {
            tracing::warn!("auto-update: cannot read config: {e}");
            Vec::new()
        }
    };
    let mut results = Vec::with_capacity(ids.len());
    for id in ids {
        let r = update_one(state, &id).await;
        match r.outcome.as_str() {
            "updated" => tracing::info!("auto-update: {} updated to {:?}", id, r.to_image),
            "failed" => tracing::warn!("auto-update: {} failed: {:?}", id, r.error),
            _ => {}
        }
        results.push(r);
    }
    let run = AutoUpdateRun {
        started_at,
        finished_at: chrono::Utc::now().to_rfc3339(),
        results,
    };
    if let Ok(mut last) = last_run().lock() {
        *last = Some(run.clone());
    }
    Some(run)
}

fn due(settings: &AutoUpdateSettings, last: Option<&AutoUpdateRun>) -> bool {
    if !settings.enabled {
        return false;
    }
    let Some(last) = last else { return true };
    let Ok(finished) = chrono::DateTime::parse_from_rfc3339(&last.finished_at) else {
        return true;
    };
    let elapsed = chrono::Utc::now().signed_duration_since(finished);
    elapsed >= chrono::Duration::hours(settings.check_interval_hours as i64)
}

pub fn spawn_auto_updater(state: AppState) {
    tokio::spawn(async move {
        // Let the reconciler settle first after boot.
        tokio::time::sleep(Duration::from_secs(5 * 60)).await;
        loop {
            if let Ok(config) = state.config.read_config().await {
                let settings = settings_from_config(&config);
                let last = last_run().lock().ok().and_then(|g| g.clone());
                if due(&settings, last.as_ref()) {
                    run_once(&state).await;
                }
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpinned_app_moves_to_catalog_image() {
        assert_eq!(
            plan_for("ghcr.io/a/xng:0.23.0", false, "ghcr.io/a/xng:0.23.1"),
            Plan::MoveToCatalog
        );
    }

    #[test]
    fn same_image_is_refreshed_without_tag_override() {
        assert_eq!(
            plan_for("ghcr.io/a/xng:latest", false, "ghcr.io/a/xng:latest"),
            Plan::Refresh { tag: None }
        );
    }

    #[test]
    fn pinned_app_keeps_its_tag() {
        assert_eq!(
            plan_for(
                "localhost:5000/a/xng:0.20.0",
                true,
                "localhost:5000/a/xng:0.23.1"
            ),
            Plan::Refresh {
                tag: Some("0.20.0".into())
            }
        );
        assert_eq!(image_tag("docker.io/library/nginx"), Some("latest".into()));
        assert_eq!(image_tag("x/y@sha256:abc"), None);
    }

    #[test]
    fn settings_default_off_and_clamped() {
        let s = AutoUpdateSettings {
            enabled: true,
            check_interval_hours: 0,
        }
        .clamped();
        assert_eq!(s.check_interval_hours, 1);
        assert!(!AutoUpdateSettings::default().enabled);
        let mut cfg_prefs = serde_json::json!({"auto_update_apps": {"enabled": true}});
        let parsed: AutoUpdateSettings =
            serde_json::from_value(cfg_prefs["auto_update_apps"].take()).unwrap();
        assert_eq!(parsed.check_interval_hours, DEFAULT_INTERVAL_HOURS);
    }

    #[test]
    fn due_only_when_enabled_and_interval_elapsed() {
        let on = AutoUpdateSettings {
            enabled: true,
            check_interval_hours: 24,
        };
        let off = AutoUpdateSettings::default();
        assert!(!due(&off, None));
        assert!(due(&on, None));
        let recent = AutoUpdateRun {
            finished_at: chrono::Utc::now().to_rfc3339(),
            ..Default::default()
        };
        assert!(!due(&on, Some(&recent)));
        let old = AutoUpdateRun {
            finished_at: (chrono::Utc::now() - chrono::Duration::hours(25)).to_rfc3339(),
            ..Default::default()
        };
        assert!(due(&on, Some(&old)));
    }
}
