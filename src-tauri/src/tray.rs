use crate::{actions, cat_icon, openwrt::RouteSelection, state::AppState};
use log::error;
use std::{thread, time::Duration};
use tauri::{
    image::Image,
    menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::TrayIconBuilder,
    App, AppHandle, Manager,
};

pub const TRAY_ID: &str = "switchcat";

pub fn create(app: &mut App) -> tauri::Result<()> {
    app.on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()));
    build_tray(app.handle())
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let state = app.state::<AppState>();
    let menu_on_left_click = state
        .config_snapshot()
        .map(|config| config.app.menu_on_left_click)
        .unwrap_or(true);
    let icon = Image::new_owned(
        cat_icon::running_cat_frame(0),
        cat_icon::ICON_SIZE,
        cat_icon::ICON_SIZE,
    );

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("SwitchCat")
        .menu(&menu)
        .show_menu_on_left_click(menu_on_left_click)
        .build(app)?;

    Ok(())
}

pub fn rebuild_menu(app: &AppHandle) -> tauri::Result<()> {
    // AppIndicator does not allow replacing a menu after it has been attached. Recreate the
    // tray icon on Linux so newly added OpenWrt nodes still appear without restarting SwitchCat.
    #[cfg(target_os = "linux")]
    {
        let _ = app.remove_tray_by_id(TRAY_ID);
        return build_tray(app);
    }

    #[cfg(not(target_os = "linux"))]
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let menu = build_menu(app)?;
        tray.set_menu(Some(menu))?;
        let tooltip = tooltip(app);
        tray.set_tooltip(Some(tooltip))?;
        if let Ok(config) = app.state::<AppState>().config_snapshot() {
            let _ = tray.set_show_menu_on_left_click(config.app.menu_on_left_click);
        }
    }
    Ok(())
}

pub fn start_animation(app: AppHandle) {
    thread::spawn(move || {
        let mut phase = 0usize;
        loop {
            thread::sleep(Duration::from_millis(145));
            let animate = app
                .state::<AppState>()
                .config_snapshot()
                .map(|config| config.app.animate_cat)
                .unwrap_or(true);
            if !animate {
                continue;
            }
            phase = (phase + 1) % cat_icon::FRAME_COUNT;
            let icon = Image::new_owned(
                cat_icon::running_cat_frame(phase),
                cat_icon::ICON_SIZE,
                cat_icon::ICON_SIZE,
            );
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                if let Err(error) = tray.set_icon_with_as_template(Some(icon), cfg!(target_os = "macos")) {
                    error!("could not animate tray icon: {error}");
                }
            }
        }
    });
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let state = app.state::<AppState>();
    let config = state.config_snapshot().unwrap_or_default();
    let runtime = state.runtime_snapshot();
    let active_id = config.app.active_profile.clone();
    let active_profile = config.profiles.get(&active_id);
    let active_runtime = runtime.get(&active_id);
    let profile_ready = active_profile
        .is_some_and(|profile| profile.enabled && profile.ssh_verified);
    let active_inventory = profile_ready
        .then(|| active_runtime.and_then(|runtime| runtime.inventory.as_ref()))
        .flatten();
    let profile_busy = active_runtime.is_some_and(|runtime| runtime.busy);
    let active_name = active_profile
        .map(|profile| profile.name.as_str())
        .unwrap_or("未配置");

    let selection = active_inventory
        .map(|inventory| inventory.selection.clone())
        .unwrap_or(RouteSelection::Unknown);
    let status_text = if active_profile.is_some_and(|profile| !profile.enabled) {
        format!("🐱 {active_name} · 环境未启用")
    } else if active_profile.is_some_and(|profile| !profile.ssh_verified) {
        format!("🐱 {active_name} · 等待 SSH 验证")
    } else if profile_busy {
        format!("🐾 {active_name} · 正在处理…")
    } else {
        format!("🐱 {active_name} · {}", selection.description())
    };
    let status_item = MenuItemBuilder::with_id("status", status_text)
        .enabled(false)
        .build(app)?;

    let mut environment_menu = SubmenuBuilder::new(app, "切换环境");
    for (profile_id, profile) in config.profiles.iter().filter(|(_, profile)| profile.enabled) {
        let item = CheckMenuItemBuilder::with_id(
            format!("env:{profile_id}"),
            profile.name.clone(),
        )
        .checked(profile_id == &active_id)
        .enabled(profile.enabled && profile.ssh_verified)
        .build(app)?;
        environment_menu = environment_menu.item(&item);
    }
    environment_menu = environment_menu.separator().text("manage-environments", "管理环境…");
    let environment_menu = environment_menu.build()?;

    let direct_item = CheckMenuItemBuilder::with_id("route:direct", "本地直连")
        .checked(matches!(&selection, RouteSelection::Direct))
        .enabled(profile_ready && active_inventory.is_some() && !profile_busy)
        .build(app)?;

    let mut menu = MenuBuilder::new(app)
        .item(&status_item)
        .item(&environment_menu)
        .separator()
        .item(&direct_item);

    if let Some(inventory) = active_inventory {
        for (index, node) in inventory.nodes.iter().enumerate() {
            let item = CheckMenuItemBuilder::with_id(
                format!("route:node:{index}"),
                node.menu_label(),
            )
            .checked(selection.is_node(&node.id))
            .enabled(profile_ready && !profile_busy)
            .build(app)?;
            menu = menu.item(&item);
        }
    } else {
        let loading_text = if !profile_ready {
            "请先保存配置并完成 SSH 验证"
        } else if active_runtime.and_then(|runtime| runtime.last_error.as_ref()).is_some() {
            "节点读取失败，请查看设置"
        } else {
            "正在读取 OpenWrt 节点…"
        };
        let placeholder = MenuItemBuilder::with_id("nodes-placeholder", loading_text)
            .enabled(false)
            .build(app)?;
        menu = menu.item(&placeholder);
    }

    if let Some(error_message) = active_runtime.and_then(|runtime| runtime.last_error.as_ref()) {
        let concise_error = truncate(error_message, 48);
        let error_item = MenuItemBuilder::with_id("last-error", format!("⚠ {concise_error}"))
            .enabled(false)
            .build(app)?;
        menu = menu.item(&error_item);
    }

    let autostart_item = CheckMenuItemBuilder::with_id("toggle-autostart", "开机启动")
        .checked(config.app.start_at_login)
        .build(app)?;
    let refresh_item = MenuItemBuilder::with_id("refresh", "刷新节点")
        .enabled(profile_ready && !profile_busy)
        .build(app)?;

    menu.separator()
        .item(&refresh_item)
        .text("settings", "设置…")
        .text("open-config", "打开配置目录")
        .item(&autostart_item)
        .separator()
        .text("quit", "退出 SwitchCat")
        .build()
}

