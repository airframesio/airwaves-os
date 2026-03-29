use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::ports::ConfigPort;
use crate::{AppError, AppState};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FleetNode {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub ip: String,
    pub status: String,
    pub role: String,
    pub mode: String,
    #[serde(default)]
    pub forwarding_target: Option<String>,
    pub last_seen: String,
}

#[derive(Debug, Serialize)]
pub struct FleetStatus {
    pub local_node: FleetNode,
    pub peers: Vec<FleetNode>,
}

/// Get fleet status: this node + any configured peers
pub async fn get_fleet(
    State(state): State<AppState>,
) -> Result<Json<FleetStatus>, AppError> {
    let config = state.config.read_config().await?;
    let sys_info = {
        use crate::ports::SystemPort;
        state.system.get_info()?
    };

    // Build local node info
    let local_node = FleetNode {
        id: config.device.id.clone(),
        name: config.device.name.clone(),
        hostname: sys_info.hostname.clone(),
        ip: get_primary_ip(),
        status: "online".to_string(),
        role: "primary".to_string(),
        mode: "standalone".to_string(),
        forwarding_target: None,
        last_seen: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    };

    // Read configured peers from config
    let peers: Vec<FleetNode> = config
        .apps
        .get("fleet_peers")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Probe each peer for liveness
    let mut live_peers = Vec::new();
    for mut peer in peers {
        let reachable = probe_peer(&peer.ip).await;
        peer.status = if reachable {
            "online".to_string()
        } else {
            "offline".to_string()
        };
        peer.last_seen = if reachable {
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string()
        } else {
            peer.last_seen.clone()
        };
        live_peers.push(peer);
    }

    Ok(Json(FleetStatus {
        local_node,
        peers: live_peers,
    }))
}

#[derive(Deserialize)]
pub struct PairRequest {
    pub ip: String,
    pub name: Option<String>,
}

/// Pair with a remote Airwaves OS node
pub async fn pair_node(
    State(state): State<AppState>,
    Json(req): Json<PairRequest>,
) -> Result<Json<FleetNode>, AppError> {
    // Probe the remote node
    let reachable = probe_peer(&req.ip).await;
    if !reachable {
        return Err(AppError::BadRequest(format!(
            "Cannot reach Airwaves OS at {}",
            req.ip
        )));
    }

    // Fetch remote node info
    let remote_info = fetch_remote_info(&req.ip).await?;

    let new_peer = FleetNode {
        id: remote_info.device_id,
        name: req.name.unwrap_or(remote_info.hostname.clone()),
        hostname: remote_info.hostname,
        ip: req.ip.clone(),
        status: "online".to_string(),
        role: "secondary".to_string(),
        mode: "standalone".to_string(),
        forwarding_target: None,
        last_seen: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    };

    // Save to config
    let mut config = state.config.read_config().await?;
    let mut peers: Vec<FleetNode> = config
        .apps
        .get("fleet_peers")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Remove existing entry for this IP
    peers.retain(|p| p.ip != req.ip);
    peers.push(new_peer.clone());

    if let Some(obj) = config.apps.as_object_mut() {
        obj.insert(
            "fleet_peers".to_string(),
            serde_json::to_value(&peers).unwrap(),
        );
    } else {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "fleet_peers".to_string(),
            serde_json::to_value(&peers).unwrap(),
        );
        config.apps = serde_json::Value::Object(obj);
    }

    state.config.write_config(&config).await?;

    Ok(Json(new_peer))
}

/// Unpair a node
pub async fn unpair_node(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut config = state.config.read_config().await?;
    let mut peers: Vec<FleetNode> = config
        .apps
        .get("fleet_peers")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    peers.retain(|p| p.id != id && p.ip != id);

    if let Some(obj) = config.apps.as_object_mut() {
        obj.insert(
            "fleet_peers".to_string(),
            serde_json::to_value(&peers).unwrap(),
        );
    }

    state.config.write_config(&config).await?;

    Ok(Json(serde_json::json!({"status": "unpaired", "id": id})))
}

/// Discovered node from mDNS scan
#[derive(Debug, Serialize)]
pub struct DiscoveredNode {
    pub hostname: String,
    pub ip: String,
    pub port: u16,
    pub already_paired: bool,
}

/// Scan local network for other Airwaves OS nodes via mDNS
pub async fn discover_nodes(
    State(state): State<AppState>,
) -> Result<Json<Vec<DiscoveredNode>>, AppError> {
    let config = state.config.read_config().await?;
    let existing_peers: Vec<FleetNode> = config
        .apps
        .get("fleet_peers")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let existing_ips: std::collections::HashSet<String> =
        existing_peers.iter().map(|p| p.ip.clone()).collect();

    let local_ip = get_primary_ip();
    let mut discovered = Vec::new();

    // Use avahi-browse to find _http._tcp services named "Airwaves OS"
    if let Ok(output) = std::process::Command::new("avahi-browse")
        .args(["-t", "-r", "-p", "_http._tcp"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let fields: Vec<&str> = line.split(';').collect();
            // avahi-browse -p format: +;iface;proto;name;type;domain
            // resolve format: =;iface;proto;name;type;domain;hostname;address;port;txt
            if fields.len() >= 9 && fields[0] == "=" {
                let name = fields[3];
                let ip = fields[7];
                let port: u16 = fields[8].parse().unwrap_or(80);

                // Only include Airwaves OS nodes
                if name.contains("Airwaves OS") && ip != local_ip {
                    discovered.push(DiscoveredNode {
                        hostname: fields[6].trim_end_matches('.').to_string(),
                        ip: ip.to_string(),
                        port,
                        already_paired: existing_ips.contains(ip),
                    });
                }
            }
        }
    }

    // Deduplicate by IP
    discovered.sort_by(|a, b| a.ip.cmp(&b.ip));
    discovered.dedup_by(|a, b| a.ip == b.ip);

    Ok(Json(discovered))
}

// ---- Helpers ----

fn get_primary_ip() -> String {
    if let Ok(output) = std::process::Command::new("ip")
        .args(["-4", "-j", "route", "get", "1.1.1.1"])
        .output()
    {
        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            if let Some(arr) = json.as_array() {
                if let Some(first) = arr.first() {
                    if let Some(src) = first["prefsrc"].as_str() {
                        return src.to_string();
                    }
                }
            }
        }
    }
    "unknown".to_string()
}

async fn probe_peer(ip: &str) -> bool {
    let url = format!("http://{}:8080/health", ip);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build();
    match client {
        Ok(c) => c.get(&url).send().await.map(|r| r.status().is_success()).unwrap_or(false),
        Err(_) => false,
    }
}

struct RemoteInfo {
    hostname: String,
    device_id: String,
}

async fn fetch_remote_info(ip: &str) -> Result<RemoteInfo, AppError> {
    let url = format!("http://{}:8080/api/v1/system/info", ip);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let resp: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Cannot reach remote: {}", e)))?
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Invalid response: {}", e)))?;

    Ok(RemoteInfo {
        hostname: resp["hostname"].as_str().unwrap_or("unknown").to_string(),
        device_id: resp["hostname"].as_str().unwrap_or("unknown").to_string(),
    })
}
