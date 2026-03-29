use axum::Json;

use crate::domain::NetworkInterface;
use crate::AppError;

pub async fn list_interfaces() -> Result<Json<Vec<NetworkInterface>>, AppError> {
    let mut interfaces = Vec::new();

    // Parse IP addresses from /proc/net/fib_trie is complex,
    // so we read /proc/net/if_inet6 and parse ifconfig-style from /sys
    let ip_map = parse_ip_addresses();

    let net_dir = std::path::Path::new("/sys/class/net");
    if let Ok(entries) = std::fs::read_dir(net_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "lo" {
                continue;
            }

            let path = entry.path();

            let mac = std::fs::read_to_string(path.join("address"))
                .unwrap_or_default()
                .trim()
                .to_string();

            let operstate = std::fs::read_to_string(path.join("operstate"))
                .unwrap_or_default()
                .trim()
                .to_string();
            let is_up = operstate == "up";

            let iface_type = if name.starts_with("wl") {
                "wifi"
            } else if name.starts_with("eth") || name.starts_with("en") {
                "ethernet"
            } else if name.starts_with("br") {
                "bridge"
            } else if name.starts_with("docker") || name.starts_with("veth") {
                "docker"
            } else {
                "other"
            };

            let ip_addresses = ip_map.get(&name).cloned().unwrap_or_default();

            interfaces.push(NetworkInterface {
                name,
                mac_address: mac,
                ip_addresses,
                is_up,
                interface_type: iface_type.to_string(),
            });
        }
    }

    Ok(Json(interfaces))
}

/// Parse IPv4 addresses from /proc/net/fib_trie_info or fall back to reading
/// /proc/net/dev + address files. Uses a simple approach: read each interface's
/// inet addr from the /sys/class/net/<iface>/.. tree isn't possible directly,
/// so we parse /proc/net/fib_trie for local addresses per interface.
fn parse_ip_addresses() -> std::collections::HashMap<String, Vec<String>> {
    let mut result: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    // Try reading from /proc/net/fib_trie - parse LOCAL entries
    // Alternatively, use the simpler approach of reading from /proc/net/route + ioctl
    // For containers, the simplest reliable method is to exec `ip -j addr show`
    if let Ok(output) = std::process::Command::new("ip")
        .args(["-j", "-4", "addr", "show"])
        .output()
    {
        if output.status.success() {
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                if let Some(ifaces) = json.as_array() {
                    for iface in ifaces {
                        let name = iface["ifname"].as_str().unwrap_or_default().to_string();
                        if name == "lo" {
                            continue;
                        }
                        if let Some(addr_info) = iface["addr_info"].as_array() {
                            let ips: Vec<String> = addr_info
                                .iter()
                                .filter_map(|a| a["local"].as_str().map(String::from))
                                .collect();
                            if !ips.is_empty() {
                                result.insert(name, ips);
                            }
                        }
                    }
                }
            }
        }
    }

    result
}
