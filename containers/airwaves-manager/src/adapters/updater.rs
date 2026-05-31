use std::sync::{Arc, Mutex};

use crate::adapters::HostAdapter;
use crate::domain::*;
use crate::error::AppError;
use crate::ports::UpdatePort;

const UPDATE_DIR: &str = "/etc/airwaves/update";
const VERSIONS_FILE: &str = "/etc/airwaves/.versions.json";
const RELEASE_FILE_REL: &str = "/etc/airwaves-release";
const OS_RELEASE_REL: &str = "/etc/os-release";
const DEFAULT_MANIFEST_BASE: &str =
    "https://raw.githubusercontent.com/airframesio/airwaves-os/main/releases";

/// On-disk version markers seeded into the image and updated by the host updater.
#[derive(serde::Deserialize, serde::Serialize, Default)]
struct VersionsFile {
    #[serde(default)]
    compose: u32,
    #[serde(default)]
    catalog: u32,
    #[serde(default = "default_channel")]
    channel: String,
}

fn default_channel() -> String {
    "stable".to_string()
}

/// Update channels, from most-stable to least.
pub const CHANNELS: &[&str] = &["stable", "beta", "dev"];

pub fn is_valid_channel(channel: &str) -> bool {
    CHANNELS.contains(&channel)
}

pub struct UpdaterAdapter {
    http: reqwest::Client,
    host: Arc<HostAdapter>,
    docker: Arc<crate::adapters::DockerAdapter>,
    /// Cached result of the most recent check.
    cache: Mutex<Option<UpdateStatus>>,
}

impl UpdaterAdapter {
    pub fn new(host: Arc<HostAdapter>, docker: Arc<crate::adapters::DockerAdapter>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("airwaves-manager")
            .build()
            .unwrap_or_default();
        Self {
            http,
            host,
            docker,
            cache: Mutex::new(None),
        }
    }

    fn host_root() -> String {
        std::env::var("HOST_ROOT")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_default()
    }

    fn read_host_file(rel: &str) -> Option<String> {
        std::fs::read_to_string(format!("{}{}", Self::host_root(), rel)).ok()
    }

    fn parse_kv(content: &str, key: &str) -> Option<String> {
        let prefix = format!("{key}=");
        content.lines().find_map(|l| {
            l.trim()
                .strip_prefix(&prefix)
                .map(|v| v.trim().trim_matches('"').to_string())
        })
    }

    fn read_versions_file() -> VersionsFile {
        std::fs::read_to_string(VERSIONS_FILE)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }

    fn manifest_url(channel: &str) -> String {
        if let Ok(url) = std::env::var("AIRWAVES_MANIFEST_URL") {
            return url;
        }
        let base = std::env::var("AIRWAVES_MANIFEST_BASE")
            .unwrap_or_else(|_| DEFAULT_MANIFEST_BASE.to_string());
        format!("{base}/{channel}.json")
    }

    /// Read the control-app version the gateway is serving. Prefer the gateway
    /// container's image label (authoritative, via the Docker socket — no
    /// network/DNS dependency); fall back to the HTTP /version.json the gateway
    /// serves, then "unknown".
    /// The manager's installed version. Prefer the concrete tag of the running
    /// manager image (e.g. "1.0.10-dev.60") so dev/beta builds are identifiable;
    /// fall back to the version compiled into this binary.
    async fn manager_version(&self) -> String {
        if let Some(tag) = self.docker.container_image_tag("airwaves-manager").await {
            return tag;
        }
        env!("CARGO_PKG_VERSION").to_string()
    }

    async fn control_app_version(&self) -> String {
        // 1. Concrete tag of the running gateway image (e.g. "1.0.10-dev.60") —
        //    the most precise "what's actually deployed", and identifies the
        //    channel build. Falls through for :latest / digest pins.
        if let Some(tag) = self.docker.container_image_tag("airwaves-gateway").await {
            return tag;
        }
        // 2. Image label set at gateway build time (base version).
        if let Some(v) = self
            .docker
            .container_label("airwaves-gateway", "io.airwaves.control-app-version")
            .await
        {
            let v = v.trim().to_string();
            if !v.is_empty() && v != "unknown" {
                return v;
            }
        }
        // 3. HTTP /version.json fallback.
        let url = std::env::var("AIRWAVES_GATEWAY_VERSION_URL")
            .unwrap_or_else(|_| "http://airwaves-gateway/version.json".to_string());
        let fetched = async {
            let resp = self.http.get(&url).send().await.ok()?;
            let json: serde_json::Value = resp.json().await.ok()?;
            json.get("version")
                .and_then(|v| v.as_str())
                .map(String::from)
        }
        .await;
        fetched.unwrap_or_else(|| "unknown".to_string())
    }

