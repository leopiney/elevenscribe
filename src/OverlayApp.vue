<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import RecordingDot from "./components/RecordingDot.vue";
import TranscriptDisplay from "./components/TranscriptDisplay.vue";
import ActionBar from "./components/ActionBar.vue";
import SetupScreen from "./components/SetupScreen.vue";
import { useScribe } from "./composables/useScribe";
import { useTauriEvents } from "./composables/useTauriEvents";

const isRecording = ref(false);
const needsSetup = ref(false);
const errorMsg = ref("");
const { partialText, committedText, micLabel, error, audioLevel, start, stop } = useScribe();

async function startRecording() {
  committedText.value = "";
  partialText.value = "";
  errorMsg.value = "";
  try {
    await start();
    isRecording.value = true;
  } catch (err) {
    errorMsg.value = String(err);
    console.error("[elevenscribe] startRecording failed:", err);
  }
}

// Called by the Stop button — keeps the overlay visible so the user can
// choose to Paste, Copy, or Dismiss via the ActionBar.
async function stopRecording() {
  try {
    await stop();
  } catch (err) {
    console.error("Error stopping recording:", err);
  }
  isRecording.value = false;
  if (committedText.value.trim()) {
    await writeText(committedText.value.trim()).catch(console.error);
  }
}

// Called by the global shortcut — stops, copies to clipboard, then dismisses.
async function stopAndDismiss() {
  try {
    await stop();
  } catch (err) {
    console.error("Error stopping recording:", err);
  }
  isRecording.value = false;
  if (committedText.value.trim()) {
    await writeText(committedText.value.trim()).catch(console.error);
  }
  await invoke("hide_overlay").catch(console.error);
}

async function toggle() {
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

  await startRecording();
}

async function onSetupDone() {
  needsSetup.value = false;
  await startRecording();
}

async function paste() {
  const text = committedText.value.trim();
  if (!text) return;
  await writeText(text);
  await invoke("paste_text", { text }).catch((err: unknown) => {
    errorMsg.value = String(err);
    console.error("[elevenscribe] paste_text failed:", err);
  });
}

async function copyOnly() {
  const text = committedText.value.trim();
  if (!text) return;
  await writeText(text).catch(console.error);
  await invoke("hide_overlay").catch(console.error);
}

async function dismiss() {
  await invoke("hide_overlay").catch(console.error);
}

// Check key status immediately when the webview first loads
invoke<boolean>("has_api_key").then((hasKey) => {
  needsSetup.value = !hasKey;
});

useTauriEvents("toggle-recording", toggle);
</script>

<template>
  <div class="overlay-wrapper">
    <div class="card">
      <SetupScreen v-if="needsSetup" @done="onSetupDone" />

      <template v-else>
        <div class="card-header">
          <RecordingDot :active="isRecording" />
          <span class="status-label">
            <span v-if="errorMsg || error" class="error-text">{{
              errorMsg || error
            }}</span>
            <template v-else-if="isRecording">
              Recording…<template v-if="micLabel"> · {{ micLabel }}</template>
            </template>
            <template v-else>Ready</template>
          </span>
          <button class="btn-toggle" @click="isRecording ? stopRecording() : toggle()">
            {{ isRecording ? "Stop" : "Record" }}
          </button>
        </div>
        <div v-if="isRecording" class="level-bar-track">
          <div class="level-bar-fill" :style="{ width: (audioLevel * 100) + '%' }"></div>
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
  padding: 14px 16px;
  box-sizing: border-box;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

.card-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
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

.level-bar-track {
  height: 3px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.08);
  margin-bottom: 8px;
  overflow: hidden;
}

.level-bar-fill {
  height: 100%;
  border-radius: 2px;
  background: #34d399;
  transition: width 0.05s linear;
}
</style>
