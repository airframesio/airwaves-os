pub mod apps;
pub mod config;
pub mod containers;
pub mod exec;
pub mod feeds;
pub mod fleet;
pub mod hardware;
pub mod network;
pub mod system;
pub mod tracking;
pub mod ws_handler;

use axum::Json;
use serde_json::json;

pub async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "airwaves-manager"
    }))
}