    /// True if `available` is a newer version than `installed`. Falls back to a
    /// string-inequality test when either side is not valid semver.
    fn is_newer(installed: &str, available: &str) -> bool {
        if available.is_empty() || available == "unknown" {
            return false;
        }
        match (
            semver::Version::parse(installed.trim_start_matches('v')),
            semver::Version::parse(available.trim_start_matches('v')),
        ) {
            (Ok(i), Ok(a)) => a > i,
            _ => !installed.is_empty() && installed != "unknown" && installed != available,
        }
    }

    fn now() -> String {
        chrono::Utc::now().to_rfc3339()
    }

    /// Store a check result in the cache.
    fn cache_set(&self, status: &UpdateStatus) {
        if let Ok(mut guard) = self.cache.lock() {
            *guard = Some(status.clone());
        }
    }

    /// Persist a new update channel into .versions.json (preserving the
    /// compose/catalog revisions) and clear the cached status so the next
    /// check hits the new channel's manifest.
    pub fn set_channel(&self, channel: &str) -> Result<(), AppError> {
        if !is_valid_channel(channel) {
            return Err(AppError::BadRequest(format!(
                "Invalid channel '{channel}'. Valid: {}",
                CHANNELS.join(", ")
            )));
        }
        let mut versions = Self::read_versions_file();
        versions.channel = channel.to_string();
        let json = serde_json::to_string_pretty(&versions)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        std::fs::write(VERSIONS_FILE, json)
            .map_err(|e| AppError::Internal(format!("Cannot write {VERSIONS_FILE}: {e}")))?;
        if let Ok(mut guard) = self.cache.lock() {
            *guard = None;
        }
        Ok(())
    }
}

impl UpdatePort for UpdaterAdapter {
    async fn fetch_manifest(&self) -> Result<UpdateManifest, AppError> {
        let versions = Self::read_versions_file();
        let url = Self::manifest_url(&versions.channel);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to fetch manifest: {e}")))?;
        if !resp.status().is_success() {
            return Err(AppError::Internal(format!(
                "Manifest fetch returned HTTP {}",
                resp.status()
            )));
        }
        let manifest: UpdateManifest = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse manifest: {e}")))?;
        Ok(manifest)
    }

    async fn installed_versions(&self) -> InstalledVersions {
        let versions = Self::read_versions_file();
        let os_release = Self::read_host_file(OS_RELEASE_REL);
        let os_codename = os_release
            .as_deref()
            .and_then(|c| Self::parse_kv(c, "VERSION_CODENAME"))
            .unwrap_or_default();
        let os_version = Self::read_host_file(RELEASE_FILE_REL)
            .as_deref()
            .and_then(|c| Self::parse_kv(c, "AIRWAVES_VERSION"))
            .unwrap_or_else(|| "unknown".to_string());

        InstalledVersions {
            os_version,
            os_codename,
            manager: self.manager_version().await,
            control_app: self.control_app_version().await,
            compose: versions.compose,
            catalog: versions.catalog,
            channel: versions.channel,
        }
    }

