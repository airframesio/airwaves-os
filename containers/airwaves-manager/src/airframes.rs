//! Signing the device in to Airframes (RFC 8628 device flow).
//!
//! The manager runs the same sign-in as `xng atc login` (client
//! `airframes-feeder`, scope `atc:feed`) and stores the result where the xng
//! app reads it: `<apps dir>/xng/atc.json`, mounted in the container at
//! /etc/xng/atc.json. The refresh token rotates on every use and only xng
//! uses it, so the manager never refreshes it itself: it only writes it once
//! and reports who is signed in.
//!
//! Routes (under /api/v1/airframes):
//! - GET    /account        -> { signed_in, username, pending }
//! - POST   /signin/start   -> { user_code, verification_uri, verification_uri_complete, expires_in, interval }
//! - POST   /signin/poll    -> { status: pending|signed_in|denied|expired|none, username? }
//!   On sign-in the manager also registers the device's station
//!   (api `POST /feeder/station`, with the one access token it sees), so a
//!   new device shows up on Airframes without the website wizard.
//! - DELETE /account        -> sign out (keeps the feeder id, so feeds keep their identity)
//! - GET    /atc/frequencies -> the Airframes ATC frequency directory
//!   (api `GET /v1/atc/frequencies`, public), so the control app can offer
//!   nearby tower and approach channels instead of a free-form list

use axum::Json;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::error::AppError;

pub const CLIENT_ID: &str = "airframes-feeder";
pub const SCOPE: &str = "atc:feed";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

fn api_base() -> String {
    std::env::var("AIRFRAMES_API")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://api.airframes.io".to_string())
        .trim_end_matches('/')
        .to_string()
}

pub fn credentials_path() -> PathBuf {
    crate::xng::apps_dir()
        .join(crate::xng::APP_ID)
        .join("atc.json")
}

/// The xng `atc.json` layout (src/airband/atc_auth.rs `Credentials`).
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Credentials {
    #[serde(skip_serializing_if = "Option::is_none")]
    api: Option<String>,
    refresh_token: Option<String>,
    username: Option<String>,
    user_id: Option<String>,
    feeder_id: Option<String>,
}

async fn load() -> Credentials {
    match tokio::fs::read(credentials_path()).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Credentials::default(),
    }
}

async fn save(c: &Credentials) -> Result<(), AppError> {
    let path = credentials_path();
    let dir = path.parent().expect("credentials path has a parent");
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| AppError::Internal(format!("create {}: {e}", dir.display())))?;
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(c)?;
    tokio::fs::write(&tmp, body)
        .await
        .map_err(|e| AppError::Internal(format!("write {}: {e}", tmp.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).await;
    }
    tokio::fs::rename(&tmp, &path)
        .await
        .map_err(|e| AppError::Internal(format!("save {}: {e}", path.display())))?;
    Ok(())
}

/// Whether xng has a sign-in to feed the ATC network with.
pub async fn signed_in() -> bool {
    load()
        .await
        .refresh_token
        .is_some_and(|t| !t.trim().is_empty())
}

struct Pending {
    device_code: String,
    expires: Instant,
    interval: Duration,
    next_poll: Instant,
}

