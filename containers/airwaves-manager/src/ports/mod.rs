use crate::domain::*;
use crate::error::AppError;

/// Port for Docker container operations
#[allow(async_fn_in_trait)]
pub trait DockerPort {
    async fn list_containers(&self) -> Result<Vec<ContainerInfo>, AppError>;
    async fn start_container(&self, id: &str) -> Result<(), AppError>;
    async fn stop_container(&self, id: &str) -> Result<(), AppError>;
    async fn restart_container(&self, id: &str) -> Result<(), AppError>;
    async fn get_logs(&self, id: &str, tail: usize) -> Result<String, AppError>;
    async fn install_app(&self, app: &CatalogApp) -> Result<ContainerInfo, AppError>;
    async fn uninstall_app(&self, id: &str) -> Result<(), AppError>;
}

/// Port for system information
pub trait SystemPort {
    fn get_info(&self) -> Result<SystemInfo, AppError>;
    fn get_stats(&self) -> Result<SystemStats, AppError>;
}

/// Port for hardware enumeration
pub trait HardwarePort {
    fn list_usb_devices(&self) -> Result<Vec<UsbDevice>, AppError>;
    fn list_sdr_devices(&self) -> Result<Vec<SdrDevice>, AppError>;
}

/// Port for configuration management
#[allow(async_fn_in_trait)]
pub trait ConfigPort {
    async fn read_config(&self) -> Result<AirwavesConfig, AppError>;
    async fn write_config(&self, config: &AirwavesConfig) -> Result<(), AppError>;
}
