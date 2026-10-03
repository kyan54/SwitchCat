use crate::{config, openwrt, state::AppState, tray};
use log::{error, info};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;

#[derive(Clone, Serialize)]
struct StateChanged {
    profile_id: String,
    operation: String,
    ok: bool,
    message: String,
}

pub fn check_ssh(app: &AppHandle, profile_id: &str) -> Result<openwrt::SshStatus, String> {
    let state = app.state::<AppState>();
    let _guard = state
        .action_lock
        .lock()
        .map_err(|_| "操作锁已损坏".to_string())?;
    if !state.config_path.exists() {
        return Err("请先保存环境配置，再测试 SSH".to_string());
    }

    let profile = state.profile(profile_id)?;
    let status = openwrt::check_ssh(profile_id, &profile, &state.config_dir);
    let mut current_config = state.config_snapshot()?;
    let current_profile = current_config
        .profiles
        .get_mut(profile_id)
        .ok_or_else(|| "环境不存在，请重新选择".to_string())?;
    let verified_changed = current_profile.ssh_verified != status.ok;
    current_profile.ssh_verified = status.ok;
    if verified_changed {
        config::save(&state.config_path, &current_config)?;
        state.replace_config(current_config)?;
        schedule_menu_rebuild(app);
    }
    Ok(status)
}

pub fn refresh_profile(app: &AppHandle, profile_id: &str, notify_on_error: bool) -> Result<openwrt::Inventory, String> {
    let state = app.state::<AppState>();
    let _guard = state
        .action_lock
        .lock()
        .map_err(|_| "操作锁已损坏".to_string())?;
    refresh_profile_unlocked(app, profile_id, notify_on_error)
}

fn refresh_profile_unlocked(app: &AppHandle, profile_id: &str, notify_on_error: bool) -> Result<openwrt::Inventory, String> {
    let state = app.state::<AppState>();
    let profile = state.ready_profile(profile_id)?;
    state.set_busy(profile_id, true);
    schedule_menu_rebuild(app);

    let result = openwrt::fetch_inventory(profile_id, &profile, &state.config_dir);
    match result {
        Ok(inventory) => {
            state.set_inventory(profile_id, inventory.clone());
            emit_change(app, profile_id, "refresh", true, "节点列表已刷新");
            schedule_menu_rebuild(app);
            Ok(inventory)
        }
        Err(error_message) => {
            state.set_error(profile_id, error_message.clone());
            error!("refresh profile {profile_id} failed: {error_message}");
            emit_change(app, profile_id, "refresh", false, &error_message);
            if notify_on_error {
                notify(app, "刷新失败", &error_message);
            }
            schedule_menu_rebuild(app);
            Err(error_message)
        }
    }
}

pub fn activate_profile(app: &AppHandle, profile_id: &str) -> Result<openwrt::Inventory, String> {
    let state = app.state::<AppState>();
    let _guard = state
        .action_lock
        .lock()
        .map_err(|_| "操作锁已损坏".to_string())?;

    // Reachability and SSH are checked before changing the active environment. A mistaken click
    // therefore cannot silently point SwitchCat at an inaccessible router.
    let inventory = refresh_profile_unlocked(app, profile_id, false).map_err(|error_message| {
        notify(app, "环境切换失败", &error_message);
        error_message
    })?;

    let mut config = state.config_snapshot()?;
    let profile_name = config
        .profiles
        .get(profile_id)
        .map(|profile| profile.name.clone())
        .ok_or_else(|| "环境不存在".to_string())?;
    config.app.active_profile = profile_id.to_string();
    config::save(&state.config_path, &config)?;
    state.replace_config(config)?;

    info!("active profile changed to {profile_id}");
    emit_change(
        app,
        profile_id,
        "activate_profile",
        true,
        &format!("当前环境已切换为：{profile_name}"),
    );
    notify(app, "环境已切换", &profile_name);
    schedule_menu_rebuild(app);
    Ok(inventory)
}

pub fn switch_direct(app: &AppHandle, profile_id: &str) -> Result<openwrt::Inventory, String> {
    switch_route(app, profile_id, None)
}

pub fn switch_node(
    app: &AppHandle,
    profile_id: &str,
    node_id: &str,
) -> Result<openwrt::Inventory, String> {
    switch_route(app, profile_id, Some(node_id))
}

