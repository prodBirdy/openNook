#!/usr/bin/env bash
# Idempotent Cloud Agent setup for the openNook GPUI client on Linux.
#
# Base image: Ubuntu with rustup preinstalled. This script installs the system
# libraries GPUI needs, a software Vulkan driver + Xvfb so the GUI can run
# headlessly, a Rust toolchain new enough for edition 2024, then builds.
set -euo pipefail

cd "$(dirname "$0")/.."

# 1. System build dependencies (X11/Wayland/Vulkan headers, OpenSSL, libstdc++).
./scripts/linux-deps.sh

# 2. Headless-run extras: lavapipe gives a CPU Vulkan device and Xvfb a display,
#    so the GPU-rendered island can run without a physical GPU or compositor.
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
  mesa-vulkan-drivers vulkan-tools libgl1-mesa-dri xvfb

# 3. GPUI 0.2.2 and its deps require edition 2024, which needs Rust >= 1.85.
#    Install/refresh the stable toolchain and make it the default.
if command -v rustup >/dev/null 2>&1; then
  rustup toolchain install stable --profile minimal
  rustup default stable
fi

# 4. Build the workspace so the agent starts with a ready binary.
cargo build --workspace
