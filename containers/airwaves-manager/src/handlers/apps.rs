use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::domain::CatalogApp;
use crate::ports::{ConfigPort, DockerPort};
use crate::{AppError, AppState};

/// Load the full app catalog: prefer /etc/airwaves/catalog.json, fall back to
/// the built-in default set.
pub async fn load_catalog() -> Vec<CatalogApp> {
    let catalog_path = std::path::Path::new("/etc/airwaves/catalog.json");
    if let Ok(content) = tokio::fs::read_to_string(catalog_path).await {
        if let Ok(catalog) = serde_json::from_str::<Vec<CatalogApp>>(&content) {
            return catalog;
        }
        tracing::warn!("catalog.json present but failed to parse; using default catalog");
    }
    default_catalog()
}

/// Returns the app catalog.
pub async fn list_catalog() -> Result<Json<Vec<CatalogApp>>, AppError> {
    Ok(Json(load_catalog().await))
}

#[derive(Deserialize)]
pub struct InstallRequest {
    pub app_id: String,
    /// Environment overrides from the install wizard, merged on top of the
    /// catalog app's defaults. The frontend composes SDR assignment here too
    /// (e.g. SOAPYSDR=driver=rtlsdr,serial=00000001), keeping the backend
    /// generic.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Optional image tag/version to install (e.g. "latest" or "v3.2"). When
    /// set, the catalog image's tag is replaced with this so a user can pin a
    /// specific app version. Empty/absent = use the catalog image as-is.
    #[serde(default)]
    pub image_tag: Option<String>,
}

/// Replace the tag portion of an image reference, preserving any registry host
/// (which may itself contain a ':port'). "ghcr.io/x/y:latest" + "v2" ->
/// "ghcr.io/x/y:v2". A digest pin (containing '@') or empty tag is left as-is.
fn retag_image(image: &str, tag: &str) -> String {
    let tag = tag.trim();
    if tag.is_empty() || image.contains('@') {
        return image.to_string();
    }
    match image.rsplit_once('/') {
        Some((prefix, last)) => {
            let last = last.split(':').next().unwrap_or(last);
            format!("{prefix}/{last}:{tag}")
        }
        None => {
            let base = image.split(':').next().unwrap_or(image);
            format!("{base}:{tag}")
        }
    }
}

pub async fn install_app(
    State(state): State<AppState>,
    Json(req): Json<InstallRequest>,
) -> Result<Json<crate::domain::ContainerInfo>, AppError> {
    let catalog = load_catalog().await;
    let app = catalog
        .iter()
        .find(|a| a.id == req.app_id)
        .ok_or_else(|| AppError::NotFound(format!("App '{}' not found in catalog", req.app_id)))?;

    // Apply the wizard's choices on top of the catalog defaults: env overrides
    // (SDR assignment, frequencies, …) and an optional pinned image tag. Without
    // the env merge, the user's selections would be silently dropped at install.
    let mut app = app.clone();
    for (k, v) in req.env {
        app.env.insert(k, v);
    }
    if let Some(tag) = req.image_tag.as_deref() {
        let tag = tag.trim();
        if !tag.is_empty() {
            app.image = retag_image(&app.image, tag);
            app.version = tag.to_string();
        }
    }

    let bundled_apps = if app.id == "acarshub" {
        prepare_acarshub_bundle(&catalog, &mut app)
    } else {
        Vec::new()
    };

    let container = state.docker.install_app(&app).await?;
    // Record the install in config.json so the app set survives reboots and the
    // manager can reconcile (re-create) it if its container ever goes missing.
    if let Err(e) = record_installed_app(&state, &app).await {
        tracing::warn!("Installed {} but failed to record in config: {}", app.id, e);
    }

    for bundled_app in bundled_apps {
        let bundled_id = bundled_app.id.clone();
        match state.docker.install_app(&bundled_app).await {
            Ok(_) => {
                if let Err(e) = record_installed_app(&state, &bundled_app).await {
                    tracing::warn!(
                        "Installed bundled {} but failed to record in config: {}",
                        bundled_id,
                        e
                    );
                }
            }
            Err(e) => {
                return Err(AppError::Internal(format!(
                    "Installed ACARS Hub, but failed to install bundled source app {bundled_id}: {e}"
                )));
            }
        }
    }

    Ok(Json(container))
}

