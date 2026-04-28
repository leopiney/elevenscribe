use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceInfo {
    pub voice_id: String,
    pub name: String,
}

const PREMADE_VOICE_LIMIT: usize = 5;

fn audio_cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let dir = config_dir.join("audio");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

#[derive(Deserialize)]
struct VoicesResponse {
    voices: Vec<VoiceEntry>,
}

#[derive(Deserialize)]
struct VoiceEntry {
    voice_id: String,
    name: String,
    #[serde(default)]
    category: Option<String>,
}

/// Fetch available voices from ElevenLabs, cache them, return the list.
#[tauri::command]
pub async fn list_voices(state: State<'_, AppState>) -> Result<Vec<VoiceInfo>, String> {
    let key = state.api_key.lock().unwrap().clone();
    if key.is_empty() {
        return Err("No API key configured".to_string());
    }

    let client = reqwest::Client::new();
    let response = client
        .get("https://api.elevenlabs.io/v1/voices")
        .header("xi-api-key", &key)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("ElevenLabs API error {status}: {body}"));
    }

    let data: VoicesResponse = response.json().await.map_err(|e| e.to_string())?;

    let mut premade: Vec<VoiceInfo> = data
        .voices
        .iter()
        .filter(|v| v.category.as_deref() == Some("premade"))
        .take(PREMADE_VOICE_LIMIT)
        .map(|v| VoiceInfo {
            voice_id: v.voice_id.clone(),
            name: v.name.clone(),
        })
        .collect();

    if premade.len() < PREMADE_VOICE_LIMIT {
        let premade_ids: Vec<String> = premade.iter().map(|v| v.voice_id.clone()).collect();
        for v in &data.voices {
            if premade.len() >= PREMADE_VOICE_LIMIT {
                break;
            }
            if !premade_ids.contains(&v.voice_id) {
                premade.push(VoiceInfo {
                    voice_id: v.voice_id.clone(),
                    name: v.name.clone(),
                });
            }
        }
    }

    *state.cached_voices.lock().unwrap() = premade.clone();
    Ok(premade)
}

#[tauri::command]
pub fn get_selected_voice(state: State<'_, AppState>) -> String {
    state.selected_voice_id.lock().unwrap().clone()
}

#[tauri::command]
pub async fn set_selected_voice(
    app: AppHandle,
    state: State<'_, AppState>,
    voice_id: String,
) -> Result<(), String> {
    *state.selected_voice_id.lock().unwrap() = voice_id.clone();

    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let config_path = config_dir.join("config.json");
    let mut config: serde_json::Value = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    config["selected_voice_id"] = serde_json::Value::String(voice_id);
    let contents = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, contents).map_err(|e| e.to_string())?;

    Ok(())
}

/// Persist a single MP3 chunk to disk for later replay from history.
/// The frontend produces the bytes via the streaming TTS SDK and ships them here.
#[tauri::command]
pub async fn cache_tts_chunk(
    app: AppHandle,
    audio_id: String,
    chunk_index: usize,
    mp3_bytes: Vec<u8>,
) -> Result<(), String> {
    let base = audio_cache_dir(&app)?;
    let dir = base.join(&audio_id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("chunk_{chunk_index}.mp3")), &mp3_bytes)
        .map_err(|e| e.to_string())?;
    Ok(())
}
