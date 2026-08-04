<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";

const props = withDefaults(
  defineProps<{
    /** Run the talking animation — mic is recording, or we're reading aloud. */
    talking?: boolean;
    /** 0–1 live input level. Drives jaw amplitude when present (mic); when it
     *  stays at 0 (read aloud) the jaw falls back to a procedural envelope. */
    level?: number;
    /** Rendered edge length in CSS pixels (the canvas is square). */
    size?: number;
  }>(),
  { talking: false, level: 0, size: 88 }
);

const MODEL_URL = "/wolf-head.glb";

// ── View tuning ──────────────────────────────────────────────────────────────
/** Base framing: camera swung out to the head's left and lifted above it, so we
 *  look down the snout at a 3/4 angle. Orthographic → isometric-flavoured. */
const BASE_AZ = THREE.MathUtils.degToRad(-42);
const BASE_EL = THREE.MathUtils.degToRad(29);
/** Idle camera drift — the head should breathe, not swim. */
const DRIFT_AZ = THREE.MathUtils.degToRad(4);
const DRIFT_EL = THREE.MathUtils.degToRad(2.5);
/** Extra swing when the pointer is over the overlay. */
const POINTER_AZ = THREE.MathUtils.degToRad(7);
const POINTER_EL = THREE.MathUtils.degToRad(5);
/** Pointer distance (px) that maps to a full swing. */
const POINTER_RANGE = 260;
/** Fully-open jaw, radians. +X rotation drops the front of the jaw. */
const JAW_OPEN = 0.38;
/** Orthographic camera distance (scale is frustum-driven, this only sets depth). */
const CAM_DIST = 6;
/** Padding on top of the worst-case fit: covers the jaw swinging open, and
 *  keeps the silhouette off the circular badge's edge (the canvas is square,
 *  so its corners are cropped). */
const FIT_MARGIN = 1.13;

const host = ref<HTMLDivElement | null>(null);

let renderer: THREE.WebGLRenderer | null = null;
let scene: THREE.Scene | null = null;
let camera: THREE.OrthographicCamera | null = null;
let model: THREE.Group | null = null;
let jaw: THREE.Object3D | null = null;
let jawRestX = 0;
let raf = 0;
let startedAt = 0;
let lastFrame = 0;
let jawValue = 0;
let pointerX = 0;
let pointerY = 0;
let smoothPointerX = 0;
let smoothPointerY = 0;
let destroyed = false;

/** Syllable-ish mouth envelope. Three incommensurate sines never repeat on a
 *  hearable period, and the gate closes the mouth between "words". */
function talkEnvelope(t: number): number {
  const s =
    0.55 * Math.sin(t * 11.0) + 0.3 * Math.sin(t * 17.3 + 1.1) + 0.15 * Math.sin(t * 6.7 + 2.7);
  const gate = Math.min(1, Math.max(0, Math.sin(t * 1.7 + 0.6) * 0.5 + 0.78));
  return Math.max(0, s) * gate;
}

/** Frame-rate independent exponential smoothing factor. */
function smoothing(dt: number, tau: number): number {
  return 1 - Math.exp(-dt / tau);
}

function placeCamera(az: number, el: number) {
  if (!camera) return;
  const cosEl = Math.cos(el);
  camera.position.set(Math.sin(az) * cosEl, Math.sin(el), Math.cos(az) * cosEl);
  camera.position.multiplyScalar(CAM_DIST);
  camera.lookAt(0, 0, 0);
}

/** Size the orthographic frustum to the model's projected bounds, and offset it
 *  so the head sits centred in the badge.
 *
 *  Measured over real vertices rather than the bounding box — a box around a
 *  long, thin head throws its corners way outside the silhouette and leaves the
 *  badge half empty — and unioned over the extremes of the camera swing, so
 *  drift can never push an ear or the snout off the edge. */
