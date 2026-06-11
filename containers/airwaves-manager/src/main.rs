use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod adapters;
mod domain;
mod error;
mod forwarding;
mod handlers;
mod ports;
mod sdr;
mod ws;

pub use error::AppError;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub docker: Arc<adapters::DockerAdapter>,
    pub system: Arc<adapters::SystemAdapter>,
    pub hardware: Arc<adapters::HardwareAdapter>,
    pub config: Arc<adapters::ConfigAdapter>,
    pub host: Arc<adapters::HostAdapter>,
    pub updater: Arc<adapters::UpdaterAdapter>,
    pub events_tx: broadcast::Sender<ws::Event>,
    pub forwarding_stats: Arc<Mutex<domain::ForwardingStats>>,
    pub message_buffer: Arc<Mutex<VecDeque<domain::DecodedMessage>>>,
}

fn api_router(state: AppState) -> Router {
    Router::new()
        // System endpoints
        .route("/api/v1/system/info", axum::routing::get(handlers::system::get_info))
        .route("/api/v1/system/stats", axum::routing::get(handlers::system::get_stats))
        .route("/api/v1/system/overview", axum::routing::get(handlers::system::get_overview))
        // Host control (privileged operations on the host)
        .route("/api/v1/system/hostname", axum::routing::post(handlers::host::set_hostname))
        .route("/api/v1/system/reboot", axum::routing::post(handlers::host::reboot))
        .route("/api/v1/system/shutdown", axum::routing::post(handlers::host::shutdown))
        .route("/api/v1/system/timezone", axum::routing::post(handlers::host::set_timezone))
        .route("/api/v1/system/service/restart", axum::routing::post(handlers::host::restart_service))
        // System updater
        .route("/api/v1/system/update/status", axum::routing::get(handlers::update::get_status))
        .route("/api/v1/system/update/check", axum::routing::post(handlers::update::check))
        .route("/api/v1/system/update/apply", axum::routing::post(handlers::update::apply))
        .route("/api/v1/system/update/progress", axum::routing::get(handlers::update::progress))
        .route("/api/v1/system/update/channel", axum::routing::post(handlers::update::set_channel))
        .route("/api/v1/system/update/refresh", axum::routing::post(handlers::update::refresh))
        // Container endpoints
        .route("/api/v1/containers", axum::routing::get(handlers::containers::list))
        .route("/api/v1/containers/stats", axum::routing::get(handlers::containers::stats))
        .route("/api/v1/containers/{id}/start", axum::routing::post(handlers::containers::start))
        .route("/api/v1/containers/{id}/stop", axum::routing::post(handlers::containers::stop))
        .route("/api/v1/containers/{id}/restart", axum::routing::post(handlers::containers::restart))
        .route("/api/v1/containers/{id}/logs", axum::routing::get(handlers::containers::logs))
        // Hardware endpoints
        .route("/api/v1/hardware/devices", axum::routing::get(handlers::hardware::list_devices))
        .route("/api/v1/hardware/sdr", axum::routing::get(handlers::hardware::list_sdr))
        .route("/api/v1/hardware/sdr/{id}", axum::routing::put(handlers::hardware::update_sdr))
        // Network endpoints
        .route("/api/v1/network/interfaces", axum::routing::get(handlers::network::list_interfaces))
        // WiFi endpoints
        .route("/api/v1/wifi/status", axum::routing::get(handlers::wifi::get_status))
        .route("/api/v1/wifi/scan", axum::routing::get(handlers::wifi::scan_networks))
        .route("/api/v1/wifi/connect", axum::routing::post(handlers::wifi::connect))
        // App proxy management
        .route("/api/v1/proxy/list", axum::routing::get(handlers::proxy::list_proxies))
        .route("/api/v1/proxy/generate", axum::routing::post(handlers::proxy::generate_nginx_config))
        // Config endpoints
        .route("/api/v1/config", axum::routing::get(handlers::config::get_config))
        .route("/api/v1/config", axum::routing::put(handlers::config::update_config))
        .route("/api/v1/config/backup", axum::routing::get(handlers::config::export_backup))
        .route("/api/v1/config/restore", axum::routing::post(handlers::config::import_backup))
        // App catalog endpoints
        .route("/api/v1/apps/catalog", axum::routing::get(handlers::apps::list_catalog))
        .route("/api/v1/apps/install", axum::routing::post(handlers::apps::install_app))
        .route(
            "/api/v1/apps/{id}/config",
            axum::routing::put(handlers::apps::update_app_config),
        )
        .route("/api/v1/apps/{id}", axum::routing::delete(handlers::apps::uninstall_app))
        // Tracking (aircraft/ship positions from decoder containers)
        .route("/api/v1/tracking/vehicles", axum::routing::get(handlers::tracking::get_vehicles))
        // Fleet management (multi-node)
        .route("/api/v1/fleet", axum::routing::get(handlers::fleet::get_fleet))
        .route("/api/v1/fleet/discover", axum::routing::get(handlers::fleet::discover_nodes))
        .route("/api/v1/fleet/pair", axum::routing::post(handlers::fleet::pair_node))
        .route("/api/v1/fleet/{id}", axum::routing::delete(handlers::fleet::unpair_node))
        // Feed management endpoints
        .route("/api/v1/feeds", axum::routing::get(handlers::feeds::list_feeds))
        .route("/api/v1/feeds", axum::routing::post(handlers::feeds::upsert_feed))
        .route("/api/v1/feeds/{id}", axum::routing::delete(handlers::feeds::delete_feed))
        // Message forwarding (multi-node)
        .route("/api/v1/messages/ingest", axum::routing::post(handlers::forwarding::ingest_messages))
        .route("/api/v1/messages", axum::routing::get(handlers::forwarding::get_messages))
        .route("/api/v1/forwarding/config", axum::routing::get(handlers::forwarding::get_forwarding_config))
        .route("/api/v1/forwarding/config", axum::routing::put(handlers::forwarding::set_forwarding_config))
        .route("/api/v1/forwarding/stats", axum::routing::get(handlers::forwarding::get_forwarding_stats))
        .route("/api/v1/messages/simulate", axum::routing::post(handlers::forwarding::simulate_messages))
        // Command execution (web terminal)
        .route("/api/v1/system/exec", axum::routing::post(handlers::exec::exec_command))
        // WebSocket
        .route("/ws/events", axum::routing::get(handlers::ws_handler::ws_handler))
        .route("/ws/logs/{id}", axum::routing::get(handlers::ws_handler::ws_logs_handler))
        // Health
        .route("/health", axum::routing::get(handlers::health))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)) // 1MB max request body
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
    let host = Arc::new(adapters::HostAdapter::new());
    let updater = Arc::new(adapters::UpdaterAdapter::new(host.clone(), docker.clone()));
    let (events_tx, _) = broadcast::channel(256);
    let forwarding_stats = Arc::new(Mutex::new(domain::ForwardingStats::default()));
    let message_buffer = Arc::new(Mutex::new(VecDeque::with_capacity(1000)));

    let state = AppState {
        docker,
        system,
        hardware,
        config,
        host,
        updater,
        events_tx,
        forwarding_stats,
        message_buffer,
    };

    // Spawn background event broadcasters
    spawn_stats_broadcaster(state.system.clone(), state.events_tx.clone());
    spawn_docker_event_watcher(state.docker.clone(), state.events_tx.clone());
    spawn_update_checker(state.updater.clone(), state.events_tx.clone());
    spawn_app_reconciler(state.clone());

    // Spawn message forwarding service
    forwarding::spawn_forwarding_service(
        state.docker.clone(),
        state.config.clone(),
        state.forwarding_stats.clone(),
        state.message_buffer.clone(),
    );

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

