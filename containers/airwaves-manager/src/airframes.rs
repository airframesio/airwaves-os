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
//! - DELETE /account        -> sign out (keeps the feeder id, so feeds keep their identity)

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
    refresh_token: Option<String>,
    #[serde(default)]
    user: Option<serde_json::Value>,
}

/// One poll of the token endpoint. The UI calls this every `interval`
/// seconds while the code is shown; calls in between return `pending`
/// without contacting the API.
pub async fn poll_signin() -> Result<Json<serde_json::Value>, AppError> {
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
        return Ok(Json(
            serde_json::json!({ "status": "signed_in", "username": c.username }),
        ));
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
