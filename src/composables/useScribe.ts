import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

// Persistent mic stream — acquired once (from a user gesture) and reused
// across recording sessions so the global shortcut flow works without
// needing a fresh getUserMedia call (which requires user activation).
let persistentStream: MediaStream | null = null;

export function useScribe() {
  const partialText = ref("");
  const committedText = ref("");
  const micLabel = ref("");
  const error = ref("");
  const audioLevel = ref(0); // 0–1 RMS, updated per chunk for the level meter
  const needsMicPermission = ref(false);

  let ws: WebSocket | null = null;
  let audioContext: AudioContext | null = null;
  let processor: ScriptProcessorNode | null = null;

  /** Acquire the microphone. Call this from a click handler (user gesture). */
  async function acquireMic(): Promise<MediaStream> {
    if (persistentStream && persistentStream.getTracks().every((t) => t.readyState === "live")) {
      return persistentStream;
    }

    console.log("[scribe] requesting microphone…");
    persistentStream = await navigator.mediaDevices.getUserMedia({
      audio: {
        sampleRate: 16000,
        channelCount: 1,
        echoCancellation: true,
        noiseSuppression: true,
      },
    });

    const tracks = persistentStream.getTracks();
    micLabel.value = tracks[0]?.label ?? "";
    console.log(
      "[scribe] mic tracks:",
      tracks.map((t) => ({ label: t.label, state: t.readyState }))
    );
    needsMicPermission.value = false;
    return persistentStream;
  }

  async function start() {
    error.value = "";

    // ── 0. Ensure we have a mic stream ────────────────────────────────────
    let stream: MediaStream;
    try {
      stream = await acquireMic();
    } catch (e) {
      // getUserMedia failed — likely no user activation. Signal the UI
      // to show a "Grant mic" button so the user can click it.
      console.warn("[scribe] getUserMedia failed, need user gesture:", e);
      needsMicPermission.value = true;
      throw e;
    }

    await invoke("duck_volume").catch((e) => console.warn("[scribe] duck_volume failed:", e));
    await invoke("stop_media").catch((e) => console.warn("[scribe] stop_media failed:", e));

    // ── 1. Get ephemeral token ──────────────────────────────────────────────
    console.log("[scribe] fetching token…");
    const token = await invoke<string>("get_scribe_token");
    console.log("[scribe] token received:", token.slice(0, 8) + "…");

    // ── 2. Open WebSocket ───────────────────────────────────────────────────
    const url = `wss://api.elevenlabs.io/v1/speech-to-text/realtime?token=${encodeURIComponent(token)}&model_id=scribe_v2_realtime&commit_strategy=vad`;
    console.log("[scribe] connecting WebSocket…");
    ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";

    await new Promise<void>((resolve, reject) => {
      ws!.onopen = () => {
        console.log("[scribe] WebSocket opened");
        resolve();
      };
      ws!.onerror = (e) => {
        console.error("[scribe] WebSocket handshake error:", e);
        reject(new Error("WebSocket connection failed"));
      };
    });

    // Log every message from the server (transcripts, session events, errors)
    ws.onmessage = (event) => {
      console.log("[scribe] ←", event.data);
      try {
        const data = JSON.parse(event.data as string) as {
          message_type: string;
          text?: string;
        };
        if (data.message_type === "partial_transcript" && data.text !== undefined) {
          partialText.value = data.text;
        } else if (
          (data.message_type === "committed_transcript" ||
            data.message_type === "committed_transcript_with_timestamps") &&
          data.text !== undefined
        ) {
          committedText.value += data.text + " ";
          partialText.value = "";
        }
      } catch {
        // ignore malformed messages
      }
    };

    ws.onclose = (e) => {
      console.warn("[scribe] WebSocket closed — code:", e.code, "reason:", e.reason);
    };

    ws.onerror = (e) => {
      console.error("[scribe] WebSocket error:", e);
      error.value = "Connection error — see console";
    };

    // ── 3. Build audio pipeline from persistent stream ────────────────────
    audioContext = new AudioContext({ sampleRate: 16000 });
    console.log(
      "[scribe] AudioContext state:",
      audioContext.state,
      "/ actual sampleRate:",
      audioContext.sampleRate
    );

    // AudioContext can start suspended on some WebViews — resume it explicitly
    if (audioContext.state === "suspended") {
      await audioContext.resume();
      console.log("[scribe] AudioContext resumed");
    }

    const source = audioContext.createMediaStreamSource(stream);
    // 4096 samples @ 16 kHz ≈ 256 ms per chunk
    processor = audioContext.createScriptProcessor(4096, 1, 1);

    let chunksSent = 0;

    processor.onaudioprocess = (e) => {
      if (!ws || ws.readyState !== WebSocket.OPEN) return;

      const float32 = e.inputBuffer.getChannelData(0);

      // Compute RMS for the level meter
      let sum = 0;
      for (let i = 0; i < float32.length; i++) sum += float32[i] * float32[i];
      const rms = Math.sqrt(sum / float32.length);
      audioLevel.value = Math.min(1, rms * 8); // scale up for visibility

      const int16 = float32ToInt16(float32);
      const audio = arrayBufferToBase64(int16.buffer);

      // Log first 3 chunks and then every 50th to avoid flooding
      if (chunksSent < 3 || chunksSent % 50 === 0) {
        console.log(
          `[scribe] → chunk #${chunksSent} | RMS: ${rms.toFixed(4)} | bytes: ${int16.byteLength}`
        );
      }
      chunksSent++;

      ws.send(
        JSON.stringify({
          message_type: "input_audio_chunk",
          audio_base_64: audio,
        })
      );
    };

    source.connect(processor);
    processor.connect(audioContext.destination);
    console.log("[scribe] audio pipeline active");
  }

  async function stop() {
    console.log("[scribe] stopping…");
    await invoke("restore_volume").catch((e) => console.warn("[scribe] restore_volume failed:", e));
    await invoke("resume_media").catch((e) => console.warn("[scribe] resume_media failed:", e));

    // Disconnect audio pipeline but keep the persistent mic stream alive
    processor?.disconnect();
    processor = null;

    await audioContext?.close().catch(() => undefined);
    audioContext = null;

    audioLevel.value = 0;

    // Fold any pending partial transcript into committed text so the
    // clipboard write that follows always contains the full dictation.
    if (partialText.value.trim()) {
      committedText.value += partialText.value + " ";
      partialText.value = "";
    }

    // Clear all handlers before closing so spurious error/close events
    // from the abrupt teardown don't surface as user-visible errors
    if (ws) {
      ws.onmessage = null;
      ws.onerror = null;
      ws.onclose = null;
      ws.close();
      ws = null;
    }
  }

  function float32ToInt16(float32: Float32Array): Int16Array {
    const int16 = new Int16Array(float32.length);
    for (let i = 0; i < float32.length; i++) {
      const s = Math.max(-1, Math.min(1, float32[i]));
      int16[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
    }
    return int16;
  }

  function arrayBufferToBase64(buffer: ArrayBuffer): string {
    const bytes = new Uint8Array(buffer);
    let binary = "";
    for (let i = 0; i < bytes.byteLength; i++) {
      binary += String.fromCharCode(bytes[i]);
    }
    return btoa(binary);
  }

  return {
    partialText,
    committedText,
    micLabel,
    error,
    audioLevel,
    needsMicPermission,
    start,
    stop,
    acquireMic,
  };
}
