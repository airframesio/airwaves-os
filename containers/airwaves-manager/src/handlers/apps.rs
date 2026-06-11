use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::domain::CatalogApp;
use crate::ports::{ConfigPort, DockerPort, HardwarePort};
use crate::{AppError, AppState};

const ACARSDEC_FORWARD_TO_HUB_KEY: &str = "AIRWAVES_FORWARD_TO_ACARSHUB";

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

#[derive(Deserialize)]
pub struct UpdateAppConfigRequest {
    /// Environment overrides to persist and apply to the installed app.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Optional image tag/version to use while recreating the app container.
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

fn apply_app_overrides(
    app: &mut CatalogApp,
    env: std::collections::HashMap<String, String>,
    image_tag: Option<String>,
) {
    for (k, v) in env {
        app.env.insert(k, v);
    }
    if let Some(tag) = image_tag.as_deref() {
        let tag = tag.trim();
        if !tag.is_empty() {
            app.image = retag_image(&app.image, tag);
            app.version = tag.to_string();
        }
    }
}

fn is_local_acarshub_target(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    matches!(
        value.as_str(),
        "airwaves-acarshub" | "acarshub" | "localhost" | "127.0.0.1"
    )
}

fn clear_acarsdec_local_hub_output(env: &mut std::collections::HashMap<String, String>) {
    // These empty values intentionally override docker-acarsdec image defaults.
    // If omitted entirely, the image may fall back to its own bridge target.
    env.insert("OUTPUT_SERVER".to_string(), String::new());
    env.insert("OUTPUT_SERVER_PORT".to_string(), String::new());
    env.insert("OUTPUT_SERVER_MODE".to_string(), String::new());
    env.insert(ACARSDEC_FORWARD_TO_HUB_KEY.to_string(), "false".to_string());
}

fn configure_acarsdec_output_policy(
    app: &mut CatalogApp,
    acarshub_installed: bool,
) -> Result<(), AppError> {
    if app.id != "acarsdec" {
        return Ok(());
    }

    if env_enabled(&app.env, ACARSDEC_FORWARD_TO_HUB_KEY) {
        if !acarshub_installed {
            return Err(AppError::BadRequest(
                "Install ACARS Hub before enabling local ACARS Hub output for acarsdec."
                    .to_string(),
            ));
        }
        app.env
            .insert("OUTPUT_SERVER".to_string(), "airwaves-acarshub".to_string());
        app.env
            .insert("OUTPUT_SERVER_PORT".to_string(), "5550".to_string());
        app.env
            .insert("OUTPUT_SERVER_MODE".to_string(), "udp".to_string());
        return Ok(());
    }

    let output_server = app
        .env
        .get("OUTPUT_SERVER")
        .map(|v| v.trim())
        .unwrap_or_default();
    if output_server.is_empty() || is_local_acarshub_target(output_server) {
        clear_acarsdec_local_hub_output(&mut app.env);
    }

    Ok(())
}

fn repair_recorded_acarsdec_output_env(
    env: &mut std::collections::HashMap<String, String>,
    acarshub_installed: bool,
) -> bool {
    let before = env.clone();
    let wants_hub = env_enabled(env, ACARSDEC_FORWARD_TO_HUB_KEY);
    if wants_hub && acarshub_installed {
        env.insert("OUTPUT_SERVER".to_string(), "airwaves-acarshub".to_string());
        env.insert("OUTPUT_SERVER_PORT".to_string(), "5550".to_string());
        env.insert("OUTPUT_SERVER_MODE".to_string(), "udp".to_string());
    } else {
        let output_server = env
            .get("OUTPUT_SERVER")
            .map(|v| v.trim())
            .unwrap_or_default();
        if wants_hub || output_server.is_empty() || is_local_acarshub_target(output_server) {
            clear_acarsdec_local_hub_output(env);
        }
    }
    *env != before
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
    apply_app_overrides(&mut app, req.env, req.image_tag);

    let config = state.config.read_config().await?;
    let recorded_ids = recorded_app_ids(&config);
    let acarshub_installed = recorded_ids.iter().any(|id| id == "acarshub");
    configure_acarsdec_output_policy(&mut app, acarshub_installed)?;

    let bundled_apps = if app.id == "acarshub" {
        prepare_acarshub_bundle(&catalog, &mut app)
    } else {
        Vec::new()
    };

    validate_sdr_assignments(&state, &[app.clone()], &bundled_apps, Some(&config)).await?;

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

pub async fn update_app_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateAppConfigRequest>,
) -> Result<Json<crate::domain::ContainerInfo>, AppError> {
    let requested_id = id
        .strip_prefix("airwaves-")
        .map(str::to_string)
        .unwrap_or(id);
    let catalog = load_catalog().await;
    let app = catalog
        .iter()
        .find(|a| a.id == requested_id)
        .ok_or_else(|| AppError::NotFound(format!("App '{requested_id}' not found in catalog")))?;

    let config = state.config.read_config().await?;
    let mut app = app.clone();
    for (k, v) in recorded_app_env(&config, &requested_id) {
        app.env.insert(k, v);
    }
    apply_app_overrides(&mut app, req.env, req.image_tag);
    let recorded_ids = recorded_app_ids(&config);
    let acarshub_installed = recorded_ids.iter().any(|id| id == "acarshub");
    configure_acarsdec_output_policy(&mut app, acarshub_installed)?;

    let bundled_apps = if app.id == "acarshub" {
        prepare_acarshub_bundle(&catalog, &mut app)
    } else {
        Vec::new()
    };

    validate_sdr_assignments(&state, &[app.clone()], &bundled_apps, Some(&config)).await?;

    let container = state.docker.install_app(&app).await?;
    if let Err(e) = record_installed_app(&state, &app).await {
        tracing::warn!("Updated {} but failed to record new config: {}", app.id, e);
    }

    for bundled_app in bundled_apps {
        let bundled_id = bundled_app.id.clone();
        match state.docker.install_app(&bundled_app).await {
            Ok(_) => {
                if let Err(e) = record_installed_app(&state, &bundled_app).await {
                    tracing::warn!(
                        "Updated bundled {} but failed to record new config: {}",
                        bundled_id,
                        e
                    );
                }
            }
            Err(e) => {
                return Err(AppError::Internal(format!(
                    "Updated ACARS Hub, but failed to update bundled source app {bundled_id}: {e}"
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

async fn validate_sdr_assignments(
    state: &AppState,
    primary_apps: &[CatalogApp],
    bundled_apps: &[CatalogApp],
    config: Option<&crate::domain::AirwavesConfig>,
) -> Result<(), AppError> {
    let owned_config;
    let config = match config {
        Some(config) => config,
        None => {
            owned_config = state.config.read_config().await?;
            &owned_config
        }
    };
    let devices = state.hardware.list_sdr_devices()?;
    let mut targets = primary_apps.to_vec();
    targets.extend(bundled_apps.iter().cloned());
    crate::sdr::validate_app_sdr_assignments(config, &targets, &devices)
        .map_err(AppError::BadRequest)
}

fn copy_sdr_metadata(
    source_env: &std::collections::HashMap<String, String>,
    source_key: &str,
    target_env: &mut std::collections::HashMap<String, String>,
    target_key: &str,
) {
    let source_meta_key = crate::sdr::sdr_id_env_key(source_key);
    let Some(device_id) = source_env
        .get(&source_meta_key)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    else {
        return;
    };

    target_env.insert(
        crate::sdr::sdr_id_env_key(target_key),
        device_id.to_string(),
    );
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
            copy_sdr_metadata(&hub.env, "LOCAL_ACARSDEC_SDR", &mut source.env, "SOAPYSDR");
            source.env.insert(
                "FREQUENCIES".to_string(),
                env_value(
                    &hub.env,
                    "LOCAL_ACARSDEC_FREQUENCIES",
                    "130.025;130.450;131.125;131.550",
                ),
            );
            source.env.insert(
                "FEED_ID".to_string(),
                env_value(&hub.env, "LOCAL_ACARSDEC_FEED_ID", "airwaves-acarsdec"),
            );
            source
                .env
                .insert(ACARSDEC_FORWARD_TO_HUB_KEY.to_string(), "true".to_string());
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
            env_value(&hub.env, "VDLM_CONNECTIONS", "udp://0.0.0.0:5555"),
        );

        if let Some(source) = catalog.iter().find(|a| a.id == "dumpvdl2").cloned() {
            let mut source = source;
            source.env.insert(
                "SOAPYSDR".to_string(),
                env_value(&hub.env, "LOCAL_DUMPVDL2_SDR", "driver=rtlsdr"),
            );
            copy_sdr_metadata(&hub.env, "LOCAL_DUMPVDL2_SDR", &mut source.env, "SOAPYSDR");
            source.env.insert(
                "FREQUENCIES".to_string(),
                env_value(
                    &hub.env,
                    "LOCAL_DUMPVDL2_FREQUENCIES",
                    "136.650;136.800;136.975",
                ),
            );
            source.env.insert(
                "FEED_ID".to_string(),
                env_value(&hub.env, "LOCAL_DUMPVDL2_FEED_ID", "airwaves-dumpvdl2"),
            );
            source
                .env
                .insert("SERVER".to_string(), "airwaves-acarshub".to_string());
            source
                .env
                .insert("SERVER_PORT".to_string(), "5555".to_string());
            source.env.remove("ZMQ_MODE");
            source.env.remove("ZMQ_ENDPOINT");
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

pub async fn migrate_acarsdec_output_policy(state: &AppState) -> Result<(), AppError> {
    use crate::ports::{ConfigPort, DockerPort};

    let mut config = state.config.read_config().await?;
    let recorded_ids = recorded_app_ids(&config);
    let acarshub_installed = recorded_ids.iter().any(|id| id == "acarshub");
    let mut changed = false;
    let mut recreate_acarsdec = false;

    if let Some(apps) = config.apps.as_array_mut() {
        for app in apps {
            if app.get("id").and_then(|v| v.as_str()) != Some("acarsdec") {
                continue;
            }

            let Some(env_value) = app.get_mut("env") else {
                continue;
            };
            let mut env: std::collections::HashMap<String, String> =
                serde_json::from_value(env_value.clone()).unwrap_or_default();
            if repair_recorded_acarsdec_output_env(&mut env, acarshub_installed) {
                *env_value =
                    serde_json::to_value(env).map_err(|e| AppError::Internal(e.to_string()))?;
                changed = true;
                recreate_acarsdec = true;
            }
        }
    }

    if !changed {
        return Ok(());
    }

    state.config.write_config(&config).await?;

    if recreate_acarsdec {
        let catalog = load_catalog().await;
        if let Some(app) = catalog.iter().find(|a| a.id == "acarsdec") {
            let mut app = app.clone();
            for (k, v) in recorded_app_env(&config, "acarsdec") {
                app.env.insert(k, v);
            }
            if let Err(e) = state.docker.install_app(&app).await {
                tracing::warn!(
                    "Migrated acarsdec output policy but failed to recreate container: {}",
                    e
                );
            }
        }
    }

    Ok(())
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
    let env = |pairs: &[(&str, &str)]| -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    };

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
            env: env(&[
                ("SOAPYSDR", "driver=rtlsdr"),
                ("FREQUENCIES", "130.025;130.450;131.125;131.550"),
                ("FEED_ID", "airwaves-acarsdec"),
                ("OUTPUT_SERVER", ""),
                ("OUTPUT_SERVER_PORT", ""),
                ("OUTPUT_SERVER_MODE", ""),
                (ACARSDEC_FORWARD_TO_HUB_KEY, "false"),
                ("QUIET_LOGS", "true"),
                ("TZ", "UTC"),
            ]),
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
            env: env(&[
                ("SOAPYSDR", "driver=rtlsdr"),
                ("FREQUENCIES", "136.650;136.800;136.975"),
                ("FEED_ID", "airwaves-dumpvdl2"),
                ("SERVER", "airwaves-acarshub"),
                ("SERVER_PORT", "5555"),
                ("TZ", "UTC"),
            ]),
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
            env: env(&[
                ("TZ", "UTC"),
                ("ENABLE_ACARS", "true"),
                ("ACARS_CONNECTIONS", "udp://0.0.0.0:5550"),
                ("ENABLE_LOCAL_ACARSDEC", "false"),
                ("LOCAL_ACARSDEC_SDR", "driver=rtlsdr"),
                (
                    "LOCAL_ACARSDEC_FREQUENCIES",
                    "130.025;130.450;131.125;131.550",
                ),
                ("LOCAL_ACARSDEC_FEED_ID", "airwaves-acarsdec"),
                ("ENABLE_VDLM", "true"),
                ("VDLM_CONNECTIONS", "udp://0.0.0.0:5555"),
                ("ENABLE_LOCAL_DUMPVDL2", "false"),
                ("LOCAL_DUMPVDL2_SDR", "driver=rtlsdr"),
                ("LOCAL_DUMPVDL2_FREQUENCIES", "136.650;136.800;136.975"),
                ("LOCAL_DUMPVDL2_FEED_ID", "airwaves-dumpvdl2"),
            ]),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn acarsdec_app(env: &[(&str, &str)]) -> CatalogApp {
        let mut app = CatalogApp {
            id: "acarsdec".to_string(),
            ..Default::default()
        };
        for (key, value) in env {
            app.env.insert((*key).to_string(), (*value).to_string());
        }
        app
    }

    #[test]
    fn acarsdec_policy_clears_implicit_local_hub_output() {
        let mut app = acarsdec_app(&[
            ("OUTPUT_SERVER", "airwaves-acarshub"),
            ("OUTPUT_SERVER_PORT", "5550"),
            ("OUTPUT_SERVER_MODE", "tcp"),
        ]);

        configure_acarsdec_output_policy(&mut app, false).unwrap();

        assert_eq!(app.env.get("OUTPUT_SERVER").map(String::as_str), Some(""));
        assert_eq!(
            app.env.get("OUTPUT_SERVER_PORT").map(String::as_str),
            Some("")
        );
        assert_eq!(
            app.env.get("OUTPUT_SERVER_MODE").map(String::as_str),
            Some("")
        );
        assert_eq!(
            app.env.get(ACARSDEC_FORWARD_TO_HUB_KEY).map(String::as_str),
            Some("false")
        );
    }

    #[test]
    fn acarsdec_policy_requires_installed_hub_for_local_forwarding() {
        let mut app = acarsdec_app(&[(ACARSDEC_FORWARD_TO_HUB_KEY, "true")]);

        assert!(configure_acarsdec_output_policy(&mut app, false).is_err());
    }

    #[test]
    fn acarsdec_policy_enables_explicit_local_hub_forwarding() {
        let mut app = acarsdec_app(&[(ACARSDEC_FORWARD_TO_HUB_KEY, "true")]);

        configure_acarsdec_output_policy(&mut app, true).unwrap();

        assert_eq!(
            app.env.get("OUTPUT_SERVER").map(String::as_str),
            Some("airwaves-acarshub")
        );
        assert_eq!(
            app.env.get("OUTPUT_SERVER_PORT").map(String::as_str),
            Some("5550")
        );
        assert_eq!(
            app.env.get("OUTPUT_SERVER_MODE").map(String::as_str),
            Some("udp")
        );
    }

    #[test]
    fn acarsdec_policy_preserves_custom_output_target() {
        let mut app = acarsdec_app(&[
            ("OUTPUT_SERVER", "feed.example.net"),
            ("OUTPUT_SERVER_PORT", "5550"),
            ("OUTPUT_SERVER_MODE", "tcp"),
        ]);

        configure_acarsdec_output_policy(&mut app, false).unwrap();

        assert_eq!(
            app.env.get("OUTPUT_SERVER").map(String::as_str),
            Some("feed.example.net")
        );
        assert_eq!(
            app.env.get("OUTPUT_SERVER_MODE").map(String::as_str),
            Some("tcp")
        );
    }
}
