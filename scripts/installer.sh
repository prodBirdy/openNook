#!/usr/bin/env bash
# Release .app + drag-to-Applications DMG.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
if [[ -z "$VERSION" ]]; then
  echo "installer: failed to read version from Cargo.toml" >&2
  exit 1
fi
DMG="$ROOT/target/openNook-${VERSION}.dmg"
STAGE="$ROOT/target/dmg"

"$ROOT/scripts/bundle.sh" release

APP="$ROOT/target/openNook.app"

echo "After copying openNook.app to /Applications, put the CLI on PATH:"
echo "  ln -sf /Applications/openNook.app/Contents/MacOS/openNook ~/.local/bin/opennook"

rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/openNook.app"
ln -s /Applications "$STAGE/Applications"

hdiutil create \
  -volname "openNook" \
  -srcfolder "$STAGE" \
  -ov \
  -format UDZO \
  "$DMG"

rm -rf "$STAGE"

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
