#!/usr/bin/env bash
# Release .app + drag-to-Applications DMG. Opened, the DMG shows a window of its own: the
# background scripts/dmg-background.swift draws, openNook.app on the left and
# Applications on the right, big icons, no toolbar. Finder lays the window out (AppleScript), so
# whatever runs this needs Automation access to Finder.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
if [[ -z "$VERSION" ]]; then
  echo "installer: failed to read version from Cargo.toml" >&2
  exit 1
fi
DMG="$ROOT/target/openNook-${VERSION}.dmg"
RW="$ROOT/target/openNook-rw.dmg"
STAGE="$ROOT/target/dmg"
VOLNAME="openNook"
# The window's content, in points (the background's size), and its frame: the title bar on top.
# The icon centres match scripts/dmg-background.swift.
WIN_W=660 WIN_H=400 FRAME_H=428
APP_X=165 APP_Y=190 APPS_X=495 APPS_Y=190

"$ROOT/scripts/bundle.sh" release

APP="$ROOT/target/openNook.app"

echo "After copying openNook.app to /Applications, put the CLI on PATH:"
echo "  ln -sf /Applications/openNook.app/Contents/MacOS/openNook ~/.local/bin/opennook"

rm -rf "$STAGE" "$DMG" "$RW"
mkdir -p "$STAGE/.background"
ditto "$APP" "$STAGE/openNook.app"
ln -s /Applications "$STAGE/Applications"
# The background, as one TIFF with both resolutions (Finder picks the one for the screen).
ART="$(mktemp -d)"
swift "$ROOT/scripts/dmg-background.swift" "$ART"
tiffutil -cathidpicheck "$ART/background.png" "$ART/background@2x.png" -out "$STAGE/.background/background.tiff" >/dev/null 2>&1
rm -rf "$ART"

# Read-write first, so Finder can write the window's layout (.DS_Store) into it. Under a name of
# its own while Finder lays it out: another "openNook" volume (an earlier DMG, open to look at)
# would make the name ambiguous to Finder. It's renamed once the layout is written.
BUILDNAME="$VOLNAME-build-$$"
hdiutil create -volname "$BUILDNAME" -srcfolder "$STAGE" -ov -fs HFS+ -format UDRW "$RW" >/dev/null
rm -rf "$STAGE"

MOUNT="/Volumes/$BUILDNAME"
DEVICE="$(hdiutil attach -readwrite -noverify -noautoopen "$RW" | awk '/Apple_HFS/ {print $1; exit}')"
for _ in $(seq 1 50); do [[ -d "$MOUNT" ]] && break; sleep 0.2; done

osascript <<EOF
tell application "Finder"
  set d to disk "$BUILDNAME"
  -- A window of its own: opening the disk can land in a tab of another Finder window (Finder's
  -- "open folders in tabs"), which keeps that window's size and ignores the bounds below.
  set w to make new Finder window to d
  set current view of w to icon view
  set toolbar visible of w to false
  set statusbar visible of w to false
  set pathbar visible of w to false
  set sidebar width of w to 0
  -- Bounds are screen points: left, top, right, bottom.
  set bounds of w to {200, 120, 200 + $WIN_W, 120 + $FRAME_H}
  set opts to icon view options of w
  set arrangement of opts to not arranged
  set icon size of opts to 128
  set text size of opts to 13
  set background picture of opts to file ".background:background.tiff" of d
  set position of item "openNook.app" of d to {$APP_X, $APP_Y}
  set position of item "Applications" of d to {$APPS_X, $APPS_Y}
  update d without registering applications
  delay 2
  close w
end tell
EOF

# The background out of sight in the window, and the name the volume ships with.
SetFile -a V "$MOUNT/.background" 2>/dev/null || chflags hidden "$MOUNT/.background"
diskutil rename "$DEVICE" "$VOLNAME" >/dev/null
sync
hdiutil detach "$DEVICE" -quiet || { sleep 2; hdiutil detach "$DEVICE" -force -quiet; }

# Once more, out of Finder's sight (it writes .DS_Store lazily, so not while it can see the volume):
# the frame Finder meant to save, since a tiling window manager on this Mac may have snapped the
# window it opened and Finder saved the snapped one (Cocoa points, from the screen's bottom-left),
# and nothing Finder needn't keep.
PRIVATE="$(mktemp -d)"
DEVICE="$(hdiutil attach -readwrite -noverify -noautoopen -nobrowse -mountpoint "$PRIVATE" "$RW" | awk '/Apple_HFS/ {print $1; exit}')"
python3 -I "$ROOT/scripts/dmg-window.py" "$PRIVATE/.DS_Store" 200 440 "$WIN_W" "$FRAME_H"
rm -rf "$PRIVATE/.fseventsd" "$PRIVATE/.Trashes"
sync
hdiutil detach "$DEVICE" -quiet || { sleep 2; hdiutil detach "$DEVICE" -force -quiet; }
rmdir "$PRIVATE" 2>/dev/null || true

hdiutil convert "$RW" -format UDZO -imagekey zlib-level=9 -o "$DMG" >/dev/null
rm -f "$RW"

if security find-identity -v -p codesigning 2>/dev/null | grep -q "Developer ID Application"; then
  IDENTITY="$(security find-identity -v -p codesigning | awk -F'\"' '/Developer ID Application/{print $2; exit}')"
  codesign --force --timestamp --sign "$IDENTITY" "$DMG"
  if [[ -n "${NOTARY_PROFILE:-}" ]]; then
    xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$DMG"
  fi
fi

echo "installer: $DMG"
ls -lh "$DMG"
