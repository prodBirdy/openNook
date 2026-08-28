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
  ICONSET="$(mktemp -d)/icon.iconset"
  mkdir -p "$ICONSET"
  for px in 16 32 128 256 512; do
    sips -z "$px" "$px" "$ROOT/resources/icon.png" --out "$ICONSET/icon_${px}x${px}.png" >/dev/null
    sips -z "$((px * 2))" "$((px * 2))" "$ROOT/resources/icon.png" --out "$ICONSET/icon_${px}x${px}@2x.png" >/dev/null
  done
  iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/icon.icns"
  /usr/libexec/PlistBuddy -c 'Set :CFBundleIconFile icon' "$APP/Contents/Info.plist" 2>/dev/null \
    || /usr/libexec/PlistBuddy -c 'Add :CFBundleIconFile string icon' "$APP/Contents/Info.plist"
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

# Clock App Intent shortcuts (user imports once from Settings).
if [[ -d "$ROOT/resources/shortcuts" ]]; then
  mkdir -p "$APP/Contents/Resources/shortcuts"
  cp -R "$ROOT/resources/shortcuts/." "$APP/Contents/Resources/shortcuts/"
fi

# Ad-hoc sign so the .app launches without "damaged" on this machine.
# A Developer ID identity, if present, is used instead.
if security find-identity -v -p codesigning 2>/dev/null | grep -q "Developer ID Application"; then
  IDENTITY="$(security find-identity -v -p codesigning | awk -F'\"' '/Developer ID Application/{print $2; exit}')"
  codesign --force --deep --options runtime --sign "$IDENTITY" "$APP"
else
  codesign --force --deep --sign - "$APP"
fi

echo "built $APP"
echo "run: open \"$APP\""