function fitFrustum(target: THREE.Object3D) {
  if (!camera) return;
  target.updateWorldMatrix(true, true);

  const meshes: THREE.Mesh[] = [];
  target.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (mesh.isMesh) meshes.push(mesh);
  });

  const v = new THREE.Vector3();
  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  const swingAz = DRIFT_AZ * 1.4 + POINTER_AZ;
  const swingEl = DRIFT_EL * 1.4 + POINTER_EL;
  for (const az of [BASE_AZ - swingAz, BASE_AZ, BASE_AZ + swingAz]) {
    for (const el of [BASE_EL - swingEl, BASE_EL, BASE_EL + swingEl]) {
      placeCamera(az, el);
      camera.updateMatrixWorld();
      for (const mesh of meshes) {
        const pos = mesh.geometry.getAttribute("position");
        for (let i = 0; i < pos.count; i++) {
          v.fromBufferAttribute(pos, i)
            .applyMatrix4(mesh.matrixWorld)
            .applyMatrix4(camera.matrixWorldInverse);
          minX = Math.min(minX, v.x);
          maxX = Math.max(maxX, v.x);
          minY = Math.min(minY, v.y);
          maxY = Math.max(maxY, v.y);
        }
      }
    }
  }
  placeCamera(BASE_AZ, BASE_EL);
  if (!Number.isFinite(minX)) return;

  const cx = (minX + maxX) / 2;
  const cy = (minY + maxY) / 2;
  const half = (Math.max(maxX - minX, maxY - minY) / 2) * FIT_MARGIN;
  camera.left = cx - half;
  camera.right = cx + half;
  camera.top = cy + half;
  camera.bottom = cy - half;
  camera.updateProjectionMatrix();
}

function buildScene(canvasHost: HTMLDivElement): boolean {
  try {
    renderer = new THREE.WebGLRenderer({
      alpha: true,
      antialias: true,
      powerPreference: "low-power",
    });
  } catch (e) {
    console.error("[wolf] WebGL unavailable:", e);
    return false;
  }
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.setSize(props.size, props.size);
  renderer.setClearAlpha(0);
  canvasHost.appendChild(renderer.domElement);

  scene = new THREE.Scene();
  camera = new THREE.OrthographicCamera(-0.6, 0.6, 0.6, -0.6, 0.1, 20);
  placeCamera(BASE_AZ, BASE_EL);

  // Key from the same side as the camera but higher, matching the reference
  // render: bright skull top and snout, the far cheek falling into shadow.
  const key = new THREE.DirectionalLight(0xffffff, 3.1);
  key.position.set(-1.6, 2.4, 2.2);
  const fill = new THREE.DirectionalLight(0xa9b6ff, 0.95);
  fill.position.set(2.4, -0.4, 1.2);
  const rim = new THREE.DirectionalLight(0xc9d4ff, 1.7);
  rim.position.set(0.8, 1.1, -2.4);
  const hemi = new THREE.HemisphereLight(0xdfe4ff, 0x1a1836, 0.85);
  scene.add(key, fill, rim, hemi);

  return true;
}

function teardown() {
  if (raf) cancelAnimationFrame(raf);
  raf = 0;
  scene?.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (!mesh.isMesh) return;
    mesh.geometry?.dispose();
    const mat = mesh.material;
    if (Array.isArray(mat)) mat.forEach((m) => m.dispose());
    else mat?.dispose();
  });
  renderer?.domElement.remove();
  renderer?.dispose();
  renderer = null;
  scene = null;
  camera = null;
  model = null;
  jaw = null;
}

function frame(now: number) {
  raf = requestAnimationFrame(frame);
  if (!renderer || !scene || !camera) return;

  const t = (now - startedAt) / 1000;
  const dt = Math.min(0.1, (now - lastFrame) / 1000) || 0.016;
  lastFrame = now;

  // Jaw: mic level when we have one, procedural syllables otherwise. A floor of
  // envelope keeps a live mic from looking frozen between words.
  const env = talkEnvelope(t);
  const drive = props.level > 0.02 ? Math.max(props.level, env * 0.35) : env * 0.9;
  const target = props.talking ? Math.min(1, drive) : 0;
  // Snap open, ease shut — a mouth that closes as fast as it opens reads robotic.
  jawValue += (target - jawValue) * smoothing(dt, target > jawValue ? 0.035 : 0.09);

  if (jaw) jaw.rotation.x = jawRestX + jawValue * JAW_OPEN;

  if (model) {
    model.rotation.z = Math.sin(t * 0.7) * 0.03;
    model.rotation.x = Math.sin(t * 0.53 + 1.3) * 0.025 - jawValue * 0.05;
    model.position.y = Math.sin(t * 0.9) * 0.012;
  }

  const p = smoothing(dt, 0.18);
  smoothPointerX += (pointerX - smoothPointerX) * p;
  smoothPointerY += (pointerY - smoothPointerY) * p;

  placeCamera(
    BASE_AZ +
      Math.sin(t * 0.31) * DRIFT_AZ +
      Math.sin(t * 0.13 + 2) * DRIFT_AZ * 0.4 -
      smoothPointerX * POINTER_AZ,
    BASE_EL + Math.sin(t * 0.23 + 1) * DRIFT_EL + smoothPointerY * POINTER_EL
  );

  renderer.render(scene, camera);
}

