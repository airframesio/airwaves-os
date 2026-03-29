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

/// System information
#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub hostname: String,
    pub os: String,
    pub architecture: String,
    pub kernel: String,
    pub uptime: u64,
    pub airwaves_version: String,
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

/// App catalog entry
#[derive(Debug, Serialize, Deserialize, Clone)]
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
}
