use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::watch;

use crate::AppState;

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceInfo {
    pub voice_id: String,
    pub name: String,
}

#[derive(Clone, Serialize)]
struct TtsChunkInfo {
    index: usize,
    total: usize,
    preview: String,
}

#[derive(Clone, Serialize)]
struct TtsAudioData {
    chunk_index: usize,
    data: String, // base64-encoded MP3
    is_last_chunk: bool,
}

// ── Text chunking ─────────────────────────────────────────────────────────────

const MAX_CHUNK_CHARS: usize = 4000;

/// Split text into chunks suitable for the TTS API.
/// First splits by paragraph (\n\n), then by sentence if a paragraph is too long,
/// then by word boundary as a last resort.
fn chunk_text(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![];
    }
    if text.len() <= MAX_CHUNK_CHARS {
        return vec![text.to_string()];
    }

    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    let mut chunks: Vec<String> = Vec::new();

    for para in paragraphs {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if para.len() <= MAX_CHUNK_CHARS {
            chunks.push(para.to_string());
        } else {
            // Split long paragraph by sentences
            chunks.extend(split_by_sentences(para));
        }
    }

    chunks
}

fn split_by_sentences(text: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    // Split on sentence-ending punctuation followed by whitespace
    let mut chars = text.char_indices().peekable();
    let mut last_split = 0;

    while let Some(&(i, c)) = chars.peek() {
        chars.next();
        if (c == '.' || c == '!' || c == '?') && i + c.len_utf8() < text.len() {
            let next_char = text[i + c.len_utf8()..].chars().next();
            if next_char == Some(' ') || next_char == Some('\n') {
                let sentence = &text[last_split..=i + c.len_utf8()]; // include the space
                let sentence = sentence.trim_end();
                if current.len() + sentence.len() + 1 > MAX_CHUNK_CHARS {
                    if !current.is_empty() {
                        chunks.push(current.trim().to_string());
                        current = String::new();
                    }
                    if sentence.len() > MAX_CHUNK_CHARS {
                        chunks.extend(split_by_words(sentence));
                    } else {
                        current.push_str(sentence);
                    }
                } else {
                    if !current.is_empty() {
                        current.push(' ');
                    }
                    current.push_str(sentence);
                }
                // Skip past the delimiter space/newline
                if let Some(&(ni, _)) = chars.peek() {
                    last_split = ni;
                }
            }
        }
    }

    // Remaining text after last sentence boundary
    let remainder = text[last_split..].trim();
    if !remainder.is_empty() {
        if current.len() + remainder.len() + 1 > MAX_CHUNK_CHARS {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
            }
            if remainder.len() > MAX_CHUNK_CHARS {
                chunks.extend(split_by_words(remainder));
            } else {
                chunks.push(remainder.to_string());
            }
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(remainder);
            chunks.push(current.trim().to_string());
        }
    } else if !current.is_empty() {
        chunks.push(current.trim().to_string());
    }

    chunks
}

fn split_by_words(text: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > MAX_CHUNK_CHARS {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
            }
            current = word.to_string();
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        chunks.push(current.trim().to_string());
    }

    chunks
}

fn chunk_preview(text: &str) -> String {
    let trimmed: String = text.chars().take(80).collect();
    if text.len() > 80 {
        format!("{trimmed}…")
    } else {
        trimmed
    }
}

/// Return the tail of a chunk (last ~200 chars) for use as previous_text context.
fn context_tail(text: &str) -> String {
    if text.len() <= 200 {
        text.to_string()
    } else {
        text[text.len() - 200..].to_string()
    }
}

/// Return the head of a chunk (first ~200 chars) for use as next_text context.
fn context_head(text: &str) -> String {
    text.chars().take(200).collect()
}

// ── ElevenLabs API helpers ────────────────────────────────────────────────────

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