    async fn check(&self) -> UpdateStatus {
        let installed = self.installed_versions().await;
        let os_packages_upgradable = self.host.upgradable_packages().await;

        let manifest = match self.fetch_manifest().await {
            Ok(m) => m,
            Err(e) => {
                // Offline / unreachable: still return installed state.
                let status = UpdateStatus {
                    installed,
                    available_os_version: None,
                    components: vec![],
                    highest_severity: None,
                    update_available: false,
                    os_packages_upgradable,
                    major_upgrade: None,
                    last_checked: Some(Self::now()),
                    error: Some(e.to_string()),
                };
                self.cache_set(&status);
                return status;
            }
        };

        let mut components: Vec<ComponentUpdate> = Vec::new();

        // Display the concrete image tag (e.g. "1.0.6-dev.48") for the available
        // version when it carries a channel suffix, so dev/beta updates are
        // identifiable; comparison stays on the base semver version.
        let display_available = |c: &ManifestComponent, base: &str| -> String {
            match &c.tag {
                Some(t) if t.contains('-') => t.clone(),
                _ => base.to_string(),
            }
        };

        // Manager image. Compare the installed value against the SAME concrete
        // value we display (e.g. "1.0.11-dev.65"), not the bare base version.
        // Otherwise, on a device already running the channel's prerelease tag,
        // semver ranks the base release ("1.0.11") above the prerelease
        // ("1.0.11-dev.65") and falsely reports an update to the identical tag.
        if let Some(c) = manifest.components.get("manager") {
            let available = display_available(c, &c.version);
            let avail = Self::is_newer(&installed.manager, &available);
            components.push(ComponentUpdate {
                name: "manager".to_string(),
                installed: installed.manager.clone(),
                available,
                update_available: avail,
                severity: c.severity,
                kind: "image".to_string(),
            });
        }

        // Gateway carries the control app; compare against the concrete tag too.
        if let Some(c) = manifest.components.get("gateway") {
            let target = c
                .control_app_version
                .clone()
                .unwrap_or_else(|| c.version.clone());
            let available = display_available(c, &target);
            let avail = Self::is_newer(&installed.control_app, &available);
            components.push(ComponentUpdate {
                name: "gateway".to_string(),
                installed: installed.control_app.clone(),
                available,
                update_available: avail,
                severity: c.severity,
                kind: "image".to_string(),
            });
        }

        // Compose + catalog config files (integer revisions).
        for (key, installed_rev) in [
            ("compose", installed.compose),
            ("catalog", installed.catalog),
        ] {
            if let Some(c) = manifest.components.get(key) {
                let avail_rev: u32 = c.version.parse().unwrap_or(0);
                components.push(ComponentUpdate {
                    name: key.to_string(),
                    installed: installed_rev.to_string(),
                    available: avail_rev.to_string(),
                    update_available: avail_rev > installed_rev,
                    severity: c.severity,
                    kind: "file".to_string(),
                });
            }
        }

        let major_upgrade = manifest.os.major_upgrade.clone().filter(|m| {
            // Only offer if the installed codename matches the upgrade source.
            installed.os_codename.is_empty() || installed.os_codename == m.from
        });

        // Highest severity across everything actionable.
        let mut highest = components
            .iter()
            .filter(|c| c.update_available)
            .map(|c| c.severity)
            .max();
        if let Some(m) = &major_upgrade {
            highest = Some(highest.map_or(m.severity, |h| h.max(m.severity)));
        }
        if os_packages_upgradable.unwrap_or(0) > 0 {
            highest = Some(highest.unwrap_or(Severity::NiceToHave));
        }

        let update_available = components.iter().any(|c| c.update_available)
            || major_upgrade.is_some()
            || os_packages_upgradable.unwrap_or(0) > 0;

        let status = UpdateStatus {
            installed,
            available_os_version: Some(manifest.os_version.clone()),
            components,
            highest_severity: highest,
            update_available,
            os_packages_upgradable,
            major_upgrade,
            last_checked: Some(Self::now()),
            error: None,
        };
        self.cache_set(&status);
        status
    }

