**Windows:** [Download and run](https://github.com/prodBirdy/openNook/releases/download/0.0.2a/openNook_0.1.0_x64-setup.exe)

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

`cargo run` is fine for UI iteration. Calendar, Reminders, and media Automation prompts need a real bundle (otherwise TCC has no `CFBundleIdentifier` to attach to):

```bash
./scripts/with-metal.sh ./scripts/bundle.sh
open target/OpenNook.app
```

Release installer (DMG with an Applications drop):

```bash
./scripts/with-metal.sh ./scripts/installer.sh
open target/openNook-0.2.0.dmg
```

See [CHANGELOG.md](CHANGELOG.md) for what landed in 0.2.0.

On macOS the process is an accessory (`LSUIElement` / `NSApplicationActivationPolicyAccessory`): no dock icon. Hover the notch to take mouse events; click or scroll up to expand. Quit and Settings live on the **Nook** menu-bar extra.

## Features (v1)

- Compact pill matching the hardware notch (idle / media / files / timer / observe / first-run)
- Hover expand, click or scroll to open the island
- Now Playing with play/pause/skip and a simulated GPU visualizer (MediaRemote on macOS, AppleScript fallback)
- Calendar and Reminders (EventKit)
- File tray (open / drag out to Finder / remove / clear), notes (external editor), timers, Cloudflare speed test
- Prometheus observe widget (pinned PromQL, time-range sparklines, hover point detail, firing alerts)
- Settings window (Nook chrome: General / Custom Widgets, per-module toggles, Observe metrics, liquid glass, island color, position, hide when an app fills the display, non-notch mode)

No plugin system in this build. Built-in widgets only.

## Permissions

macOS will prompt for Calendar, Reminders, and (only if MediaRemote is unavailable) Automation. Usage strings live in `Info.plist` and only apply when you run the `.app` from `scripts/bundle.sh`.

Now Playing on macOS uses [mediaremote-adapter](https://github.com/ungive/mediaremote-adapter): `/usr/bin/perl` loads a bundled helper framework and calls `MRMediaRemoteGetNowPlayingInfo` / `MRMediaRemoteSendCommand` (`kMRATogglePlayPause` 2, `kMRANextTrack` 4, `kMRAPreviousTrack` 5). That is the path that still works on macOS 15.4+. `scripts/bundle.sh` builds and copies the framework into `OpenNook.app/Contents/Resources`. `cargo run` uses the same framework from `third_party/mediaremote-adapter` after `./scripts/build-mediaremote-adapter.sh`.