static PENDING: Mutex<Option<Pending>> = Mutex::const_new(None);

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("airwaves-manager/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

async fn post(path: &str, body: serde_json::Value) -> Result<(u16, serde_json::Value), AppError> {
    let resp = http()
        .post(format!("{}{path}", api_base()))
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Airframes API unreachable: {e}")))?;
    let status = resp.status().as_u16();
    let v = resp
        .json::<serde_json::Value>()
        .await
        .unwrap_or(serde_json::Value::Null);
    Ok((status, v))
}

fn api_message(v: &serde_json::Value) -> String {
    ["error_description", "message", "error"]
        .iter()
        .find_map(|k| v.get(*k).and_then(|m| m.as_str()))
        .unwrap_or("unknown error")
        .to_string()
}

#[derive(Serialize)]
pub struct Account {
    signed_in: bool,
    username: Option<String>,
    pending: bool,
}

pub async fn get_account() -> Result<Json<Account>, AppError> {
    let c = load().await;
    let pending = PENDING
        .lock()
        .await
        .as_ref()
        .is_some_and(|p| p.expires > Instant::now());
    Ok(Json(Account {
        signed_in: c.refresh_token.is_some_and(|t| !t.trim().is_empty()),
        username: c.username,
        pending,
    }))
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    interval: Option<u64>,
}

pub async fn start_signin() -> Result<Json<serde_json::Value>, AppError> {
    let (status, v) = post(
        "/auth/device/code",
        serde_json::json!({ "client_id": CLIENT_ID, "scope": SCOPE }),
    )
    .await?;
    if status >= 400 {
        return Err(AppError::Internal(format!(
            "Airframes sign-in could not start ({status}): {}",
            api_message(&v)
        )));
    }
    let dc: DeviceCodeResponse = serde_json::from_value(v)?;
    let expires_in = dc.expires_in.unwrap_or(900);
    let interval = dc.interval.unwrap_or(5).max(1);
    *PENDING.lock().await = Some(Pending {
        device_code: dc.device_code,
        expires: Instant::now() + Duration::from_secs(expires_in),
        interval: Duration::from_secs(interval),
        next_poll: Instant::now(),
    });
    Ok(Json(serde_json::json!({
        "user_code": dc.user_code,
        "verification_uri": dc.verification_uri,
        "verification_uri_complete": dc.verification_uri_complete,
        "expires_in": expires_in,
        "interval": interval,
    })))
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    user: Option<serde_json::Value>,
}

/// One poll of the token endpoint. The UI calls this every `interval`
/// seconds while the code is shown; calls in between return `pending`
/// without contacting the API.
pub async fn poll_signin(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut guard = PENDING.lock().await;
    let Some(p) = guard.as_mut() else {
        return Ok(Json(serde_json::json!({ "status": "none" })));
    };
    let now = Instant::now();
    if now >= p.expires {
        *guard = None;
        return Ok(Json(serde_json::json!({ "status": "expired" })));
    }
    if now < p.next_poll {
        return Ok(Json(serde_json::json!({ "status": "pending" })));
    }
    let (status, v) = post(
        "/auth/token",
        serde_json::json!({
            "grant_type": DEVICE_GRANT,
            "device_code": p.device_code,
            "client_id": CLIENT_ID,
        }),
    )
    .await?;
    if status < 400 {
        let t: TokenResponse = serde_json::from_value(v)?;
        let refresh = t.refresh_token.filter(|r| !r.is_empty()).ok_or_else(|| {
            AppError::Internal("Airframes returned no refresh token for the feeder sign-in".into())
        })?;
        let mut c = load().await;
        c.refresh_token = Some(refresh);
        let base = api_base();
        c.api = (base != "https://api.airframes.io").then_some(base);
        if let Some(u) = &t.user {
            c.username = u
                .get("username")
                .and_then(|n| n.as_str())
                .map(str::to_string);
            c.user_id = u.get("id").map(|id| {
                id.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| id.to_string())
            });
        }
        if c.feeder_id.is_none() {
            c.feeder_id = Some(uuid::Uuid::new_v4().to_string());
        }
        save(&c).await?;
        *guard = None;
        tracing::info!(
            "Signed in to Airframes as {}",
            c.username.as_deref().unwrap_or("(unknown user)")
        );
        let station = match t.access_token.filter(|a| !a.is_empty()) {
            Some(token) => register_station(&state, &token).await,
            None => serde_json::json!({ "status": "skipped", "reason": "no access token" }),
        };
        return Ok(Json(serde_json::json!({
            "status": "signed_in",
            "username": c.username,
            "station": station,
        })));
    }
    let next = match v.get("error").and_then(|e| e.as_str()) {
        Some("authorization_pending") => "pending",
        Some("slow_down") => {
            p.interval += Duration::from_secs(5);
            "pending"
        }
        Some("access_denied") => "denied",
        Some("expired_token") => "expired",
        _ => {
            return Err(AppError::Internal(format!(
                "Airframes sign-in failed ({status}): {}",
                api_message(&v)
            )))
        }
    };
    if next == "pending" {
        p.next_poll = now + p.interval;
    } else {
        *guard = None;
    }
    Ok(Json(serde_json::json!({ "status": next })))
}

