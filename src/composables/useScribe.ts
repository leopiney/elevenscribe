import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

// Active mic stream — held only while a session is running, then released
// in stop() so the macOS mic indicator turns off when not transcribing.
// Once OS-level mic permission is granted, getUserMedia works for the
// global-shortcut flow without needing fresh user activation each time.
let persistentStream: MediaStream | null = null;

// Sample rate we send to ElevenLabs Scribe. The AudioWorklet resamples the
// mic (whatever native rate the WebView gives us) down to exactly this.
const TARGET_SAMPLE_RATE = 16000;

export function useScribe() {
  const partialText = ref("");
  const committedText = ref("");
  const micLabel = ref("");
  const error = ref("");
  const audioLevel = ref(0); // 0–1 RMS, updated per chunk for the level meter
  const needsMicPermission = ref(false);
  // Path of the last saved debug .wav — lets the caller surface where the exact
  // audio we sent to the model was stored, for listening back / validation.
  const lastRecordingPath = ref("");

  let ws: WebSocket | null = null;
  let audioContext: AudioContext | null = null;
  let source: MediaStreamAudioSourceNode | null = null;
  let workletNode: AudioWorkletNode | null = null;

  // Every PCM16 chunk we actually send, kept so stop() can write a .wav that is
  // a byte-exact copy of what the model received.
  let recordedChunks: Int16Array[] = [];

  /** True only while a real recording session is live (WebSocket open). Lets
   * callers distinguish an actual session from an `isRecording` flag left
   * stuck-true by an interrupted session. */
  function isLive(): boolean {
    return ws !== null && ws.readyState === WebSocket.OPEN;
  }

  /** Acquire the microphone. Call this from a click handler (user gesture). */
  async function acquireMic(): Promise<MediaStream> {
    if (persistentStream && persistentStream.getTracks().every((t) => t.readyState === "live")) {
      return persistentStream;
    }

    console.log("[scribe] requesting microphone…");
    // Raw audio only. echoCancellation / noiseSuppression / autoGainControl are
    // browser DSP tuned for conferencing — for dictation they gate quiet
    // syllables and clip word onsets, dropping speech before it ever reaches
    // Scribe. The ElevenLabs demo captures raw audio; so do we now. We also
    // don't pin sampleRate here — the AudioWorklet resamples to 16 kHz, so the
    // capture rate no longer has to be guessed/honored by the OS.
    persistentStream = await navigator.mediaDevices.getUserMedia({
      audio: {
        channelCount: 1,
        echoCancellation: false,
        noiseSuppression: false,
        autoGainControl: false,
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

    try {
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

      // ── 3. Build audio pipeline from the active stream ────────────────────
      // Let the context use its native rate (most reliable); the worklet
      // resamples to 16 kHz. Forcing a rate here is what could silently fail
      // and ship wrong-rate audio, so we no longer do it.
      audioContext = new AudioContext();
      console.log("[scribe] AudioContext sampleRate:", audioContext.sampleRate);

      // AudioContext can start suspended on some WebViews — resume it explicitly
      if (audioContext.state === "suspended") {
        await audioContext.resume();
        console.log("[scribe] AudioContext resumed");
      }

      // Load the capture worklet (served from public/ at the app root).
      await audioContext.audioWorklet.addModule("/scribe-processor.js");

      recordedChunks = [];
      lastRecordingPath.value = "";
      let chunksSent = 0;

      source = audioContext.createMediaStreamSource(stream);
      workletNode = new AudioWorkletNode(audioContext, "scribe-processor", {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        channelCount: 1,
      });

      workletNode.port.onmessage = (e: MessageEvent) => {
        const { pcm, rms } = e.data as { pcm: Int16Array; rms: number };

        audioLevel.value = Math.min(1, rms * 8); // scale up for visibility

        // Keep a copy for the byte-exact debug recording.
        recordedChunks.push(pcm);

        if (!ws || ws.readyState !== WebSocket.OPEN) return;

        const audio = int16ToBase64(pcm);

        // Log first 3 chunks and then every 50th to avoid flooding
        if (chunksSent < 3 || chunksSent % 50 === 0) {
          console.log(
            `[scribe] → chunk #${chunksSent} | RMS: ${rms.toFixed(4)} | bytes: ${pcm.byteLength}`
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

      // Connecting the worklet to the destination keeps `process()` pulled by
      // the graph. It writes no output, so this emits silence (no feedback).
      source.connect(workletNode);
      workletNode.connect(audioContext.destination);
      console.log("[scribe] audio pipeline active (AudioWorklet @ 16 kHz)");
    } catch (e) {
      // Release the mic and any partial pipeline so the macOS indicator
      // doesn't linger if start fails after acquireMic.
      await stop();
      throw e;
    }
  }

  async function stop() {
    console.log("[scribe] stopping…");
    await invoke("restore_volume").catch((e) => console.warn("[scribe] restore_volume failed:", e));
    await invoke("resume_media").catch((e) => console.warn("[scribe] resume_media failed:", e));

    // Flush the worklet's final partial chunk, then tear the pipeline down.
    if (workletNode) {
      try {
        workletNode.port.postMessage("flush");
        // Give the flushed chunk a beat to arrive on the message port so it
        // makes it into the recording (and the last WS send).
        await new Promise((r) => setTimeout(r, 40));
      } catch {
        /* ignore */
      }
      workletNode.port.onmessage = null;
      workletNode.disconnect();
      workletNode = null;
    }

    source?.disconnect();
    source = null;

    await audioContext?.close().catch(() => undefined);
    audioContext = null;

    // Stop the mic tracks so macOS turns off its capture indicator.
    if (persistentStream) {
      for (const track of persistentStream.getTracks()) track.stop();
      persistentStream = null;
    }

    audioLevel.value = 0;

    // Persist the exact PCM we sent as a 16 kHz mono .wav so it can be played
    // back to confirm the audio the model received is clean.
    await saveDebugRecording();

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

  /** Encode the recorded PCM as a WAV and hand it to Rust to store on disk. */
  async function saveDebugRecording() {
    const chunks = recordedChunks;
    recordedChunks = [];
    const totalSamples = chunks.reduce((n, c) => n + c.length, 0);
    if (totalSamples === 0) return;

    try {
      const wav = encodeWav(chunks, totalSamples, TARGET_SAMPLE_RATE);
      const wavData = uint8ToBase64(wav);
      const path = await invoke<string>("save_debug_recording", { wavData });
      lastRecordingPath.value = path;
      const seconds = (totalSamples / TARGET_SAMPLE_RATE).toFixed(1);
      console.log(`[scribe] 🎙️ saved ${seconds}s debug recording → ${path}`);
    } catch (e) {
      console.error("[scribe] failed to save debug recording:", e);
    }
  }

  return {
    partialText,
    committedText,
    micLabel,
    error,
    audioLevel,
    needsMicPermission,
    lastRecordingPath,
    start,
    stop,
    acquireMic,
    isLive,
  };
}

/** Build a 16-bit PCM mono WAV (44-byte header + samples) from Int16 chunks. */
function encodeWav(chunks: Int16Array[], totalSamples: number, sampleRate: number): Uint8Array {
  const dataBytes = totalSamples * 2;
  const buffer = new ArrayBuffer(44 + dataBytes);
  const view = new DataView(buffer);

  const writeStr = (off: number, s: string) => {
    for (let i = 0; i < s.length; i++) view.setUint8(off + i, s.charCodeAt(i));
  };

  writeStr(0, "RIFF");
  view.setUint32(4, 36 + dataBytes, true);
  writeStr(8, "WAVE");
  writeStr(12, "fmt ");
  view.setUint32(16, 16, true); // fmt chunk size
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true); // byte rate (mono, 2 bytes/sample)
  view.setUint16(32, 2, true); // block align
  view.setUint16(34, 16, true); // bits per sample
  writeStr(36, "data");
  view.setUint32(40, dataBytes, true);

  let off = 44;
  for (const c of chunks) {
    for (let i = 0; i < c.length; i++) {
      view.setInt16(off, c[i], true);
      off += 2;
    }
  }
  return new Uint8Array(buffer);
}

function int16ToBase64(pcm: Int16Array): string {
  return uint8ToBase64(new Uint8Array(pcm.buffer, pcm.byteOffset, pcm.byteLength));
}

function uint8ToBase64(bytes: Uint8Array): string {
  let binary = "";
  const CHUNK = 0x8000; // avoid arg-count limits on String.fromCharCode
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}
