<script setup lang="ts">
import { ref, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { writeText, readText } from "@tauri-apps/plugin-clipboard-manager";
import RecordingDot from "./components/RecordingDot.vue";
import TranscriptDisplay from "./components/TranscriptDisplay.vue";
import ActionBar from "./components/ActionBar.vue";
import ChunkProgress from "./components/ChunkProgress.vue";
import PlaybackControls from "./components/PlaybackControls.vue";
import SetupScreen from "./components/SetupScreen.vue";
import { useScribe } from "./composables/useScribe";
import { useReadAloud } from "./composables/useReadAloud";
import { useTauriEvents } from "./composables/useTauriEvents";

// ── Tab state ────────────────────────────────────────────────────────────────
const activeTab = ref<"scribe" | "readaloud">("scribe");

// ── Shared state ─────────────────────────────────────────────────────────────
const needsSetup = ref(false);
const errorMsg = ref("");

// ── Scribe ───────────────────────────────────────────────────────────────────
const isRecording = ref(false);
const {
  partialText,
  committedText,
  micLabel,
  error: scribeError,
  audioLevel,
  needsMicPermission,
  start: scribeStart,
  stop: scribeStop,
  acquireMic,
} = useScribe();

// ── Read Aloud ───────────────────────────────────────────────────────────────
const readaloudErrorMsg = ref("");
const readaloudText = ref(""); // text being read aloud (from clipboard)

const {
  isPlaying,
  isPaused,
  isLoading,
  currentChunkIndex,
  totalChunks,
  chunkPreview,
  error: readaloudError,
  start: readaloudStart,
  stop: readaloudStop,
  pause,
  resume,
  prev,
  next,
  cleanup: readaloudCleanup,
} = useReadAloud();

// ── Scribe actions ───────────────────────────────────────────────────────────

async function startRecording() {
  committedText.value = "";
  partialText.value = "";
  errorMsg.value = "";
  try {
    await scribeStart();
    isRecording.value = true;
  } catch (err) {
    if (!needsMicPermission.value) {
      errorMsg.value = String(err);
    }
    console.error("[elevenscribe] startRecording failed:", err);
  }
}

async function grantMicAndRecord() {
  errorMsg.value = "";
  try {
    await acquireMic();
    await startRecording();
  } catch (err) {
    errorMsg.value = String(err);
  }
}

async function stopRecording() {
  try {
    await scribeStop();
  } catch (err) {
    console.error("Error stopping recording:", err);
  }
  isRecording.value = false;
  const text = committedText.value.trim();
  if (text) {
    await writeText(text).catch(console.error);
    await invoke("save_transcription", { text }).catch(console.error);
  }
}

async function stopAndDismiss() {
  try {
    await scribeStop();
  } catch (err) {
    console.error("Error stopping recording:", err);
  }
  isRecording.value = false;
  const text = committedText.value.trim();
  if (text) {
    await writeText(text).catch(console.error);
    await invoke("save_transcription", { text }).catch(console.error);
  }
  await invoke("hide_overlay").catch(console.error);
}

async function toggleScribe() {
  if (needsSetup.value) return;

  if (isRecording.value) {
    await stopAndDismiss();
    return;
  }

  const hasKey = await invoke<boolean>("has_api_key");
  if (!hasKey) {
    needsSetup.value = true;
    return;
  }

  activeTab.value = "scribe";
  await startRecording();
}

async function paste() {
  const text = committedText.value.trim();
  if (!text) return;
  await writeText(text);
  await invoke("paste_text", { text }).catch((err: unknown) => {
    errorMsg.value = String(err);
  });
}

async function copyOnly() {
  const text = committedText.value.trim();
  if (!text) return;
  await writeText(text).catch(console.error);
  await invoke("hide_overlay").catch(console.error);
}

// ── Read Aloud actions ───────────────────────────────────────────────────────

async function startReadAloud() {
  readaloudErrorMsg.value = "";
  readaloudText.value = "";
  try {
    const clipboardText = await readText();
    if (!clipboardText || !clipboardText.trim()) {
      readaloudErrorMsg.value = "Clipboard is empty";
      return;
    }
    readaloudText.value = clipboardText.trim();
    const audioId = await readaloudStart(clipboardText);
    // Save history immediately — audio chunks are cached to disk as they stream,
    // so even partial sessions are preserved if the user stops early.
    if (audioId) {
      await invoke("save_readaloud", { text: readaloudText.value, audioId }).catch(console.error);
    }
  } catch (err) {
    readaloudErrorMsg.value = String(err);
    console.error("[readaloud] start failed:", err);
  }
}

async function toggleReadAloud() {
  if (needsSetup.value) return;

  if (isPlaying.value) {
    await readaloudStop();
    await startReadAloud();
    return;
  }

  const hasKey = await invoke<boolean>("has_api_key");
  if (!hasKey) {
    needsSetup.value = true;
    return;
  }

  activeTab.value = "readaloud";
  await startReadAloud();
}

async function stopReadAloudAndDismiss() {
  await readaloudStop();
  await invoke("hide_overlay").catch(console.error);
}

function togglePause() {
  if (isPaused.value) {
    resume();
  } else {
    pause();
  }
}

// ── Shared ───────────────────────────────────────────────────────────────────

async function dismiss() {
  if (isRecording.value) {
    try {
      await scribeStop();
    } catch {
      /* ignore */
    }
    isRecording.value = false;
  }
  if (isPlaying.value) {
    await readaloudStop();
  }
  await invoke("hide_overlay").catch(console.error);
}

async function onSetupDone() {
  needsSetup.value = false;
  if (activeTab.value === "scribe") {
    await startRecording();
  } else {
    await startReadAloud();
  }
}

onUnmounted(() => {
  readaloudCleanup();
});

// Check key status on load
invoke<boolean>("has_api_key").then((hasKey) => {
  needsSetup.value = !hasKey;
});

useTauriEvents("toggle-recording", toggleScribe);
useTauriEvents("toggle-readaloud", toggleReadAloud);
useTauriEvents("show-setup", () => {
  needsSetup.value = true;
});
</script>

<template>
  <div class="overlay-wrapper">
    <div class="card">
      <SetupScreen v-if="needsSetup" @done="onSetupDone" />

      <template v-else>
        <!-- Tab bar (drag region for moving the window) -->
        <div class="tab-bar">
          <button
            class="tab"
            :class="{ active: activeTab === 'scribe' }"
            @click="activeTab = 'scribe'"
          >
            Scribe
          </button>
          <button
            class="tab"
            :class="{ active: activeTab === 'readaloud' }"
            @click="activeTab = 'readaloud'"
          >
            Read Aloud
          </button>
          <div class="tab-spacer"></div>

          <!-- Scribe controls -->
          <template v-if="activeTab === 'scribe'">
            <button
              v-if="needsMicPermission"
              class="btn-toggle btn-grant"
              @click="grantMicAndRecord"
            >
              Grant Microphone
            </button>
            <button
              v-else
              class="btn-toggle"
              @click="isRecording ? stopRecording() : toggleScribe()"
            >
              {{ isRecording ? "Stop" : "Record" }}
            </button>
          </template>

          <!-- Read Aloud controls -->
          <template v-if="activeTab === 'readaloud'">
            <button
              class="btn-toggle"
              @click="isPlaying ? stopReadAloudAndDismiss() : toggleReadAloud()"
            >
              {{ isPlaying ? "Stop" : "Start" }}
            </button>
          </template>

          <button class="btn-close" title="Close" @click="dismiss">&times;</button>
        </div>

        <!-- SCRIBE TAB -->
        <template v-if="activeTab === 'scribe'">
          <div class="card-status">
            <RecordingDot :active="isRecording" />
            <span class="status-label">
              <span v-if="needsMicPermission" class="error-text">Microphone access required</span>
              <span v-else-if="errorMsg || scribeError" class="error-text">{{
                errorMsg || scribeError
              }}</span>
              <template v-else-if="isRecording">
                Recording…<template v-if="micLabel"> · {{ micLabel }}</template>
              </template>
              <template v-else>Ready</template>
            </span>
          </div>
          <div v-if="isRecording" class="level-bar-track">
            <div class="level-bar-fill" :style="{ width: audioLevel * 100 + '%' }"></div>
          </div>
          <TranscriptDisplay
            :is-recording="isRecording"
            :partial-text="partialText"
            :committed-text="committedText"
          />
          <ActionBar
            v-if="!isRecording && committedText.trim()"
            @paste="paste"
            @copy="copyOnly"
            @dismiss="dismiss"
          />
        </template>

        <!-- READ ALOUD TAB -->
        <template v-if="activeTab === 'readaloud'">
          <div class="card-status">
            <div class="speaker-dot" :class="{ active: isPlaying && !isPaused }"></div>
            <span class="status-label">
              <span v-if="readaloudErrorMsg || readaloudError" class="error-text">{{
                readaloudErrorMsg || readaloudError
              }}</span>
              <template v-else-if="isPlaying">
                Reading aloud
                <template v-if="totalChunks > 1">
                  · {{ currentChunkIndex + 1 }}/{{ totalChunks }}
                </template>
                <template v-if="isPaused"> (paused)</template>
              </template>
              <template v-else>Ready · ⌥⇧Space to read clipboard</template>
            </span>
          </div>

          <ChunkProgress
            v-if="isPlaying || totalChunks > 0"
            :current-index="currentChunkIndex"
            :total="totalChunks"
            :preview="chunkPreview"
            :is-loading="isLoading"
          />

          <PlaybackControls
            v-if="isPlaying"
            :is-paused="isPaused"
            :can-prev="currentChunkIndex > 0"
            :can-next="currentChunkIndex < totalChunks - 1"
            @prev="prev"
            @next="next"
            @toggle-pause="togglePause"
            @dismiss="dismiss"
          />
        </template>
      </template>
    </div>
  </div>
</template>

<style>
html,
body {
  margin: 0;
  padding: 0;
  background: rgba(20, 20, 20, 0.95);
  overflow: hidden;
}
</style>

<style scoped>
.overlay-wrapper {
  width: 100vw;
  height: 100vh;
  box-sizing: border-box;
}

.card {
  background: transparent;
  color: white;
  width: 100%;
  height: 100%;
  padding: 14px 16px;
  box-sizing: border-box;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* ── Tab bar ────────────────────────────────────────────────────────────── */

.tab-bar {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-bottom: 8px;
  flex-shrink: 0;
}

.tab {
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.4);
  font-size: 12px;
  font-weight: 500;
  padding: 4px 10px;
  border-radius: 6px;
  cursor: pointer;
  transition:
    color 0.15s,
    background 0.15s;
}

.tab:hover {
  color: rgba(255, 255, 255, 0.7);
  background: rgba(255, 255, 255, 0.06);
}

.tab.active {
  color: white;
  background: rgba(255, 255, 255, 0.12);
}

.tab-spacer {
  flex: 1;
}

/* ── Status row ─────────────────────────────────────────────────────────── */

.card-status {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
  flex-shrink: 0;
}

.status-label {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.5);
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.error-text {
  color: #f87171;
}

/* ── Buttons ────────────────────────────────────────────────────────────── */

.btn-toggle {
  background: rgba(255, 255, 255, 0.1);
  border: 1px solid rgba(255, 255, 255, 0.2);
  color: white;
  border-radius: 8px;
  padding: 4px 12px;
  cursor: pointer;
  font-size: 12px;
  transition: background 0.15s;
  flex-shrink: 0;
}

.btn-toggle:hover {
  background: rgba(255, 255, 255, 0.2);
}

.btn-close {
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.35);
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
  padding: 2px 6px;
  border-radius: 6px;
  flex-shrink: 0;
  transition:
    color 0.15s,
    background 0.15s;
}

.btn-close:hover {
  color: rgba(255, 255, 255, 0.8);
  background: rgba(255, 255, 255, 0.1);
}

.btn-grant {
  background: rgba(99, 102, 241, 0.75);
  border-color: rgba(99, 102, 241, 0.9);
}

.btn-grant:hover {
  background: rgba(99, 102, 241, 1);
}

/* ── Level bar ──────────────────────────────────────────────────────────── */

.level-bar-track {
  height: 3px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.08);
  margin-bottom: 8px;
  overflow: hidden;
  flex-shrink: 0;
}

.level-bar-fill {
  height: 100%;
  border-radius: 2px;
  background: #34d399;
  transition: width 0.05s linear;
}

/* ── Speaker dot (read aloud) ───────────────────────────────────────────── */

.speaker-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.25);
  flex-shrink: 0;
  transition: background 0.2s;
}

.speaker-dot.active {
  background: #6366f1;
  animation: pulse 1.2s ease-in-out infinite;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 1;
    transform: scale(1);
  }
  50% {
    opacity: 0.6;
    transform: scale(0.85);
  }
}
</style>
