//! First boot after a network install from Airframes Desktop.
//!
//! The network installer writes `enroll` and `station` blocks into the AWCFG
//! partition's airwaves-install.json; `airwaves-preconfig` moves them to
//! /etc/airwaves/netinstall.json on first boot. The manager then:
//!
//! 1. asks Desktop to enroll the device (`POST <enroll.url>` with the one-time
//!    code). Desktop starts an `airframes-feeder` device sign-in, approves it
//!    as the Desktop user and returns only the `device_code` (and, with the
//!    api-server devices API, a `device_id` and `device_token`);
//! 2. polls `/auth/token` with that code and stores xng's sign-in, as the
//!    dashboard sign-in does;
//! 3. registers the station (`POST /feeder/station`), applies its location
//!    and name, and starts xng with the station's settings when an SDR is free;
//! 4. reports the outcome to Desktop (`POST <enroll.url>/status`).
//!
//! With a device token it also sends `POST /devices/heartbeat` about once a
//! minute. The contract is in docs/NETBOOT.md.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::error::AppError;
use crate::ports::{ConfigPort, HardwarePort, SystemPort};
use crate::AppState;

fn netinstall_path() -> PathBuf {
    config_dir().join("netinstall.json")
}

fn device_path() -> PathBuf {
    config_dir().join("device.json")
}

fn config_dir() -> PathBuf {
    std::env::var("AIRWAVES_CONFIG_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/airwaves"))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Enroll {
    pub url: String,
    pub code: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct XngSettings {
    pub feed_airframes: Option<bool>,
    pub modes: Option<String>,
    pub airband_channels: Option<String>,
    pub atc_feed: Option<bool>,
    pub atc_station_name: Option<String>,
    pub transcribe: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Station {
    pub ident: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude_m: Option<i64>,
    pub name: Option<String>,
    pub xng: Option<XngSettings>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NetinstallFile {
    pub enroll: Option<Enroll>,
    pub station: Option<Station>,
    /// Set after a failed enrollment so it is not retried on every start.
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The device record from the api-server devices API.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceFile {
    pub device_id: Option<serde_json::Value>,
    pub device_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EnrollResponse {
    device_code: String,
    #[serde(default)]
    interval: Option<u64>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    device_id: Option<serde_json::Value>,
    #[serde(default)]
    device_token: Option<String>,
}

async fn read_json<T: for<'de> Deserialize<'de>>(path: &PathBuf) -> Option<T> {
    let bytes = tokio::fs::read(path).await.ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!("{}: {e}", path.display());
            None
        }
    }
}

async fn write_private(path: &PathBuf, value: &impl Serialize) -> Result<(), AppError> {
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, serde_json::to_vec_pretty(value)?)
        .await
        .map_err(|e| AppError::Internal(format!("write {}: {e}", tmp.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).await;
    }
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| AppError::Internal(format!("save {}: {e}", path.display())))
}

/// The device token from a network install, if there is one.
pub async fn device_token() -> Option<String> {
    read_json::<DeviceFile>(&device_path())
        .await?
        .device_token
        .filter(|t| !t.trim().is_empty())
}

/// Starts first-boot enrollment (when /etc/airwaves/netinstall.json exists)
/// and the device heartbeat (when the device has a device token).
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        if let Some(file) = read_json::<NetinstallFile>(&netinstall_path()).await {
            if !file.done {
                // Let the network and Docker settle after first boot.
                tokio::time::sleep(Duration::from_secs(10)).await;
                first_boot(&state, file).await;
            }
        }
        heartbeat_loop(state).await;
    });
}

