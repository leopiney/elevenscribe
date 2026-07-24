// AudioWorklet mic-capture processor for ElevenLabs Scribe realtime.
//
// Runs on the dedicated audio-rendering thread, so — unlike the old
// ScriptProcessorNode, whose onaudioprocess ran on the main thread and
// silently dropped buffers whenever the UI was busy — it never loses samples
// under main-thread load. That dropped-buffer behaviour was the likely cause
// of the app "missing a lot of speech".
//
// Responsibilities:
//   1. Resample the mic from the AudioContext's native rate (usually 44.1/48
//      kHz on macOS) down to exactly 16 kHz, which is what Scribe expects.
//      This removes the fragile `new AudioContext({ sampleRate: 16000 })`
//      assumption: we now produce true 16 kHz no matter what rate the WebView
//      actually gives us.
//   2. Convert to mono PCM16 and post finished chunks to the main thread,
//      which base64-encodes and ships them over the WebSocket (and keeps a
//      copy for the debug .wav recording).
//
// Plain ES only — files in public/ are copied verbatim (not transpiled), and
// this executes directly in AudioWorkletGlobalScope.

const TARGET_RATE = 16000;
const CHUNK_SAMPLES = 2048; // output samples per message (~128 ms @ 16 kHz)

class ScribeProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    // `sampleRate` is a global in AudioWorkletGlobalScope: the context's rate.
    this.ratio = sampleRate / TARGET_RATE; // input samples consumed per output sample
    this.pos = 0; // fractional read position within the current input block
    this.last = 0; // last sample of the previous block (for cross-block interpolation)
    this.haveLast = false;
    this.out = new Int16Array(CHUNK_SAMPLES);
    this.outLen = 0;

    // On stop the main thread posts "flush" so the final partial chunk (< 128
    // ms) isn't lost from the recording / final commit.
    this.port.onmessage = (e) => {
      if (e.data === "flush") this.flush();
    };
  }

  flush() {
    if (this.outLen === 0) return;
    const pcm = this.out.slice(0, this.outLen);
    // RMS for the level meter — computed here to keep the main thread light.
    let sum = 0;
    for (let i = 0; i < pcm.length; i++) {
      const s = pcm[i] / 32768;
      sum += s * s;
    }
    const rms = Math.sqrt(sum / pcm.length);
    this.port.postMessage({ pcm, rms }, [pcm.buffer]);
    this.outLen = 0;
  }

  push(sample) {
    const s = Math.max(-1, Math.min(1, sample));
    this.out[this.outLen++] = s < 0 ? s * 0x8000 : s * 0x7fff;
    if (this.outLen === this.out.length) this.flush();
  }

  process(inputs) {
    const input = inputs[0];
    if (!input || input.length === 0) return true;
    const ch = input[0];
    if (!ch || ch.length === 0) return true;

    const N = ch.length;

    // Emit every output sample whose (fractional) source position lands in this
    // block, linearly interpolating between neighbouring input samples.
    while (this.pos < N) {
      const i = Math.floor(this.pos);
      const frac = this.pos - i;
      let s0;
      let s1;
      if (i < 0) {
        // Between the previous block's last sample and this block's first.
        s0 = this.haveLast ? this.last : ch[0];
        s1 = ch[0];
      } else if (i + 1 < N) {
        s0 = ch[i];
        s1 = ch[i + 1];
      } else {
        // Need the next block's first sample — defer this output sample.
        break;
      }
      this.push(s0 + (s1 - s0) * frac);
      this.pos += this.ratio;
    }

    // Re-base the read position onto the next block and remember the boundary
    // sample so a deferred output can interpolate across the block seam.
    this.pos -= N;
    this.last = ch[N - 1];
    this.haveLast = true;
    return true;
  }
}

registerProcessor("scribe-processor", ScribeProcessor);