fn switch_route(
    app: &AppHandle,
    profile_id: &str,
    node_id: Option<&str>,
) -> Result<openwrt::Inventory, String> {
    crate::timing::measure(profile_id, "switch_total", || {
        switch_route_inner(app, profile_id, node_id)
    })
}

fn switch_route_inner(
    app: &AppHandle,
    profile_id: &str,
    node_id: Option<&str>,
) -> Result<openwrt::Inventory, String> {
    info!("switch requested profile={profile_id} target={}", node_id.unwrap_or("direct"));
    let state = app.state::<AppState>();
    let _guard = crate::timing::measure(profile_id, "wait_action_lock", || {
        state.action_lock.lock().map_err(|_| "操作锁已损坏".to_string())
    })?;
    let profile = state.ready_profile(profile_id)?;
    state.set_busy(profile_id, true);
    schedule_menu_rebuild(app);

    let result = match node_id {
        Some(node_id) => openwrt::switch_node(profile_id, &profile, &state.config_dir, node_id),
        None => openwrt::switch_direct(profile_id, &profile, &state.config_dir),
    };

    #[cfg(target_os = "windows")]
    let result = result.and_then(|inventory| {
        crate::timing::measure(profile_id, "windows_network", || {
            crate::windows_network::apply_route(&profile, node_id.is_some())
        })
            .map(|_| inventory)
            .map_err(|error_message| {
                format!("OpenWrt ACL 已更新，但 Windows 网络切换失败：{error_message}")
            })
    });

    match result {
        Ok(inventory) => {
            let route_name = inventory.selection.description();
            state.set_inventory(profile_id, inventory.clone());
            info!("profile {profile_id} switched to {route_name}");
            emit_change(app, profile_id, "switch_route", true, &route_name);
            notify(app, "切换成功", &route_name);
            schedule_menu_rebuild(app);
            Ok(inventory)
        }
        Err(error_message) => {
            state.set_error(profile_id, error_message.clone());
            error!("switch route for {profile_id} failed: {error_message}");
            emit_change(app, profile_id, "switch_route", false, &error_message);
            notify(app, "切换失败", &error_message);
            schedule_menu_rebuild(app);
            Err(error_message)
        }
    }
}

pub fn save_config(app: &AppHandle, mut new_config: crate::config::AppConfig) -> Result<(), String> {
    let state = app.state::<AppState>();
    let previous_config = state.config_snapshot()?;
    for (profile_id, profile) in &mut new_config.profiles {
        profile.ssh_verified = previous_config
            .profiles
            .get(profile_id)
            .is_some_and(|previous| {
                previous.ssh_verified && ssh_context_unchanged(previous, profile)
            });
    }
    new_config.validate()?;
    config::save(&state.config_path, &new_config)?;
    let start_at_login = new_config.app.start_at_login;
    state.replace_config(new_config)?;

    let autostart = app.autolaunch();
    let autostart_result = if start_at_login {
        autostart.enable()
    } else {
        autostart.disable()
    };
    if let Err(error) = autostart_result {
        error!("could not update autostart: {error}");
    }

    emit_change(app, "", "save_config", true, "配置已保存");
    schedule_menu_rebuild(app);
    Ok(())
}

fn ssh_context_unchanged(previous: &config::Profile, current: &config::Profile) -> bool {
    previous.openwrt.host == current.openwrt.host
        && previous.openwrt.port == current.openwrt.port
        && previous.openwrt.user == current.openwrt.user
        && previous.openwrt.identity_file == current.openwrt.identity_file
        && previous.device.client_ip == current.device.client_ip
        && previous.device.acl_remarks == current.device.acl_remarks
}

pub fn toggle_autostart(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let mut config = state.config_snapshot()?;
    config.app.start_at_login = !config.app.start_at_login;
    let enabled = config.app.start_at_login;
    save_config(app, config)?;
    Ok(enabled)
}

pub fn schedule_menu_rebuild(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(error) = tray::rebuild_menu(&handle) {
            error!("could not rebuild tray menu: {error}");
        }
    });
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show();
}

fn emit_change(app: &AppHandle, profile_id: &str, operation: &str, ok: bool, message: &str) {
    let _ = app.emit(
        "switchcat-state",
        StateChanged {
            profile_id: profile_id.to_string(),
            operation: operation.to_string(),
            ok,
            message: message.to_string(),
        },
    );
}