/// Turns the xng block into the xng app env (the Feed Airframes page's keys).
fn xng_env(station: &Station) -> std::collections::HashMap<String, String> {
    let mut env = std::collections::HashMap::new();
    env.insert(
        crate::xng::STATION_ID_KEY.to_string(),
        station.ident.trim().to_uppercase(),
    );
    let x = station.xng.clone().unwrap_or_default();
    let flag = |b: Option<bool>, default: bool| b.unwrap_or(default).to_string();
    env.insert(
        crate::xng::FEED_KEY.to_string(),
        flag(x.feed_airframes, true),
    );
    env.insert(crate::xng::ATC_FEED_KEY.to_string(), flag(x.atc_feed, true));
    env.insert(
        crate::xng::TRANSCRIBE_KEY.to_string(),
        flag(x.transcribe, false),
    );
    for (key, value) in [
        (crate::xng::MODES_KEY, x.modes),
        (crate::xng::AIRBAND_CHANNELS_KEY, x.airband_channels),
        (crate::xng::ATC_NAME_KEY, x.atc_station_name),
    ] {
        if let Some(v) = value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            env.insert(key.to_string(), v);
        }
    }
    env
}

/// The `POST /feeder/station` body for the station block.
fn station_body(station: &Station) -> serde_json::Value {
    let mut body = serde_json::json!({ "ident": station.ident.trim().to_uppercase() });
    if let (Some(lat), Some(lon)) = (station.latitude, station.longitude) {
        if lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0 {
            body["latitude"] = serde_json::json!(lat);
            body["longitude"] = serde_json::json!(lon);
        }
    }
    body
}

/// Writes the station's location and name into the device settings.
async fn apply_station_settings(state: &AppState, station: &Station) -> Result<(), AppError> {
    let mut config = state.config.read_config().await?;
    if let (Some(lat), Some(lon)) = (station.latitude, station.longitude) {
        if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
            config.station.latitude = lat;
            config.station.longitude = lon;
        }
    }
    if let Some(alt) = station.altitude_m {
        config.station.altitude_m = alt as _;
    }
    if let Some(name) = station
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        config.device.name = name.chars().take(64).collect();
    }
    state.config.write_config(&config).await
}

fn deadline_passed(enroll: &Enroll) -> bool {
    enroll
        .expires_at
        .as_deref()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .is_some_and(|t| t < chrono::Utc::now())
}

async fn report(enroll: &Enroll, mut body: serde_json::Value) {
    body["code"] = serde_json::json!(enroll.code);
    let url = format!("{}/status", enroll.url.trim_end_matches('/'));
    for attempt in 0..3 {
        match crate::airframes::http().post(&url).json(&body).send().await {
            Ok(r) if r.status().is_success() || r.status().is_client_error() => return,
            _ => tokio::time::sleep(Duration::from_secs(5 * (attempt + 1))).await,
        }
    }
}

async fn finish(file: &mut NetinstallFile, error: Option<String>) {
    match error {
        None => {
            let _ = tokio::fs::remove_file(netinstall_path()).await;
        }
        Some(e) => {
            tracing::warn!("Network install enrollment stopped: {e}");
            file.done = true;
            file.error = Some(e);
            if let Some(en) = file.enroll.as_mut() {
                en.code.clear();
            }
            let _ = write_private(&netinstall_path(), file).await;
        }
    }
}

