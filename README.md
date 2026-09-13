# openNook

openNook is a native macOS and Linux Dynamic Island client, GPU-rendered with [GPUI](https://www.gpui.rs), that surfaces Now Playing, calendar, reminders, timers, weather, battery, a file tray, and live coding-agent status in the notch.

## What it is

openNook is a small desktop overlay. It stays out of the Dock on macOS, expands from the notch, and keeps useful information one glance away. Linux uses the same GPUI client with desktop-friendly fallbacks.

## Features

- Agents: live coding-agent status
- Media: Now Playing controls and album art
- Calendar and Reminders
- Timers
- Weather
- Battery
- Files: tray for opening and moving files
- Notes
- Speed test
- Mirror camera preview on macOS
- Terminal: an opt-in real login shell, off by default
- Tray sharing through AirDrop and LocalSend

Experimental widgets are available behind a Settings toggle.

## Install and build

Build and run the macOS client with the included Metal wrapper:

```bash
./scripts/with-metal.sh cargo run -p nook
```

For Calendar, Reminders, Camera, Location, and Automation prompts, build the app bundle:

```bash
./scripts/with-metal.sh ./scripts/bundle.sh
open target/OpenNook.app
```

Linux uses the same `nook` crate after installing the system packages listed by `scripts/linux-deps.sh`.

## Permissions

Calendar and Reminders access is requested only when those widgets are enabled. Camera access is requested when you enable Mirror. Location access is requested when Weather uses your location; manual city entry is available instead. Automation is requested when media control needs a fallback. Microphone and Speech Recognition are requested when you enable the experimental Voice recorder. Accessibility is requested for media-key and HUD interception. Full Disk Access is requested by the experimental Messages and Notifications widgets. Local Network access is requested when LocalSend is enabled.

## Keyboard

Hover or click the notch to expand. Press Esc to collapse, ⌘, to open Settings, and ⌘Q to quit.

## Platform notes

macOS provides the notch overlay, MediaRemote, EventKit, Camera, Liquid Glass, AirDrop, and AppKit file drag-out. Linux has no camera-housing notch, MediaRemote, Liquid Glass, Camera Mirror, AirDrop, AppKit drag-out, or EventKit; global hover polling is stubbed. Linux settings and the file tray remain available, and the client needs a Vulkan driver.

## Screenshots

![Linux compact view](docs/linux-03-compact.png)
![Linux expanded view](docs/linux-03-expanded.png)
![Linux Settings](docs/linux-03-settings.png)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the build, test, crate layout, and design rules.

## License

openNook is released under the MIT License. See [LICENSE](LICENSE).
