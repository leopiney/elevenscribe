#!/usr/bin/env bash
set -euo pipefail

# Wolfbud app-icon generator.
# Edit the PROMPT heredoc below and re-run to regenerate the icon source, then:
#   pnpm tauri icon ./images/icon-source.png
WRAPPER="/Users/leo/.claude/plugins/cache/pento-marketplace/growth/1.6.0/skills/image-generation/resources/gpt-image-2.py"
OUT_DIR="./images"

mkdir -p "$OUT_DIR"

# --- icon-source ---
# (Prompt is staged to a temp file so it works on macOS's stock bash 3.2, which
#  mis-parses heredocs nested directly inside $(...).)
PROMPT_FILE="$(mktemp)"
trap 'rm -f "$PROMPT_FILE"' EXIT
cat > "$PROMPT_FILE" <<'PROMPT'
macOS app icon for "Wolfbud", a friendly Mac voice-dictation utility. Modern flat
minimal vector style: clean geometric shapes, confident line weights, high contrast,
subtle depth, friendly (not aggressive or snarling). Tight palette only — deep
indigo-to-violet background gradient (#4F46E5 to #7C3AED), a pale off-white mark
(#F5F5FA), and one bright cyan accent (#22D3EE). Premium, polished, tech-forward,
the quality of a well-designed menu-bar app icon. No text, letters, or numbers
anywhere. No photorealism, no fur texture noise, no realistic microphone hardware,
no busy background, no drop-shadow clutter. Not similar to any existing brand logo.

A single bold, front-facing, symmetrical geometric wolf head as the focal mark, its
lower muzzle/chin formed by and merging into a bright cyan soundwave / audio
equalizer motif that signals voice and speech. The deep indigo-to-violet gradient
fills the entire square canvas edge-to-edge as a full-bleed app-icon background, with
the wolf head centered and generous padding so it stays legible at small sizes.
PROMPT

uv run "$WRAPPER" \
  --prompt "$(cat "$PROMPT_FILE")" \
  --output-path "$OUT_DIR/icon-source.png" \
  --aspect-ratio "1:1" \
  --quality "high" \
  --output-format "png"

echo "Done. Icon source written to $OUT_DIR/icon-source.png"