/// The body for `POST /feeder/station`: the xng station ID and the
/// receiver location, or None when xng has no station ID yet.
fn station_request(config: &crate::domain::AirwavesConfig) -> Option<serde_json::Value> {
    let env = crate::handlers::apps::recorded_app_env(config, crate::xng::APP_ID);
    let ident = crate::xng::normalize_station_id(
        env.get(crate::xng::STATION_ID_KEY)
            .map(String::as_str)
            .unwrap_or(""),
    )
    .ok()?;
    let mut body = serde_json::json!({ "ident": ident });
    if let Some((lat, lon)) = crate::xng::receiver_pos(config) {
        body["latitude"] = serde_json::json!(lat);
        body["longitude"] = serde_json::json!(lon);
    }
    Some(body)
}

/// Registers this device's station with Airframes (created for the account,
/// or claimed if Airframes already made one from this device's feed). Never
/// fails the sign-in: the result is only reported to the control app.
async fn register_station(state: &crate::AppState, access_token: &str) -> serde_json::Value {
    use crate::ports::ConfigPort;
    let config = match state.config.read_config().await {
        Ok(c) => c,
        Err(e) => return serde_json::json!({ "status": "error", "message": e.to_string() }),
    };
    let Some(body) = station_request(&config) else {
        return serde_json::json!({ "status": "skipped", "reason": "no station ID set" });
    };
    let ident = body["ident"].as_str().unwrap_or_default().to_string();
    let resp = http()
        .post(format!("{}/feeder/station", api_base()))
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Airframes station registration failed: {e}");
            return serde_json::json!({ "status": "error", "ident": ident, "message": "Airframes API unreachable" });
        }
    };
    let code = resp.status().as_u16();
    let v = resp
        .json::<serde_json::Value>()
        .await
        .unwrap_or(serde_json::Value::Null);
    let status = match code {
        200 | 201 if v.get("created").and_then(|c| c.as_bool()) == Some(true) => "created",
        200 | 201 if v.get("claimed").and_then(|c| c.as_bool()) == Some(true) => "claimed",
        200 | 201 => "exists",
        409 => "taken",
        _ => "error",
    };
    tracing::info!("Airframes station {ident}: {status} ({code})");
    let mut out = serde_json::json!({ "status": status, "ident": ident });
    if let Some(id) = v.pointer("/station/id") {
        out["station_id"] = id.clone();
    }
    if status == "error" || status == "taken" {
        out["message"] = serde_json::json!(api_message(&v));
    }
    out
}

/// Query parameters passed through to the frequency directory.
const FREQUENCY_QUERY: &[&str] = &[
    "near",
    "radius_nm",
    "service",
    "band",
    "q",
    "facility_id",
    "mhz",
    "limit",
    "page",
];

pub async fn atc_frequencies(
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let params: Vec<(&str, &str)> = FREQUENCY_QUERY
        .iter()
        .filter_map(|k| query.get(*k).map(|v| (*k, v.as_str())))
        .filter(|(_, v)| !v.is_empty() && v.len() <= 200)
        .collect();
    let resp = http()
        .get(format!("{}/v1/atc/frequencies", api_base()))
        .query(&params)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Airframes API unreachable: {e}")))?;
    let status = resp.status().as_u16();
    let v = resp
        .json::<serde_json::Value>()
        .await
        .unwrap_or(serde_json::Value::Null);
    if status >= 400 {
        return Err(AppError::Internal(format!(
            "Airframes frequency lookup failed ({status}): {}",
            api_message(&v)
        )));
    }
    Ok(Json(v))
}

/// Fields of a frequency report passed through to the directory.
const REPORT_FIELDS: &[&str] = &[
    "frequency_mhz",
    "label",
    "service",
    "facility",
    "lat",
    "lon",
];

