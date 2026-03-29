use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod adapters;
mod domain;
mod error;
mod handlers;
mod ports;
mod ws;

pub use error::AppError;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub docker: Arc<adapters::DockerAdapter>,
    pub system: Arc<adapters::SystemAdapter>,
    pub hardware: Arc<adapters::HardwareAdapter>,
    pub config: Arc<adapters::ConfigAdapter>,
    pub events_tx: broadcast::Sender<ws::Event>,
}

fn api_router(state: AppState) -> Router {
    Router::new()
        // System endpoints
        .route("/api/v1/system/info", axum::routing::get(handlers::system::get_info))
        .route("/api/v1/system/stats", axum::routing::get(handlers::system::get_stats))
        // Container endpoints
        .route("/api/v1/containers", axum::routing::get(handlers::containers::list))
        .route("/api/v1/containers/{id}/start", axum::routing::post(handlers::containers::start))
        .route("/api/v1/containers/{id}/stop", axum::routing::post(handlers::containers::stop))
        .route("/api/v1/containers/{id}/restart", axum::routing::post(handlers::containers::restart))
        .route("/api/v1/containers/{id}/logs", axum::routing::get(handlers::containers::logs))
        // Hardware endpoints
        .route("/api/v1/hardware/devices", axum::routing::get(handlers::hardware::list_devices))
        .route("/api/v1/hardware/sdr", axum::routing::get(handlers::hardware::list_sdr))
        // Network endpoints
        .route("/api/v1/network/interfaces", axum::routing::get(handlers::network::list_interfaces))
        // Config endpoints
        .route("/api/v1/config", axum::routing::get(handlers::config::get_config))
        .route("/api/v1/config", axum::routing::put(handlers::config::update_config))
        // App catalog endpoints
        .route("/api/v1/apps/catalog", axum::routing::get(handlers::apps::list_catalog))
        .route("/api/v1/apps/install", axum::routing::post(handlers::apps::install_app))
        .route("/api/v1/apps/{id}", axum::routing::delete(handlers::apps::uninstall_app))
        // WebSocket
        .route("/ws/events", axum::routing::get(handlers::ws_handler::ws_handler))
        // Health
        .route("/health", axum::routing::get(handlers::health))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "airwaves_manager=info,tower_http=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Airwaves OS Manager");

    // Initialize adapters
    let docker = Arc::new(adapters::DockerAdapter::new().await?);
    let system = Arc::new(adapters::SystemAdapter::new());
    let hardware = Arc::new(adapters::HardwareAdapter::new());
    let config = Arc::new(adapters::ConfigAdapter::new("/etc/airwaves/config.json"));
    let (events_tx, _) = broadcast::channel(256);

    let state = AppState {
        docker,
        system,
        hardware,
        config,
        events_tx,
    };

    let app = api_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Airwaves OS Manager stopped");
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    tracing::info!("Shutdown signal received");
}