// ── Tauri commands ────────────────────────────────────────────────────────────

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

    // Prefer "premade" voices, take first 5
    let mut premade: Vec<VoiceInfo> = data
        .voices
        .iter()
        .filter(|v| v.category.as_deref() == Some("premade"))
        .take(5)
        .map(|v| VoiceInfo {
            voice_id: v.voice_id.clone(),
            name: v.name.clone(),
        })
        .collect();

    // If fewer than 5 premade, fill from the rest
    if premade.len() < 5 {
        let premade_ids: Vec<String> = premade.iter().map(|v| v.voice_id.clone()).collect();
        for v in &data.voices {
            if premade.len() >= 5 {
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

    // Persist to config.json
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

/// Start TTS playback for the given text. Chunks it, streams each chunk from
/// ElevenLabs, and emits audio data events back to the frontend.
#[tauri::command]
pub async fn start_tts(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> Result<(), String> {
    let key = state.api_key.lock().unwrap().clone();
    if key.is_empty() {
        return Err("No API key configured".to_string());
    }

    let voice_id = {
        let vid = state.selected_voice_id.lock().unwrap().clone();
        if vid.is_empty() {
            // Default to "Rachel" voice
            "21m00Tcm4TlvDq8ikWAM".to_string()
        } else {
            vid
        }
    };

    // Cancel any existing TTS session
    {
        let mut cancel = state.tts_cancel.lock().unwrap();
        if let Some(sender) = cancel.take() {
            let _ = sender.send(true);
        }
    }

    let chunks = chunk_text(&text);
    if chunks.is_empty() {
        return Err("Clipboard is empty".to_string());
    }

    // Create cancellation channel
    let (cancel_tx, cancel_rx) = watch::channel(false);
    *state.tts_cancel.lock().unwrap() = Some(cancel_tx);

    // Emit chunk metadata
    let chunk_infos: Vec<TtsChunkInfo> = chunks
        .iter()
        .enumerate()
        .map(|(i, c)| TtsChunkInfo {
            index: i,
            total: chunks.len(),
            preview: chunk_preview(c),
        })
        .collect();

    let _ = app.emit("tts-chunks-ready", &chunk_infos);

    // Spawn the streaming task
    let app_handle = app.clone();
    let chunks_clone = chunks.clone();
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let total = chunks_clone.len();

        for (i, chunk) in chunks_clone.iter().enumerate() {
            // Check cancellation
            if *cancel_rx.borrow() {
                let _ = app_handle.emit("tts-stopped", ());
                return;
            }

            // Emit which chunk is starting
            let _ = app_handle.emit(
                "tts-chunk-started",
                serde_json::json!({ "index": i, "total": total, "preview": chunk_preview(chunk) }),
            );

            // Build request body with context for smooth transitions
            let mut body = serde_json::json!({
                "text": chunk,
                "model_id": "eleven_multilingual_v2",
            });

            if i > 0 {
                body["previous_text"] =
                    serde_json::Value::String(context_tail(&chunks_clone[i - 1]));
            }
            if i + 1 < total {
                body["next_text"] = serde_json::Value::String(context_head(&chunks_clone[i + 1]));
            }

            let url = format!(
                "https://api.elevenlabs.io/v1/text-to-speech/{}/stream?output_format=mp3_44100_128",
                voice_id
            );

            let result = client
                .post(&url)
                .header("xi-api-key", &key)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            match result {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let err_body = response.text().await.unwrap_or_default();
                        let _ = app_handle.emit(
                            "tts-error",
                            serde_json::json!({ "message": format!("TTS API error {status}: {err_body}") }),
                        );
                        return;
                    }

                    // Accumulate the full audio response for this chunk
                    let bytes = match response.bytes().await {
                        Ok(b) => b,
                        Err(e) => {
                            let _ = app_handle.emit(
                                "tts-error",
                                serde_json::json!({ "message": format!("Stream read error: {e}") }),
                            );
                            return;
                        }
                    };

                    // Check cancellation before emitting
                    if *cancel_rx.borrow() {
                        let _ = app_handle.emit("tts-stopped", ());
                        return;
                    }

                    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);

                    let _ = app_handle.emit(
                        "tts-audio-data",
                        TtsAudioData {
                            chunk_index: i,
                            data: encoded,
                            is_last_chunk: i + 1 == total,
                        },
                    );
                }
                Err(e) => {
                    let _ = app_handle.emit(
                        "tts-error",
                        serde_json::json!({ "message": format!("Request failed: {e}") }),
                    );
                    return;
                }
            }
        }

        let _ = app_handle.emit("tts-complete", ());
    });

    Ok(())
}

