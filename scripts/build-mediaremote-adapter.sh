#!/usr/bin/env bash
# Clone and build ungive/mediaremote-adapter. The framework is loaded by
# /usr/bin/perl (not linked into openNook) so MediaRemote works on macOS 15.4+.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ADAPTER_DIR="$ROOT/third_party/mediaremote-adapter"
ADAPTER_REPO="https://github.com/ungive/mediaremote-adapter.git"
# Pin to a known-good revision of https://github.com/ungive/mediaremote-adapter
ADAPTER_REV="3ac3d4bdf862c7b5399b4fba4df5689f5c38609a"

if ! command -v cmake >/dev/null; then
  echo "cmake is required to build MediaRemoteAdapter.framework" >&2
  exit 1
fi

mkdir -p "$ROOT/third_party"
if [[ ! -d "$ADAPTER_DIR/.git" ]]; then
  git clone "$ADAPTER_REPO" "$ADAPTER_DIR"
fi
git -C "$ADAPTER_DIR" fetch --quiet origin "$ADAPTER_REV" || git -C "$ADAPTER_DIR" fetch --quiet origin
git -C "$ADAPTER_DIR" checkout --quiet --detach "$ADAPTER_REV"

PATCH_DIR="$ROOT/third_party/patches"
if [[ -d "$PATCH_DIR" ]]; then
  shopt -s nullglob
  for patch in "$PATCH_DIR"/*.patch; do
    name="$(basename "$patch")"
    if git -C "$ADAPTER_DIR" apply -R --check "$patch" >/dev/null 2>&1; then
      echo "already applied $name"
      continue
    fi
    if git -C "$ADAPTER_DIR" apply --check "$patch" >/dev/null 2>&1; then
      git -C "$ADAPTER_DIR" apply "$patch"
      echo "applied $name"
    else
      echo "failed to apply $name" >&2
      exit 1
    fi
  done
fi

cmake -S "$ADAPTER_DIR" -B "$ADAPTER_DIR/build"
cmake --build "$ADAPTER_DIR/build"

FRAMEWORK="$ADAPTER_DIR/build/MediaRemoteAdapter.framework/MediaRemoteAdapter"
if [[ ! -f "$FRAMEWORK" ]]; then
  echo "MediaRemoteAdapter.framework missing after build: $FRAMEWORK" >&2
  exit 1
fi
echo "built $ADAPTER_DIR/build/MediaRemoteAdapter.framework"
