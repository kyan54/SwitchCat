use crate::{
    actions,
    config::AppConfig,
    openwrt::{self, Inventory, SshInstructions, SshStatus},
    state::{AppState, ProfileRuntime},
    system_info::{self, NetworkSnapshot},
};
use serde::Serialize;
use std::collections::BTreeMap;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
pub struct Bootstrap {
    config_exists: bool,
    config_path: String,
    config_dir: String,
    platform: String,
    config: AppConfig,
    runtime: BTreeMap<String, ProfileRuntime>,
    network: NetworkSnapshot,
}

#[tauri::command]
pub fn get_bootstrap(app: AppHandle) -> Result<Bootstrap, String> {
    let state = app.state::<AppState>();
    Ok(Bootstrap {
        config_exists: state.config_path.exists(),
        config_path: state.config_path.display().to_string(),
        config_dir: state.config_dir.display().to_string(),
        platform: std::env::consts::OS.to_string(),
        config: state.config_snapshot()?,
        runtime: state.runtime_snapshot(),
        network: system_info::snapshot(),
    })
}

#[tauri::command]
pub async fn save_config(app: AppHandle, config: AppConfig) -> Result<(), String> {
    run_blocking(move || actions::save_config(&app, config)).await
}

#[tauri::command]
pub async fn check_ssh(app: AppHandle, profile_id: String) -> Result<SshStatus, String> {
    run_blocking(move || actions::check_ssh(&app, &profile_id)).await
}

#[tauri::command]
pub fn get_ssh_instructions(
    app: AppHandle,
    profile_id: String,
) -> Result<SshInstructions, String> {
    let state = app.state::<AppState>();
    let profile = state.profile(&profile_id)?;
    Ok(openwrt::ssh_instructions(
        &profile_id,
        &profile,
        &state.config_dir,
    ))
}

#[tauri::command]
pub async fn refresh_profile(app: AppHandle, profile_id: String) -> Result<Inventory, String> {
    run_blocking(move || actions::refresh_profile(&app, &profile_id, false)).await
}

#[tauri::command]
pub async fn activate_profile(app: AppHandle, profile_id: String) -> Result<Inventory, String> {
    run_blocking(move || actions::activate_profile(&app, &profile_id)).await
}

#[tauri::command]
pub async fn switch_direct(app: AppHandle, profile_id: String) -> Result<Inventory, String> {
    run_blocking(move || actions::switch_direct(&app, &profile_id)).await
}

#[tauri::command]
pub async fn switch_node(
    app: AppHandle,
    profile_id: String,
    node_id: String,
) -> Result<Inventory, String> {
    run_blocking(move || actions::switch_node(&app, &profile_id, &node_id)).await
}

#[tauri::command]
pub fn current_network() -> NetworkSnapshot {
    system_info::snapshot()
}

#[tauri::command]
pub fn hide_settings(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

async fn run_blocking<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("后台任务异常结束：{error}"))?
}