fn env_enabled(env: &std::collections::HashMap<String, String>, key: &str) -> bool {
    env.get(key)
        .map(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("true") || v == "1" || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn env_value(env: &std::collections::HashMap<String, String>, key: &str, default: &str) -> String {
    env.get(key)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .unwrap_or(default)
        .to_string()
}

fn prepare_acarshub_bundle(catalog: &[CatalogApp], hub: &mut CatalogApp) -> Vec<CatalogApp> {
    let mut bundled = Vec::new();

    if env_enabled(&hub.env, "ENABLE_LOCAL_ACARSDEC") {
        hub.env
            .insert("ENABLE_ACARS".to_string(), "true".to_string());
        hub.env.insert(
            "ACARS_CONNECTIONS".to_string(),
            env_value(&hub.env, "ACARS_CONNECTIONS", "udp://0.0.0.0:5550"),
        );

        if let Some(source) = catalog.iter().find(|a| a.id == "acarsdec").cloned() {
            let mut source = source;
            source.env.insert(
                "SOAPYSDR".to_string(),
                env_value(&hub.env, "LOCAL_ACARSDEC_SDR", "driver=rtlsdr"),
            );
            source.env.insert(
                "FREQUENCIES".to_string(),
                env_value(
                    &hub.env,
                    "LOCAL_ACARSDEC_FREQUENCIES",
                    "130.025;130.450;131.125;131.550",
                ),
            );
            source
                .env
                .insert("OUTPUT_SERVER".to_string(), "airwaves-acarshub".to_string());
            source
                .env
                .insert("OUTPUT_SERVER_PORT".to_string(), "5550".to_string());
            source
                .env
                .insert("OUTPUT_SERVER_MODE".to_string(), "udp".to_string());
            source
                .env
                .insert("QUIET_LOGS".to_string(), "true".to_string());
            source.env.insert(
                "TZ".to_string(),
                env_value(
                    &hub.env,
                    "TZ",
                    source.env.get("TZ").map(String::as_str).unwrap_or("UTC"),
                ),
            );
            bundled.push(source);
        }
    }

    if env_enabled(&hub.env, "ENABLE_LOCAL_DUMPVDL2") {
        hub.env
            .insert("ENABLE_VDLM".to_string(), "true".to_string());
        hub.env.insert(
            "VDLM_CONNECTIONS".to_string(),
            "zmq://airwaves-dumpvdl2:45555".to_string(),
        );

        if let Some(source) = catalog.iter().find(|a| a.id == "dumpvdl2").cloned() {
            let mut source = source;
            source.env.insert(
                "SOAPYSDR".to_string(),
                env_value(&hub.env, "LOCAL_DUMPVDL2_SDR", "driver=rtlsdr"),
            );
            source.env.insert(
                "FREQUENCIES".to_string(),
                env_value(
                    &hub.env,
                    "LOCAL_DUMPVDL2_FREQUENCIES",
                    "136.650;136.800;136.975",
                ),
            );
            source
                .env
                .insert("ZMQ_MODE".to_string(), "server".to_string());
            source.env.insert(
                "ZMQ_ENDPOINT".to_string(),
                "tcp://0.0.0.0:45555".to_string(),
            );
            source.env.insert(
                "TZ".to_string(),
                env_value(
                    &hub.env,
                    "TZ",
                    source.env.get("TZ").map(String::as_str).unwrap_or("UTC"),
                ),
            );
            bundled.push(source);
        }
    }

    bundled
}

pub async fn uninstall_app(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let container_name = format!("airwaves-{}", id);
    state.docker.uninstall_app(&container_name).await?;
    if let Err(e) = forget_installed_app(&state, &id).await {
        tracing::warn!("Uninstalled {} but failed to update config: {}", id, e);
    }
    Ok(Json(serde_json::json!({"status": "uninstalled", "id": id})))
}

/// The persisted record of an installed app (stored under config.apps).
/// Includes the resolved env (SDR assignment, frequencies, etc.) so the user's
/// configuration survives reboots and is re-applied if the app is recreated.
fn installed_record(app: &CatalogApp) -> serde_json::Value {
    serde_json::json!({
        "id": app.id,
        "name": app.name,
        "image": app.image,
        "category": app.category,
        "env": app.env,
    })
}

/// The persisted env overrides for a recorded app, if any. Used by the
/// reconciler to re-create a missing container with the SAME configuration the
/// user chose (e.g. the assigned SDR), not the bare catalog defaults.
pub fn recorded_app_env(
    config: &crate::domain::AirwavesConfig,
    id: &str,
) -> std::collections::HashMap<String, String> {
    config
        .apps
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(id))
                .and_then(|a| a.get("env"))
                .and_then(|e| serde_json::from_value(e.clone()).ok())
        })
        .unwrap_or_default()
}

