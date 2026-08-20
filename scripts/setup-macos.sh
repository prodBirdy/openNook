#!/usr/bin/env bash
# Prefer the local Metal.xctoolchain. Official xcodebuild -downloadComponent
# hangs on Xcode 26 ("Beginning asset download...").
set -euo pipefail
if [[ -x "$HOME/Library/Developer/Metal.xctoolchain/usr/bin/metal" ]]; then
  echo "Metal compiler already at ~/Library/Developer/Metal.xctoolchain"
  echo "Run: ./scripts/with-metal.sh cargo run -p nook"
  exit 0
fi
echo "Missing ~/Library/Developer/Metal.xctoolchain" >&2
echo "If you have ~/Downloads/MetalToolchain/MetalToolchain.dmg:" >&2
echo "  open ~/Downloads/MetalToolchain/MetalToolchain.dmg" >&2
echo "  ditto /Volumes/MetalToolchainCryptex/Metal.xctoolchain ~/Library/Developer/Metal.xctoolchain" >&2
exit 1

