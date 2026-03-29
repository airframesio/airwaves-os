use axum::Json;

use crate::domain::NetworkInterface;
use crate::AppError;

pub async fn list_interfaces() -> Result<Json<Vec<NetworkInterface>>, AppError> {
    let mut interfaces = Vec::new();

    // Read from /sys/class/net/
    let net_dir = std::path::Path::new("/sys/class/net");
    if let Ok(entries) = std::fs::read_dir(net_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "lo" {
                continue;
            }

            let path = entry.path();

            // Read MAC address
            let mac = std::fs::read_to_string(path.join("address"))
                .unwrap_or_default()
                .trim()
                .to_string();

            // Check if up
            let operstate = std::fs::read_to_string(path.join("operstate"))
                .unwrap_or_default()
                .trim()
                .to_string();
            let is_up = operstate == "up";

            // Determine type
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

            // Get IP addresses via /proc/net/fib_trie or ip command
            // For simplicity, we parse from the address file
            let ip_addresses = Vec::new(); // Will be populated by reading from ip command

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
