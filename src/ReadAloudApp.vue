<script setup lang="ts">
import { ref, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import ChunkProgress from "./components/ChunkProgress.vue";
import PlaybackControls from "./components/PlaybackControls.vue";
import SetupScreen from "./components/SetupScreen.vue";
import { useReadAloud } from "./composables/useReadAloud";
import { useTauriEvents } from "./composables/useTauriEvents";

const needsSetup = ref(false);
const errorMsg = ref("");

const {
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
} = useReadAloud();

async function startReadAloud() {
  errorMsg.value = "";
  try {
    const clipboardText = await readText();
    if (!clipboardText || !clipboardText.trim()) {
      errorMsg.value = "Clipboard is empty";
      return;
    }
    await start(clipboardText);
  } catch (err) {
    errorMsg.value = String(err);
    console.error("[readaloud] start failed:", err);
  }
}

async function toggle() {
  if (needsSetup.value) return;

  // If already playing, stop and re-read clipboard (restart behavior)
  if (isPlaying.value) {
    await stop();
    await startReadAloud();
    return;
  }

  const hasKey = await invoke<boolean>("has_api_key");
  if (!hasKey) {
    needsSetup.value = true;
    return;
  }

  await startReadAloud();
}

async function stopAndDismiss() {
  await stop();
  await invoke("hide_readaloud").catch(console.error);
}

async function dismiss() {
  await stop();
  await invoke("hide_readaloud").catch(console.error);
}

function togglePause() {
  if (isPaused.value) {
    resume();
  } else {
    pause();
  }
}

async function onSetupDone() {
  needsSetup.value = false;
  await startReadAloud();
}

// Check key status on load
invoke<boolean>("has_api_key").then((hasKey) => {
  needsSetup.value = !hasKey;
});

useTauriEvents("toggle-readaloud", toggle);
useTauriEvents("show-setup", () => {
  needsSetup.value = true;
});

onUnmounted(() => {
  cleanup();
});
</script>

<template>
  <div class="overlay-wrapper">
    <div class="card">
      <SetupScreen v-if="needsSetup" @done="onSetupDone" />

      <template v-else>
        <div class="card-header">
          <div class="speaker-dot" :class="{ active: isPlaying && !isPaused }"></div>
          <span class="status-label">
            <span v-if="errorMsg || error" class="error-text">{{ errorMsg || error }}</span>
            <template v-else-if="isPlaying">
              Reading aloud
              <template v-if="totalChunks > 1">
                · {{ currentChunkIndex + 1 }}/{{ totalChunks }}
              </template>
              <template v-if="isPaused"> (paused)</template>
            </template>
            <template v-else>Ready · ⌘⌥Space to read clipboard</template>
          </span>
          <button class="btn-toggle" @click="isPlaying ? stopAndDismiss() : toggle()">
            {{ isPlaying ? "Stop" : "Start" }}
          </button>
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
    </div>
  </div>
</template>

<style>
html,
body {
  margin: 0;
  padding: 0;
  background: transparent;
  overflow: hidden;
}
</style>

<style scoped>
.overlay-wrapper {
  display: flex;
  justify-content: center;
  align-items: center;
  width: 100vw;
  height: 100vh;
  padding: 12px;
  box-sizing: border-box;
}

.card {
  background: rgba(20, 20, 20, 0.88);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-radius: 16px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  color: white;
  width: 100%;
  max-height: calc(100vh - 24px);
  padding: 14px 16px;
  box-sizing: border-box;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.card-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
  flex-shrink: 0;
}

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
</style>
