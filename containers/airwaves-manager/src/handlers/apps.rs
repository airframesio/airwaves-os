use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::domain::CatalogApp;
use crate::ports::DockerPort;
use crate::{AppError, AppState};

/// Returns the app catalog from the catalog directory
pub async fn list_catalog() -> Result<Json<Vec<CatalogApp>>, AppError> {
    // Read catalog from /etc/airwaves/catalog/ or embedded defaults
    let catalog_path = std::path::Path::new("/etc/airwaves/catalog.json");

    if catalog_path.exists() {
        let content = tokio::fs::read_to_string(catalog_path).await?;
        let catalog: Vec<CatalogApp> = serde_json::from_str(&content)?;
        Ok(Json(catalog))
    } else {
        // Return built-in default catalog
        Ok(Json(default_catalog()))
    }
}

#[derive(Deserialize)]
pub struct InstallRequest {
    pub app_id: String,
}

pub async fn install_app(
    State(state): State<AppState>,
    Json(req): Json<InstallRequest>,
) -> Result<Json<crate::domain::ContainerInfo>, AppError> {
    let catalog = default_catalog();
    let app = catalog
        .iter()
        .find(|a| a.id == req.app_id)
        .ok_or_else(|| AppError::NotFound(format!("App '{}' not found in catalog", req.app_id)))?;

    let container = state.docker.install_app(app).await?;
    Ok(Json(container))
}

pub async fn uninstall_app(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let container_name = format!("airwaves-{}", id);
    state.docker.uninstall_app(&container_name).await?;
    Ok(Json(serde_json::json!({"status": "uninstalled", "id": id})))
}

fn default_catalog() -> Vec<CatalogApp> {
    // Minimal fallback catalog when /etc/airwaves/catalog.json is not present.
    // The full catalog is in the JSON file; this just ensures core apps are available.
    vec![
        CatalogApp {
            id: "ultrafeeder".to_string(),
            name: "ADS-B Ultrafeeder".to_string(),
            description: "All-in-one ADS-B: readsb, tar1090, graphs1090, autogain, multi-feeder".to_string(),
            version: "latest".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/sdr-enthusiasts/docker-adsb-ultrafeeder:latest".to_string(),
            icon: None,
            ports: vec![crate::domain::PortBinding { container_port: 80, host_port: Some(8080), protocol: "tcp".to_string() }],
            requires_sdr: true,
            sdr_types: vec![crate::domain::SdrType::RtlSdr],
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
            sdr_types: vec![crate::domain::SdrType::RtlSdr, crate::domain::SdrType::Airspy],
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
            sdr_types: vec![crate::domain::SdrType::RtlSdr, crate::domain::SdrType::Airspy],
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
            sdr_types: vec![crate::domain::SdrType::RtlSdr, crate::domain::SdrType::Airspy, crate::domain::SdrType::AirspyHf],
        },
        CatalogApp {
            id: "acarshub".to_string(),
            name: "ACARS Hub".to_string(),
            description: "Web-based ACARS/VDL2/HFDL message viewer".to_string(),
            version: "latest".to_string(),
            category: "visualization".to_string(),
            image: "ghcr.io/sdr-enthusiasts/docker-acarshub:latest".to_string(),
            icon: None,
            ports: vec![crate::domain::PortBinding { container_port: 80, host_port: Some(8900), protocol: "tcp".to_string() }],
            requires_sdr: false,
            sdr_types: vec![],
        },
        CatalogApp {
            id: "ais-catcher".to_string(),
            name: "AIS-Catcher".to_string(),
            description: "AIS receiver and decoder for ship tracking".to_string(),
            version: "latest".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/jvde-github/ais-catcher:latest".to_string(),
            icon: None,
            ports: vec![crate::domain::PortBinding { container_port: 8100, host_port: Some(8100), protocol: "tcp".to_string() }],
            requires_sdr: true,
            sdr_types: vec![crate::domain::SdrType::RtlSdr, crate::domain::SdrType::Airspy],
        },
    ]
}
