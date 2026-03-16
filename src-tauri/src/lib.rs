mod commands;
mod tray;

use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::ShortcutState;

pub struct AppState {
    pub api_key: Mutex<String>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcut("CmdOrCtrl+Shift+Space")
                .expect("failed to parse shortcut")
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            handle_toggle(&app).await;
                        });
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState {
            api_key: Mutex::new(String::new()),
        })
        .setup(|app| {
            if let Ok(config_dir) = app.path().app_config_dir() {
                if let Some(key) = load_key_from_config(&config_dir) {
                    *app.state::<AppState>().api_key.lock().unwrap() = key;
                }
            }

            tray::setup_tray(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::has_api_key,
            commands::save_api_key,
            commands::get_scribe_token,
            commands::paste_text,
            commands::hide_overlay,
            commands::show_overlay,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn load_key_from_config(config_dir: &std::path::Path) -> Option<String> {
    let contents = std::fs::read_to_string(config_dir.join("config.json")).ok()?;
    let config: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let key = config["elevenlabs_api_key"].as_str()?.to_string();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

async fn handle_toggle(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };

    // Only show the window here — hiding is owned by the frontend so the
    // clipboard write always completes before focus shifts back.
    if !window.is_visible().unwrap_or(true) {
        if let Ok(Some(monitor)) = window.current_monitor() {
            let sw = monitor.size().width as i32;
            let sh = monitor.size().height as i32;
            if let Ok(outer) = window.outer_size() {
                let x = (sw - outer.width as i32) / 2;
                let y = sh - outer.height as i32 - 80;
                let _ = window.set_position(tauri::PhysicalPosition { x, y });
            }
        }
        let _ = window.show();
    }

    let _ = app.emit("toggle-recording", ());
}
