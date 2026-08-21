#!/usr/bin/env bash
# Release .app + drag-to-Applications DMG.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
VERSION="${VERSION:-0.2.0}"
DMG="$ROOT/target/openNook-${VERSION}.dmg"
STAGE="$ROOT/target/dmg"

"$ROOT/scripts/bundle.sh" release

APP="$ROOT/target/OpenNook.app"
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
echo "installer: $DMG"
ls -lh "$DMG"
