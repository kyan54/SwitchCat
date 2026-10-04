use crate::{
    config::{self, Profile},
    network,
    state::{AppState, AppliedProfile, Preferences},
    timing, tray,
};
use log::{error, info};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

pub fn save_config(
    app: &AppHandle,
    mut profiles: Vec<Profile>,
    adapter_id: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _guard = state
        .operation_lock
        .try_lock()
        .map_err(|_| "正在切换，请完成后再保存配置".to_string())?;
    network::validate_adapter_id(&adapter_id)?;
    for profile in &mut profiles {
        profile.normalize();
    }
    let raw = config::serialize(&profiles)?;
    let preferences = serde_json::to_string_pretty(&Preferences { adapter_id })
        .map_err(|error| error.to_string())?;
    let previous_preferences = match std::fs::read_to_string(&state.preferences_path) {
        Ok(raw) => Some(raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("读取已有网卡设置失败：{error}")),
    };
    config::write_atomic(&state.preferences_path, &preferences)?;
    if let Err(error) = config::write_atomic(&state.config_path, &raw) {
        let restored = match previous_preferences {
            Some(previous) => config::write_atomic(&state.preferences_path, &previous),
            None => {
                std::fs::remove_file(&state.preferences_path).map_err(|error| error.to_string())
            }
        };
        return Err(match restored {
            Ok(()) => error,
            Err(restore_error) => {
                format!("{error}；恢复网卡选择失败：{restore_error}。请重新选择网卡并保存")
            }
        });
    }
    info!("saved {} manual network profiles", profiles.len());
    let _ = app.emit("config-saved", ());
    Ok(())
}

pub fn apply_profile(app: &AppHandle, name: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _guard = state
        .operation_lock
        .try_lock()
        .map_err(|_| "正在切换，请稍后再试".to_string())?;
    if let Ok(mut runtime) = state.runtime.write() {
        runtime.busy = true;
        runtime.last_error = None;
    }
    emit_runtime(app);
    let result = timing::measure(name, "switch_total", || {
        let profiles = config::load(&state.config_path)?;
        let profile = profiles
            .into_iter()
            .find(|profile| profile.name == name)
            .ok_or_else(|| "配置已被删除或改名，请重新打开菜单".to_string())?;
        let preferences = state.preferences()?;
        network::validate_adapter_id(&preferences.adapter_id)?;
        timing::measure(name, "windows_network", || {
            network::apply(&preferences.adapter_id, &profile)
        })?;
        Ok::<_, String>(AppliedProfile {
            adapter_id: preferences.adapter_id,
            profile,
        })
    });
    if let Ok(mut runtime) = state.runtime.write() {
        runtime.busy = false;
        match &result {
            Ok(applied) => {
                runtime.last_applied = Some(applied.clone());
                runtime.last_error = None;
            }
            Err(message) => {
                runtime.last_applied = None;
                runtime.last_error = Some(message.clone());
            }
        }
    }
    emit_runtime(app);
    match result {
        Ok(_) => {
            info!("applied manual network profile {name}");
            notify_result(app, "切换成功", &format!("已切换到：{name}"));
            Ok(())
        }
        Err(message) => {
            error!("applying {name} failed: {message}");
            notify_result(app, "切换失败", &format!("{name}\n{message}"));
            tray::show_settings(app);
            Err(message)
        }
    }
}
fn notify_result(app: &AppHandle, title: &str, body: &str) {
    if let Err(error) = app
        .notification()
        .builder()
        .title(format!("SwitchCat · {title}"))
        .body(body)
        .show()
    {
        // A notification failure must not change the completed network operation's result.
        log::warn!("could not show switch result notification: {error}");
    }
}
fn emit_runtime(app: &AppHandle) {
    let _ = app.emit("switchcat-state", app.state::<AppState>().snapshot());
}
