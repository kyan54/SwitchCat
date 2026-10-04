use crate::{
    actions,
    config::{self, Profile},
    network::{self, Adapter},
    state::{AppState, RuntimeState},
};
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
pub struct Bootstrap {
    config_path: String,
    profiles: Vec<Profile>,
    adapter_id: String,
    config_error: Option<String>,
    runtime: RuntimeState,
    version: &'static str,
}

#[tauri::command]
pub async fn get_bootstrap(app: AppHandle) -> Result<Bootstrap, String> {
    run_blocking(move || {
        let state = app.state::<AppState>();
        let (profiles, mut config_error) = match config::load(&state.config_path) {
            Ok(profiles) => (profiles, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        let adapter_id = match state.preferences() {
            Ok(preferences) => preferences.adapter_id,
            Err(error) => {
                config_error.get_or_insert(error);
                String::new()
            }
        };
        Ok(Bootstrap {
            config_path: state.config_path.display().to_string(),
            profiles,
            adapter_id,
            config_error,
            runtime: state.snapshot(),
            version: env!("CARGO_PKG_VERSION"),
        })
    })
    .await
}
#[tauri::command]
pub async fn list_adapters() -> Result<Vec<Adapter>, String> {
    run_blocking(network::list_adapters).await
}
#[tauri::command]
pub async fn save_config(
    app: AppHandle,
    profiles: Vec<Profile>,
    adapter_id: String,
) -> Result<(), String> {
    run_blocking(move || actions::save_config(&app, profiles, adapter_id)).await
}
#[tauri::command]
pub async fn apply_profile(app: AppHandle, name: String) -> Result<(), String> {
    run_blocking(move || actions::apply_profile(&app, &name)).await
}
#[tauri::command]
pub fn open_config_folder(app: AppHandle) -> Result<(), String> {
    tauri_plugin_opener::open_path(&app.state::<AppState>().config_dir, None::<&str>)
        .map_err(|error| format!("打开配置目录失败：{error}"))
}
#[tauri::command]
pub fn hide_settings(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}
async fn run_blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("后台任务异常：{error}"))?
}
