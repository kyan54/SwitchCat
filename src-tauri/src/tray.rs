use crate::{actions, config, state::AppState};
use tauri::{
    image::Image,
    menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};

pub fn create(app: &mut App) -> tauri::Result<()> {
    app.on_menu_event(|app, event| {
        let id = event.id().as_ref();
        match id {
            "settings" => show_settings(app),
            "quit" => app.exit(0),
            _ if id.starts_with("profile:") => {
                let name = id.strip_prefix("profile:").unwrap_or_default().to_string();
                let handle = app.clone();
                std::thread::spawn(move || {
                    let _ = actions::apply_profile(&handle, &name);
                });
            }
            _ => {}
        }
    });
    // A menu is read and displayed on demand; the static icon needs no animation thread.
    TrayIconBuilder::with_id("switchcat")
        .icon(Image::from_bytes(include_bytes!("../icons/32x32.png"))?)
        .tooltip("SwitchCat · 右键切换，左键配置")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                match button {
                    MouseButton::Left => show_settings(app),
                    MouseButton::Right => {
                        if let Err(error) = popup(app) {
                            log::error!("could not open tray menu: {error}");
                            show_settings(app);
                        }
                    }
                    _ => {}
                }
            }
        })
        .build(app)?;
    Ok(())
}
fn popup(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    if let Some(window) = app.get_webview_window("main") {
        window.popup_menu(&menu)?;
    }
    Ok(())
}
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let state = app.state::<AppState>();
    let runtime = state.snapshot();
    let preferences = state.preferences();
    let adapter_id = preferences
        .as_ref()
        .map(|value| value.adapter_id.as_str())
        .unwrap_or("");
    let ready = crate::network::validate_adapter_id(adapter_id).is_ok();
    let mut menu = MenuBuilder::new(app);
    if runtime.busy {
        menu = menu.item(
            &MenuItemBuilder::with_id("busy", "正在切换…")
                .enabled(false)
                .build(app)?,
        );
    } else if !ready {
        menu = menu.item(
            &MenuItemBuilder::with_id("setup", "请先选择目标网卡并保存")
                .enabled(false)
                .build(app)?,
        );
    }
    match config::load(&state.config_path) {
        Ok(profiles) if !profiles.is_empty() => {
            for profile in profiles {
                let checked = runtime.last_applied.as_ref().is_some_and(|applied| {
                    applied.adapter_id == adapter_id && applied.profile == profile
                });
                let item = CheckMenuItemBuilder::with_id(
                    format!("profile:{}", profile.name),
                    profile.name.replace('&', "&&"),
                )
                .checked(checked)
                .enabled(ready && !runtime.busy)
                .build(app)?;
                menu = menu.item(&item);
            }
        }
        Ok(_) => {
            menu = menu.item(
                &MenuItemBuilder::with_id("empty", "还没有配置，请打开配置页")
                    .enabled(false)
                    .build(app)?,
            );
        }
        Err(error) => {
            log::error!("could not read tray configuration: {error}");
            menu = menu.item(
                &MenuItemBuilder::with_id("invalid", "配置文件有误，请打开配置页")
                    .enabled(false)
                    .build(app)?,
            );
        }
    }
    menu.separator()
        .text("settings", "配置…")
        .separator()
        .text("quit", "退出 SwitchCat")
        .build()
}
pub fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
