<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

const emit = defineEmits<{ done: [] }>();

const apiKey = ref("");
const saving = ref(false);
const errorMsg = ref("");

async function save() {
  const key = apiKey.value.trim();
  if (!key) return;
  saving.value = true;
  errorMsg.value = "";
  try {
    await invoke("save_api_key", { key });
    emit("done");
  } catch (err) {
    errorMsg.value = String(err);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="setup">
    <p class="heading">ElevenLabs API Key</p>
    <p class="hint">
      Paste your key from<br />
      elevenlabs.io → Profile → API Keys
    </p>
    <div class="input-row">
      <input
        v-model="apiKey"
        type="password"
        placeholder="sk_…"
        class="key-input"
        autocomplete="off"
        spellcheck="false"
        @keydown.enter="save"
      />
      <button
        class="btn-save"
        :disabled="!apiKey.trim() || saving"
        @click="save"
      >
        {{ saving ? "Saving…" : "Save" }}
      </button>
    </div>
    <p v-if="errorMsg" class="error">{{ errorMsg }}</p>
  </div>
</template>

<style scoped>
.setup {
  padding: 2px 0 4px;
}

.heading {
  margin: 0 0 4px;
  font-size: 13px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.9);
}

.hint {
  margin: 0 0 10px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.4);
  line-height: 1.5;
}

.input-row {
  display: flex;
  gap: 6px;
}

.key-input {
  flex: 1;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 8px;
  color: white;
  font-size: 13px;
  padding: 6px 10px;
  outline: none;
  font-family: "SF Mono", "Menlo", monospace;
}

.key-input::placeholder {
  color: rgba(255, 255, 255, 0.25);
}

.key-input:focus {
  border-color: rgba(99, 102, 241, 0.7);
}

.btn-save {
  background: rgba(99, 102, 241, 0.8);
  border: 1px solid rgba(99, 102, 241, 0.9);
  color: white;
  border-radius: 8px;
  padding: 6px 14px;
  font-size: 12px;
  cursor: pointer;
  transition: background 0.15s;
  white-space: nowrap;
}

.btn-save:hover:not(:disabled) {
  background: rgba(99, 102, 241, 1);
}

.btn-save:disabled {
  opacity: 0.4;
  cursor: default;
}

.error {
  margin: 8px 0 0;
  font-size: 11px;
  color: #f87171;
}
</style>
