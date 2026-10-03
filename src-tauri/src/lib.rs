mod actions;
mod cat_icon;
mod commands;
mod config;
mod openwrt;
mod state;
mod system_info;
mod tray;
mod timing;
#[cfg(target_os = "windows")]
mod windows_network;

use log::{error, info};
use state::AppState;
use std::{fs, io, thread, time::Duration};
use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_settings(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap,
            commands::save_config,
            commands::check_ssh,
            commands::get_ssh_instructions,
            commands::refresh_profile,
            commands::activate_profile,
            commands::switch_direct,
            commands::switch_node,
            commands::current_network,
            commands::hide_settings,
        ])
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .map_err(|error| io::Error::other(error.to_string()))?;
            fs::create_dir_all(&config_dir)?;
            let config_path = config_dir.join("config.toml");
            let config_existed = config_path.exists();

            let config = match config::load(&config_path) {
                Ok(Some(config)) => config,
                Ok(None) => {
                    let network = system_info::snapshot();
                    let local_ip = network
                        .gateway
                        .as_deref()
                        .and_then(|gateway| system_info::best_local_ip_for(gateway, 9))
                        .or_else(|| network.ipv4_addresses.into_iter().next())
                        .unwrap_or_default();
                    config::AppConfig::with_local_ip(local_ip)
                }
                Err(error_message) => {
                    error!("configuration could not be loaded: {error_message}");
                    return Err(io::Error::other(error_message).into());
                }
            };

            app.manage(AppState::new(
                config_dir,
                config_path,
                config.clone(),
            ));

            #[cfg(target_os = "macos")]
            app.handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory)?;

            tray::create(app)?;
            tray::start_animation(app.handle().clone());

            if config_existed {
                if config.app.start_at_login {
                    if let Err(error) = app.autolaunch().enable() {
                        error!("could not enable autostart: {error}");
                    }
                } else if let Err(error) = app.autolaunch().disable() {
                    error!("could not disable autostart: {error}");
                }
            }

            if !config_existed {
                tray::show_settings(app.handle());
            } else {
                start_initial_refresh(app.handle().clone());
            }
            start_periodic_refresh(app.handle().clone());
            info!("SwitchCat {} started", env!("CARGO_PKG_VERSION"));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running SwitchCat");
}

fn start_initial_refresh(app: tauri::AppHandle) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(400));
        detect_environment(&app);
        refresh_active_profile_if_ready(&app);
    });
}

fn start_periodic_refresh(app: tauri::AppHandle) {
    thread::spawn(move || loop {
        let seconds = app
            .state::<AppState>()
            .config_snapshot()
            .map(|config| config.app.refresh_seconds)
            .unwrap_or(60)
            .clamp(10, 3600);
        thread::sleep(Duration::from_secs(seconds));

        if !app.state::<AppState>().config_path.exists() {
            continue;
        }
        detect_environment(&app);
        refresh_active_profile_if_ready(&app);
    });
}

fn refresh_active_profile_if_ready(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    if !state.config_path.exists() {
        return;
    }
    let Ok(profile_id) = state.active_profile_id() else {
        return;
    };
    if state.ready_profile(&profile_id).is_err() {
        return;
    }
    let _ = actions::refresh_profile(app, &profile_id, false);
}

fn detect_environment(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let Ok(config) = state.config_snapshot() else {
        return;
    };
    if !config.app.auto_detect_profile {
        return;
    }
    let Some(profile_id) = system_info::detect_profile(&config, &system_info::snapshot()) else {
        return;
    };
    if profile_id != config.app.active_profile {
        // activate_profile probes SSH first, so a false-positive match cannot select an
        // inaccessible router.
        let _ = actions::activate_profile(app, &profile_id);
    }
}