/// Broadcasts system stats to WebSocket clients every 5 seconds
fn spawn_stats_broadcaster(
    system: Arc<adapters::SystemAdapter>,
    tx: broadcast::Sender<ws::Event>,
) {
    use crate::ports::SystemPort;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if tx.receiver_count() == 0 {
                continue; // No clients connected, skip
            }
            if let Ok(stats) = system.get_stats() {
                let _ = tx.send(ws::Event::SystemStats {
                    cpu_usage: stats.cpu_usage,
                    memory_percent: stats.memory_percent,
                    disk_percent: stats.disk_percent,
                    temperature: stats.temperature,
                });
            }
        }
    });
}

/// On startup, reconcile installed apps: any app recorded in config.json whose
/// container is missing is re-created. Docker's restart policy already brings
/// apps back after a reboot; this is the backstop for cases where a container
/// was pruned or failed to be recreated, so the recorded app set is authoritative.
fn spawn_app_reconciler(state: AppState) {
    use crate::ports::{ConfigPort, DockerPort};

    tokio::spawn(async move {
        // Give Docker a moment to bring restart-policy containers back first.
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;

        if let Err(e) = handlers::apps::migrate_acarsdec_output_policy(&state).await {
            tracing::warn!("App reconcile: failed ACARS decoder output migration: {}", e);
        }

        if let Err(e) = handlers::apps::migrate_sdr_assignment_metadata(&state).await {
            tracing::warn!("App reconcile: failed SDR assignment migration: {}", e);
        }

        let config = match state.config.read_config().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("App reconcile: cannot read config: {}", e);
                return;
            }
        };
        let recorded = handlers::apps::recorded_app_ids(&config);
        let recorded_set: std::collections::HashSet<String> = recorded.iter().cloned().collect();
        match state
            .docker
            .prune_unrecorded_app_containers(&recorded_set)
            .await
        {
            Ok(removed) => {
                for name in removed {
                    tracing::info!("Reconcile: removed unrecorded app container {}", name);
                }
            }
            Err(e) => tracing::warn!("App reconcile: failed to prune unrecorded apps: {}", e),
        }

        if recorded.is_empty() {
            return;
        }

        let existing: std::collections::HashSet<String> = state
            .docker
            .list_containers()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.name.trim_start_matches('/').to_string())
            .collect();

        let catalog = handlers::apps::load_catalog().await;
        for id in recorded {
            let cname = format!("airwaves-{id}");
            if existing.contains(&cname) {
                continue;
            }
            match catalog.iter().find(|a| a.id == id) {
                Some(app) => {
                    tracing::info!("Reconcile: re-creating missing app container {}", cname);
                    // Re-apply the user's stored configuration (e.g. assigned
                    // SDR) on top of the catalog defaults, so a recreated app
                    // keeps the radio/settings it was installed with instead of
                    // reverting to the generic default.
                    let mut app = app.clone();
                    for (k, v) in handlers::apps::recorded_app_env(&config, &id) {
                        app.env.insert(k, v);
                    }
                    if let Err(e) = state.docker.install_app(&app).await {
                        tracing::warn!("Reconcile: failed to re-create {}: {}", cname, e);
                    }
                }
                None => tracing::warn!("Reconcile: app {} not in catalog; skipping", id),
            }
        }
    });
}

