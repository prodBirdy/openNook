# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Packaging

- Linux GPUI artifact: `cargo build --release -p nook` on Ubuntu, uploaded as
  `openNook-0.3.0-x86_64-unknown-linux-gnu.tar.gz` from `.github/workflows/linux-release.yml`
  (`workflow_dispatch` or a `v*-linux` tag). Publishes `v0.3.0-linux`; does not
  retag or rewrite the macOS `v0.3.0` notes.
- This is the current GPUI product. It is not the Tauri `0.0.2a` AppImage.

Linux does not get Metal, camera-housing notch metrics, MediaRemote, Liquid
Glass, the menu-bar extra, camera Mirror, AirDrop, AppKit file drag-out,
EventKit, hide-when-maximized occupancy, or `installer.dmg`. Hover polling is
stubbed. Settings / Quit: Ctrl+, and Ctrl+Q.

## [0.3.0] - 2026-08-23

Settings rebuilt as a native sidebar window, a compact island that hugs the
camera housing, and much more reliable agent detection. macOS only.

### Added

- Settings window rebuilt as a sidebar + grouped-pane layout (System Settings
  style) on `gpui-component`, with native sliders and a transparent titlebar
- Island preview in Settings sits on the actual desktop wallpaper, captured
  from the Dock's rendered frame so dynamic and HEIC wallpapers work
- Island silhouette border color option

### Changed

- Compact island wraps the camera housing by 1px on every side so the
  hardware sits inside the island instead of on the painted edge; bottom
  corners follow the housing's rounded-rect (no more capsule rounding)
- Settings underlay pinned to Dark Aqua so Regular glass keeps its dark
  luminosity recipe in Light Mode

### Fixed

- Agent detection false alarms: Claude sessions now trust the session file's
  own busy/idle status instead of CPU noise from MCP servers and TUI repaints
- Agent status no longer flaps between Working and Waiting: only descendant
  CPU (a running tool) counts as work, above 5% and debounced over two polls
- A short-lived helper process spawned from the agent's own binary no longer
  displaces (or outlives) the real session in the agent list
- A stale session file whose pid was reused no longer labels an unrelated
  process as an agent
- A Finder drag elsewhere on screen no longer opens the island; the widened
  region is only used to capture a drag before it reaches the painted pill

### Packaging

- `./scripts/with-metal.sh ./scripts/installer.sh` produces `target/openNook-0.3.0.dmg`
- Ad-hoc signed; a Developer ID identity is used when one is on the keychain

## [0.2.0] - 2026-08-22

First **native GPUI** release of openNook. The React / Tauri WebView is gone;
the island is a GPU-rendered accessory overlay that pins into the Mac notch.

macOS only in this build. Windows and Linux installers from 0.1.0 are not
superseded here. There is no plugin system in this build — built-in widgets
only.

### Added

- Native GPUI island (transparent `PopUp` window, no dock icon, Nook menu-bar extra)
- Hardware-notch compact pill with hover, click, and scroll-to-expand
- Now Playing via MediaRemote on macOS 15.4+ (AppleScript fallback), with play / pause / skip
- GPU visualizer tinted from album artwork
- Calendar and Reminders through EventKit
- File tray: drop in, open, drag out to Finder, remove, clear
- In-island markdown notes editor
- Timers with a progress ring
- Cloudflare speed test
- Prometheus Observe widget (pinned PromQL, time-range sparklines, hover point, firing alerts)
- Camera Mirror control
- Settings window (General / Custom Widgets)
- Liquid Glass island (`NSGlassEffectView`) that yields to Reduce Transparency
- Island fill swatches (black, graphite, navy, forest, burgundy, indigo, olive)
- Island position: horizontal / vertical, attach or detach from the notch
- Hide the overlay when another app is full screen or zoomed to fill the display
- Widget cell grid: reorder, resize, per-module on/off
- Apple-parameterized springs; Reduce Motion collapses morphs to a dissolve
- `.app` bundle with Calendar / Reminders / Camera / Automation usage strings
- Drag-to-Applications DMG installer (`scripts/installer.sh`)

### Changed

- Island chrome is a flat top, concave 6px wings, and a rounded bottom (not a capsule)
- Compact and expanded widgets restyled to Nook density and type
- Notch swipe direction inverted to match the previous client

### Fixed

- Finder file drops after GPUI window chrome
- Overlay click-through vs hover hit-testing
- Spring integration exploding on slow frames / hitches

### Packaging

- `./scripts/with-metal.sh ./scripts/installer.sh` produces `target/openNook-0.2.0.dmg`
- Ad-hoc signed; a Developer ID identity is used when one is on the keychain

[0.3.0]: https://github.com/prodBirdy/openNook/releases/tag/v0.3.0
[0.2.0]: https://github.com/prodBirdy/openNook/releases/tag/v0.2.0
