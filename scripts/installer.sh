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
