use serde::{Deserialize, Serialize};

/// Container information as exposed by the API
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub state: String,
    pub created: i64,
    pub ports: Vec<PortBinding>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortBinding {
    pub container_port: u16,
    pub host_port: Option<u16>,
    pub protocol: String,
}

/// Live resource usage for a single running container.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContainerStats {
    pub id: String,
    pub name: String,
    /// CPU usage as a percentage of total host CPU.
    pub cpu_percent: f64,
    /// Memory used in bytes.
    pub memory_used: u64,
    /// Memory limit in bytes (0 if unknown/unlimited).
    pub memory_limit: u64,
}

/// System information
#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub hostname: String,
    /// Full OS name, e.g. "Debian GNU/Linux 13 (trixie)"
    pub os: String,
    /// OS codename, e.g. "trixie"
    pub os_codename: String,
    pub architecture: String,
    pub kernel: String,
    pub uptime: u64,
    /// Hardware model, e.g. "Raspberry Pi 4 Model B" or "QEMU Standard PC"
    pub model: String,
    /// CPU brand string, e.g. "Intel(R) Core(TM) i5"
    pub cpu_model: String,
    /// Number of logical CPU cores
    pub cpu_cores: usize,
    pub airwaves_version: String,
    pub airwaves_codename: String,
    pub airwaves_build_date: String,
    pub airwaves_board: String,
}

/// System resource stats
#[derive(Debug, Serialize, Deserialize)]
pub struct SystemStats {
    pub cpu_usage: f32,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_percent: f32,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_percent: f32,
    pub temperature: Option<f32>,
    pub load_average: [f64; 3],
}

/// USB device info
#[derive(Debug, Serialize, Deserialize)]
pub struct UsbDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub vendor_name: Option<String>,
    pub product_name: Option<String>,
    pub serial: Option<String>,
    pub bus: u8,
    pub address: u8,
}

/// SDR device (identified USB device)
#[derive(Debug, Serialize, Deserialize)]
pub struct SdrDevice {
    pub id: String,
    pub name: String,
    pub device_type: SdrType,
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial: Option<String>,
    pub status: String,
    pub assigned_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum SdrType {
    RtlSdr,
    Airspy,
    AirspyHf,
    HackRf,
    SdrPlay,
    FuncubeDongle,
    Unknown,
}

/// Network interface info
#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub mac_address: String,
    pub ip_addresses: Vec<String>,
    pub is_up: bool,
    pub interface_type: String,
}

