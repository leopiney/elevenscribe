<script setup lang="ts">
defineProps<{
  isPaused: boolean;
  canPrev: boolean;
  canNext: boolean;
}>();

const emit = defineEmits<{
  prev: [];
  next: [];
  "toggle-pause": [];
  dismiss: [];
}>();
</script>

<template>
  <div class="playback-controls">
    <button class="btn" :disabled="!canPrev" @click="emit('prev')">← Prev</button>
    <button class="btn primary" @click="emit('toggle-pause')">
      {{ isPaused ? "Resume" : "Pause" }}
    </button>
    <button class="btn" :disabled="!canNext" @click="emit('next')">Next →</button>
    <button class="btn" @click="emit('dismiss')">Dismiss</button>
  </div>
</template>

<style scoped>
.playback-controls {
  display: flex;
  gap: 8px;
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid rgba(255, 255, 255, 0.1);
}

.btn {
  background: rgba(255, 255, 255, 0.1);
  border: 1px solid rgba(255, 255, 255, 0.18);
  color: white;
  border-radius: 8px;
  padding: 5px 14px;
  cursor: pointer;
  font-size: 12px;
  transition: background 0.15s;
}

.btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.2);
}

.btn:disabled {
  opacity: 0.3;
  cursor: default;
}

.btn.primary {
  background: rgba(99, 102, 241, 0.75);
  border-color: rgba(99, 102, 241, 0.9);
}

.btn.primary:hover {
  background: rgba(99, 102, 241, 1);
}
</style>
