#!/usr/bin/env bash
# Run the openNook client on a headless Linux box (no GPU, no compositor).
#
# Starts an Xvfb virtual display if none is set, forces Mesa's lavapipe CPU
# Vulkan device, then launches `nook`. On a normal desktop with $DISPLAY
# already set this just runs the client against your real display/GPU.
set -euo pipefail

cd "$(dirname "$0")/.."

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/xdg-runtime}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

STARTED_XVFB=""
if [ -z "${DISPLAY:-}" ]; then
  Xvfb :99 -screen 0 1280x720x24 -ac +extension GLX +render -noreset &
  STARTED_XVFB=$!
  export DISPLAY=:99
  sleep 2
fi

# Prefer lavapipe (CPU) so rendering works without a physical GPU. Skip this if
# you have a real Vulkan-capable GPU driver installed.
LVP_ICD=/usr/share/vulkan/icd.d/lvp_icd.json
if [ -f "$LVP_ICD" ]; then
  export VK_ICD_FILENAMES="$LVP_ICD"
fi

cargo run -p nook "$@"

if [ -n "$STARTED_XVFB" ]; then
  kill "$STARTED_XVFB" 2>/dev/null || true
fi
