import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
// Import the base client directly to avoid the package's wrapper barrel,
// which imports node-only modules (node:stream, node:child_process) for
// helpers like play() that we don't use.
import { ElevenLabsClient } from "@elevenlabs/elevenlabs-js/Client";

const MAX_CHUNK_CHARS = 4000;
const PREVIEW_CHARS = 80;
const CONTEXT_CHARS = 200;
const DEFAULT_MODEL_ID = "eleven_multilingual_v2";
const DEFAULT_OUTPUT_FORMAT = "mp3_44100_128";
const FALLBACK_VOICE_ID = "21m00Tcm4TlvDq8ikWAM";
const MEDIA_SOURCE_MIME = "audio/mpeg";

interface ChunkInfo {
  index: number;
  preview: string;
  // start time in <audio>.currentTime once buffered (set after stream completes)
  startSeconds: number;
}

export function useReadAloud() {
  const isPlaying = ref(false);
  const isPaused = ref(false);
  const isLoading = ref(false);
  const currentChunkIndex = ref(0);
  const totalChunks = ref(0);
  const chunkPreview = ref("");
  const error = ref("");

  let client: ElevenLabsClient | null = null;
  let audioEl: HTMLAudioElement | null = null;
  let mediaSource: MediaSource | null = null;
  let sourceBuffer: SourceBuffer | null = null;
  let abortController: AbortController | null = null;
  let chunks: string[] = [];
  let chunkInfos: ChunkInfo[] = [];
  let currentAudioId = "";
  let appendQueue: Uint8Array[] = [];
  let timeUpdateHandler: (() => void) | null = null;
  let endedHandler: (() => void) | null = null;

  function chunkText(text: string): string[] {
    const trimmed = text.trim();
    if (!trimmed) return [];
    if (trimmed.length <= MAX_CHUNK_CHARS) return [trimmed];

    const out: string[] = [];
    for (const para of trimmed.split("\n\n")) {
      const p = para.trim();
      if (!p) continue;
      if (p.length <= MAX_CHUNK_CHARS) {
        out.push(p);
      } else {
        out.push(...splitBySentences(p));
      }
    }
    return out;
  }

  function splitBySentences(text: string): string[] {
    const out: string[] = [];
    let current = "";
    const sentences = text.match(/[^.!?]+[.!?]+(?:\s|$)|[^.!?]+$/g) || [text];

    for (const raw of sentences) {
      const s = raw.trim();
      if (!s) continue;
      if (current.length + s.length + 1 > MAX_CHUNK_CHARS) {
        if (current) {
          out.push(current.trim());
          current = "";
        }
        if (s.length > MAX_CHUNK_CHARS) {
          out.push(...splitByWords(s));
        } else {
          current = s;
        }
      } else {
        current = current ? `${current} ${s}` : s;
      }
    }
    if (current) out.push(current.trim());
    return out;
  }

  function splitByWords(text: string): string[] {
    const out: string[] = [];
    let current = "";
    for (const word of text.split(/\s+/)) {
      if (!word) continue;
      if (current.length + word.length + 1 > MAX_CHUNK_CHARS) {
        if (current) out.push(current.trim());
        current = word;
      } else {
        current = current ? `${current} ${word}` : word;
      }
    }
    if (current) out.push(current.trim());
    return out;
  }

  function preview(text: string): string {
    if (text.length <= PREVIEW_CHARS) return text;
    return `${text.slice(0, PREVIEW_CHARS)}…`;
  }

  function tail(text: string): string {
    return text.length <= CONTEXT_CHARS ? text : text.slice(text.length - CONTEXT_CHARS);
  }

  function head(text: string): string {
    return text.slice(0, CONTEXT_CHARS);
  }

  async function ensureClient(): Promise<ElevenLabsClient> {
    if (client) return client;
    const apiKey = await invoke<string>("get_api_key");
    if (!apiKey) throw new Error("No API key configured");
    client = new ElevenLabsClient({ apiKey });
    return client;
  }

  function setupMediaPipeline() {
    audioEl = new Audio();
    audioEl.autoplay = true;
    mediaSource = new MediaSource();
    audioEl.src = URL.createObjectURL(mediaSource);

    return new Promise<void>((resolve, reject) => {
      if (!mediaSource) return reject(new Error("MediaSource not initialized"));
      mediaSource.addEventListener(
        "sourceopen",
        () => {
          if (!mediaSource) return reject(new Error("MediaSource closed"));
          try {
            sourceBuffer = mediaSource.addSourceBuffer(MEDIA_SOURCE_MIME);
            sourceBuffer.mode = "sequence";
            sourceBuffer.addEventListener("updateend", drainQueue);
            resolve();
          } catch (e) {
            reject(e);
          }
        },
        { once: true }
      );
    });
  }

  function drainQueue() {
    if (!sourceBuffer || sourceBuffer.updating) return;
    const next = appendQueue.shift();
    if (next) {
      try {
        sourceBuffer.appendBuffer(next);
      } catch (e) {
        console.error("[readaloud] appendBuffer failed:", e);
      }
    }
  }

  function appendBytes(bytes: Uint8Array) {
    appendQueue.push(bytes);
    drainQueue();
  }

  function attachTimeTracking() {
    if (!audioEl) return;
    timeUpdateHandler = () => {
      if (!audioEl) return;
      const t = audioEl.currentTime;
      // Find the latest chunk whose startSeconds <= t
      let idx = 0;
      for (let i = 0; i < chunkInfos.length; i++) {
        if (chunkInfos[i].startSeconds <= t) idx = i;
        else break;
      }
      if (idx !== currentChunkIndex.value) {
        currentChunkIndex.value = idx;
        chunkPreview.value = chunkInfos[idx]?.preview ?? "";
      }
    };
    endedHandler = () => {
      isPlaying.value = false;
      isPaused.value = false;
      isLoading.value = false;
    };
    audioEl.addEventListener("timeupdate", timeUpdateHandler);
    audioEl.addEventListener("ended", endedHandler);
  }

  async function streamChunk(chunkIndex: number, signal: AbortSignal) {
    const c = await ensureClient();
    const text = chunks[chunkIndex];
    const voiceId = await getVoiceId();

    const request: {
      text: string;
      modelId: string;
      outputFormat: typeof DEFAULT_OUTPUT_FORMAT;
      previousText?: string;
      nextText?: string;
    } = {
      text,
      modelId: DEFAULT_MODEL_ID,
      outputFormat: DEFAULT_OUTPUT_FORMAT,
    };
    if (chunkIndex > 0) request.previousText = tail(chunks[chunkIndex - 1]);
    if (chunkIndex + 1 < chunks.length) request.nextText = head(chunks[chunkIndex + 1]);

    const stream = await c.textToSpeech.stream(voiceId, request, { abortSignal: signal });

    const reader = stream.getReader();
    const collected: Uint8Array[] = [];
    let firstByte = true;

    while (true) {
      if (signal.aborted) {
        try {
          await reader.cancel();
        } catch {
          /* ignore */
        }
        return;
      }
      const { value, done } = await reader.read();
      if (done) break;
      if (value && value.byteLength > 0) {
        collected.push(value);
        appendBytes(value);
        if (firstByte) {
          firstByte = false;
          isLoading.value = false;
          // Trigger play once we have data buffered
          if (audioEl && audioEl.paused && !isPaused.value) {
            audioEl.play().catch((e) => console.error("[readaloud] play() failed:", e));
          }
        }
      }
    }

    // Persist the full chunk MP3 to disk for history replay
    const total = collected.reduce((n, b) => n + b.byteLength, 0);
    const merged = new Uint8Array(total);
    let off = 0;
    for (const b of collected) {
      merged.set(b, off);
      off += b.byteLength;
    }
    invoke("cache_tts_chunk", {
      audioId: currentAudioId,
      chunkIndex,
      mp3Bytes: Array.from(merged),
    }).catch((e) => console.error("[readaloud] cache_tts_chunk failed:", e));

    // Record where this chunk ends so we know where the *next* one starts
    if (sourceBuffer && sourceBuffer.buffered.length > 0) {
      // Wait for any pending appends to drain so buffered.end() reflects this chunk
      await waitForQueueDrain();
      const endTime = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
      const next = chunkInfos[chunkIndex + 1];
      if (next) next.startSeconds = endTime;
    }
  }

  function waitForQueueDrain(): Promise<void> {
    return new Promise((resolve) => {
      const check = () => {
        if (appendQueue.length === 0 && sourceBuffer && !sourceBuffer.updating) {
          resolve();
          return;
        }
        setTimeout(check, 20);
      };
      check();
    });
  }

  async function getVoiceId(): Promise<string> {
    try {
      const id = await invoke<string>("get_selected_voice");
      return id || FALLBACK_VOICE_ID;
    } catch {
      return FALLBACK_VOICE_ID;
    }
  }

  async function start(text: string): Promise<string> {
    error.value = "";
    await stop();

    chunks = chunkText(text);
    if (chunks.length === 0) {
      error.value = "Clipboard is empty";
      throw new Error(error.value);
    }

    chunkInfos = chunks.map((c, i) => ({
      index: i,
      preview: preview(c),
      startSeconds: 0,
    }));

    totalChunks.value = chunks.length;
    currentChunkIndex.value = 0;
    chunkPreview.value = chunkInfos[0].preview;
    isPlaying.value = true;
    isPaused.value = false;
    isLoading.value = true;
    currentAudioId = Date.now().toString();

    abortController = new AbortController();
    const { signal } = abortController;

    try {
      await setupMediaPipeline();
      attachTimeTracking();
    } catch (e) {
      error.value = `Failed to initialize audio: ${e}`;
      isPlaying.value = false;
      isLoading.value = false;
      throw e;
    }

    (async () => {
      try {
        for (let i = 0; i < chunks.length; i++) {
          if (signal.aborted) return;
          await streamChunk(i, signal);
        }
        // All bytes appended — close MediaSource so 'ended' fires when playback finishes
        await waitForQueueDrain();
        if (mediaSource && mediaSource.readyState === "open") {
          try {
            mediaSource.endOfStream();
          } catch {
            /* ignore */
          }
        }
      } catch (e: unknown) {
        if (signal.aborted) return;
        const msg = e instanceof Error ? e.message : String(e);
        console.error("[readaloud] stream error:", e);
        error.value = msg;
        isPlaying.value = false;
        isLoading.value = false;
      }
    })();

    return currentAudioId;
  }

  async function stop() {
    isPlaying.value = false;
    isPaused.value = false;
    isLoading.value = false;

    if (abortController) {
      abortController.abort();
      abortController = null;
    }

    if (audioEl) {
      if (timeUpdateHandler) audioEl.removeEventListener("timeupdate", timeUpdateHandler);
      if (endedHandler) audioEl.removeEventListener("ended", endedHandler);
      try {
        audioEl.pause();
      } catch {
        /* ignore */
      }
      audioEl.src = "";
      audioEl = null;
    }
    timeUpdateHandler = null;
    endedHandler = null;

    if (sourceBuffer) {
      sourceBuffer.removeEventListener("updateend", drainQueue);
      sourceBuffer = null;
    }
    if (mediaSource) {
      try {
        if (mediaSource.readyState === "open") mediaSource.endOfStream();
      } catch {
        /* ignore */
      }
      mediaSource = null;
    }
    appendQueue = [];
  }

  function pause() {
    if (audioEl && isPlaying.value && !isPaused.value) {
      audioEl.pause();
      isPaused.value = true;
    }
  }

  function resume() {
    if (audioEl && isPaused.value) {
      audioEl.play().catch((e) => console.error("[readaloud] resume play() failed:", e));
      isPaused.value = false;
    }
  }

  function skipTo(chunkIndex: number) {
    if (!audioEl) return;
    if (chunkIndex < 0 || chunkIndex >= chunkInfos.length) return;
    const info = chunkInfos[chunkIndex];
    // If the chunk hasn't been buffered yet, its startSeconds is 0 (default) for indices > 0
    if (chunkIndex > 0 && info.startSeconds === 0) return;
    audioEl.currentTime = info.startSeconds;
    currentChunkIndex.value = chunkIndex;
    chunkPreview.value = info.preview;
  }

  function prev() {
    skipTo(currentChunkIndex.value - 1);
  }

  function next() {
    skipTo(currentChunkIndex.value + 1);
  }

  function cleanup() {
    stop().catch(() => undefined);
  }

  return {
    isPlaying,
    isPaused,
    isLoading,
    currentChunkIndex,
    totalChunks,
    chunkPreview,
    error,
    start,
    stop,
    pause,
    resume,
    prev,
    next,
    cleanup,
  };
}
