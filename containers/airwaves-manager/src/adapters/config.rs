use std::path::PathBuf;
use tokio::fs;

use crate::domain::AirwavesConfig;
use crate::error::AppError;
use crate::ports::ConfigPort;

pub struct ConfigAdapter {
    path: PathBuf,
}

impl ConfigAdapter {
    pub fn new(path: &str) -> Self {
        Self {
            path: PathBuf::from(path),
        }
    }
}

impl ConfigPort for ConfigAdapter {
    async fn read_config(&self) -> Result<AirwavesConfig, AppError> {
        let content = fs::read_to_string(&self.path).await?;
        let config: AirwavesConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    async fn write_config(&self, config: &AirwavesConfig) -> Result<(), AppError> {
        let content = serde_json::to_string_pretty(config)?;
        fs::write(&self.path, content).await?;
        Ok(())
    }
}
