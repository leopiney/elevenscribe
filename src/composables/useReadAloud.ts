import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface ChunkInfo {
  index: number;
  total: number;
  preview: string;
}

interface AudioData {
  chunk_index: number;
  data: string; // base64-encoded MP3
  is_last_chunk: boolean;
}

interface TtsError {
  message: string;
}

export function useReadAloud() {
  const isPlaying = ref(false);
  const isPaused = ref(false);
  const isLoading = ref(false);
  const currentChunkIndex = ref(0);
  const totalChunks = ref(0);
  const chunkPreview = ref("");
  const error = ref("");

  let audioContext: AudioContext | null = null;
  // Buffered audio per chunk index
  const audioBuffers = new Map<number, AudioBuffer>();
  let currentSource: AudioBufferSourceNode | null = null;
  let fullText = ""; // the full clipboard text, kept for skip_to_chunk

  // Event unlisten functions
  const unlisteners: (() => void)[] = [];

  async function setupListeners() {
    // Only set up once
    if (unlisteners.length > 0) return;

    unlisteners.push(
      await listen<ChunkInfo[]>("tts-chunks-ready", (event) => {
        const infos = event.payload;
        totalChunks.value = infos.length;
        audioBuffers.clear();
        if (infos.length > 0) {
          chunkPreview.value = infos[0].preview;
        }
      })
    );

    unlisteners.push(
      await listen<{ index: number; total: number; preview: string }>(
        "tts-chunk-started",
        (event) => {
          currentChunkIndex.value = event.payload.index;
          chunkPreview.value = event.payload.preview;
          isLoading.value = true;
        }
      )
    );

    unlisteners.push(
      await listen<AudioData>("tts-audio-data", async (event) => {
        const { chunk_index, data } = event.payload;
        isLoading.value = false;

        try {
          const audioBuffer = await decodeBase64Audio(data);
          audioBuffers.set(chunk_index, audioBuffer);

          // If this is the chunk we're waiting for, play it
          if (chunk_index === currentChunkIndex.value && isPlaying.value && !isPaused.value) {
            playBuffer(audioBuffer, chunk_index);
          }
        } catch (e) {
          console.error("[readaloud] failed to decode audio chunk:", e);
          error.value = "Failed to decode audio";
        }
      })
    );

    unlisteners.push(
      await listen<void>("tts-complete", () => {
        // Backend is done streaming. Audio may still be playing.
        isLoading.value = false;
      })
    );

    unlisteners.push(
      await listen<void>("tts-stopped", () => {
        isLoading.value = false;
      })
    );

    unlisteners.push(
      await listen<TtsError>("tts-error", (event) => {
        error.value = event.payload.message;
        isPlaying.value = false;
        isLoading.value = false;
      })
    );
  }

  async function decodeBase64Audio(base64: string): Promise<AudioBuffer> {
    if (!audioContext) {
      audioContext = new AudioContext();
    }
    if (audioContext.state === "suspended") {
      await audioContext.resume();
    }

    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }

    return audioContext.decodeAudioData(bytes.buffer.slice(0));
  }

  function playBuffer(buffer: AudioBuffer, chunkIndex: number) {
    if (!audioContext) return;

    // Stop any currently playing source
    if (currentSource) {
      try {
        currentSource.onended = null;
        currentSource.stop();
      } catch {
        // ignore if already stopped
      }
    }

    const source = audioContext.createBufferSource();
    source.buffer = buffer;
    source.connect(audioContext.destination);

    source.onended = () => {
      if (!isPlaying.value || isPaused.value) return;

      // Auto-advance to next chunk
      const nextIndex = chunkIndex + 1;
      if (nextIndex < totalChunks.value) {
        currentChunkIndex.value = nextIndex;
        // Update preview from chunks-ready data if we have the buffer
        const nextBuffer = audioBuffers.get(nextIndex);
        if (nextBuffer) {
          playBuffer(nextBuffer, nextIndex);
        }
        // If buffer not ready yet, it will be played when tts-audio-data arrives
      } else {
        // All chunks done
        isPlaying.value = false;
        currentSource = null;
      }
    };

    source.start(0);
    currentSource = source;
  }

  async function start(text: string) {
    error.value = "";
    fullText = text;
    audioBuffers.clear();
    currentChunkIndex.value = 0;
    totalChunks.value = 0;
    chunkPreview.value = "";

    await setupListeners();

    // Stop any current playback
    if (currentSource) {
      try {
        currentSource.onended = null;
        currentSource.stop();
      } catch {
        // ignore
      }
      currentSource = null;
    }

    // Reset audio context for fresh playback
    if (audioContext) {
      await audioContext.close().catch(() => undefined);
      audioContext = null;
    }

    isPlaying.value = true;
    isPaused.value = false;
    isLoading.value = true;

    await invoke("start_tts", { text });
  }

  async function stop() {
    isPlaying.value = false;
    isPaused.value = false;
    isLoading.value = false;

    if (currentSource) {
      try {
        currentSource.onended = null;
        currentSource.stop();
      } catch {
        // ignore
      }
      currentSource = null;
    }

    await invoke("stop_tts").catch(console.error);

    if (audioContext) {
      await audioContext.close().catch(() => undefined);
      audioContext = null;
    }
    audioBuffers.clear();
  }

  function pause() {
    if (audioContext && isPlaying.value) {
      audioContext.suspend();
      isPaused.value = true;
    }
  }

  function resume() {
    if (audioContext && isPaused.value) {
      audioContext.resume();
      isPaused.value = false;
    }
  }

  async function skipTo(chunkIndex: number) {
    if (chunkIndex < 0 || chunkIndex >= totalChunks.value) return;

    // Stop current audio
    if (currentSource) {
      try {
        currentSource.onended = null;
        currentSource.stop();
      } catch {
        // ignore
      }
      currentSource = null;
    }

    // Reset audio context
    if (audioContext) {
      await audioContext.close().catch(() => undefined);
      audioContext = null;
    }
    audioBuffers.clear();

    currentChunkIndex.value = chunkIndex;
    isPlaying.value = true;
    isPaused.value = false;
    isLoading.value = true;

    await invoke("skip_to_chunk", { text: fullText, chunkIndex });
  }

  function prev() {
    skipTo(currentChunkIndex.value - 1);
  }

  function next() {
    skipTo(currentChunkIndex.value + 1);
  }

  function cleanup() {
    for (const unlisten of unlisteners) {
      unlisten();
    }
    unlisteners.length = 0;
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