/// Add (or update) an app entry in config.apps.
async fn record_installed_app(state: &AppState, app: &CatalogApp) -> Result<(), AppError> {
    let mut config = state.config.read_config().await?;
    let mut apps: Vec<serde_json::Value> = match config.apps.take() {
        serde_json::Value::Array(a) => a,
        _ => Vec::new(),
    };
    apps.retain(|e| e.get("id").and_then(|v| v.as_str()) != Some(app.id.as_str()));
    apps.push(installed_record(app));
    config.apps = serde_json::Value::Array(apps);
    state.config.write_config(&config).await
}

/// Remove an app entry from config.apps.
async fn forget_installed_app(state: &AppState, id: &str) -> Result<(), AppError> {
    let mut config = state.config.read_config().await?;
    if let serde_json::Value::Array(mut apps) = config.apps.take() {
        apps.retain(|e| e.get("id").and_then(|v| v.as_str()) != Some(id));
        config.apps = serde_json::Value::Array(apps);
        state.config.write_config(&config).await?;
    }
    Ok(())
}

/// Returns the list of recorded installed app IDs from config.apps.
pub fn recorded_app_ids(config: &crate::domain::AirwavesConfig) -> Vec<String> {
    match &config.apps {
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|e| e.get("id").and_then(|v| v.as_str()).map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

fn default_catalog() -> Vec<CatalogApp> {
    // Minimal fallback catalog when /etc/airwaves/catalog.json is not present.
    // The full catalog is in the JSON file; this just ensures core apps are available.
    vec![
        CatalogApp {
            id: "ultrafeeder".to_string(),
            name: "ADS-B Ultrafeeder".to_string(),
            description: "All-in-one ADS-B: readsb, tar1090, graphs1090, autogain, multi-feeder"
                .to_string(),
            version: "latest".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/sdr-enthusiasts/docker-adsb-ultrafeeder:latest".to_string(),
            icon: None,
            ports: vec![crate::domain::PortBinding {
                container_port: 80,
                host_port: Some(8080),
                protocol: "tcp".to_string(),
            }],
            requires_sdr: true,
            sdr_types: vec![crate::domain::SdrType::RtlSdr],
            ..Default::default()
        },
        CatalogApp {
            id: "acarsdec".to_string(),
            name: "acarsdec".to_string(),
            description: "Multi-channel ACARS decoder".to_string(),
            version: "latest".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/sdr-enthusiasts/docker-acarsdec:latest".to_string(),
            icon: None,
            ports: vec![],
            requires_sdr: true,
            sdr_types: vec![
                crate::domain::SdrType::RtlSdr,
                crate::domain::SdrType::Airspy,
            ],
            ..Default::default()
        },
        CatalogApp {
            id: "dumpvdl2".to_string(),
            name: "dumpvdl2".to_string(),
            description: "VDL Mode 2 decoder".to_string(),
            version: "latest".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/sdr-enthusiasts/docker-dumpvdl2:latest".to_string(),
            icon: None,
            ports: vec![],
            requires_sdr: true,
            sdr_types: vec![
                crate::domain::SdrType::RtlSdr,
                crate::domain::SdrType::Airspy,
            ],
            ..Default::default()
        },
        CatalogApp {
            id: "dumphfdl".to_string(),
            name: "dumphfdl".to_string(),
            description: "HFDL decoder".to_string(),
            version: "latest".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/sdr-enthusiasts/docker-dumphfdl:latest".to_string(),
            icon: None,
            ports: vec![],
            requires_sdr: true,
            sdr_types: vec![
                crate::domain::SdrType::RtlSdr,
                crate::domain::SdrType::Airspy,
                crate::domain::SdrType::AirspyHf,
            ],
            ..Default::default()
        },
        CatalogApp {
            id: "acarshub".to_string(),
            name: "ACARS Hub".to_string(),
            description: "Web-based ACARS/VDL2/HFDL message viewer".to_string(),
            version: "latest".to_string(),
            category: "visualization".to_string(),
            image: "ghcr.io/sdr-enthusiasts/docker-acarshub:latest".to_string(),
            icon: None,
            ports: vec![crate::domain::PortBinding {
                container_port: 80,
                host_port: Some(8900),
                protocol: "tcp".to_string(),
            }],
            requires_sdr: false,
            sdr_types: vec![],
            ..Default::default()
        },
        CatalogApp {
            id: "ais-catcher".to_string(),
            name: "AIS-Catcher".to_string(),
            description: "AIS receiver and decoder for ship tracking".to_string(),
            version: "latest".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/jvde-github/ais-catcher:latest".to_string(),
            icon: None,
            ports: vec![crate::domain::PortBinding {
                container_port: 8100,
                host_port: Some(8100),
                protocol: "tcp".to_string(),
            }],
            requires_sdr: true,
            sdr_types: vec![
                crate::domain::SdrType::RtlSdr,
                crate::domain::SdrType::Airspy,
            ],
            ..Default::default()
        },
    ]
}