/// Airwaves OS configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AirwavesConfig {
    pub version: u32,
    pub device: DeviceConfig,
    pub station: StationConfig,
    pub network: NetworkConfig,
    pub services: ServicesConfig,
    #[serde(default)]
    pub aggregators: serde_json::Value,
    #[serde(default)]
    pub apps: serde_json::Value,
    /// UI / user preferences (theme, etc). Free-form so the control app can
    /// extend it without a schema change; persisted and included in backups.
    #[serde(default)]
    pub preferences: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceConfig {
    pub id: String,
    pub name: String,
    pub hostname: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StationConfig {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: i32,
    pub operator: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkConfig {
    pub mode: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServicesConfig {
    pub gateway: ServiceState,
    pub manager: ServiceState,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServiceState {
    pub enabled: bool,
}

/// Data feed / aggregator configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FeedConfig {
    pub id: String,
    pub name: String,
    pub feed_type: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub enabled: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub app_id: Option<String>,
}

/// App catalog entry
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CatalogApp {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub category: String,
    pub image: String,
    pub icon: Option<String>,
    pub ports: Vec<PortBinding>,
    pub requires_sdr: bool,
    pub sdr_types: Vec<SdrType>,
    /// Default environment variables passed to the container at install. User
    /// overrides from the install wizard are merged on top of these.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Optional wizard schema: fields the install dialog should prompt for.
    /// Each maps to an environment variable (or the special SDR assignment).
    #[serde(default)]
    pub config_fields: Vec<ConfigField>,
}

/// One configurable field shown in the pre-install wizard.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConfigField {
    /// Environment variable this field sets (ignored when kind == "sdr").
    pub key: String,
    /// Human label shown in the form.
    pub label: String,
    /// Optional helper text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// Field kind: "text" | "number" | "select" | "sdr".
    #[serde(default = "default_field_kind")]
    pub kind: String,
    /// For kind == "sdr": how the picked device is encoded into the env value.
    /// "soapy" → "driver=<drv>,serial=<serial>" (SoapySDR apps); "serial" →
    /// the bare serial (readsb/dump978-style apps). Defaults to "soapy".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Default value (prefilled in the form).
    #[serde(default)]
    pub default: String,
    /// Options for kind == "select".
    #[serde(default)]
    pub options: Vec<String>,
    /// Whether the field must be non-empty to proceed.
    #[serde(default)]
    pub required: bool,
}

fn default_field_kind() -> String {
    "text".to_string()
}

/// Decoded message from a decoder container, forwarded between nodes
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DecodedMessage {
    pub id: String,
    pub timestamp: String,
    pub source_node: String,
    pub decoder: String,
    pub message_type: String,
    pub frequency: Option<String>,
    pub signal_level: Option<f64>,
    pub raw: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Forwarding configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ForwardingConfig {
    pub enabled: bool,
    pub target_ip: String,
    pub target_port: u16,
    pub mode: ForwardingMode,
    pub decoders: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ForwardingMode {
    /// Forward all decoded messages to the target
    All,
    /// Only forward specific decoder types
    Selective,
    /// Don't forward (standalone mode)
    Disabled,
}

/// Forwarding stats
#[derive(Debug, Serialize, Clone, Default)]
pub struct ForwardingStats {
    pub messages_forwarded: u64,
    pub messages_received: u64,
    pub messages_failed: u64,
    pub last_forwarded: Option<String>,
    pub last_received: Option<String>,
    pub connected_peers: usize,
}

// ----------------------------------------------------------------------------
// System updater
// ----------------------------------------------------------------------------

/// How important an available update is.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// Optional, quality-of-life improvement.
    NiceToHave,
    /// Preferred; should be applied soon.
    Recommended,
    /// Necessary (security/compatibility); strongly urged.
    Required,
}

impl Default for Severity {
    fn default() -> Self {
        Severity::NiceToHave
    }
}

/// A single component entry inside the remote release manifest.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ManifestComponent {
    pub version: String,
    #[serde(default)]
    pub severity: Severity,
    /// Container image (manager/gateway).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Control-app version bundled in the gateway image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_app_version: Option<String>,
    /// Download URL for file components (compose/catalog), pinned to a tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Expected sha256 of the downloaded file (lowercase hex).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// Optional base-OS section of the manifest.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ManifestOs {
    /// Present when an in-place major Debian upgrade is offered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub major_upgrade: Option<MajorUpgrade>,
    #[serde(default)]
    pub reboot_expected: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MajorUpgrade {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub severity: Severity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guide_url: Option<String>,
}

/// The remote release manifest (releases/<channel>.json).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateManifest {
    #[serde(default = "default_schema")]
    pub schema: u32,
    pub channel: String,
    pub os_version: String,
    #[serde(default)]
    pub codename: String,
    #[serde(default)]
    pub released: String,
    #[serde(default)]
    pub severity: Severity,
    #[serde(default)]
    pub min_os_version: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_url: Option<String>,
    pub components: std::collections::HashMap<String, ManifestComponent>,
    #[serde(default)]
    pub os: ManifestOs,
    /// Host files (userpatch scripts, systemd units, config) to sync onto the
    /// device during an update, so changes to userpatches reach deployed
    /// devices without a reflash.
    #[serde(default)]
    pub host_files: Vec<HostFile>,
}

/// A file delivered to the host filesystem as part of an update.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HostFile {
    /// Download URL. MUST be pinned to an immutable ref (a release tag), never
    /// a branch like `main`, so a single branch commit cannot push arbitrary
    /// root-owned code to deployed devices.
    pub url: String,
    /// Absolute destination path on the host. The host updater additionally
    /// rejects traversal and confines this to an allow-list of roots.
    pub dest: String,
    /// Optional octal mode string (e.g. "0755"); applied after write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Mandatory sha256 (lowercase hex). These files land as host root, so
    /// integrity verification is required — a manifest entry without one fails
    /// to deserialize rather than silently shipping unverified content.
    pub sha256: String,
}

fn default_schema() -> u32 {
    1
}

/// Per-component comparison of installed vs available.
#[derive(Debug, Serialize, Clone)]
pub struct ComponentUpdate {
    /// Component key: manager | gateway | control-app | compose | catalog.
    pub name: String,
    pub installed: String,
    pub available: String,
    pub update_available: bool,
    pub severity: Severity,
    /// "image" | "file" | "os".
    pub kind: String,
}

/// Snapshot of what is installed right now.
#[derive(Debug, Serialize, Clone, Default)]
pub struct InstalledVersions {
    pub os_version: String,
    pub os_codename: String,
    pub manager: String,
    pub control_app: String,
    pub compose: u32,
    pub catalog: u32,
    pub channel: String,
}

/// Result of an update check: installed state + available components.
#[derive(Debug, Serialize, Clone)]
pub struct UpdateStatus {
    pub installed: InstalledVersions,
    /// Target release version, if the manifest was reachable.
    pub available_os_version: Option<String>,
    pub components: Vec<ComponentUpdate>,
    /// Highest severity among available updates.
    pub highest_severity: Option<Severity>,
    pub update_available: bool,
    /// Count of upgradable apt packages on the host (None if not checked).
    pub os_packages_upgradable: Option<u32>,
    /// Offered major OS upgrade, if any.
    pub major_upgrade: Option<MajorUpgrade>,
    /// RFC3339 timestamp of when this check ran.
    pub last_checked: Option<String>,
    /// Populated when the manifest could not be fetched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What the manager asks the host updater to do (written to request.json).
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UpdateRequest {
    /// RFC3339 timestamp.
    pub requested_at: String,
    /// Components to apply: manager,gateway,compose,catalog,os_packages,os_major.
    pub components: Vec<String>,
    /// Resolved download details for file components.
    #[serde(default)]
    pub compose_url: Option<String>,
    #[serde(default)]
    pub compose_sha256: Option<String>,
    #[serde(default)]
    pub compose_version: Option<u32>,
    #[serde(default)]
    pub catalog_url: Option<String>,
    #[serde(default)]
    pub catalog_sha256: Option<String>,
    #[serde(default)]
    pub catalog_version: Option<u32>,
    /// Target codename for a major OS upgrade.
    #[serde(default)]
    pub os_major_to: Option<String>,
    /// Pinned container image refs (image + tag) so updates pull immutable
    /// version tags rather than `:latest`.
    #[serde(default)]
    pub manager_image: Option<String>,
    #[serde(default)]
    pub manager_tag: Option<String>,
    #[serde(default)]
    pub gateway_image: Option<String>,
    #[serde(default)]
    pub gateway_tag: Option<String>,
    /// Host files (userpatches) to sync during this update.
    #[serde(default)]
    pub host_files: Vec<HostFile>,
    /// Repair mode: re-pull the images CURRENTLY pinned in docker-compose.yml
    /// and force-recreate the stack at the installed versions, without changing
    /// any tags or fetching the manifest. Used by "Force refresh".
    #[serde(default)]
    pub recreate: bool,
}

/// Progress written by the host updater (status.json).
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UpdateProgress {
    /// idle | running | success | failed | rolled_back.
    pub state: String,
    /// Current phase label.
    pub phase: String,
    /// 0-100 best-effort.
    #[serde(default)]
    pub percent: u8,
    #[serde(default)]
    pub log: Vec<String>,
    #[serde(default)]
    pub reboot_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}
