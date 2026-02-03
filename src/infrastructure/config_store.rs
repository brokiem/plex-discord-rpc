use crate::domain::models::{AppConfig, AppError, AppResult};
use crate::domain::traits::ConfigStore;
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

pub struct FileConfigStore {
    path: Option<PathBuf>,
}

impl FileConfigStore {
    pub fn new() -> Self {
        let path = ProjectDirs::from("com", "plex-discord-rpc", "plex-discord-rpc")
            .map(|dirs| dirs.config_dir().join("config.json"));
        Self { path }
    }
}

impl ConfigStore for FileConfigStore {
    fn load(&self) -> AppResult<AppConfig> {
        if let Some(path) = &self.path {
            if path.exists() {
                let data = fs::read_to_string(path)?;
                let config: AppConfig = serde_json::from_str(&data)
                    .map_err(|e| AppError::Config(format!("Failed to parse config: {}", e)))?;
                return Ok(config);
            }
        }
        Ok(AppConfig::default())
    }

    fn save(&self, config: &AppConfig) -> AppResult<()> {
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let data = serde_json::to_string_pretty(config)
                .map_err(|e| AppError::Config(format!("Failed to serialize config: {}", e)))?;
            fs::write(path, data)?;
            Ok(())
        } else {
            Err(AppError::Config("Could not determine config path".into()))
        }
    }
}
