<script setup lang="ts">
defineProps<{
  isRecording: boolean;
  partialText: string;
  committedText: string;
}>();
</script>

<template>
  <div class="transcript">
    <span class="committed">{{ committedText }}</span>
    <span class="partial">{{ partialText }}</span>
    <span v-if="isRecording && !committedText && !partialText" class="placeholder">
      Start speaking…
    </span>
  </div>
</template>

<style scoped>
.transcript {
  min-height: 44px;
  font-size: 14px;
  line-height: 1.55;
  padding: 4px 0;
  word-break: break-word;
}

.committed {
  color: rgba(255, 255, 255, 0.95);
}

.partial {
  color: rgba(255, 255, 255, 0.45);
}

.placeholder {
  color: rgba(255, 255, 255, 0.3);
  font-style: italic;
}
</style>
