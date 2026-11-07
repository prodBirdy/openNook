#!/usr/bin/env bash
# Run cargo (or anything) with the local Metal compiler on PATH so GPUI
# can compile shaders without Xcode's stuck MetalToolchain download.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOLCHAIN="${METAL_TOOLCHAIN:-$HOME/Library/Developer/Metal.xctoolchain}"
if [[ ! -x "$TOOLCHAIN/usr/bin/metal" ]]; then
  echo "Metal compiler missing at $TOOLCHAIN" >&2
  echo "Mount ~/Downloads/MetalToolchain/MetalToolchain.dmg, then:" >&2
  echo "  ditto /Volumes/MetalToolchainCryptex/Metal.xctoolchain \"$TOOLCHAIN\"" >&2
  exit 1
fi
export PATH="$ROOT/scripts/path-shim:$PATH"
exec "$@"