    async fn apply(&self, components: Vec<String>) -> Result<(), AppError> {
        // Resolve file URLs/hashes from a fresh manifest.
        let manifest = self.fetch_manifest().await?;
        let comp = |key: &str| manifest.components.get(key);

        let want = |name: &str| components.iter().any(|c| c == name) || components.iter().any(|c| c == "all");

        let mut request = UpdateRequest {
            requested_at: Self::now(),
            components: components.clone(),
            compose_url: None,
            compose_sha256: None,
            compose_version: None,
            catalog_url: None,
            catalog_sha256: None,
            catalog_version: None,
            os_major_to: None,
            manager_image: None,
            manager_tag: None,
            gateway_image: None,
            gateway_tag: None,
            // Always sync the manifest's host files (userpatches) on any update,
            // so script/unit/config changes reach deployed devices.
            host_files: manifest.host_files.clone(),
            recreate: false,
        };

        // Always reconcile BOTH container image tags to the manifest's channel
        // tags whenever the stack is touched — not only the component being
        // updated. The compose file is shared, so pinning one image while
        // leaving the other on :latest would leave the stack inconsistent
        // (e.g. updating the manager left the gateway on :latest). This keeps
        // both images aligned with the selected channel's tags.
        if let Some(c) = comp("manager") {
            request.manager_image = c.image.clone();
            request.manager_tag = c.tag.clone().or_else(|| Some(c.version.clone()));
        }
        if let Some(c) = comp("gateway") {
            request.gateway_image = c.image.clone();
            request.gateway_tag = c
                .tag
                .clone()
                .or_else(|| c.control_app_version.clone())
                .or_else(|| Some(c.version.clone()));
        }

        if want("compose") {
            if let Some(c) = comp("compose") {
                request.compose_url = c.url.clone();
                request.compose_sha256 = c.sha256.clone();
                request.compose_version = c.version.parse().ok();
            }
        }
        if want("catalog") {
            if let Some(c) = comp("catalog") {
                request.catalog_url = c.url.clone();
                request.catalog_sha256 = c.sha256.clone();
                request.catalog_version = c.version.parse().ok();
            }
        }
        if want("os_major") {
            request.os_major_to = manifest.os.major_upgrade.as_ref().map(|m| m.to.clone());
        }

        // Ensure the update dir exists and write the request + a queued status.
        std::fs::create_dir_all(UPDATE_DIR)
            .map_err(|e| AppError::Internal(format!("Cannot create {UPDATE_DIR}: {e}")))?;
        let req_json = serde_json::to_string_pretty(&request)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        std::fs::write(format!("{UPDATE_DIR}/request.json"), req_json)
            .map_err(|e| AppError::Internal(format!("Cannot write request.json: {e}")))?;

        let queued = UpdateProgress {
            state: "running".to_string(),
            phase: "queued".to_string(),
            percent: 0,
            log: vec!["Update requested".to_string()],
            reboot_required: false,
            error: None,
            started_at: Some(Self::now()),
            finished_at: None,
        };
        let _ = std::fs::write(
            format!("{UPDATE_DIR}/status.json"),
            serde_json::to_string_pretty(&queued).unwrap_or_default(),
        );

        // Self-heal: refresh the host updater script before running, so devices
        // pick up updater fixes (tag pinning, backups) automatically.
        let _ = self.host.refresh_updater_files().await;

        // Kick off the host-side updater (runs outside this container).
        self.host.start_update_service().await
    }

    async fn progress(&self) -> UpdateProgress {
        std::fs::read_to_string(format!("{UPDATE_DIR}/status.json"))
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_else(|| UpdateProgress {
                state: "idle".to_string(),
                phase: String::new(),
                ..Default::default()
            })
    }
}

impl UpdaterAdapter {
    /// Force-refresh = REPAIR at the installed version. Re-pull the images
    /// currently pinned in docker-compose.yml and force-recreate the stack —
    /// WITHOUT fetching the manifest, changing any image tags, or bumping
    /// versions. This recovers a wedged/half-applied stack (e.g. a container
    /// that won't come up) without turning into an upgrade.
    pub async fn refresh(&self) -> Result<(), AppError> {
        let request = UpdateRequest {
            requested_at: Self::now(),
            components: vec!["recreate".into()],
            recreate: true,
            ..Default::default()
        };

        std::fs::create_dir_all(UPDATE_DIR)
            .map_err(|e| AppError::Internal(format!("Cannot create {UPDATE_DIR}: {e}")))?;
        let req_json = serde_json::to_string_pretty(&request)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        std::fs::write(format!("{UPDATE_DIR}/request.json"), req_json)
            .map_err(|e| AppError::Internal(format!("Cannot write request.json: {e}")))?;

        let queued = UpdateProgress {
            state: "running".to_string(),
            phase: "queued".to_string(),
            percent: 0,
            log: vec!["Force refresh (repair at installed version) requested".to_string()],
            reboot_required: false,
            error: None,
            started_at: Some(Self::now()),
            finished_at: None,
        };
        let _ = std::fs::write(
            format!("{UPDATE_DIR}/status.json"),
            serde_json::to_string_pretty(&queued).unwrap_or_default(),
        );

        self.host.start_update_service().await
    }

