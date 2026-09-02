use crate::{
    config::{AppConfig, Profile},
    openwrt::Inventory,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Mutex, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProfileRuntime {
    pub inventory: Option<Inventory>,
    pub last_error: Option<String>,
    pub last_refreshed_unix: Option<u64>,
    pub busy: bool,
}

pub struct AppState {
    pub config_dir: PathBuf,
    pub config_path: PathBuf,
    pub config: RwLock<AppConfig>,
    pub runtime: RwLock<BTreeMap<String, ProfileRuntime>>,
    pub action_lock: Mutex<()>,
}

impl AppState {
    pub fn new(
        config_dir: PathBuf,
        config_path: PathBuf,
        config: AppConfig,
    ) -> Self {
        let runtime = config
            .profiles
            .keys()
            .map(|id| (id.clone(), ProfileRuntime::default()))
            .collect();
        Self {
            config_dir,
            config_path,
            config: RwLock::new(config),
            runtime: RwLock::new(runtime),
            action_lock: Mutex::new(()),
        }
    }

    pub fn config_snapshot(&self) -> Result<AppConfig, String> {
        self.config
            .read()
            .map(|config| config.clone())
            .map_err(|_| "配置状态锁已损坏".to_string())
    }

    pub fn profile(&self, profile_id: &str) -> Result<Profile, String> {
        self.config_snapshot()?
            .profiles
            .get(profile_id)
            .filter(|profile| profile.enabled)
            .cloned()
            .ok_or_else(|| format!("环境“{profile_id}”不存在或已禁用"))
    }

    pub fn active_profile_id(&self) -> Result<String, String> {
        Ok(self.config_snapshot()?.app.active_profile)
    }

    pub fn replace_config(&self, config: AppConfig) -> Result<(), String> {
        let profile_ids: Vec<String> = config.profiles.keys().cloned().collect();
        *self
            .config
            .write()
            .map_err(|_| "配置状态锁已损坏".to_string())? = config;

        let mut runtime = self
            .runtime
            .write()
            .map_err(|_| "运行状态锁已损坏".to_string())?;
        runtime.retain(|id, _| profile_ids.contains(id));
        for id in profile_ids {
            runtime.entry(id).or_default();
        }
        Ok(())
    }

    pub fn set_busy(&self, profile_id: &str, busy: bool) {
        if let Ok(mut runtime) = self.runtime.write() {
            runtime.entry(profile_id.to_string()).or_default().busy = busy;
        }
    }

    pub fn set_inventory(&self, profile_id: &str, inventory: Inventory) {
        if let Ok(mut runtime) = self.runtime.write() {
            let entry = runtime.entry(profile_id.to_string()).or_default();
            entry.inventory = Some(inventory);
            entry.last_error = None;
            entry.last_refreshed_unix = Some(now_unix());
            entry.busy = false;
        }
    }

    pub fn set_error(&self, profile_id: &str, error: String) {
        if let Ok(mut runtime) = self.runtime.write() {
            let entry = runtime.entry(profile_id.to_string()).or_default();
            entry.last_error = Some(error);
            entry.last_refreshed_unix = Some(now_unix());
            entry.busy = false;
        }
    }

    pub fn runtime_snapshot(&self) -> BTreeMap<String, ProfileRuntime> {
        self.runtime
            .read()
            .map(|runtime| runtime.clone())
            .unwrap_or_default()
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
