use axum::Json;
use serde::{Deserialize, Serialize};

use crate::AppError;

#[derive(Debug, Serialize)]
pub struct WifiNetwork {
    pub ssid: String,
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

/// Get current WiFi status
pub async fn get_status() -> Result<Json<WifiStatus>, AppError> {
    // Check if a wireless interface exists
    let iface = find_wifi_interface();

    if iface.is_none() {
        return Ok(Json(WifiStatus {
            enabled: false,
            connected: false,
            ssid: None,
            ip: None,
            interface: String::new(),
        }));
    }

    let iface = iface.unwrap();

    // Check connection status via iw
    let connected_ssid = get_connected_ssid(&iface);
    let ip = get_interface_ip(&iface);

    Ok(Json(WifiStatus {
        enabled: true,
        connected: connected_ssid.is_some(),
        ssid: connected_ssid,
        ip,
        interface: iface,
    }))
}

/// Scan for available WiFi networks
pub async fn scan_networks() -> Result<Json<Vec<WifiNetwork>>, AppError> {
    let iface = find_wifi_interface()
        .ok_or_else(|| AppError::NotFound("No WiFi interface found".to_string()))?;

    let connected_ssid = get_connected_ssid(&iface);

    // Trigger scan
    let _ = std::process::Command::new("iw")
        .args(["dev", &iface, "scan", "trigger"])
        .output();

    // Wait briefly for scan to complete
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Get scan results
    let output = std::process::Command::new("iw")
        .args(["dev", &iface, "scan", "dump"])
        .output()
        .map_err(|e| AppError::Internal(format!("WiFi scan failed: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut networks = Vec::new();
    let mut current_ssid = String::new();
    let mut current_signal: i32 = -100;
    let mut current_freq = String::new();
    let mut current_security = "Open".to_string();

    for line in stdout.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("BSS ") {
            // Save previous network
            if !current_ssid.is_empty() {
                networks.push(WifiNetwork {
                    connected: connected_ssid.as_deref() == Some(&current_ssid),
                    ssid: std::mem::take(&mut current_ssid),
                    signal: current_signal,
                    frequency: std::mem::take(&mut current_freq),
                    security: std::mem::take(&mut current_security),
                });
            }
            current_signal = -100;
            current_security = "Open".to_string();
        } else if trimmed.starts_with("SSID: ") {
            current_ssid = trimmed[6..].to_string();
        } else if trimmed.starts_with("signal: ") {
            current_signal = trimmed
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(-100);
        } else if trimmed.starts_with("freq: ") {
            let freq_mhz: u32 = trimmed[6..].trim().parse().unwrap_or(0);
            current_freq = if freq_mhz > 5000 { "5 GHz".to_string() } else { "2.4 GHz".to_string() };
        } else if trimmed.contains("WPA") || trimmed.contains("RSN") {
            current_security = if trimmed.contains("WPA2") || trimmed.contains("RSN") {
                "WPA2".to_string()
            } else {
                "WPA".to_string()
            };
        }
    }

    // Push last network
    if !current_ssid.is_empty() {
        networks.push(WifiNetwork {
            connected: connected_ssid.as_deref() == Some(&current_ssid),
            ssid: current_ssid,
            signal: current_signal,
            frequency: current_freq,
            security: current_security,
        });
    }

    // Sort by signal strength (strongest first), deduplicate by SSID
    networks.sort_by(|a, b| b.signal.cmp(&a.signal));
    networks.dedup_by(|a, b| a.ssid == b.ssid);

    Ok(Json(networks))
}

#[derive(Deserialize)]
pub struct ConnectRequest {
    pub ssid: String,
    pub password: Option<String>,
}

/// Connect to a WiFi network
pub async fn connect(Json(req): Json<ConnectRequest>) -> Result<Json<serde_json::Value>, AppError> {
    let iface = find_wifi_interface()
        .ok_or_else(|| AppError::NotFound("No WiFi interface found".to_string()))?;

    // Write wpa_supplicant config
    let wpa_config = if let Some(ref psk) = req.password {
        format!(
            "network={{\n    ssid=\"{}\"\n    psk=\"{}\"\n    key_mgmt=WPA-PSK\n}}\n",
            req.ssid, psk
        )
    } else {
        format!(
            "network={{\n    ssid=\"{}\"\n    key_mgmt=NONE\n}}\n",
            req.ssid
        )
    };

    // Write network config for systemd-networkd
    let network_config = format!(
        "[Match]\nName={}\n\n[Network]\nDHCP=yes\n",
        iface
    );

    std::fs::write(format!("/etc/wpa_supplicant/wpa_supplicant-{}.conf", iface), &wpa_config)
        .map_err(|e| AppError::Internal(format!("Failed to write wpa config: {}", e)))?;

    std::fs::write(format!("/etc/systemd/network/20-{}.network", iface), &network_config)
        .map_err(|e| AppError::Internal(format!("Failed to write network config: {}", e)))?;

    // Restart networking
    let _ = std::process::Command::new("systemctl")
        .args(["restart", &format!("wpa_supplicant@{}", iface)])
        .output();
    let _ = std::process::Command::new("systemctl")
        .args(["restart", "systemd-networkd"])
        .output();

    Ok(Json(serde_json::json!({
        "status": "connecting",
        "ssid": req.ssid,
        "interface": iface,
    })))
}

// ---- Helpers ----

fn find_wifi_interface() -> Option<String> {
    let net_dir = std::path::Path::new("/sys/class/net");
    if let Ok(entries) = std::fs::read_dir(net_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Check if it's a wireless interface
            if entry.path().join("wireless").exists() || name.starts_with("wl") {
                return Some(name);
            }
        }
    }
    None
}

fn get_connected_ssid(iface: &str) -> Option<String> {
    std::process::Command::new("iw")
        .args(["dev", iface, "link"])
        .output()
        .ok()
        .and_then(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .find(|l| l.trim().starts_with("SSID:"))
                .map(|l| l.trim()[6..].to_string())
        })
}

fn get_interface_ip(iface: &str) -> Option<String> {
    std::process::Command::new("ip")
        .args(["-4", "-j", "addr", "show", iface])
        .output()
        .ok()
        .and_then(|output| {
            serde_json::from_slice::<serde_json::Value>(&output.stdout).ok()
        })
        .and_then(|json| {
            json.as_array()?.first()?["addr_info"]
                .as_array()?.first()?["local"]
                .as_str().map(String::from)
        })
}
