mod commands;
mod tray;
mod tts;

use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::ShortcutState;

pub struct AppState {
    pub api_key: Mutex<String>,
    pub saved_volume: Mutex<Option<u32>>,
    pub duck_volume_enabled: Mutex<bool>,
    pub stop_media_enabled: Mutex<bool>,
    pub was_media_playing: Mutex<bool>,
    // Read Aloud state
    pub selected_voice_id: Mutex<String>,
    pub cached_voices: Mutex<Vec<tts::VoiceInfo>>,
    pub tts_cancel: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts(["CmdOrCtrl+Shift+Space", "Alt+Shift+Space"])
                .expect("failed to parse shortcuts")
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let app = app.clone();
                        let key = shortcut.key;
                        let mods = shortcut.mods;
                        tauri::async_runtime::spawn(async move {
                            // Alt+Shift+Space → read aloud, Cmd+Shift+Space → recording
                            if mods.contains(tauri_plugin_global_shortcut::Modifiers::ALT) {
                                handle_readaloud_toggle(&app).await;
                            } else {
                                handle_toggle(&app).await;
                            }
                            let _ = key; // suppress unused warning
                        });
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState {
            api_key: Mutex::new(String::new()),
            saved_volume: Mutex::new(None),
            duck_volume_enabled: Mutex::new(true),
            stop_media_enabled: Mutex::new(false),
            was_media_playing: Mutex::new(false),
            selected_voice_id: Mutex::new(String::new()),
            cached_voices: Mutex::new(Vec::new()),
            tts_cancel: Mutex::new(None),
        })
        .setup(|app| {
            if let Ok(config_dir) = app.path().app_config_dir() {
                if let Some(key) = load_key_from_config(&config_dir) {
                    *app.state::<AppState>().api_key.lock().unwrap() = key;
                }
                if let Some(voice_id) = load_voice_from_config(&config_dir) {
                    *app.state::<AppState>().selected_voice_id.lock().unwrap() = voice_id;
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
            commands::duck_volume,
            commands::restore_volume,
            commands::stop_media,
            commands::resume_media,
            tts::list_voices,
            tts::get_selected_voice,
            tts::set_selected_voice,
            tts::start_tts,
            tts::stop_tts,
            tts::skip_to_chunk,
            tts::hide_readaloud,
            tts::show_readaloud,
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

fn load_voice_from_config(config_dir: &std::path::Path) -> Option<String> {
    let contents = std::fs::read_to_string(config_dir.join("config.json")).ok()?;
    let config: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let voice_id = config["selected_voice_id"].as_str()?.to_string();
    if voice_id.is_empty() {
        None
    } else {
        Some(voice_id)
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

    // Focus the WebView so WebKit treats the subsequent getUserMedia call
    // as having user activation (required on macOS Tahoe+).
    let _ = window.set_focus();

    let _ = app.emit("toggle-recording", ());
}

async fn handle_readaloud_toggle(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("readaloud") else {
        return;
    };

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

    let _ = app.emit("toggle-readaloud", ());
}
