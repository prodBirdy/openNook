#!/usr/bin/env bash
# Renders the README / launch-page screenshots into assets/screens with `opennook shot`.
# Each shot gets its own throwaway OPENNOOK_HOME, so your config is never read or written and
# shots don't race on a shared workspace. Live data (weather, ISS, Herdr agents, Now Playing)
# is whatever this Mac sees right now: check the agent list before committing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BIN="${BIN:-$ROOT/target/release/opennook}"
OUT="$ROOT/assets/screens"
[[ -x "$BIN" ]] || cargo build -p nook --release
mkdir -p "$OUT"

LAYOUT='{
  "island": { "width": 560, "display": "notch", "glass": false },
  "compact": ["herdr", "timer", "now-playing", "weather"],
  "expanded": ["now-playing", "weather", "world-clock", "iss", "timer", "herdr"],
  "location": { "mode": "set", "name": "Berlin", "latitude": 52.52, "longitude": 13.41, "timezone": "Europe/Berlin" }
}'

shot() {
  local name="$1"; shift
  local home; home="$(mktemp -d)"
  printf '%s\n' "$LAYOUT" > "$home/layout.json"
  OPENNOOK_HOME="$home" "$BIN" shot "$OUT/$name.png" "$@" >/dev/null
  rm -rf "$home"
  # The island sits on a transparent canvas wider and taller than it.
  swift "$ROOT/scripts/trim-png.swift" "$OUT/$name.png" 24
}

for mod in now-playing weather world-clock iss timer herdr; do
  shot "$mod" --mod "$mod" --wait 9000
done
shot closed --wait 6000
shot hover --hover --wait 6000
echo "screens: $OUT"
