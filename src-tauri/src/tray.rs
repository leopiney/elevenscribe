use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItem, MenuItemBuilder,
    PredefinedMenuItem, Submenu, SubmenuBuilder,
};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

use crate::tts::VoiceInfo;
use crate::AppState;

/// Five well-known ElevenLabs premade voices as defaults (before API fetch).
const DEFAULT_VOICES: &[(&str, &str)] = &[
    ("21m00Tcm4TlvDq8ikWAM", "Rachel"),
    ("29vD33N1CtxCmqQRPOHJ", "Drew"),
    ("EXAVITQu4vr4xnSDxMaL", "Sarah"),
    ("ErXwobaYiN019PkySvjV", "Antoni"),
    ("MF3mGyEYCl7XYWbV9V6O", "Emily"),
];

pub struct TrayState {
    pub api_key_item: MenuItem<tauri::Wry>,
    pub duck_item: CheckMenuItem<tauri::Wry>,
    pub media_item: CheckMenuItem<tauri::Wry>,
    #[allow(dead_code)]
    pub voice_submenu: Submenu<tauri::Wry>,
}

/// Returns the tray label for the API key item.
/// "Clear API Key: abc12345••••••"  when a key is set
/// "Set up API Key"                 when no key is configured
pub fn api_key_label(key: &str) -> String {
    if key.is_empty() {
        "Set up API Key".to_string()
    } else {
        let prefix: String = key.chars().take(8).collect();
        format!("Clear API Key: {prefix}••••••")
    }
}

pub fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let key = app.state::<AppState>().api_key.lock().unwrap().clone();
    let selected_voice = app
        .state::<AppState>()
        .selected_voice_id
        .lock()
        .unwrap()
        .clone();

    let api_key_item = MenuItemBuilder::with_id("api_key", api_key_label(&key)).build(app)?;

    // Build voice submenu with defaults
    let voice_submenu = build_voice_submenu(app, &selected_voice)?;

    let duck_item = CheckMenuItemBuilder::with_id("duck_volume", "Duck volume while recording")
        .checked(true)
        .build(app)?;

    let media_item = CheckMenuItemBuilder::with_id("stop_media", "Pause media while recording")
        .checked(false)
        .build(app)?;

    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Elevenscribe").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[
            &api_key_item,
            &sep1,
            &voice_submenu,
            &sep2,
            &duck_item,
            &media_item,
            &sep3,
            &quit,
        ])
        .build()?;

    app.manage(TrayState {
        api_key_item: api_key_item.clone(),
        duck_item: duck_item.clone(),
        media_item: media_item.clone(),
        voice_submenu: voice_submenu.clone(),
    });

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .on_menu_event(|app, event| {
            let app_state = app.state::<AppState>();
            let id = event.id.as_ref();

            // Check if this is a voice selection event
            if let Some(voice_id) = id.strip_prefix("voice_") {
                *app_state.selected_voice_id.lock().unwrap() = voice_id.to_string();
                // Persist to config
                let app = app.clone();
                let voice_id = voice_id.to_string();
                tauri::async_runtime::spawn(async move {
                    if let Ok(config_dir) = app.path().app_config_dir() {
                        let config_path = config_dir.join("config.json");
                        let mut config: serde_json::Value = std::fs::read_to_string(&config_path)
                            .ok()
                            .and_then(|s| serde_json::from_str(&s).ok())
                            .unwrap_or_else(|| serde_json::json!({}));
                        config["selected_voice_id"] = serde_json::Value::String(voice_id);
                        if let Ok(contents) = serde_json::to_string_pretty(&config) {
                            let _ = std::fs::write(&config_path, contents);
                        }
                    }
                });
                return;
            }

            match id {
                "api_key" => {
                    let has_key = !app_state.api_key.lock().unwrap().is_empty();
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if has_key {
                            // Clear the stored key and update the config file.
                            *app.state::<AppState>().api_key.lock().unwrap() = String::new();
                            if let Ok(config_dir) = app.path().app_config_dir() {
                                let config_path = config_dir.join("config.json");
                                let mut config: serde_json::Value =
                                    std::fs::read_to_string(&config_path)
                                        .ok()
                                        .and_then(|s| serde_json::from_str(&s).ok())
                                        .unwrap_or_else(|| serde_json::json!({}));
                                config["elevenlabs_api_key"] =
                                    serde_json::Value::String(String::new());
                                if let Ok(contents) = serde_json::to_string_pretty(&config) {
                                    let _ = std::fs::write(&config_path, contents);
                                }
                            }
                            let _ = app
                                .state::<TrayState>()
                                .api_key_item
                                .set_text("Set up API Key");
                        } else {
                            // Show the overlay setup screen.
                            if let Some(w) = app.get_webview_window("overlay") {
                                if let Ok(Some(monitor)) = w.current_monitor() {
                                    let sw = monitor.size().width as i32;
                                    let sh = monitor.size().height as i32;
                                    if let Ok(outer) = w.outer_size() {
                                        let x = (sw - outer.width as i32) / 2;
                                        let y = sh - outer.height as i32 - 80;
                                        let _ = w.set_position(tauri::PhysicalPosition { x, y });
                                    }
                                }
                                let _ = w.show();
                            }
                            let _ = app.emit("show-setup", ());
                        }
                    });
                }
                "duck_volume" => {
                    let enabled = app
                        .state::<TrayState>()
                        .duck_item
                        .is_checked()
                        .unwrap_or(true);
                    *app_state.duck_volume_enabled.lock().unwrap() = enabled;
                }
                "stop_media" => {
                    let enabled = app
                        .state::<TrayState>()
                        .media_item
                        .is_checked()
                        .unwrap_or(false);
                    *app_state.stop_media_enabled.lock().unwrap() = enabled;
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}

fn build_voice_submenu(
    app: &tauri::App,
    selected_voice_id: &str,
) -> tauri::Result<Submenu<tauri::Wry>> {
    let mut builder = SubmenuBuilder::with_id(app, "voice_menu", "Voice");

    // Use cached voices if available, otherwise use defaults
    let state = app.state::<AppState>();
    let cached = state.cached_voices.lock().unwrap();
    let voices: Vec<VoiceInfo> = if cached.is_empty() {
        DEFAULT_VOICES
            .iter()
            .map(|(id, name)| VoiceInfo {
                voice_id: id.to_string(),
                name: name.to_string(),
            })
            .collect()
    } else {
        cached.clone()
    };
    drop(cached);

    for voice in &voices {
        let is_selected = voice.voice_id == selected_voice_id
            || (selected_voice_id.is_empty() && voice.voice_id == DEFAULT_VOICES[0].0);
        let label = if is_selected {
            format!("✓ {}", voice.name)
        } else {
            format!("   {}", voice.name)
        };
        let item_id = format!("voice_{}", voice.voice_id);
        let item = MenuItemBuilder::with_id(item_id, label).build(app)?;
        builder = builder.item(&item);
    }

    builder.build()
}
