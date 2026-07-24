mod commands;
mod history;
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
    // A shortcut action ("toggle-recording"/"toggle-readaloud") stashed while a
    // missing overlay is rebuilt, then pulled by the overlay once it mounts.
    pub pending_action: Mutex<Option<String>>,
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
            pending_action: Mutex::new(None),
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

            // Closing either window via the native traffic-light button (or
            // Cmd+W) must HIDE it, not destroy it. A destroyed overlay can
            // never be re-shown, so get_webview_window("overlay") returns None
            // and the global shortcut silently no-ops until the app is
            // relaunched — the bug this guards against.
            for label in ["main", "overlay"] {
                if let Some(window) = app.get_webview_window(label) {
                    attach_hide_on_close(&window);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::has_api_key,
            commands::get_api_key,
            commands::save_api_key,
            commands::get_scribe_token,
            commands::paste_text,
            commands::hide_overlay,
            commands::show_overlay,
            commands::duck_volume,
            commands::restore_volume,
            commands::stop_media,
            commands::resume_media,
            commands::set_activity,
            commands::take_pending_action,
            commands::save_debug_recording,
            commands::open_recordings_dir,
            tts::list_voices,
            tts::get_selected_voice,
            tts::set_selected_voice,
            tts::cache_tts_chunk,
            history::save_transcription,
            history::save_readaloud,
            history::get_history,
            history::delete_history_entry,
            history::clear_history,
            history::get_cached_audio,
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

/// Intercept the window's close request so the native traffic-light button
/// (and Cmd+W) HIDE the window instead of destroying it. For the overlay we
/// also reset the tray status, reverse any audio side-effects, and tell the
/// frontend to tear down an in-flight STT/TTS session — so closing mid-session
/// leaves a clean state instead of stuck flags and a half-ducked volume.
fn attach_hide_on_close(window: &tauri::WebviewWindow) {
    let win = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = win.hide();

            if win.label() == "overlay" {
                let app = win.app_handle().clone();
                if let Some(tray_state) = app.try_state::<tray::TrayState>() {
                    let _ = tray_state
                        .status_item
                        .set_text(tray::activity_label("idle"));
                }
                let _ = app.emit("overlay-hidden", ());
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let _ = commands::restore_volume_inner(state.inner()).await;
                    let _ = commands::resume_media_inner(state.inner()).await;
                });
            }
        }
    });
}

/// Recreate the overlay window from scratch (matching tauri.conf.json) and
/// re-attach the close interceptor. Used as a self-healing fallback so a lost
/// overlay can never permanently kill the shortcut.
fn build_overlay_window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    let window = tauri::WebviewWindowBuilder::new(
        app,
        "overlay",
        tauri::WebviewUrl::App("overlay.html".into()),
    )
    .title("")
    .inner_size(480.0, 240.0)
    .decorations(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(false)
    .build()
    .ok()?;
    attach_hide_on_close(&window);
    Some(window)
}

/// Shows the overlay, rebuilding it first if it was lost. Returns the window and
/// `was_built` = true when it had to be recreated (a cold WebView that is not
/// yet listening — the caller must stash the action instead of emitting it).
async fn show_overlay_window(app: &tauri::AppHandle) -> Option<(tauri::WebviewWindow, bool)> {
    // Self-heal: if the overlay was somehow lost (e.g. a WebView crash, or a
    // close that slipped past the interceptor), rebuild it rather than giving
    // up and leaving the shortcut dead.
    let (window, was_built) = match app.get_webview_window("overlay") {
        Some(w) => (w, false),
        None => (build_overlay_window(app)?, true),
    };
    let was_hidden = !window.is_visible().unwrap_or(true);

    if was_hidden {
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
        // Give the WebView a beat to resume JS execution after being hidden
        // — macOS WebKit throttles/suspends offscreen WebViews and the first
        // emit after show() can otherwise be dropped or queued indefinitely.
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    }

    // Focus the WebView so WebKit treats subsequent getUserMedia / AudioContext
    // calls as having user activation (required on macOS Tahoe+).
    let _ = window.set_focus();
    Some((window, was_built))
}

/// Deliver a toggle action to the overlay: emit it directly when the overlay is
/// already loaded, or stash it for the overlay to pull on mount when it was just
/// rebuilt (its WebView isn't listening yet, so a direct emit would be dropped).
fn dispatch_overlay_action(app: &tauri::AppHandle, was_built: bool, event: &'static str) {
    if was_built {
        *app.state::<AppState>().pending_action.lock().unwrap() = Some(event.to_string());
    } else {
        let _ = app.emit(event, ());
    }
}

pub async fn handle_toggle(app: &tauri::AppHandle) {
    let Some((_, was_built)) = show_overlay_window(app).await else {
        return;
    };
    dispatch_overlay_action(app, was_built, "toggle-recording");
}

pub async fn handle_readaloud_toggle(app: &tauri::AppHandle) {
    let Some((_, was_built)) = show_overlay_window(app).await else {
        return;
    };
    dispatch_overlay_action(app, was_built, "toggle-readaloud");
}

/// Force the app back to a clean idle state — the "Reset / Recover" tray action.
/// Reverses any leftover audio side-effects, resets the tray status, guarantees
/// the overlay exists and is hidden (so the shortcut works), and tells the
/// frontend to tear down any in-flight STT/TTS session. Driven from Rust so it
/// works even if the overlay WebView is throttled, stuck, or gone.
pub async fn reset_app(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let _ = commands::restore_volume_inner(state.inner()).await;
    let _ = commands::resume_media_inner(state.inner()).await;
    *state.pending_action.lock().unwrap() = None;

    if let Some(tray_state) = app.try_state::<tray::TrayState>() {
        let _ = tray_state
            .status_item
            .set_text(tray::activity_label("idle"));
    }

    match app.get_webview_window("overlay") {
        Some(window) => {
            let _ = window.hide();
        }
        None => {
            let _ = build_overlay_window(app);
        }
    }

    let _ = app.emit("reset-overlay", ());
}