async fn first_boot(state: &AppState, mut file: NetinstallFile) {
    let station = file.station.clone().filter(|s| !s.ident.trim().is_empty());
    if let Some(s) = &station {
        if let Err(e) = apply_station_settings(state, s).await {
            tracing::warn!("Network install: could not save the station settings: {e}");
        }
    }
    let Some(enroll) = file
        .enroll
        .clone()
        .filter(|e| !e.url.is_empty() && !e.code.is_empty())
    else {
        finish(&mut file, None).await;
        return;
    };
    if crate::airframes::signed_in().await {
        tracing::info!("Network install: already signed in to Airframes, skipping enrollment");
        report(
            &enroll,
            serde_json::json!({ "stage": "done", "message": "already signed in" }),
        )
        .await;
        finish(&mut file, None).await;
        return;
    }

    // 1. Ask Desktop for an approved device code.
    let info = state.system.get_info().ok();
    let mac = primary_mac().await;
    let request = serde_json::json!({
        "code": enroll.code,
        "hostname": info.as_ref().map(|i| i.hostname.clone()),
        "model": info.as_ref().map(|i| i.model.clone()),
        "os_version": info.as_ref().map(|i| i.airwaves_version.clone()),
        "mac": mac,
    });
    let resp: EnrollResponse = loop {
        if deadline_passed(&enroll) {
            return finish(
                &mut file,
                Some("the enrollment code expired before Desktop answered".into()),
            )
            .await;
        }
        match crate::airframes::http()
            .post(&enroll.url)
            .json(&request)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => match r.json::<EnrollResponse>().await {
                Ok(v) => break v,
                Err(e) => {
                    return finish(
                        &mut file,
                        Some(format!("Desktop sent an unreadable answer: {e}")),
                    )
                    .await
                }
            },
            Ok(r) if matches!(r.status().as_u16(), 404 | 410 | 401 | 403) => {
                return finish(
                    &mut file,
                    Some(format!(
                        "Desktop refused the enrollment code ({})",
                        r.status().as_u16()
                    )),
                )
                .await;
            }
            Ok(r) => tracing::info!("Network install: Desktop answered {}, retrying", r.status()),
            Err(e) => tracing::info!("Network install: Desktop not reachable ({e}), retrying"),
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    };
    if resp.device_token.is_some() || resp.device_id.is_some() {
        let dev = DeviceFile {
            device_id: resp.device_id.clone(),
            device_token: resp.device_token.clone(),
        };
        if let Err(e) = write_private(&device_path(), &dev).await {
            tracing::warn!("Network install: could not save the device token: {e}");
        }
    }

    // 2. Sign in with the approved code.
    let mut interval = Duration::from_secs(resp.interval.unwrap_or(5).clamp(1, 60));
    let until = tokio::time::Instant::now() + Duration::from_secs(resp.expires_in.unwrap_or(600));
    let (username, access_token) = loop {
        match crate::airframes::poll_device_code(&resp.device_code).await {
            Ok(crate::airframes::TokenPoll::SignedIn {
                username,
                access_token,
            }) => break (username, access_token),
            Ok(crate::airframes::TokenPoll::Pending) => {}
            Ok(crate::airframes::TokenPoll::SlowDown) => interval += Duration::from_secs(5),
            Ok(crate::airframes::TokenPoll::Denied) => {
                report(
                    &enroll,
                    serde_json::json!({ "stage": "failed", "message": "sign-in denied" }),
                )
                .await;
                return finish(&mut file, Some("the sign-in was denied".into())).await;
            }
            Ok(crate::airframes::TokenPoll::Expired) => {
                report(
                    &enroll,
                    serde_json::json!({ "stage": "failed", "message": "sign-in code expired" }),
                )
                .await;
                return finish(&mut file, Some("the sign-in code expired".into())).await;
            }
            Err(e) => tracing::info!("Network install: token poll failed ({e}), retrying"),
        }
        if tokio::time::Instant::now() > until {
            report(
                &enroll,
                serde_json::json!({ "stage": "failed", "message": "sign-in timed out" }),
            )
            .await;
            return finish(&mut file, Some("the sign-in timed out".into())).await;
        }
        tokio::time::sleep(interval).await;
    };
    report(
        &enroll,
        serde_json::json!({ "stage": "signed_in", "username": username }),
    )
    .await;

    // 3. Register the station, then start xng.
    let mut outcome = serde_json::json!({ "stage": "done", "username": username });
    if let Some(s) = &station {
        let reg = match access_token.as_deref() {
            Some(token) => crate::airframes::register_station_body(token, station_body(s)).await,
            None => serde_json::json!({ "status": "skipped", "reason": "no access token" }),
        };
        if let Some(id) = reg.get("station_id") {
            outcome["station_id"] = id.clone();
        }
        outcome["ident"] = serde_json::json!(s.ident.trim().to_uppercase());
        if matches!(
            reg["status"].as_str(),
            Some("created" | "claimed" | "exists")
        ) {
            let mut r =
                serde_json::json!({ "stage": "station_registered", "ident": outcome["ident"] });
            if let Some(id) = reg.get("station_id") {
                r["station_id"] = id.clone();
            }
            report(&enroll, r).await;
        } else {
            outcome["message"] = serde_json::json!(format!(
                "station {}: {}",
                reg["status"].as_str().unwrap_or("error"),
                reg["message"].as_str().unwrap_or("")
            ));
        }
        match start_xng(state, s).await {
            Ok(msg) => tracing::info!("Network install: {msg}"),
            Err(e) => {
                tracing::warn!("Network install: xng not started: {e}");
                outcome["xng"] = serde_json::json!(e.to_string());
            }
        }
    }
    report(&enroll, outcome).await;
    finish(&mut file, None).await;
}

