use base64::Engine as _;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State, Window};

use crate::tray::{activity_label, api_key_label, TrayState};
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

/// Returns the configured API key for use by the local renderer.
#[tauri::command]
pub fn get_api_key(state: State<'_, AppState>) -> String {
    state.api_key.lock().unwrap().clone()
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

    *state.api_key.lock().unwrap() = key.clone();

    // Update the tray menu label to show the new (masked) key.
    if let Some(tray_state) = app.try_state::<TrayState>() {
        let _ = tray_state.api_key_item.set_text(api_key_label(&key));
    }

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

/// Save the current system output volume and halve it while recording.
#[tauri::command]
pub async fn duck_volume(state: State<'_, AppState>) -> Result<(), String> {
    if !*state.duck_volume_enabled.lock().unwrap() {
        return Ok(());
    }

    let output = tokio::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg("output volume of (get volume settings)")
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let current: u32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(50);

    *state.saved_volume.lock().unwrap() = Some(current);
    let reduced = current / 2;

    tokio::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(format!("set volume output volume {reduced}"))
        .output()
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Restore the system output volume to the value saved before recording started.
/// Idempotent: a no-op when no volume was saved, so it is safe to call from the
/// window-close handler, the Reset action, and the frontend stop path alike.
pub async fn restore_volume_inner(state: &AppState) -> Result<(), String> {
    // Take-and-clear under a single lock so concurrent callers (the window-close
    // handler and the frontend stop path) can't both run the restore — exactly
    // one wins and the other becomes a true no-op.
    let saved = state.saved_volume.lock().unwrap().take();
    if let Some(volume) = saved {
        tokio::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(format!("set volume output volume {volume}"))
            .output()
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn restore_volume(state: State<'_, AppState>) -> Result<(), String> {
    restore_volume_inner(state.inner()).await
}

/// Pause any currently playing media (Music, Spotify, Podcasts) before recording.
#[tauri::command]
pub async fn stop_media(state: State<'_, AppState>) -> Result<(), String> {
    if !*state.stop_media_enabled.lock().unwrap() {
        return Ok(());
    }

    let script = r#"
set wasPlaying to false
if application "Music" is running then
  try
    if player state of application "Music" is playing then
      tell application "Music" to pause
      set wasPlaying to true
    end if
  end try
end if
if application "Spotify" is running then
  try
    if player state of application "Spotify" is playing then
      tell application "Spotify" to pause
      set wasPlaying to true
    end if
  end try
end if
if application "Podcasts" is running then
  try
    if player state of application "Podcasts" is playing then
      tell application "Podcasts" to pause
      set wasPlaying to true
    end if
  end try
end if
return wasPlaying
"#;

    let output = tokio::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let was_playing = String::from_utf8_lossy(&output.stdout).trim() == "true";
    *state.was_media_playing.lock().unwrap() = was_playing;

    Ok(())
}

/// Resume media that was playing before recording started.
/// Idempotent: a no-op when nothing was paused, so it is safe to call from the
/// window-close handler, the Reset action, and the frontend stop path alike.
pub async fn resume_media_inner(state: &AppState) -> Result<(), String> {
    // Take-and-clear atomically (see restore_volume_inner) so the resume
    // AppleScript runs at most once across concurrent callers.
    let was_playing = {
        let mut guard = state.was_media_playing.lock().unwrap();
        std::mem::replace(&mut *guard, false)
    };
    if !was_playing {
        return Ok(());
    }

    let script = r#"
if application "Music" is running then
  try
    tell application "Music" to play
  end try
end if
if application "Spotify" is running then
  try
    tell application "Spotify" to play
  end try
end if
if application "Podcasts" is running then
  try
    tell application "Podcasts" to play
  end try
end if
"#;

    tokio::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn resume_media(state: State<'_, AppState>) -> Result<(), String> {
    resume_media_inner(state.inner()).await
}

/// Update the tray status indicator to reflect the overlay's current activity.
/// `kind` is one of "idle" | "recording" | "reading". Called by the frontend on
/// every state transition so the menu always shows the true current state.
#[tauri::command]
pub fn set_activity(app: AppHandle, kind: String) {
    if let Some(tray_state) = app.try_state::<TrayState>() {
        let _ = tray_state.status_item.set_text(activity_label(&kind));
    }
}

/// Return and clear any shortcut action that was queued while the overlay was
/// being rebuilt. A freshly-rebuilt overlay's WebView isn't listening yet when
/// the triggering shortcut fires, so the action is stashed and the overlay
/// pulls it on mount — making the self-heal recovery work on the first press.
#[tauri::command]
pub fn take_pending_action(state: State<'_, AppState>) -> Option<String> {
    state.pending_action.lock().unwrap().take()
}

/// The cursor's position in the calling window's CSS pixel space — the same
/// coordinates as a DOM `MouseEvent.clientX/Y`, so the frontend can compare it
/// against `getBoundingClientRect()` directly.
///
/// The wolf badge polls this so it can keep following the pointer after it
/// leaves the window: `mousemove` stops at the window edge, and the overlay is
/// a small window sitting in the middle of the desktop. `None` when either
/// position is unavailable rather than reporting a bogus (0, 0).
#[tauri::command]
pub fn cursor_in_window(app: AppHandle, window: Window) -> Option<(f64, f64)> {
    // Both are physical pixels measured from the top-left of the desktop.
    let cursor = app.cursor_position().ok()?;
    let origin = window.outer_position().ok()?;
    let scale = window.scale_factor().unwrap_or(1.0);
    Some((
        (cursor.x - f64::from(origin.x)) / scale,
        (cursor.y - f64::from(origin.y)) / scale,
    ))
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

/// Directory where debug mic recordings are stored:
/// ~/Library/Application Support/{identifier}/debug-recordings.
fn recordings_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("debug-recordings");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Keep only the newest `keep` .wav files so recordings don't grow unbounded.
/// Filenames are timestamped (scribe-YYYY-MM-DD_HH-MM-SS.wav) so lexical order
/// is chronological.
fn prune_recordings(dir: &Path, keep: usize) {
    let mut wavs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("wav"))
        .collect();
    if wavs.len() <= keep {
        return;
    }
    wavs.sort();
    for path in &wavs[..wavs.len() - keep] {
        let _ = std::fs::remove_file(path);
    }
}

/// Persist a debug recording of the exact 16 kHz mono PCM sent to Scribe.
/// The frontend encodes the WAV (byte-identical to what the model received)
/// and ships it here base64-encoded. Returns the saved file path.
#[tauri::command]
pub async fn save_debug_recording(app: AppHandle, wav_data: String) -> Result<String, String> {
    let dir = recordings_dir(&app)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(wav_data.as_bytes())
        .map_err(|e| e.to_string())?;

    let filename = format!(
        "scribe-{}.wav",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let path = dir.join(&filename);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;

    prune_recordings(&dir, 50);

    Ok(path.to_string_lossy().to_string())
}

/// Open the debug-recordings folder in Finder so recordings can be played back.
#[tauri::command]
pub async fn open_recordings_dir(app: AppHandle) -> Result<(), String> {
    let dir = recordings_dir(&app)?;
    tokio::process::Command::new("/usr/bin/open")
        .arg(&dir)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
