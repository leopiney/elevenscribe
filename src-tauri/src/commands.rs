use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use crate::AppState;

#[derive(Deserialize)]
struct TokenResponse {
    token: String,
}

/// Returns true if an API key has been configured.
#[tauri::command]
pub fn has_api_key(state: State<'_, AppState>) -> bool {
    !state.api_key.lock().unwrap().is_empty()
}

/// Persist the API key to the user config file and update the in-memory state.
/// Config is stored at ~/Library/Application Support/{identifier}/config.json.
#[tauri::command]
pub async fn save_api_key(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<(), String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;

    let json = serde_json::json!({ "elevenlabs_api_key": key });
    let contents = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(config_dir.join("config.json"), contents).map_err(|e| e.to_string())?;

    *state.api_key.lock().unwrap() = key;
    Ok(())
}

/// Exchange the server-side API key for a single-use ephemeral token that
/// the frontend uses to open a WebSocket directly to ElevenLabs Scribe realtime.
#[tauri::command]
pub async fn get_scribe_token(state: State<'_, AppState>) -> Result<String, String> {
    let key = state.api_key.lock().unwrap().clone();
    if key.is_empty() {
        return Err("No API key configured".to_string());
    }

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.elevenlabs.io/v1/single-use-token/realtime_scribe")
        .header("xi-api-key", &key)
        .json(&serde_json::json!({})) // ensures Content-Length header is set
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("ElevenLabs API error {status}: {body}"));
    }

    let token_response: TokenResponse = response.json().await.map_err(|e| e.to_string())?;
    Ok(token_response.token)
}

/// Hide the floating overlay window.
#[tauri::command]
pub async fn hide_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Show the floating overlay window.
#[tauri::command]
pub async fn show_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Hide the overlay, wait 150 ms for focus to return to the previously active
/// app, then simulate Cmd+V to paste the clipboard contents.
#[tauri::command]
pub async fn paste_text(app: AppHandle, text: String) -> Result<(), String> {
    let _ = text; // clipboard already set by frontend

    if let Some(window) = app.get_webview_window("overlay") {
        window.hide().map_err(|e| e.to_string())?;
    }

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Accessibility permission may be required: {e:?}"))?;

    enigo
        .key(Key::Meta, Direction::Press)
        .map_err(|e| e.to_string())?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| e.to_string())?;
    enigo
        .key(Key::Meta, Direction::Release)
        .map_err(|e| e.to_string())?;

    Ok(())
}