/// Installs xng with the station's settings unless it is installed already.
async fn start_xng(state: &AppState, station: &Station) -> Result<String, AppError> {
    let config = state.config.read_config().await?;
    if crate::handlers::apps::recorded_app_ids(&config)
        .iter()
        .any(|id| id == crate::xng::APP_ID)
    {
        return Ok("xng is already installed".into());
    }
    let req = crate::handlers::apps::InstallRequest {
        app_id: crate::xng::APP_ID.to_string(),
        env: xng_env(station),
        image_tag: None,
    };
    let _ =
        crate::handlers::apps::install_app(axum::extract::State(state.clone()), axum::Json(req))
            .await?;
    Ok(format!(
        "xng started as {}",
        station.ident.trim().to_uppercase()
    ))
}

async fn primary_mac() -> Option<String> {
    let mut dir = tokio::fs::read_dir("/sys/class/net").await.ok()?;
    let mut macs = Vec::new();
    while let Ok(Some(e)) = dir.next_entry().await {
        let name = e.file_name().to_string_lossy().to_string();
        if name == "lo"
            || name.starts_with("docker")
            || name.starts_with("veth")
            || name.starts_with("br-")
        {
            continue;
        }
        if let Ok(m) = tokio::fs::read_to_string(e.path().join("address")).await {
            let m = m.trim().to_string();
            if m != "00:00:00:00:00:00" && !m.is_empty() {
                macs.push((name, m));
            }
        }
    }
    macs.sort();
    // Wired first (en*/eth*), then the rest.
    macs.iter()
        .find(|(n, _)| n.starts_with("en") || n.starts_with("eth"))
        .or(macs.first())
        .map(|(_, m)| m.clone())
}

/// The heartbeat body (api-server `POST /devices/heartbeat`).
fn heartbeat_body(state: &AppState) -> serde_json::Value {
    let mut body = serde_json::json!({
        "agent_version": env!("CARGO_PKG_VERSION"),
        "capabilities": [],
    });
    if let Ok(info) = state.system.get_info() {
        body["os_version"] = serde_json::json!(info.airwaves_version);
        body["model"] = serde_json::json!(info.model);
        body["board"] = serde_json::json!(info.airwaves_board);
        body["uptime_seconds"] = serde_json::json!(info.uptime);
    }
    if let Ok(s) = state.system.get_stats() {
        body["memory_used_bytes"] = serde_json::json!(s.memory_used);
        body["memory_total_bytes"] = serde_json::json!(s.memory_total);
        body["disk_used_bytes"] = serde_json::json!(s.disk_used);
        body["disk_total_bytes"] = serde_json::json!(s.disk_total);
        body["load_1m"] = serde_json::json!(s.load_average[0]);
        if let Some(t) = s.temperature {
            body["temperature_c"] = serde_json::json!(t);
        }
    }
    if let Ok(devices) = state.hardware.list_sdr_devices() {
        body["sdrs"] = serde_json::Value::Array(
            devices
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "type": serde_json::to_value(&d.device_type).unwrap_or_default(),
                        "serial": d.serial,
                        "role": d.assigned_to,
                    })
                })
                .collect(),
        );
    }
    body
}

