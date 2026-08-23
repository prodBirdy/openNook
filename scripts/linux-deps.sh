#!/usr/bin/env bash
# System packages needed to compile crates/nook (GPUI 0.2.2) on Debian/Ubuntu.
# Metal / with-metal.sh is macOS-only. Linux uses Vulkan (blade) + X11/Wayland.
set -euo pipefail
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
  build-essential g++ pkg-config libssl-dev \
  libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libvulkan-dev \
  libx11-dev libxrandr-dev libxi-dev libxcursor-dev libxinerama-dev \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
