<script setup lang="ts">
import { computed, onBeforeUnmount, ref, useTemplateRef } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
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

// ── Pet / drag ───────────────────────────────────────────────────────────────
// The badge is both the wolf you pet and one of the only two handles for moving
// this undecorated window, so `data-tauri-drag-region` is off: it hijacks
// mousedown and hands the gesture to the window server, which swallows the
// mouseup a click needs. Instead the press is claimed as a drag only once it
// travels — a press that stays put is a pet.

/** Travel (px) that turns a press into a window drag. */
const DRAG_THRESHOLD = 4;

const head = useTemplateRef<InstanceType<typeof WolfHead>>("head");
const pressed = ref(false);
let pressX = 0;
let pressY = 0;

function endPress() {
  pressed.value = false;
  window.removeEventListener("mousemove", onPressMove);
  window.removeEventListener("mouseup", onPressUp);
}

function onPressMove(e: MouseEvent) {
  if (Math.hypot(e.clientX - pressX, e.clientY - pressY) < DRAG_THRESHOLD) return;
  // Release the press *first*: once the window server owns the drag, no further
  // mouse events reach the WebView, so these listeners would never clear.
  endPress();
  getCurrentWindow().startDragging().catch(console.error);
}

function onPressUp() {
  endPress();
  head.value?.pet();
}

function onPressDown(e: MouseEvent) {
  if (e.button !== 0) return;
  pressX = e.clientX;
  pressY = e.clientY;
  pressed.value = true;
  window.addEventListener("mousemove", onPressMove);
  window.addEventListener("mouseup", onPressUp);
}

onBeforeUnmount(endPress);
</script>

<template>
  <div
    class="wolf-badge"
    :class="[`is-${state}`, { 'is-pressed': pressed }]"
    :style="{ width: `${size}px`, height: `${size}px` }"
    title="Pet the wolf"
    @mousedown="onPressDown"
  >
    <WolfHead
      ref="head"
      class="wolf-badge-head"
      :talking="talking"
      :level="level"
      :size="headSize"
    />
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
  cursor: pointer;
  transition:
    box-shadow 0.25s ease,
    transform 0.12s ease;
}

/* Squish under the finger. Driven by JS rather than `:active` so it can't stick
   after a drag, where the mouseup never comes back to the WebView. */
.wolf-badge.is-pressed {
  transform: scale(0.94);
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