async fn heartbeat_loop(state: AppState) {
    use crate::ports::DockerPort;
    let mut failures = 0u32;
    loop {
        let Some(token) = device_token().await else {
            // No device token (not a network install, or before enrollment):
            // check again later in case enrollment adds one.
            tokio::time::sleep(Duration::from_secs(300)).await;
            continue;
        };
        let mut body = heartbeat_body(&state);
        if let Ok(containers) = state.docker.list_containers().await {
            body["apps"] = serde_json::Value::Array(
                containers
                    .iter()
                    .filter(|c| c.name.trim_start_matches('/').starts_with("airwaves-"))
                    .map(|c| {
                        let name = c.name.trim_start_matches('/').to_string();
                        let version = c.image.rsplit_once(':').map(|(_, t)| t.to_string());
                        serde_json::json!({ "name": name, "state": c.state, "version": version })
                    })
                    .collect(),
            );
        }
        let resp = crate::airframes::http()
            .post(format!(
                "{}/devices/heartbeat",
                crate::airframes::api_base()
            ))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await;
        let wait = match resp {
            Ok(r) if r.status().is_success() => {
                failures = 0;
                60
            }
            Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => {
                // The device was removed from the account or enrolled again.
                tracing::info!(
                    "Device heartbeat refused ({}); stopping until the next restart",
                    r.status()
                );
                return;
            }
            _ => {
                failures += 1;
                (60 * u64::from(failures.min(10))).min(600)
            }
        };
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station() -> Station {
        serde_json::from_value(serde_json::json!({
            "ident": " ke-hangar ",
            "latitude": 38.6766,
            "longitude": -121.0158,
            "altitude_m": 260,
            "name": "Hangar",
            "xng": { "feed_airframes": true, "modes": "", "airband_channels": "120.500=Tower", "atc_feed": false }
        }))
        .unwrap()
    }

    #[test]
    fn netinstall_file_reads_the_awcfg_blocks() {
        let f: NetinstallFile = serde_json::from_value(serde_json::json!({
            "enroll": { "url": "http://192.168.1.20:47811/netboot/v1/enroll", "code": "abc", "expires_at": "2030-01-01T00:00:00Z" },
            "station": { "ident": "KE-HANGAR" },
            "unknown": 1
        }))
        .unwrap();
        assert_eq!(f.enroll.unwrap().code, "abc");
        assert_eq!(f.station.unwrap().ident, "KE-HANGAR");
        assert!(!f.done);
    }

    #[test]
    fn xng_env_uses_the_feed_airframes_keys() {
        let env = xng_env(&station());
        assert_eq!(env["XNG_STATION_ID"], "KE-HANGAR");
        assert_eq!(env["XNG_FEED_AIRFRAMES"], "true");
        assert_eq!(env["XNG_ATC_FEED"], "false");
        assert_eq!(env["XNG_TRANSCRIBE"], "false");
        assert_eq!(env["XNG_AIRBAND_CHANNELS"], "120.500=Tower");
        assert!(
            !env.contains_key("XNG_MODES"),
            "empty modes keep the defaults"
        );
    }

    #[test]
    fn station_body_has_ident_and_valid_location() {
        assert_eq!(
            station_body(&station()),
            serde_json::json!({ "ident": "KE-HANGAR", "latitude": 38.6766, "longitude": -121.0158 })
        );
        let mut s = station();
        s.latitude = Some(123.0);
        assert_eq!(
            station_body(&s),
            serde_json::json!({ "ident": "KE-HANGAR" })
        );
    }

    #[test]
    fn expired_enrollment_is_detected() {
        let mut e = Enroll {
            url: "u".into(),
            code: "c".into(),
            expires_at: Some("2000-01-01T00:00:00Z".into()),
        };
        assert!(deadline_passed(&e));
        e.expires_at = Some("2999-01-01T00:00:00Z".into());
        assert!(!deadline_passed(&e));
        e.expires_at = None;
        assert!(!deadline_passed(&e));
    }
}
