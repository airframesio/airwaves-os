use std::collections::{HashMap, HashSet};

use crate::domain::{AirwavesConfig, CatalogApp, SdrDevice};

pub const SDR_ID_ENV_PREFIX: &str = "AIRWAVES_SDR_ID__";
pub const SDR_USB_ACCESS_ENV_KEY: &str = "AIRWAVES_SDR_USB_ACCESS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdrReference {
    pub app_id: Option<String>,
    pub field_key: String,
    pub serial: Option<String>,
    pub device_id: Option<String>,
}

pub fn sdr_id_env_key(field_key: &str) -> String {
    format!("{SDR_ID_ENV_PREFIX}{field_key}")
}

pub fn is_sdr_id_env_key(key: &str) -> bool {
    key.starts_with(SDR_ID_ENV_PREFIX)
}

pub fn is_internal_sdr_env_key(key: &str) -> bool {
    is_sdr_id_env_key(key) || key == SDR_USB_ACCESS_ENV_KEY
}

pub fn requires_full_usb_bus_access(env: &HashMap<String, String>) -> bool {
    env.get(SDR_USB_ACCESS_ENV_KEY)
        .map(|value| {
            let normalized = value.trim().replace('-', "_").to_ascii_lowercase();
            matches!(normalized.as_str(), "full" | "full_bus" | "bus" | "all")
        })
        .unwrap_or(false)
}

pub fn serial_key(serial: &str) -> String {
    serial.trim().to_ascii_lowercase()
}

pub fn driver_from_sdr_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    for part in value.split(',') {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("driver") {
            let driver = val.trim();
            if !driver.is_empty() {
                return Some(driver.to_string());
            }
        }
    }

    None
}

pub fn serial_from_sdr_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    for part in value.split(',') {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("serial") {
            let serial = val.trim();
            if !serial.is_empty() {
                return Some(serial.to_string());
            }
        }
    }

    None
}

fn key_allows_bare_serial(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    if key.contains("TYPE")
        || key.contains("FREQUENC")
        || key.contains("GAIN")
        || key.contains("PPM")
        || key == "DEVICE_INDEX"
    {
        return false;
    }

    key.contains("RTLSDR_DEVICE")
        || key.ends_with("RTL_SERIAL")
        || key.ends_with("AIRSPY_SERIAL")
        || key == "SERIAL"
        || key.ends_with("_SERIAL")
        || (key.contains("SDR") && key.ends_with("_DEVICE"))
}

