<script setup lang="ts">
import { ref, watch, nextTick } from "vue";

const props = defineProps<{
  isRecording: boolean;
  partialText: string;
  committedText: string;
}>();

const transcriptEl = ref<HTMLElement | null>(null);

function scrollToBottom() {
  nextTick(() => {
    if (transcriptEl.value) {
      transcriptEl.value.scrollTop = transcriptEl.value.scrollHeight;
    }
  });
}

watch(() => props.committedText, scrollToBottom);
watch(() => props.partialText, scrollToBottom);
</script>

<template>
  <div ref="transcriptEl" class="transcript">
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
  overflow-y: auto;
  flex: 1 1 0;
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
