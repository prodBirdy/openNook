#!/usr/bin/env bash
# Assemble a real .app so Info.plist (LSUIElement, privacy strings, bundle id)
# actually applies. `cargo run` still works for iteration, but Calendar /
# Reminders / Automation prompts need this bundle.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PROFILE="${1:-release}"
if [[ "$PROFILE" == "release" ]]; then
  cargo build -p nook --release
  BIN="$ROOT/target/release/nook"
else
  cargo build -p nook
  BIN="$ROOT/target/debug/nook"
fi

APP="$ROOT/target/OpenNook.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$ROOT/Info.plist" "$APP/Contents/Info.plist"
if [[ -f "$ROOT/resources/icon.png" ]]; then
  cp "$ROOT/resources/icon.png" "$APP/Contents/Resources/icon.png"
fi
cp "$BIN" "$APP/Contents/MacOS/openNook"
chmod +x "$APP/Contents/MacOS/openNook"

# Bundle mediaremote-adapter (not linked; /usr/bin/perl loads the framework).
if "$ROOT/scripts/build-mediaremote-adapter.sh"; then
  ditto "$ROOT/third_party/mediaremote-adapter/build/MediaRemoteAdapter.framework" \
    "$APP/Contents/Resources/MediaRemoteAdapter.framework"
  cp "$ROOT/third_party/mediaremote-adapter/bin/mediaremote-adapter.pl" \
    "$APP/Contents/Resources/mediaremote-adapter.pl"
  cp "$ROOT/third_party/mediaremote-adapter/LICENSE" \
    "$APP/Contents/Resources/MediaRemoteAdapter.LICENSE"
else
  echo "warning: MediaRemote adapter not bundled; Now Playing will use AppleScript" >&2
fi

echo "built $APP"
echo "run: open \"$APP\""