fn looks_like_bare_serial(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 64
        && !value.eq_ignore_ascii_case("rtlsdr")
        && !value.eq_ignore_ascii_case("airspy")
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

pub fn references_from_env(env: &HashMap<String, String>) -> Vec<SdrReference> {
    let mut refs = Vec::new();
    let mut fields_with_metadata = HashSet::new();

    for (key, value) in env {
        if is_sdr_id_env_key(key) {
            continue;
        }

        let serial = serial_from_sdr_value(value).or_else(|| {
            (key_allows_bare_serial(key) && looks_like_bare_serial(value))
                .then(|| value.trim().to_string())
        });

        let Some(serial) = serial else {
            continue;
        };

        let device_id = env
            .get(&sdr_id_env_key(key))
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(str::to_string);

        if device_id.is_some() {
            fields_with_metadata.insert(key.clone());
        }

        refs.push(SdrReference {
            app_id: None,
            field_key: key.clone(),
            serial: Some(serial),
            device_id,
        });
    }

    for (key, value) in env {
        let Some(field_key) = key.strip_prefix(SDR_ID_ENV_PREFIX) else {
            continue;
        };
        let device_id = value.trim();
        if device_id.is_empty() || fields_with_metadata.contains(field_key) {
            continue;
        }
        refs.push(SdrReference {
            app_id: None,
            field_key: field_key.to_string(),
            serial: None,
            device_id: Some(device_id.to_string()),
        });
    }

    refs
}

pub fn device_id_to_usb_path(id: &str) -> Option<String> {
    let (before_dev, dev) = id.rsplit_once("-dev")?;
    let (_, bus) = before_dev.rsplit_once("-bus")?;
    let bus: u16 = bus.parse().ok()?;
    let dev: u16 = dev.parse().ok()?;
    Some(format!("/dev/bus/usb/{bus:03}/{dev:03}"))
}

fn usb_paths_for_serial(serial: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let sys_usb = std::path::Path::new("/sys/bus/usb/devices");
    let Ok(entries) = std::fs::read_dir(sys_usb) else {
        return paths;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(device_serial) = std::fs::read_to_string(path.join("serial")) else {
            continue;
        };
        if serial_key(device_serial.trim()) != serial_key(serial) {
            continue;
        }

        let bus = std::fs::read_to_string(path.join("busnum"))
            .ok()
            .and_then(|s| s.trim().parse::<u16>().ok());
        let dev = std::fs::read_to_string(path.join("devnum"))
            .ok()
            .and_then(|s| s.trim().parse::<u16>().ok());

        if let (Some(bus), Some(dev)) = (bus, dev) {
            let usb_path = format!("/dev/bus/usb/{bus:03}/{dev:03}");
            if std::path::Path::new(&usb_path).exists() {
                paths.push(usb_path);
            }
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

pub fn usb_device_paths_for_env(env: &HashMap<String, String>) -> Vec<String> {
    let mut paths = Vec::new();

    for reference in references_from_env(env) {
        if let Some(path) = reference
            .device_id
            .as_deref()
            .and_then(device_id_to_usb_path)
            .filter(|path| std::path::Path::new(path).exists())
        {
            paths.push(path);
            continue;
        }

        let Some(serial) = reference.serial.as_deref() else {
            continue;
        };
        let serial_paths = usb_paths_for_serial(serial);
        if serial_paths.len() == 1 {
            paths.push(serial_paths[0].clone());
        } else if serial_paths.len() > 1 {
            tracing::warn!(
                "SDR serial {} is not unique and no exact Airwaves SDR id was recorded; falling back to full USB bus access",
                serial
            );
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

fn assignments_from_apps(apps: &[serde_json::Value]) -> Vec<SdrReference> {
    let mut assignments = Vec::new();

    for app in apps {
        let app_id = app.get("id").and_then(|v| v.as_str()).map(str::to_string);
        let Some(env_value) = app.get("env") else {
            continue;
        };
        let Ok(env) = serde_json::from_value::<HashMap<String, String>>(env_value.clone()) else {
            continue;
        };

        for mut reference in references_from_env(&env) {
            reference.app_id = app_id.clone();
            assignments.push(reference);
        }
    }

    assignments
}

pub fn assignments_from_config(config: &AirwavesConfig) -> Vec<SdrReference> {
    let apps: Vec<serde_json::Value> = match &config.apps {
        serde_json::Value::Array(apps) => apps.clone(),
        serde_json::Value::Object(apps) => apps.values().cloned().collect(),
        _ => Vec::new(),
    };
    assignments_from_apps(&apps)
}

pub fn annotate_sdr_devices(devices: &mut [SdrDevice], config: &AirwavesConfig) {
    let mut serial_counts: HashMap<String, usize> = HashMap::new();
    for device in devices.iter() {
        if let Some(serial) = device.serial.as_deref() {
            *serial_counts.entry(serial_key(serial)).or_insert(0) += 1;
        }
    }

    let mut exact: HashMap<String, Vec<String>> = HashMap::new();
    let mut serial: HashMap<String, Vec<String>> = HashMap::new();
    for assignment in assignments_from_config(config) {
        let app_id = assignment.app_id.unwrap_or_else(|| "unknown".to_string());
        if let Some(device_id) = assignment.device_id {
            exact.entry(device_id).or_default().push(app_id);
        } else if let Some(s) = assignment.serial {
            serial.entry(serial_key(&s)).or_default().push(app_id);
        }
    }

    for device in devices {
        let mut users = exact.remove(&device.id).unwrap_or_default();
        if let Some(s) = device.serial.as_deref() {
            if let Some(serial_users) = serial.get(&serial_key(s)) {
                users.extend(serial_users.iter().cloned());
            }
        }
        users.sort();
        users.dedup();

        if !users.is_empty() {
            device.assigned_to = Some(users.join(", "));
            device.status = if users.len() > 1 {
                "conflict".to_string()
            } else if device
                .serial
                .as_deref()
                .map(|s| serial_counts.get(&serial_key(s)).copied().unwrap_or(0) > 1)
                .unwrap_or(false)
            {
                "ambiguous".to_string()
            } else {
                "assigned".to_string()
            };
        } else if device
            .serial
            .as_deref()
            .map(|s| serial_counts.get(&serial_key(s)).copied().unwrap_or(0) > 1)
            .unwrap_or(false)
        {
            device.status = "ambiguous".to_string();
            device.assigned_to = None;
        }
    }
}

fn references_for_app(app: &CatalogApp) -> Vec<SdrReference> {
    references_from_env(&app.env)
        .into_iter()
        .map(|mut reference| {
            reference.app_id = Some(app.id.clone());
            reference
        })
        .collect()
}

fn references_conflict(a: &SdrReference, b: &SdrReference) -> bool {
    match (a.device_id.as_deref(), b.device_id.as_deref()) {
        (Some(left), Some(right)) => left == right,
        _ => match (a.serial.as_deref(), b.serial.as_deref()) {
            (Some(left), Some(right)) => serial_key(left) == serial_key(right),
            _ => false,
        },
    }
}

pub fn validate_app_sdr_assignments(
    config: &AirwavesConfig,
    target_apps: &[CatalogApp],
    devices: &[SdrDevice],
) -> Result<(), String> {
    let target_ids: HashSet<String> = target_apps.iter().map(|app| app.id.clone()).collect();
    let mut target_refs = Vec::new();
    for app in target_apps {
        target_refs.extend(references_for_app(app));
    }

    let mut serial_counts: HashMap<String, usize> = HashMap::new();
    for device in devices {
        if let Some(serial) = device.serial.as_deref() {
            *serial_counts.entry(serial_key(serial)).or_insert(0) += 1;
        }
    }

    for reference in &target_refs {
        if reference.device_id.is_none() {
            if let Some(serial) = reference.serial.as_deref() {
                if serial_counts.get(&serial_key(serial)).copied().unwrap_or(0) > 1 {
                    return Err(format!(
                        "SDR serial {serial} appears on more than one connected tuner. Select the exact SDR from the picker again before saving."
                    ));
                }
            }
        }
    }

    for (index, left) in target_refs.iter().enumerate() {
        for right in target_refs.iter().skip(index + 1) {
            if references_conflict(left, right) {
                let left_app = left.app_id.as_deref().unwrap_or("this app");
                let right_app = right.app_id.as_deref().unwrap_or("another app");
                let serial = left
                    .serial
                    .as_deref()
                    .or(right.serial.as_deref())
                    .unwrap_or("selected");
                return Err(format!(
                    "SDR {serial} is selected for both {left_app} and {right_app}. Choose a different SDR for one of them."
                ));
            }
        }
    }

    let existing_refs: Vec<SdrReference> = assignments_from_config(config)
        .into_iter()
        .filter(|reference| {
            reference
                .app_id
                .as_ref()
                .map(|id| !target_ids.contains(id))
                .unwrap_or(true)
        })
        .collect();

    for target in &target_refs {
        for existing in &existing_refs {
            if references_conflict(target, existing) {
                let app_id = existing.app_id.as_deref().unwrap_or("another app");
                let serial = target
                    .serial
                    .as_deref()
                    .or(existing.serial.as_deref())
                    .unwrap_or("selected");
                return Err(format!(
                    "SDR {serial} is already assigned to {app_id}. Choose another SDR or update that app first."
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_soapy_serial_and_metadata() {
        let env = HashMap::from([
            (
                "SOAPYSDR".to_string(),
                "driver=rtlsdr,serial=001".to_string(),
            ),
            (
                sdr_id_env_key("SOAPYSDR"),
                "0bda:2838-001-bus009-dev011".to_string(),
            ),
        ]);

        let refs = references_from_env(&env);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].serial.as_deref(), Some("001"));
        assert_eq!(
            refs[0].device_id.as_deref(),
            Some("0bda:2838-001-bus009-dev011")
        );
    }

    #[test]
    fn ignores_device_type_as_bare_serial() {
        let env = HashMap::from([
            ("READSB_DEVICE_TYPE".to_string(), "rtlsdr".to_string()),
            ("READSB_RTLSDR_DEVICE".to_string(), "002".to_string()),
        ]);

        let refs = references_from_env(&env);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].serial.as_deref(), Some("002"));
    }

    #[test]
    fn parses_usb_path_from_sdr_id() {
        assert_eq!(
            device_id_to_usb_path("0bda:2838-001-bus009-dev011").as_deref(),
            Some("/dev/bus/usb/009/011")
        );
    }

    #[test]
    fn accepts_metadata_only_reference() {
        let env = HashMap::from([(
            sdr_id_env_key("SOAPYSDR"),
            "0bda:2838-unknown-bus009-dev011".to_string(),
        )]);

        let refs = references_from_env(&env);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].serial, None);
        assert_eq!(
            refs[0].device_id.as_deref(),
            Some("0bda:2838-unknown-bus009-dev011")
        );
    }

    #[test]
    fn extracts_soapy_driver() {
        assert_eq!(
            driver_from_sdr_value("driver=rtlsdr,serial=003").as_deref(),
            Some("rtlsdr")
        );
        assert_eq!(
            driver_from_sdr_value("serial=003, driver = airspy").as_deref(),
            Some("airspy")
        );
        assert_eq!(driver_from_sdr_value("serial=003"), None);
    }

    #[test]
    fn recognizes_full_usb_bus_access_flag() {
        let env = HashMap::from([(SDR_USB_ACCESS_ENV_KEY.to_string(), "full-bus".to_string())]);
        assert!(requires_full_usb_bus_access(&env));
        assert!(is_internal_sdr_env_key(SDR_USB_ACCESS_ENV_KEY));
        assert!(is_internal_sdr_env_key(&sdr_id_env_key("SOAPYSDR")));
        assert!(!is_internal_sdr_env_key("SOAPYSDR"));
    }
}
