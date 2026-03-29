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
    vec![
        CatalogApp {
            id: "readsb".to_string(),
            name: "readsb".to_string(),
            description: "ADS-B decoder and feed server".to_string(),
            version: "3.14".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/sdr-enthusiasts/docker-readsb-protobuf:latest".to_string(),
            icon: None,
            ports: vec![],
            requires_sdr: true,
            sdr_types: vec![crate::domain::SdrType::RtlSdr],
        },
        CatalogApp {
            id: "acarsdec".to_string(),
            name: "acarsdec".to_string(),
            description: "ACARS decoder".to_string(),
            version: "3.7".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/airframesio/acarsdec:latest".to_string(),
            icon: None,
            ports: vec![],
            requires_sdr: true,
            sdr_types: vec![crate::domain::SdrType::RtlSdr],
        },
        CatalogApp {
            id: "dumpvdl2".to_string(),
            name: "dumpvdl2".to_string(),
            description: "VDL Mode 2 decoder".to_string(),
            version: "2.3".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/airframesio/dumpvdl2:latest".to_string(),
            icon: None,
            ports: vec![],
            requires_sdr: true,
            sdr_types: vec![crate::domain::SdrType::RtlSdr],
        },
        CatalogApp {
            id: "acarshub".to_string(),
            name: "ACARS Hub".to_string(),
            description: "Web-based ACARS message viewer and aggregator".to_string(),
            version: "3.0".to_string(),
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
        },
        CatalogApp {
            id: "tar1090".to_string(),
            name: "tar1090".to_string(),
            description: "ADS-B web-based aircraft tracker with map".to_string(),
            version: "latest".to_string(),
            category: "visualization".to_string(),
            image: "ghcr.io/sdr-enthusiasts/docker-tar1090:latest".to_string(),
            icon: None,
            ports: vec![crate::domain::PortBinding {
                container_port: 80,
                host_port: Some(8080),
                protocol: "tcp".to_string(),
            }],
            requires_sdr: false,
            sdr_types: vec![],
        },
        CatalogApp {
            id: "dumphfdl".to_string(),
            name: "dumphfdl".to_string(),
            description: "HFDL decoder".to_string(),
            version: "1.4".to_string(),
            category: "decoder".to_string(),
            image: "ghcr.io/airframesio/dumphfdl:latest".to_string(),
            icon: None,
            ports: vec![],
            requires_sdr: true,
            sdr_types: vec![crate::domain::SdrType::RtlSdr, crate::domain::SdrType::Airspy],
        },
    ]
}