/// Cancel active TTS streaming.
#[tauri::command]
pub fn stop_tts(app: AppHandle, state: State<'_, AppState>) {
    let mut cancel = state.tts_cancel.lock().unwrap();
    if let Some(sender) = cancel.take() {
        let _ = sender.send(true);
    }
    let _ = app.emit("tts-stopped", ());
}

/// Re-start TTS from a specific chunk index. The frontend passes the full text
/// so we can re-chunk and start from the right offset.
#[tauri::command]
pub async fn skip_to_chunk(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
    chunk_index: usize,
) -> Result<(), String> {
    let key = state.api_key.lock().unwrap().clone();
    if key.is_empty() {
        return Err("No API key configured".to_string());
    }

    let voice_id = {
        let vid = state.selected_voice_id.lock().unwrap().clone();
        if vid.is_empty() {
            "21m00Tcm4TlvDq8ikWAM".to_string()
        } else {
            vid
        }
    };

    // Cancel current
    {
        let mut cancel = state.tts_cancel.lock().unwrap();
        if let Some(sender) = cancel.take() {
            let _ = sender.send(true);
        }
    }

    let chunks = chunk_text(&text);
    if chunk_index >= chunks.len() {
        return Err("Chunk index out of bounds".to_string());
    }

    let (cancel_tx, cancel_rx) = watch::channel(false);
    *state.tts_cancel.lock().unwrap() = Some(cancel_tx);

    // Emit full chunk metadata so frontend stays in sync
    let chunk_infos: Vec<TtsChunkInfo> = chunks
        .iter()
        .enumerate()
        .map(|(i, c)| TtsChunkInfo {
            index: i,
            total: chunks.len(),
            preview: chunk_preview(c),
        })
        .collect();
    let _ = app.emit("tts-chunks-ready", &chunk_infos);

    let app_handle = app.clone();
    let chunks_clone = chunks.clone();
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let total = chunks_clone.len();

        for i in chunk_index..total {
            if *cancel_rx.borrow() {
                let _ = app_handle.emit("tts-stopped", ());
                return;
            }

            let chunk = &chunks_clone[i];

            let _ = app_handle.emit(
                "tts-chunk-started",
                serde_json::json!({ "index": i, "total": total, "preview": chunk_preview(chunk) }),
            );

            let mut body = serde_json::json!({
                "text": chunk,
                "model_id": "eleven_multilingual_v2",
            });

            if i > 0 {
                body["previous_text"] =
                    serde_json::Value::String(context_tail(&chunks_clone[i - 1]));
            }
            if i + 1 < total {
                body["next_text"] = serde_json::Value::String(context_head(&chunks_clone[i + 1]));
            }

            let url = format!(
                "https://api.elevenlabs.io/v1/text-to-speech/{}/stream?output_format=mp3_44100_128",
                voice_id
            );

            let result = client
                .post(&url)
                .header("xi-api-key", &key)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            match result {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let err_body = response.text().await.unwrap_or_default();
                        let _ = app_handle.emit(
                            "tts-error",
                            serde_json::json!({ "message": format!("TTS API error {status}: {err_body}") }),
                        );
                        return;
                    }

                    let bytes = match response.bytes().await {
                        Ok(b) => b,
                        Err(e) => {
                            let _ = app_handle.emit(
                                "tts-error",
                                serde_json::json!({ "message": format!("Stream read error: {e}") }),
                            );
                            return;
                        }
                    };

                    if *cancel_rx.borrow() {
                        let _ = app_handle.emit("tts-stopped", ());
                        return;
                    }

                    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let _ = app_handle.emit(
                        "tts-audio-data",
                        TtsAudioData {
                            chunk_index: i,
                            data: encoded,
                            is_last_chunk: i + 1 == total,
                        },
                    );
                }
                Err(e) => {
                    let _ = app_handle.emit(
                        "tts-error",
                        serde_json::json!({ "message": format!("Request failed: {e}") }),
                    );
                    return;
                }
            }
        }

        let _ = app_handle.emit("tts-complete", ());
    });

    Ok(())
}

/// Hide the read-aloud window.
#[tauri::command]
pub async fn hide_readaloud(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("readaloud") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Show the read-aloud window.
#[tauri::command]
pub async fn show_readaloud(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("readaloud") {
        window.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}
