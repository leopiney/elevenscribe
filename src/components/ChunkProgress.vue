<script setup lang="ts">
defineProps<{
  currentIndex: number;
  total: number;
  preview: string;
  isLoading: boolean;
}>();
</script>

<template>
  <div class="chunk-progress">
    <div class="chunk-counter">
      <span class="chunk-num">{{ currentIndex + 1 }}</span>
      <span class="chunk-sep">/</span>
      <span class="chunk-total">{{ total }}</span>
      <span v-if="isLoading" class="loading-indicator">⏳</span>
    </div>
    <div class="chunk-preview">"{{ preview }}"</div>
  </div>
</template>

<style scoped>
.chunk-progress {
  min-height: 36px;
  padding: 4px 0;
  flex: 1 1 0;
  overflow: hidden;
}

.chunk-counter {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.4);
  margin-bottom: 4px;
  display: flex;
  align-items: center;
  gap: 2px;
}

.chunk-num {
  color: rgba(255, 255, 255, 0.7);
  font-weight: 600;
}

.loading-indicator {
  margin-left: 6px;
  font-size: 10px;
}

.chunk-preview {
  font-size: 13px;
  line-height: 1.4;
  color: rgba(255, 255, 255, 0.75);
  font-style: italic;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
