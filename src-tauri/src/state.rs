use crate::config::Profile;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Mutex, RwLock},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Preferences {
    pub adapter_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppliedProfile {
    pub adapter_id: String,
    pub profile: Profile,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeState {
    pub busy: bool,
    pub last_applied: Option<AppliedProfile>,
    pub last_error: Option<String>,
}

pub struct AppState {
    pub config_dir: PathBuf,
    pub config_path: PathBuf,
    pub preferences_path: PathBuf,
    pub runtime: RwLock<RuntimeState>,
    pub operation_lock: Mutex<()>,
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_path: config_dir.join("config.ini"),
            preferences_path: config_dir.join("settings.json"),
            config_dir,
            runtime: RwLock::new(RuntimeState::default()),
            operation_lock: Mutex::new(()),
        }
    }
    pub fn preferences(&self) -> Result<Preferences, String> {
        match std::fs::read_to_string(&self.preferences_path) {
            Ok(raw) => {
                serde_json::from_str(&raw).map_err(|error| format!("读取目标网卡设置失败：{error}"))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(Preferences::default())
            }
            Err(error) => Err(format!("读取目标网卡设置失败：{error}")),
        }
    }
    pub fn snapshot(&self) -> RuntimeState {
        self.runtime
            .read()
            .map(|state| state.clone())
            .unwrap_or_default()
    }
}