function startLoop() {
  if (raf || destroyed || !renderer) return;
  lastFrame = performance.now();
  raf = requestAnimationFrame(frame);
}

function stopLoop() {
  if (raf) cancelAnimationFrame(raf);
  raf = 0;
}

function onPointerMove(e: MouseEvent) {
  const el = renderer?.domElement;
  if (!el) return;
  const r = el.getBoundingClientRect();
  pointerX = THREE.MathUtils.clamp((e.clientX - (r.left + r.width / 2)) / POINTER_RANGE, -1, 1);
  pointerY = THREE.MathUtils.clamp((e.clientY - (r.top + r.height / 2)) / POINTER_RANGE, -1, 1);
}

function onPointerLeave() {
  pointerX = 0;
  pointerY = 0;
}

function onVisibility() {
  if (document.hidden) stopLoop();
  else startLoop();
}

function onContextLost(e: Event) {
  // Let WebKit hand the context back instead of killing the canvas — an
  // always-on-top overlay gets its GPU context yanked on hide/show cycles.
  e.preventDefault();
  stopLoop();
}

function onContextRestored() {
  teardown();
  void init();
}

async function init() {
  const canvasHost = host.value;
  if (!canvasHost || destroyed) return;
  if (!buildScene(canvasHost)) return;

  renderer!.domElement.addEventListener("webglcontextlost", onContextLost);
  renderer!.domElement.addEventListener("webglcontextrestored", onContextRestored);

  try {
    const gltf = await new GLTFLoader().loadAsync(MODEL_URL);
    if (destroyed || !scene) return;

    // Re-origin on the visual centre and normalise to unit size, so framing and
    // the idle wobble are independent of however the model was exported.
    const root = gltf.scene;
    const box = new THREE.Box3().setFromObject(root);
    const size = box.getSize(new THREE.Vector3());
    const center = box.getCenter(new THREE.Vector3());
    root.position.sub(center);

    model = new THREE.Group();
    model.add(root);
    model.scale.setScalar(1 / Math.max(size.x, size.y, size.z));
    scene.add(model);

    jaw = root.getObjectByName("jaw") ?? null;
    jawRestX = jaw?.rotation.x ?? 0;
    if (!jaw) console.warn("[wolf] no 'jaw' node in model — talk animation disabled");

    fitFrustum(model);
  } catch (e) {
    console.error("[wolf] failed to load model:", e);
  }

  startedAt = performance.now();
  startLoop();
}

watch(
  () => props.size,
  (size) => renderer?.setSize(size, size)
);

onMounted(() => {
  void init();
  window.addEventListener("mousemove", onPointerMove);
  window.addEventListener("mouseleave", onPointerLeave);
  document.addEventListener("visibilitychange", onVisibility);
});

onBeforeUnmount(() => {
  destroyed = true;
  window.removeEventListener("mousemove", onPointerMove);
  window.removeEventListener("mouseleave", onPointerLeave);
  document.removeEventListener("visibilitychange", onVisibility);
  teardown();
});
</script>

<template>
  <div ref="host" class="wolf-canvas" :style="{ width: `${size}px`, height: `${size}px` }"></div>
</template>

<style scoped>
.wolf-canvas {
  display: block;
  pointer-events: none;
}

.wolf-canvas :deep(canvas) {
  display: block;
}
</style>