    /// Return the cached status if present, else run a fresh check.
    pub async fn status_cached(&self) -> UpdateStatus {
        if let Some(s) = self.cache.lock().ok().and_then(|g| g.clone()) {
            return s;
        }
        self.check().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison() {
        assert!(UpdaterAdapter::is_newer("1.0.0", "1.1.0"));
        assert!(UpdaterAdapter::is_newer("v1.0.0", "1.0.1"));
        assert!(!UpdaterAdapter::is_newer("1.1.0", "1.1.0"));
        assert!(!UpdaterAdapter::is_newer("1.2.0", "1.1.0"));
        assert!(!UpdaterAdapter::is_newer("1.0.0", "unknown"));
        assert!(!UpdaterAdapter::is_newer("1.0.0", ""));
        // Non-semver fallback: differing strings count as an update.
        assert!(UpdaterAdapter::is_newer("alpha", "beta"));
        assert!(!UpdaterAdapter::is_newer("unknown", "beta"));
    }

    #[test]
    fn manifest_parses() {
        let json = r#"{
            "schema": 1, "channel": "stable", "os_version": "1.1.0",
            "codename": "Sideband", "severity": "recommended",
            "components": {
                "manager": {"version": "1.1.0", "severity": "recommended", "image": "ghcr.io/x", "tag": "1.1.0"},
                "gateway": {"version": "1.1.0", "control_app_version": "1.1.0", "severity": "required"},
                "compose": {"version": "4", "severity": "required", "url": "http://x", "sha256": "abc"},
                "catalog": {"version": "5", "severity": "nice-to-have", "url": "http://y", "sha256": "def"}
            },
            "os": {"major_upgrade": {"from": "bookworm", "to": "trixie", "severity": "recommended"}, "reboot_expected": true}
        }"#;
        let m: UpdateManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.os_version, "1.1.0");
        assert_eq!(m.components.get("compose").unwrap().version, "4");
        assert_eq!(m.os.major_upgrade.as_ref().unwrap().to, "trixie");
        assert_eq!(m.components.get("gateway").unwrap().severity, Severity::Required);
    }

    #[test]
    fn host_file_requires_sha256() {
        // A host_files entry without sha256 must fail to deserialize, so a
        // manifest can never ship a root-owned file with no integrity check.
        let missing = r#"{"url": "https://example/v1/x", "dest": "/opt/airwaves/x"}"#;
        assert!(
            serde_json::from_str::<HostFile>(missing).is_err(),
            "HostFile without sha256 must be rejected"
        );
        let ok = r#"{"url": "https://example/v1/x", "dest": "/opt/airwaves/x", "sha256": "deadbeef"}"#;
        let hf: HostFile = serde_json::from_str(ok).expect("parse with sha256");
        assert_eq!(hf.sha256, "deadbeef");
    }

    /// Every published channel manifest must deserialize against the strict
    /// types (in particular: every host_files entry carries a sha256, and its
    /// url is pinned to an immutable tag, not a mutable branch like `main`).
    #[test]
    fn published_manifests_are_valid_and_pinned() {
        for (chan, raw) in [
            ("stable", include_str!("../../../../releases/stable.json")),
            ("dev", include_str!("../../../../releases/dev.json")),
            ("beta", include_str!("../../../../releases/beta.json")),
        ] {
            let m: UpdateManifest = serde_json::from_str(raw)
                .unwrap_or_else(|e| panic!("{chan}.json failed to parse: {e}"));
            for hf in &m.host_files {
                assert!(
                    !hf.sha256.is_empty(),
                    "{chan}.json: host_file {} has empty sha256",
                    hf.dest
                );
                assert!(
                    !hf.url.contains("/main/"),
                    "{chan}.json: host_file {} url must pin to an immutable tag, not /main/",
                    hf.dest
                );
            }
        }
    }
}
