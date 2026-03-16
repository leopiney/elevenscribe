<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" alt="Elevenscribe icon" />
</p>

<h1 align="center">Elevenscribe</h1>

<p align="center">
  Press a hotkey, speak, and your words are typed wherever your cursor is.
</p>

---

Elevenscribe is a macOS menubar app that turns your voice into text using the [ElevenLabs Scribe](https://elevenlabs.io/blog/introducing-scribe-v2) real-time API. Press **⌘ Shift Space** to start recording, speak, then press again to stop — the transcript is pasted directly into the active app.

- Floating overlay shows live transcription as you speak
- Automatically ducks system volume while recording
- Runs silently in the menu bar, always one shortcut away
- Requires an [ElevenLabs API key](https://elevenlabs.io/blog/introducing-scribe-v2)

## Install from Releases

1. Go to the [Releases](../../releases) page and download the latest `.dmg`
2. Open the `.dmg`, drag **Elevenscribe** to your Applications folder
3. Launch the app and enter your ElevenLabs API key when prompted
4. Grant **Accessibility** and **Microphone** permissions if macOS asks

## Build Locally

**Prerequisites:** [Rust](https://rustup.rs), [Node.js](https://nodejs.org), [pnpm](https://pnpm.io)

```bash
git clone https://github.com/pentoai/elevenscribe
cd elevenscribe
pnpm install
pnpm tauri build
```

The `.dmg` will be at `src-tauri/target/release/bundle/dmg/`.

For development with hot-reload:

```bash
pnpm tauri dev
```
