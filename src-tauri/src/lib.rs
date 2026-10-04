mod actions;
mod commands;
mod config;
mod network;
mod state;
mod timing;
mod tray;
#[cfg(target_os = "windows")]
mod windows_network;

use state::AppState;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            tray::show_settings(app)
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap,
            commands::list_adapters,
            commands::save_config,
            commands::apply_profile,
            commands::open_config_folder,
            commands::hide_settings,
        ])
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let state = AppState::new(config_dir);
            let needs_setup = config::load(&state.config_path)
                .map(|profiles| profiles.is_empty())
                .unwrap_or(true)
                || state
                    .preferences()
                    .map(|preferences| {
                        network::validate_adapter_id(&preferences.adapter_id).is_err()
                    })
                    .unwrap_or(true);
            app.manage(state);
            tray::create(app)?;
            if needs_setup {
                tray::show_settings(app.handle());
            }
            log::info!(
                "SwitchCat {} started (manual network switching)",
                env!("CARGO_PKG_VERSION")
            );
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
