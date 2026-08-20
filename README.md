# openNook (GPUI)

Native Dynamic Island client — the same macOS/Windows/Linux island as [openNook](https://github.com/prodBirdy/openNook), with the React/Tauri WebView replaced by [GPUI](https://www.gpui.rs).

Most of the original Rust backend is reused as `nook-core`: Now Playing, EventKit calendar/reminders, file tray, notes, widgets, notch metrics, haptics, and hover hit-testing. The frontend is GPU-rendered and lives in the menu-bar notch.

## Why GPUI

openNook's Tauri shell was a transparent always-on-top WebView. GPUI gives the same overlay (transparent `WindowKind::PopUp`, no titlebar, status-item window level) without a browser process, so compact ↔ expanded animation and the visualizer run on the GPU.

UI kit: [awesome-gpui](https://github.com/zed-industries/awesome-gpui).

## Layout

```
crates/
  nook-core/   Tauri-free port of src-tauri (audio, calendar, files, db, …)
  nook/        GPUI app — compact island, expanded widgets, settings
```

## Run

macOS needs a working `metal` compiler. Xcode 26’s in-app / `xcodebuild -downloadComponent` download often hangs on “Preparing to download”. If you already extracted the toolchain to `~/Library/Developer/Metal.xctoolchain` (done once on this machine):

```bash
cd ~/openNook-gpui
./scripts/with-metal.sh cargo run -p nook
```

On macOS the process is an accessory (`LSUIElement` / `NSApplicationActivationPolicyAccessory`): no dock icon. Hover the notch to take mouse events; click or scroll up to expand.

## Features (v1)

- Compact pill matching the hardware notch (idle / media / files / timer / first-run)
- Hover expand, click or scroll to open the island
- Now Playing with play/pause/skip (MediaPlayer + simulation visualizer)
- Calendar and Reminders (EventKit)
- File tray, notes, timers, Cloudflare speed test
- Prometheus observe widget (pinned PromQL + firing alerts on the island)
- Settings window (media/calendar/reminders/observability, liquid glass, non-notch mode)

No plugin system in this build. Built-in widgets only.

## Permissions

macOS will prompt for Calendar and Reminders. Usage strings live in `Info.plist`.