fn tooltip(app: &AppHandle) -> String {
    let state = app.state::<AppState>();
    let Ok(config) = state.config_snapshot() else {
        return "SwitchCat".to_string();
    };
    let runtime = state.runtime_snapshot();
    let profile_name = config
        .profiles
        .get(&config.app.active_profile)
        .map(|profile| profile.name.as_str())
        .unwrap_or("未配置");
    let active_profile = config.profiles.get(&config.app.active_profile);
    let route = if active_profile.is_some_and(|profile| !profile.enabled) {
        "环境未启用".to_string()
    } else if active_profile.is_some_and(|profile| !profile.ssh_verified) {
        "等待 SSH 验证".to_string()
    } else {
        runtime
            .get(&config.app.active_profile)
            .and_then(|runtime| runtime.inventory.as_ref())
            .map(|inventory| inventory.selection.description())
            .unwrap_or_else(|| "状态未知".to_string())
    };
    format!("SwitchCat · {profile_name} · {route}")
}

fn handle_menu_event(app: &AppHandle, event_id: &str) {
    match event_id {
        "quit" => app.exit(0),
        "settings" | "manage-environments" => show_settings(app),
        "open-config" => {
            let path = app.state::<AppState>().config_dir.clone();
            if let Err(error) = tauri_plugin_opener::open_path(&path, None::<&str>) {
                error!("could not open config directory: {error}");
            }
        }
        "refresh" => {
            if let Ok(profile_id) = app.state::<AppState>().active_profile_id() {
                spawn_action(app, move |handle| {
                    let _ = actions::refresh_profile(&handle, &profile_id, true);
                });
            }
        }
        "route:direct" => {
            if let Ok(profile_id) = app.state::<AppState>().active_profile_id() {
                spawn_action(app, move |handle| {
                    let _ = actions::switch_direct(&handle, &profile_id);
                });
            }
        }
        "toggle-autostart" => {
            let handle = app.clone();
            if let Err(error) = actions::toggle_autostart(&handle) {
                error!("could not toggle autostart: {error}");
            }
        }
        _ if event_id.starts_with("env:") => {
            let profile_id = event_id.trim_start_matches("env:").to_string();
            spawn_action(app, move |handle| {
                let _ = actions::activate_profile(&handle, &profile_id);
            });
        }
        _ if event_id.starts_with("route:node:") => {
            let index = event_id
                .trim_start_matches("route:node:")
                .parse::<usize>()
                .ok();
            let state = app.state::<AppState>();
            let profile_id = state.active_profile_id().ok();
            let node_id = profile_id.as_ref().and_then(|profile_id| {
                state
                    .runtime_snapshot()
                    .get(profile_id)
                    .and_then(|runtime| runtime.inventory.as_ref())
                    .and_then(|inventory| index.and_then(|index| inventory.nodes.get(index)))
                    .map(|node| node.id.clone())
            });
            if let (Some(profile_id), Some(node_id)) = (profile_id, node_id) {
                spawn_action(app, move |handle| {
                    let _ = actions::switch_node(&handle, &profile_id, &node_id);
                });
            }
        }
        _ => {}
    }
}

pub fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn spawn_action<F>(app: &AppHandle, action: F)
where
    F: FnOnce(AppHandle) + Send + 'static,
{
    let handle = app.clone();
    thread::spawn(move || action(handle));
}

fn truncate(value: &str, max_characters: usize) -> String {
    let mut characters = value.chars();
    let prefix: String = characters.by_ref().take(max_characters).collect();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}
