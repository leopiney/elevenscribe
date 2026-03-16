<script setup lang="ts">
const emit = defineEmits<{
  paste: [];
  copy: [];
  dismiss: [];
}>();
</script>

<template>
  <div class="action-bar">
    <button class="btn primary" @click="emit('paste')">Paste</button>
    <button class="btn" @click="emit('copy')">Copy</button>
    <button class="btn" @click="emit('dismiss')">Dismiss</button>
  </div>
</template>

<style scoped>
.action-bar {
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

.btn:hover {
  background: rgba(255, 255, 255, 0.2);
}

.btn.primary {
  background: rgba(99, 102, 241, 0.75);
  border-color: rgba(99, 102, 241, 0.9);
}

.btn.primary:hover {
  background: rgba(99, 102, 241, 1);
}
</style>
