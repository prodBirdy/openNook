# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.2.0]: https://github.com/prodBirdy/openNook/releases/tag/v0.2.0
