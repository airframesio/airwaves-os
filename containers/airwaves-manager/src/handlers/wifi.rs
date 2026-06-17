use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::{AppError, AppState};

#[derive(Debug, Serialize)]
pub struct WifiNetwork {
    pub ssid: String,
    /// Signal quality as a 0-100 percentage (nmcli's SIGNAL), not dBm.
    pub signal: i32,
    pub security: String,
    pub frequency: String,
    pub connected: bool,
}

#[derive(Debug, Serialize)]
pub struct WifiStatus {
    pub enabled: bool,
    pub connected: bool,
    pub ssid: Option<String>,
    pub ip: Option<String>,
    pub interface: String,
}

fn is_simulated() -> bool {
    std::env::var("SIMULATE_HARDWARE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

/// Split one `nmcli --terse` line into fields, honoring nmcli's backslash
/// escaping (`\:` is a literal colon inside a value, `\\` a literal backslash).
fn split_terse(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(n) => cur.push(n),
                None => cur.push('\\'),
            },
            ':' => fields.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// Find the first Wi-Fi device and its connection state from
/// `nmcli -t -f DEVICE,TYPE,STATE device status`.
async fn wifi_device(state: &AppState) -> Option<(String, String)> {
    let out = state
        .host
        .nmcli_capture(&["-t", "-f", "DEVICE,TYPE,STATE", "device", "status"])
        .await?;
    for line in out.lines() {
        let f = split_terse(line);
        if f.len() >= 3 && f[1] == "wifi" {
            return Some((f[0].clone(), f[2].clone()));
        }
    }
    None
}

/// Get current Wi-Fi status.
pub async fn get_status(State(state): State<AppState>) -> Result<Json<WifiStatus>, AppError> {
    if is_simulated() {
        return Ok(Json(WifiStatus {
            enabled: true,
            connected: false,
            ssid: None,
            ip: None,
            interface: "wlan0".to_string(),
        }));
    }

    let (iface, dev_state) = match wifi_device(&state).await {
        Some(v) => v,
        // No Wi-Fi hardware present.
        None => {
            return Ok(Json(WifiStatus {
                enabled: false,
                connected: false,
                ssid: None,
                ip: None,
                interface: String::new(),
            }))
        }
    };

    let enabled = state
        .host
        .nmcli_capture(&["-t", "radio", "wifi"])
        .await
        .map(|s| s.trim() == "enabled")
        .unwrap_or(false);

    // SSID of the active AP on this interface.
    let ssid = state
        .host
        .nmcli_capture(&["-t", "-f", "ACTIVE,SSID", "device", "wifi", "list", "ifname", &iface])
        .await
        .and_then(|out| {
            out.lines().find_map(|l| {
                let f = split_terse(l);
                if f.len() >= 2 && f[0] == "yes" && !f[1].is_empty() {
                    Some(f[1].clone())
                } else {
                    None
                }
            })
        });

    // IPv4 address (strip the /prefix). nmcli prints `IP4.ADDRESS[1]:1.2.3.4/24`.
    let ip = state
        .host
        .nmcli_capture(&["-t", "-f", "IP4.ADDRESS", "device", "show", &iface])
        .await
        .and_then(|out| {
            out.lines().find_map(|l| {
                let f = split_terse(l);
                if f.len() >= 2 && f[0].starts_with("IP4.ADDRESS") && !f[1].is_empty() {
                    Some(f[1].split('/').next().unwrap_or(&f[1]).to_string())
                } else {
                    None
                }
            })
        });

    let connected = ssid.is_some() || dev_state.starts_with("connected");

    Ok(Json(WifiStatus {
        enabled,
        connected,
        ssid,
        ip,
        interface: iface,
    }))
}

/// Scan for available Wi-Fi networks.
pub async fn scan_networks(
    State(state): State<AppState>,
) -> Result<Json<Vec<WifiNetwork>>, AppError> {
    if is_simulated() {
        return Ok(Json(vec![
            WifiNetwork { ssid: "AirwavesNet".to_string(), signal: 92, security: "WPA2".to_string(), frequency: "2.4 GHz".to_string(), connected: false },
            WifiNetwork { ssid: "HomeWiFi-5G".to_string(), signal: 74, security: "WPA2".to_string(), frequency: "5 GHz".to_string(), connected: false },
            WifiNetwork { ssid: "Neighbor_Guest".to_string(), signal: 55, security: "WPA2".to_string(), frequency: "2.4 GHz".to_string(), connected: false },
            WifiNetwork { ssid: "CoffeeShop".to_string(), signal: 40, security: "".to_string(), frequency: "2.4 GHz".to_string(), connected: false },
        ]));
    }

    // Ensure there is Wi-Fi hardware before asking NM to scan.
    if wifi_device(&state).await.is_none() {
        return Err(AppError::NotFound("No Wi-Fi interface found".to_string()));
    }

    let out = state
        .host
        .nmcli_capture(&[
            "-t",
            "-f",
            "ACTIVE,SSID,SIGNAL,SECURITY,FREQ",
            "device",
            "wifi",
            "list",
            "--rescan",
            "yes",
        ])
        .await
        .ok_or_else(|| AppError::Internal("Wi-Fi scan failed (nmcli)".to_string()))?;

    let mut networks = Vec::new();
    for line in out.lines() {
        let f = split_terse(line);
        if f.len() < 5 {
            continue;
        }
        let ssid = f[1].clone();
        if ssid.is_empty() {
            continue; // hidden network
        }
        let signal: i32 = f[2].trim().parse().unwrap_or(0);
        let security = {
            let s = f[3].trim();
            if s.is_empty() { "Open".to_string() } else { s.to_string() }
        };
        let freq_mhz: u32 = f[4].split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let frequency = if freq_mhz >= 5000 { "5 GHz".to_string() } else { "2.4 GHz".to_string() };
        networks.push(WifiNetwork {
            ssid,
            signal,
            security,
            frequency,
            connected: f[0] == "yes",
        });
    }

    // Strongest first, then keep one entry per SSID (drops band/BSS duplicates).
    networks.sort_by(|a, b| b.signal.cmp(&a.signal));
    networks.dedup_by(|a, b| a.ssid == b.ssid);

    Ok(Json(networks))
}

#[derive(Deserialize)]
pub struct ConnectRequest {
    pub ssid: String,
    pub password: Option<String>,
}

/// Reject SSID / passphrase values with control characters (NUL/newline etc.).
/// Args go to nmcli as discrete argv (no shell), so this is belt-and-suspenders.
fn clean_field(v: &str) -> bool {
    !v.is_empty() && v.len() <= 256 && !v.chars().any(|c| c.is_control())
}

/// Connect to a Wi-Fi network via NetworkManager. NM creates/updates the
/// connection profile and brings up DHCP; it persists across reboots.
pub async fn connect(
    State(state): State<AppState>,
    Json(req): Json<ConnectRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !clean_field(&req.ssid) {
        return Err(AppError::BadRequest("Invalid SSID".to_string()));
    }
    if let Some(ref psk) = req.password {
        if !psk.is_empty() && !clean_field(psk) {
            return Err(AppError::BadRequest("Invalid password".to_string()));
        }
    }

    // Make sure Wi-Fi hardware exists and the radio is on before connecting.
    if wifi_device(&state).await.is_none() {
        return Err(AppError::NotFound("No Wi-Fi interface found".to_string()));
    }
    let _ = state.host.nmcli_run(&["radio", "wifi", "on"]).await;

    let result = match req.password.as_deref().filter(|p| !p.is_empty()) {
        Some(psk) => {
            state
                .host
                .nmcli_run(&["device", "wifi", "connect", &req.ssid, "password", psk])
                .await
        }
        None => {
            state
                .host
                .nmcli_run(&["device", "wifi", "connect", &req.ssid])
                .await
        }
    };

    result.map_err(|e| AppError::Internal(format!("Failed to connect to '{}': {e}", req.ssid)))?;

    Ok(Json(serde_json::json!({
        "status": "connected",
        "ssid": req.ssid,
    })))
}
