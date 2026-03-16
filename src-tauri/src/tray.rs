use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItem, MenuItemBuilder,
    PredefinedMenuItem,
};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

use crate::AppState;

pub struct TrayState {
    pub api_key_item: MenuItem<tauri::Wry>,
    pub duck_item: CheckMenuItem<tauri::Wry>,
    pub media_item: CheckMenuItem<tauri::Wry>,
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

    let api_key_item =
        MenuItemBuilder::with_id("api_key", api_key_label(&key)).build(app)?;

    let duck_item = CheckMenuItemBuilder::with_id("duck_volume", "Duck volume while recording")
        .checked(true)
        .build(app)?;

    let media_item =
        CheckMenuItemBuilder::with_id("stop_media", "Pause media while recording")
            .checked(false)
            .build(app)?;

    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Elevenscribe").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[&api_key_item, &sep1, &duck_item, &media_item, &sep2, &quit])
        .build()?;

    app.manage(TrayState {
        api_key_item: api_key_item.clone(),
        duck_item: duck_item.clone(),
        media_item: media_item.clone(),
    });

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .on_menu_event(|app, event| {
            let app_state = app.state::<AppState>();
            match event.id.as_ref() {
                "api_key" => {
                    let has_key = !app_state.api_key.lock().unwrap().is_empty();
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if has_key {
                            // Clear the stored key and update the config file.
                            *app.state::<AppState>().api_key.lock().unwrap() = String::new();
                            if let Ok(config_dir) = app.path().app_config_dir() {
                                let json = serde_json::json!({ "elevenlabs_api_key": "" });
                                if let Ok(contents) = serde_json::to_string_pretty(&json) {
                                    let _ = std::fs::write(config_dir.join("config.json"), contents);
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