/// Report a channel the user added by hand, so Airframes can add it to the
/// frequency directory. Sent without the feeder sign-in: its refresh token
/// rotates on every use and belongs to xng.
pub async fn atc_frequency_report(
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut out = serde_json::Map::new();
    if let Some(obj) = body.as_object() {
        for k in REPORT_FIELDS {
            if let Some(v) = obj.get(*k).filter(|v| v.is_string() || v.is_number()) {
                out.insert((*k).to_string(), v.clone());
            }
        }
    }
    if !out.contains_key("frequency_mhz") {
        return Err(AppError::BadRequest("frequency_mhz is required".into()));
    }
    out.insert("source".into(), "airwaves-os".into());
    out.insert("client_version".into(), env!("CARGO_PKG_VERSION").into());
    let resp = http()
        .post(format!("{}/v1/atc/frequency-reports", api_base()))
        .json(&out)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Airframes API unreachable: {e}")))?;
    let status = resp.status().as_u16();
    let v = resp
        .json::<serde_json::Value>()
        .await
        .unwrap_or(serde_json::Value::Null);
    if status >= 400 {
        return Err(AppError::Internal(format!(
            "Airframes frequency report failed ({status}): {}",
            api_message(&v)
        )));
    }
    Ok(Json(v))
}

pub async fn sign_out() -> Result<Json<serde_json::Value>, AppError> {
    *PENDING.lock().await = None;
    let mut c = load().await;
    let was = c.refresh_token.take().is_some();
    c.username = None;
    c.user_id = None;
    if was || c.feeder_id.is_some() {
        save(&c).await?;
    }
    Ok(Json(serde_json::json!({ "signed_out": was })))
}

#[cfg(test)]
mod tests {
    fn config(lat: f64, lon: f64, apps: serde_json::Value) -> crate::domain::AirwavesConfig {
        crate::domain::AirwavesConfig {
            version: 1,
            device: crate::domain::DeviceConfig {
                id: "device".to_string(),
                name: "Airwaves".to_string(),
                hostname: "airwaves".to_string(),
            },
            station: crate::domain::StationConfig {
                latitude: lat,
                longitude: lon,
                altitude_m: 0,
                operator: String::new(),
            },
            network: crate::domain::NetworkConfig {
                mode: "dhcp".to_string(),
            },
            services: crate::domain::ServicesConfig {
                gateway: crate::domain::ServiceState { enabled: true },
                manager: crate::domain::ServiceState { enabled: true },
            },
            aggregators: serde_json::Value::Null,
            apps,
            hardware: serde_json::json!({}),
            preferences: serde_json::json!({}),
        }
    }

    #[test]
    fn station_request_uses_the_xng_station_id_and_location() {
        let apps = serde_json::json!([{ "id": "xng", "env": { "XNG_STATION_ID": " xx-ksea " } }]);
        assert_eq!(
            super::station_request(&config(47.45, -122.3, apps.clone())),
            Some(serde_json::json!({ "ident": "XX-KSEA", "latitude": 47.45, "longitude": -122.3 }))
        );
        // No location set: the station is registered unplaced.
        assert_eq!(
            super::station_request(&config(0.0, 0.0, apps)),
            Some(serde_json::json!({ "ident": "XX-KSEA" }))
        );
    }

    #[test]
    fn station_request_needs_a_valid_station_id() {
        assert_eq!(
            super::station_request(&config(47.0, -122.0, serde_json::json!([]))),
            None
        );
        let bad = serde_json::json!([{ "id": "xng", "env": { "XNG_STATION_ID": "XX KSEA" } }]);
        assert_eq!(super::station_request(&config(47.0, -122.0, bad)), None);
    }

    use super::*;

    #[test]
    fn credentials_match_xng_layout() {
        let c = Credentials {
            api: None,
            refresh_token: Some("r".into()),
            username: Some("kev".into()),
            user_id: Some("7".into()),
            feeder_id: Some("f".into()),
        };
        let v = serde_json::to_value(&c).unwrap();
        // xng reads these exact keys; `api` is omitted for the default API.
        assert_eq!(
            v,
            serde_json::json!({"refresh_token":"r","username":"kev","user_id":"7","feeder_id":"f"})
        );
        let back: Credentials = serde_json::from_str(r#"{"refresh_token":"x","extra":1}"#).unwrap();
        assert_eq!(back.refresh_token.as_deref(), Some("x"));
    }
}
