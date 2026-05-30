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
    /// Live CPU/memory stats for all running containers.
    async fn container_stats(&self) -> Result<Vec<ContainerStats>, AppError>;
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

/// Port for the system updater.
#[allow(async_fn_in_trait)]
pub trait UpdatePort {
    /// Fetch + parse the remote release manifest for the configured channel.
    async fn fetch_manifest(&self) -> Result<UpdateManifest, AppError>;
    /// Detect what is currently installed.
    async fn installed_versions(&self) -> InstalledVersions;
    /// Compare installed vs manifest and build a full status.
    async fn check(&self) -> UpdateStatus;
    /// Hand an apply request to the host updater; returns immediately.
    async fn apply(&self, components: Vec<String>) -> Result<(), AppError>;
    /// Read the host updater's current progress.
    async fn progress(&self) -> UpdateProgress;
}

/// Port for privileged host operations (executed in the host's namespaces).
#[allow(async_fn_in_trait)]
pub trait HostPort {
    /// Set the system hostname (persistent, via the host's hostnamed).
    async fn set_hostname(&self, hostname: &str) -> Result<(), AppError>;
    /// Reboot the host.
    async fn reboot(&self) -> Result<(), AppError>;
    /// Power off the host.
    async fn shutdown(&self) -> Result<(), AppError>;
    /// Restart a systemd service on the host (allowlisted names only).
    async fn restart_service(&self, service: &str) -> Result<(), AppError>;
    /// Set the system timezone (via the host's timedated).
    async fn set_timezone(&self, timezone: &str) -> Result<(), AppError>;
}
