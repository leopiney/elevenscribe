<script setup lang="ts">
import { computed } from "vue";
import WolfHead from "./WolfHead.vue";

const props = withDefaults(
  defineProps<{
    /** Drives the ring colour + glow. */
    state?: "idle" | "recording" | "reading";
    /** Run the talking animation. */
    talking?: boolean;
    /** 0–1 live mic level (scribe only). */
    level?: number;
    size?: number;
  }>(),
  { state: "idle", talking: false, level: 0, size: 59 }
);

/** The canvas fills the disc *inside* the 1px ring. Sizing it to the full badge
 *  pushes it under `overflow: hidden`, which shaves a pixel off the ring. */
const headSize = computed(() => Math.max(0, props.size - 2));
</script>

<template>
  <div
    class="wolf-badge"
    :class="`is-${state}`"
    :style="{ width: `${size}px`, height: `${size}px` }"
    data-tauri-drag-region
  >
    <WolfHead class="wolf-badge-head" :talking="talking" :level="level" :size="headSize" />
  </div>
</template>

<style scoped>
.wolf-badge {
  position: relative;
  /* Border-box so `size` is the outer diameter — the ring has to land exactly
     where the layout expects it, or it gets clipped by the window edge. */
  box-sizing: border-box;
  border-radius: 50%;
  overflow: hidden;
  background:
    radial-gradient(circle at 30% 24%, rgba(129, 122, 255, 0.55), transparent 62%),
    linear-gradient(158deg, #3b34a4 0%, #241f5c 52%, #16142f 100%);
  /* Run the gradient under the border too. Left at the default padding-box the
     gradient clamps to its darkest stop beneath the border, which paints the
     rim as a dark notch across the top instead of a hairline. */
  background-origin: border-box;
  border: 1px solid rgba(255, 255, 255, 0.16);
  /* Kept deliberately tight: the badge sits over the desktop, and a wide or
     hard-edged shadow smudges whatever is behind it. */
  box-shadow:
    0 2px 8px rgba(0, 0, 0, 0.3),
    inset 0 1px 0 rgba(255, 255, 255, 0.18);
  transition: box-shadow 0.25s ease;
}

.wolf-badge-head {
  position: absolute;
  inset: 0;
}

.wolf-badge.is-recording {
  box-shadow:
    0 2px 8px rgba(0, 0, 0, 0.3),
    0 0 0 1.5px rgba(239, 68, 68, 0.85),
    0 0 12px rgba(239, 68, 68, 0.35),
    inset 0 1px 0 rgba(255, 255, 255, 0.18);
  animation: badge-pulse 1.8s ease-in-out infinite;
}

.wolf-badge.is-reading {
  box-shadow:
    0 2px 8px rgba(0, 0, 0, 0.3),
    0 0 0 1.5px rgba(99, 102, 241, 0.9),
    0 0 12px rgba(99, 102, 241, 0.4),
    inset 0 1px 0 rgba(255, 255, 255, 0.18);
  animation: badge-pulse 1.8s ease-in-out infinite;
}

@keyframes badge-pulse {
  0%,
  100% {
    filter: brightness(1);
  }
  50% {
    filter: brightness(1.14);
  }
}
</style>