/// Periodically checks for system updates and broadcasts when one is available.
fn spawn_update_checker(
    updater: Arc<adapters::UpdaterAdapter>,
    tx: broadcast::Sender<ws::Event>,
) {
    use crate::ports::UpdatePort;

    tokio::spawn(async move {
        // Small startup delay so the gateway is reachable first.
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(12 * 60 * 60));
        loop {
            interval.tick().await;
            let status = updater.check().await;
            if status.update_available {
                if let Some(sev) = status.highest_severity {
                    let severity = serde_json::to_value(sev)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_else(|| "nice-to-have".to_string());
                    let _ = tx.send(ws::Event::UpdateAvailable {
                        severity,
                        os_version: status.available_os_version.clone(),
                    });
                }
            }
        }
    });
}

/// Watches Docker events and broadcasts container state changes
fn spawn_docker_event_watcher(
    docker: Arc<adapters::DockerAdapter>,
    tx: broadcast::Sender<ws::Event>,
) {
    use futures::StreamExt;

    tokio::spawn(async move {
        loop {
            let mut stream = docker.watch_events().await;
            while let Some(event) = stream.next().await {
                if let Ok(evt) = event {
                    if let Some(action) = evt.action {
                        let actor = evt.actor.unwrap_or_default();
                        let id = actor.id.unwrap_or_default();
                        let name = actor
                            .attributes
                            .as_ref()
                            .and_then(|a| a.get("name").cloned())
                            .unwrap_or_default();

                        let status = action.to_string();
                        tracing::debug!("Docker event: {} {} {}", name, status, id);

                        let _ = tx.send(ws::Event::ContainerStatusChanged {
                            id: id[..12.min(id.len())].to_string(),
                            name,
                            status,
                        });
                    }
                }
            }
            // Stream ended (Docker daemon restarted?), retry after delay
            tracing::warn!("Docker event stream ended, reconnecting in 5s...");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}
