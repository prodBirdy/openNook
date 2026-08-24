# openNook — Cloud Agent Handoff

Written 2026-08-24. Source: 25 structured feasibility reports (two research fan-outs over this repo and current macOS), plus performance work landed the same day. Each work package below is self-contained: scope, concrete approach, exact APIs, files to touch, battery rules, honest blockers, and a definition of done. Pick a package, read the ground rules first, ship it.

## What openNook is

A Rust "Dynamic Island" notch overlay for macOS, built on GPUI (Zed's UI framework, crates.io `gpui 0.2.2`). Workspace crates:

- `crates/nook` — the app: `src/island/` (island UI: `mod.rs` state + poll loops, `render.rs`, `compact.rs`, `expanded.rs`, `media.rs`, `files.rs`, `chrome.rs`, `settings.rs`), `src/platform.rs` (AppKit interop via `objc2` `msg_send!`), `src/widgets/` (expanded-view cards), `src/motion.rs` (springs).
- `crates/nook-core` — platform-independent core: `audio.rs` (now-playing via AppleScript fallback + optional bundled MediaRemote adapter), `calendar.rs` (EventKit), `agents.rs` (Claude/Codex/Cursor session detection), `files.rs` (file tray), `notch.rs` (screen/overlay geometry), `mouse.rs` (global cursor polling + hit tests), `observe.rs` (service monitoring), `settings.rs`.

Already shipped: media controls with artwork + scrubber, local timers, calendar/reminders, agent status with dot-matrix loader, file tray with drag in/out, screen mirror, notes card, speed test, Settings window, Liquid-Glass island option.

Distribution: dev-signed/unsigned, outside the App Store, unsandboxed accessory app (no dock icon). Private frameworks are an option where they still work; restricted **entitlements are not** (anything needing `com.apple.private.*` or provisioning-gated entitlements is off the table).

## Build, test, measure

```sh
cargo test -p nook -p nook-core        # full test suite (~166 tests, must stay green)
cargo build --release -p nook          # release binary at target/release/nook
scripts/bundle.sh                      # app bundle (bundles the MediaRemote adapter + licenses)
```

Battery check (the bar every package must clear — no regression at idle):

```sh
./target/release/nook & PID=$!; sleep 10
T0=$(ps -p $PID -o cputime= ); sleep 30; T1=$(ps -p $PID -o cputime= )
# (T1-T0)/30 = average cores. Idle (no media, no agents, cursor away) must not grow.
```

Memory check: `footprint <pid>` — the overlay window is a top strip, ~160 MB total is the current normal (~117 MB of that is the Metal device floor).

## Ground rules (read before coding)

1. **Event-driven or nothing.** No new polling loops. macOS pushes events for nearly everything: `AudioObjectAddPropertyListenerBlock` (CoreAudio), `SCDynamicStore` (network), `notify_register_dispatch` (power), `NSWorkspace.notificationCenter` (apps/wake/space), `NSDistributedNotificationCenter` (Spotify/Music), FSEvents (files), `EKEventStoreChangedNotification` (calendar). Wire callbacks into a `tokio::sync::watch` channel or an `AtomicBool` the existing island tick consumes. If a package genuinely needs sampling, it must be gated on visibility (only while its card/face is on screen).
2. **The island tick is adaptive.** `spawn_loops` in `island/mod.rs` runs at 20 ms only while something is live (hover, springs, drags, media, agents working) or the cursor is within the 96 px near-zone (`nook_core::mouse::hit_test_near`); otherwise 80 ms. Don't add work to the tick body; don't reintroduce per-tick AppKit calls (accessibility flags are cached 1 s in `platform.rs`; settings are re-cloned only when `nook_core::settings::settings_generation()` moves).
3. **Never resize or restyle the window inside `render`.** `Window::resize` fires `windowDidResize` synchronously and re-enters GPUI's window RefCell — the resize is silently dropped ("RefCell already borrowed") and layout breaks. The pattern that works: publish state, then `cx.spawn` a foreground task that calls `platform::pin_island_windows()` (see `spawn_strip_resize` in `island/render.rs`).
4. **The overlay window is a strip, not the screen.** Full display width, top-anchored, height published via `nook_core::notch::set_overlay_height` (quantized in `quantized_overlay_height`). Screen coordinates == window coordinates. The strip pre-grows when the cursor approaches (`cursor_near`) so an NSWindow resize never lands mid-animation (it shows one stretched frame if it does); shrinks only when springs are parked. If your feature draws taller content (HUDs, panels), feed its bottom edge into `sync_overlay_strip`'s `bottom` computation — don't resize windows yourself.
5. **Smoothness beats battery for visible animation.** Owner's explicit call: the dot-matrix agent loader repaints per tick (50 fps) and the media visualizer runs 30 fps — do not throttle visible animation rates. Battery wins must come from idle behavior and polling, not animation.
6. **Spring/dt discipline.** `SpringValue::step` substeps internally (stable at any dt), but perceived dt is capped at 1/30 s in the tick so an idle tick doesn't fast-forward a fresh animation. Reuse `motion::MORPH`/`CROSSFADE`/`REVEAL` springs; follow the reduce-motion branch (`platform::reduce_motion()` — HIG: motion must be optional).
7. **AppKit interop style.** Raw `objc2` `msg_send!` in `platform.rs` (nook) or per-module `#[cfg(target_os = "macos")]` blocks (nook-core), `block2::RcBlock` for ObjC blocks, `extern "C"` + `#[link(name = "...", kind = "framework")]` for C frameworks. `objc2-foundation`/`objc2-event-kit` crates are precedent for adding `objc2-*` framework crates.
8. **TCC reality.** The app is dev-signed: TCC grants are keyed to the signature and vanish on re-sign. Every permission-gated feature must degrade gracefully when denied (detect, hide/disable, hint in Settings). Info.plist usage strings go through `scripts/bundle.sh`.
9. **UI placement conventions.** Ambient status → compact face (`CompactMode` enum + `compact.rs` arms + `available_modes()` priority list). Rich interaction → expanded card (`widgets/` + registration in `widgets/mod.rs` + enable toggle in settings). Transient feedback → HUD takeover of the compact face with auto-dismiss. Every feature gets a Settings toggle.
10. **Tests.** Pure logic gets unit tests in-module (see `island/mod.rs` `test_island()` — new `Island` fields must be added there too). `cargo test -p nook -p nook-core` green is non-negotiable.

## State landed on 2026-08-24 (uncommitted, on `main`)

- **Memory**: overlay window shrunk from full-screen (432 MB) to an island-tracking strip (~160 MB); strip pre-grows on cursor approach, shrinks at rest; deferred AppKit-side resize (never via `Window::resize` in render).
- **Idle CPU**: timer branch no longer dirties every second; media app detection via `NSRunningApplication` instead of full process-table scans; agents scan fetches argv/exe once per pid (`OnlyIfNotSet`) and polls 2 s→6 s adaptively; drag-pasteboard XPC only while the left button is down; accessibility flags cached 1 s; settings cloned only on generation change; adaptive 20/80 ms tick; now-playing polls 1 s playing / 5 s idle with instant wake via `NSDistributedNotificationCenter` observers (`install_media_observers` in `platform.rs`) and `nook_core::audio::note_media_event()` pokes from transport commands.
- **Known cost by design**: while an agent is working or media plays, the island renders continuously (see ground rule 5).
- Working tree also carries an unrelated in-progress content-transition animation (`content_x/content_y` in `island/mod.rs`, `motion.rs`) — don't revert it.

## Cross-package notes

- WP01 (perf backlog) contains the remaining infrastructure several packages want: the **MediaRemote adapter `stream --diff` long-lived subprocess** (kills per-poll perl/osascript spawns; media packages should build on it) and the **NSEvent global mouse monitor** to replace the `nook-mouse` polling thread.
- Packages sharing plumbing: WP02 + WP11 + WP12 all touch CoreAudio (share `audio_devices.rs`/`sysvol.rs` bindings); WP04/WP18 share `NSDistributedNotificationCenter` helpers; WP10/WP17/WP18 build on the media pipeline.
- One agent per package. If two packages share a file (`island/mod.rs` is the hub), coordinate or serialize — merge conflicts there are expensive.

## Priority index
| WP | Wave | Feature | Feasibility | Effort |
|----|------|---------|-------------|--------|
| WP01 | W0 | [Performance / battery backlog (remaining items)](#wp01--perf-backlog) | yes | L |
| WP02 | W1 | [Volume & brightness HUDs in the island](#wp02--volume-brightness-hud) | yes | M |
| WP03 | W1 | [Live VPN status with session timer](#wp03--vpn-status) | yes | M |
| WP04 | W1 | [Natural-language calendar & reminder entry](#wp04--calendar-nl-entry) | yes | M |
| WP05 | W1 | [Battery alerts + one-tap Low Power Mode](#wp05--battery-alerts-lpm) | yes | M |
| WP06 | W1 | [High Alert (keep-awake) + Pomodoro](#wp06--focus-tools) | yes | M |
| WP07 | W1 | [Weather on the shelf](#wp07--weather) | yes | M |
| WP08 | W1 | [System Stats on the shelf](#wp08--system-stats) | yes | M |
| WP09 | W1 | [Obsidian vault on the shelf](#wp09--obsidian-shelf) | yes | M |
| WP10 | W1 | [Time-synced lyrics](#wp10--lyrics) | yes | M |
| WP11 | W2 | [Audio output / AirPlay picker](#wp11--airplay-output-picker) | partial | M |
| WP12 | W2 | [Per-app volume mixer](#wp12--audio-per-app) | yes | L |
| WP13 | W2 | [Automation entry points (Alfred / URL scheme / Finder / shell)](#wp13--automation-entry) | yes | L |
| WP14 | W2 | [LocalSend LAN sharing + drop-a-link](#wp14--lan-sharing) | yes | L |
| WP15 | W2 | [Keyboard sounds + smooth scrolling (Mechey / LiquidMouse)](#wp15--input-feel) | yes | L |
| WP16 | W2 | [Voice recordings with live transcription](#wp16--voice-memos) | yes | L |
| WP17 | W2 | [Live motion art (animated album covers)](#wp17--live-motion-art) | yes | L |
| WP18 | W2 | [Full media player with Playing Next queue](#wp18--media-queue) | partial | L |
| WP19 | W2 | [Meeting controls (Zoom / Teams / Meet)](#wp19--meetings) | partial | L |
| WP20 | W3 | [File-processing suite (Convert / Compress / BG-removal / OCR)](#wp20--file-processing) | yes | XL |
| WP21 | W3 | [Universal search + clipboard history (Thunderstorm)](#wp21--search-clipboard) | yes | XL |
| WP22 | W3 | [Window Snap + menu-bar hiding (Thaw)](#wp22--window-management) | partial | XL |
| WP23 | W3 | [iMessage / WhatsApp replies](#wp23--messaging-replies) | partial | L |
| WP24 | W3 | [Notification shelf (other apps’ notifications)](#wp24--notification-shelf) | partial | L |
| WP25 | W3 | [Apple Clock timer sync](#wp25--clock-timers) | partial | L |

Waves: **W0** = do first (perf debt already scoped) · **W1** = high value, moderate effort, few permission hurdles · **W2** = valuable, larger or permission-gated · **W3** = large or partially blocked; read the risks before committing. Feasibility "partial" means part of the marketed feature is achievable and the rest is blocked — the package says exactly which part.

---

# Work packages

## WP01 — perf-backlog

**Performance / battery backlog (remaining items)**

- **Wave:** W0 · **Feasibility:** yes · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Idle CPU / battery audit of openNook (~8-11% idle) with ranked fixes

### Approach

Profiler evidence (idle-sample.txt, 3715 x 1ms samples, macOS 26.6.1): (a) main thread ~180/3715 samples (~4.8% of a core) servicing the GPUI dispatch source — the island is re-rendering while 'idle', including per-frame NSAppearance/NSDynamicNamedColor -> CoreUI color resolution; (b) the background-executor queue (DispatchQueue_16, ~289 samples) is dominated by two full process-table scans (sysctl KERN_PROCARGS2 + proc_pidinfo/proc_pidpath, ~110 samples ~3% of a core) — one from agents::snapshot() every 2s with argv/cwd/exe UpdateKind, one from audio::get_now_playing()'s own sysinfo System refreshing ALL processes every 400ms — plus posix_spawn + 167 samples blocked in poll() waiting on the media child process; (c) nook-mouse thread ~50 samples (~1.3%): NSEvent mouseLocation (SkyLight), NSPasteboard 'Apple CFPasteboard drag' changeCount (an XPC dispatch_sync to the pasteboard server every 20-33ms), pressedMouseButtons; (d) Metal/CoreAnimation queues ~113 samples (~3%) — real frames being drawn at idle, driven by the tick loop's unconditional dirty=true every second (island/mod.rs ~line 413-429 sets dirty in the timer branch even when no timer runs) and any now-playing churn. The single biggest cost is NOT in this sample at all: the media poll spawns a fresh /usr/bin/perl (MediaRemote adapter 'get --now', mediaremote.rs run() line 183) or /usr/bin/osascript (audio.rs run_osascript line 666) child every 400ms while media plays (2s only after 8 idle polls) — each spawn loads the framework/AppleScript runtime and talks to mediaremoted, costing 20-60ms of CPU in the child, i.e. several percent system-wide charged to perl/osascript, not nook.

Ranked fixes: (1) Media: replace per-poll spawning with ONE long-lived adapter process running the adapter's 'stream' command (ungive/mediaremote-adapter supports `perl mediaremote-adapter.pl FRAMEWORK stream --diff`, which subscribes to kMRMediaRemoteNowPlayingInfoDidChangeNotification inside the entitled perl host and emits a JSON line only on change). Read its stdout on one thread, publish into a watch/crossbeam channel, delete the 400ms poll loop; interpolate elapsed_time locally from the last event + Instant while the media face is visible. AppleScript fallback (no adapter bundled): drop to 5s cadence, detect Spotify/Music via NSWorkspace didLaunch/didTerminateApplicationNotification instead of the full sysinfo scan, and prefer DistributedNotificationCenter observers for 'com.spotify.client.PlaybackStateChanged' and 'com.apple.Music.playerInfo' (both apps broadcast track changes — zero polling; this is how MediaMate/older notch apps did it). (2) Agents: two-stage scan — proc_listallpids + proc_pidpath first (cheap), fetch argv (sysctl KERN_PROCARGS2) and cwd only for candidate binaries (claude/codex/cursor names) and only for pids not already cached by (pid, start_time); stretch poll_interval() (agents.rs:974) to 5s when no session is active. (3) Tick loop: make the 20ms tick (island/mod.rs:287-292) adaptive — 20ms only while hovered || expanded || spring animating || file_drag || mirror_on, else 150-250ms; fix the timer branch to set dirty only when a timer is actually running; cache reduce_motion/reduce_transparency behind NSWorkspace.notificationCenter's accessibilityDisplayOptionsDidChangeNotification instead of msg_send per tick (platform.rs:1328/1347); gate occupancy::frontmost_fills_display() (CGWindowListCopyWindowInfo, occupancy.rs CACHE_MS=250) on settings.hide_when_maximized and refresh it from NSWorkspace didActivateApplicationNotification + a 1s floor; add a generation AtomicU64 to settings so the per-tick get_app_settings() clone+compare short-circuits. (4) Mouse: replace the nook-mouse polling thread (mouse.rs:176-196) with NSEvent addGlobalMonitorForEventsMatchingMask: (MouseMoved|LeftMouseDragged|LeftMouseUp) plus a local monitor — no TCC required for mouse-event global monitors, zero wakeups when the pointer is still (boring.notch does exactly this); reorder file_drag_active() (files.rs:158) to read pressedMouseButtons (cheap SLS shared-memory) BEFORE touching NSPasteboard changeCount (XPC round-trip). (5) Render hygiene: cache resolved appearance colors per NSAppearance change so idle frames stop walking CoreUI; ensure audio_levels comparison can't dirty when static. (6) spawn_pin's 2s pin loop (island/mod.rs:266-285): re-pin from NSApplicationDidChangeScreenParametersNotification + NSWorkspace.activeSpaceDidChangeNotification instead. Expected outcome: nook idle CPU under ~1%, plus elimination of the perl/osascript spawn-every-400ms storm that never shows up in nook's own Activity Monitor number.

### APIs

- mediaremote-adapter `stream --diff` long-lived subprocess (wraps private MediaRemote.framework: MRMediaRemoteRegisterForNowPlayingNotifications / kMRMediaRemoteNowPlayingInfoDidChangeNotification, hosted by entitled /usr/bin/perl) — replaces per-poll `get --now` spawns
- DistributedNotificationCenter: com.spotify.client.PlaybackStateChanged, com.apple.Music.playerInfo (event-driven now-playing fallback, no TCC)
- NSWorkspace.notificationCenter: NSWorkspaceDidLaunchApplicationNotification / NSWorkspaceDidTerminateApplicationNotification (app-running detection without process scans)
- NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification (cache reduce_motion/reduce_transparency instead of per-tick msg_send)
- NSEvent addGlobalMonitorForEventsMatchingMask:handler: + addLocalMonitorForEventsMatchingMask: with NSEventMaskMouseMoved|LeftMouseDragged|LeftMouseUp (no Accessibility TCC needed for mouse masks)
- NSEvent pressedMouseButtons (cheap SkyLight SLSInputButtonState read — check before pasteboard XPC)
- proc_listallpids / proc_pidpath / proc_pidinfo(PROC_PIDVNODEPATHINFO) / sysctl KERN_PROCARGS2 (targeted two-stage agent scan instead of sysinfo refresh-all)
- CGWindowListCopyWindowInfo (keep, but gate on hide_when_maximized and drive from NSWorkspaceDidActivateApplicationNotification)
- NSApplicationDidChangeScreenParametersNotification + NSWorkspace.activeSpaceDidChangeNotification (replace 2s window-pin loop)
- CVDisplayLink (already GPUI-managed; stop feeding it idle dirty frames)

### Permissions / TCC

No new TCC prompts or entitlements for any recommended fix: NSEvent global monitors for mouse-move/drag masks need no Accessibility grant (only keyboard masks do); DistributedNotificationCenter and NSWorkspace notifications are unrestricted; the MediaRemote adapter keeps its current posture (private framework loaded by entitled /usr/bin/perl — works unsigned, already bundled per scripts/build-mediaremote-adapter.sh); proc_pidinfo/sysctl on other processes needs no permission for the fields used. Existing Automation (AppleEvents) prompts for the AppleScript fallback become rarer, not new.

### Integration map (files to touch)

- /Users/jonasvogel/openNook/crates/nook-core/src/mediaremote.rs — add a `stream` backend: spawn one persistent `perl mediaremote-adapter.pl FRAMEWORK stream --diff`, reader thread -> latest-track cell + change flag; keep run(["get","--now"]) only as a startup primer; restart child on exit with backoff
- /Users/jonasvogel/openNook/crates/nook-core/src/audio.rs — get_now_playing() (line ~310): consume the stream cell instead of spawning; delete the per-call sysinfo System full-process refresh (lines ~327-355); AppleScript fallback path (run_osascript, line 666) drops to 5s and gates on NSWorkspace launch/terminate notifications + DistributedNotificationCenter observers (new fn setup_media_notifications())
- /Users/jonasvogel/openNook/crates/nook/src/island/mod.rs — spawn_loops (line 287): adaptive tick interval (20ms active / 150-250ms idle); timer branch (~413-429) sets dirty only if a timer is running; media loop (466-519) becomes change-flag driven, no 400ms poll; spawn_pin (266-285) replaced by notification-driven re-pin
- /Users/jonasvogel/openNook/crates/nook-core/src/mouse.rs — replace start_polling() thread (176-196) with NSEvent global+local monitors writing the same MOUSE_X/MOUSE_Y/DRAG_ACTIVE atomics (monitors must be installed on the main thread; do it from app launch in crates/nook/src/platform.rs or main.rs)
- /Users/jonasvogel/openNook/crates/nook-core/src/files.rs — file_drag_active() (line 158): check pressedMouseButtons before NSPasteboard changeCount to skip the XPC call when no button is down
- /Users/jonasvogel/openNook/crates/nook-core/src/agents.rs — snapshot() (line 130): two-stage pid scan with (pid,start_time) argv/cwd cache; poll_interval() (line 974): 5s when no active session, 2s while one is working
- /Users/jonasvogel/openNook/crates/nook-core/src/occupancy.rs — raise CACHE_MS to 1000 and add an early-out when hide_when_maximized is off (caller passes the flag or reads settings)
- /Users/jonasvogel/openNook/crates/nook/src/platform.rs — reduce_motion()/reduce_transparency() (1328/1347): back with AtomicBool caches refreshed by an accessibilityDisplayOptionsDidChange observer; new module or extension: notification observer registration (objc2 block2 blocks)
- /Users/jonasvogel/openNook/crates/nook-core/src/settings.rs — add SETTINGS_GEN: AtomicU64 bumped in update_app_settings/update_window_settings so the island tick skips clone+compare when unchanged
- New module suggestion: crates/nook-core/src/events.rs — central NSWorkspace/DistributedNotificationCenter observer registry so audio, occupancy, and pinning all share it; no UI changes — this is all under the compact/expanded faces

### Battery requirements

Target: <1% nook CPU at true idle and zero periodic subprocess spawns. Wins by rank: (1) media stream instead of 2.5 spawns/s of perl/osascript — biggest system-wide win, largely invisible in nook's own CPU column today; (2) process-scan diet (~3% of a core now); (3) adaptive tick + no unconditional 1s dirty — stops idle Metal frames (~3% across Metal/CA queues) and main-thread render work (~4.8%); (4) event-driven mouse — removes 30-50 wakeups/s and the per-tick pasteboard XPC (~1.3%); (5) color caching trims per-frame CoreUI walks; (6) notification-driven pinning removes a 2s wakeup. Everything replacing a poll is a real event source (MediaRemote notification via adapter stream, DistributedNotificationCenter, NSWorkspace notifications, NSEvent monitors), so idle cost is genuinely zero, not just slower polling. Note the visualizer sim thread already sleeps 200ms when not playing — fine; while playing, 30fps animation is inherent UI cost. Verify with `sample nook 10` (expect main thread >99% mach_msg) and `sudo powermetrics --samplers tasks` to confirm the perl/osascript children are gone and wakeups/s drop from ~100 to <5.

### Risks & honest blockers

MediaRemote adapter stream: the child can die (perl update, mediaremoted restart, macOS point release breaking the perl entitlement trick as Apple did to direct linking in 15.4) — needs supervised restart with backoff and silent fallback to slow AppleScript polling; on some macOS 26 builds the adapter's first connect can stall, so keep a watchdog timeout. NSEvent global monitors deliver mouseMoved for every screen move — per-event cost while the user moves (correlated with activity, fine for battery, but keep the handler to two atomic stores); monitors also stop delivering during some secure-input/full-screen game scenarios, so keep a 250ms safety poll as backstop for hover exit. Spotify/Music distributed notifications are app-specific and won't cover Safari/YouTube — that path stays poll-based (slow) unless the adapter is present. Two-stage agent scan must not regress detection of agents launched inside terminals with unusual argv (keep existing detection tests, agents.rs tests at line ~980+). Adaptive tick must still tighten immediately on hover — drive the switch from the mouse-event monitor, not from the slow tick, or the island will feel laggy to open.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP02 — volume-brightness-hud

**Volume & brightness HUDs in the island**

- **Wave:** W1 · **Feasibility:** yes · **Effort:** M (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Island volume/brightness HUDs replacing the system bezels (intercept or observe value changes, show slider HUD in the notch, suppress the system OSD)

### Approach

Build it in two decoupled layers. Layer 1 (observation, zero permissions, fully event-driven): for volume, register CoreAudio property listeners — `AudioObjectAddPropertyListenerBlock` on `kAudioObjectSystemObject` for `kAudioHardwarePropertyDefaultOutputDevice` (device swaps), and on the current default output device for `kAudioHardwareServiceDeviceProperty_VirtualMainVolume` (fallback: per-channel `kAudioDevicePropertyVolumeScalar`) and `kAudioDevicePropertyMute`, output scope. Read with `AudioObjectGetPropertyData`, set (for the draggable island slider) with `AudioObjectSetPropertyData`. For brightness, `dlopen` the private `/System/Library/PrivateFrameworks/DisplayServices.framework` and `dlsym` `DisplayServicesGetBrightness`, `DisplayServicesSetBrightness`, and `DisplayServicesRegisterForBrightnessChangeNotifications` (this is exactly what MonitorControl does; it works on Apple Silicon internal panels through Sequoia — verify the symbols still resolve on Tahoe/26 at startup and degrade to hiding the brightness HUD if not). Every value change fires a callback; forward it through a tokio `watch` channel into the Island entity, show a transient HUD (icon + slider) on the compact face, auto-dismiss after ~1.5 s via the existing spring/motion system. Layer 2 (bezel suppression): default to the SlimHUD technique, which needs no TCC at all — run `launchctl kickstart gui/$UID/com.apple.OSDUIHelper` to ensure the daemon is up, then `pkill -STOP OSDUIHelper` so launchd considers it alive but it can never draw; on app quit (Drop guard + SIGTERM handler) send SIGCONT and `launchctl kickstart -k` to restore the system bezel, and re-assert suppression on `NSWorkspaceDidWakeNotification` since launchd can respawn it. With this design the system still processes the volume/brightness keys (you never touch the keyboard), your CoreAudio/DisplayServices listeners fire, and only your island HUD is visible — this is how SlimHUD and boring.notch do HUD replacement without Accessibility.

Optional Layer 2b (true key interception — only if you later want to change key behavior, e.g. custom steps or per-app routing): an active `CGEventTapCreate` at `kCGSessionEventTap` with `kCGEventTapOptionDefault` and mask `1 << NX_SYSDEFINED` (event type 14), filtering subtype 8 (`NX_SUBTYPE_AUX_CONTROL_BUTTONS`) and key codes `NX_KEYTYPE_SOUND_UP`=0, `SOUND_DOWN`=1, `MUTE`=7, `BRIGHTNESS_UP`=2, `BRIGHTNESS_DOWN`=3 (handle the key-down/repeat bits packed in `data1`). Return NULL from the callback to swallow the event, then apply the delta yourself via CoreAudio / `DisplayServicesSetBrightness`. This requires the Accessibility TCC grant (`AXIsProcessTrustedWithOptions`), which an unsigned/dev-signed app can hold — but the grant is keyed to the code signature, so ad-hoc re-builds drop it (sign with a stable Developer ID or self-signed cert to keep it). NotchNook takes this route (it prompts for Accessibility for HUD replacement); the SIGSTOP route is strictly better for openNook's default because it also catches Control Center / menu-bar slider changes, which a key tap never sees. Recommendation: ship Layer 1 + SIGSTOP suppression behind a "Replace system volume/brightness HUD" settings toggle (default off), event tap deferred.

### APIs

- CoreAudio: AudioObjectAddPropertyListenerBlock / AudioObjectGetPropertyData / AudioObjectSetPropertyData on kAudioObjectSystemObject + default output device (kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwareServiceDeviceProperty_VirtualMainVolume, kAudioDevicePropertyVolumeScalar, kAudioDevicePropertyMute)
- DisplayServices.framework (private): DisplayServicesGetBrightness, DisplayServicesSetBrightness, DisplayServicesRegisterForBrightnessChangeNotifications — dlopen/dlsym, same as MonitorControl (private)
- CoreDisplay.framework: CoreDisplay_Display_SetUserBrightness / CoreDisplay_Display_GetUserBrightness as fallback setter (private)
- OSDUIHelper suppression: /bin/launchctl kickstart gui/$UID/com.apple.OSDUIHelper + SIGSTOP via pkill -STOP OSDUIHelper; SIGCONT + kickstart -k to restore (SlimHUD technique; undocumented behavior, not an API) (private)
- NSWorkspace.sharedWorkspace.notificationCenter — NSWorkspaceDidWakeNotification to re-assert suppression after sleep
- Optional interception: CGEventTapCreate(kCGSessionEventTap, kCGEventTapOptionDefault, mask 1<<14 NX_SYSDEFINED), subtype 8 NX_SUBTYPE_AUX_CONTROL_BUTTONS, key codes NX_KEYTYPE_SOUND_UP/DOWN=0/1, MUTE=7, BRIGHTNESS_UP/DOWN=2/3; AXIsProcessTrustedWithOptions for the Accessibility prompt
- CGDirectDisplayID via CGMainDisplayID() for the internal panel

### Permissions / TCC

Recommended path (observe + OSDUIHelper SIGSTOP): no TCC prompts, no entitlements, works unsigned — CoreAudio listeners and DisplayServices private symbols are unrestricted, and OSDUIHelper runs as the same user so signaling it is allowed. Optional event-tap path: Accessibility (kTCCServiceAccessibility) via AXIsProcessTrustedWithOptions prompt; note a listen-only tap would instead need Input Monitoring (kTCCServiceListenEvent), and any TCC grant is lost whenever an ad-hoc signature changes on rebuild — use a stable signing identity if you ship this. No Full Disk Access, no admin, no restricted entitlements anywhere.

### Integration map (files to touch)

- NEW crates/nook-core/src/sysvol.rs — CoreAudio volume/mute listener + getter/setter, extern "C" bindings (link CoreAudio in build.rs or add coreaudio-sys); exposes a tokio watch::Receiver<HudEvent> from nook_core::runtime()
- NEW crates/nook-core/src/brightness.rs — dlopen DisplayServices, symbol probe at startup (feature-detect Tahoe breakage), get/set + change-notification callback feeding the same channel
- NEW crates/nook-core/src/osd.rs — OSDUIHelper suppress/restore (launchctl kickstart + SIGSTOP/SIGCONT), Drop guard wired into app shutdown, re-assert on wake
- crates/nook-core/src/lib.rs — register the three modules
- crates/nook-core/src/settings.rs — add replace_system_hud: bool (and maybe hud_shows_brightness/volume toggles)
- crates/nook/src/island/mod.rs — add hud: Option<HudState { kind: Volume|Mute|Brightness, value: f32, shown_at: Instant }> to Island; in spawn_loops add one cx.spawn that awaits the watch channel (event-driven, no timer) and sets/refreshes the HUD with notify + auto-dismiss delay
- crates/nook/src/island/compact.rs — render the HUD as a temporary compact-face takeover: kind icon on compact_left, slider fill bar on compact_right (mirroring the iPhone island volume HUD); drag on the slider calls sysvol::set_volume / brightness::set
- crates/nook/src/motion.rs — reuse existing springs for HUD fade/slide; slider fill animates with the same easing
- crates/nook/src/island/settings.rs — settings row + first-enable explainer for the bezel-suppression toggle

### Battery requirements

Near-zero idle cost by construction: CoreAudio property listeners and DisplayServices brightness notifications are pure callbacks — no polling thread, no wakeups until the user actually changes a value. OSDUIHelper suppression is a one-shot action re-asserted only on the wake notification. The only animation cost is the HUD's ~1.5 s visible window, driven by the island's existing frame loop that already runs when the face changes. If DisplayServices notifications prove unreliable on Tahoe, do NOT fall back to continuous polling — sample brightness at 4–8 Hz only for ~2 s after a HUD trigger or while the expanded card is open. Avoid the CGEventTap variant if battery is paramount: an active tap inserts your process into the HID event path for the masked event types (cheap, but nonzero and adds latency risk if your callback stalls — macOS disables slow taps via kCGEventTapDisabledByTimeout, which you must handle by re-enabling).

### Risks & honest blockers

1) Biggest unknown: whether OSDUIHelper is still the bezel renderer on macOS Tahoe/26 — it survived through Sequoia, but Tahoe's redesigned HUD needs verification (`launchctl print gui/$UID/com.apple.OSDUIHelper`, then test the SIGSTOP trick); if Apple moved OSD into another process the suppression silently stops working and users see double HUDs — detect by checking the process exists before claiming suppression, and degrade to 'show island HUD alongside system bezel'. 2) Suppressing OSDUIHelper kills ALL bezels including caps-lock and keyboard-backlight — either accept it (document in settings) or also render those (keyboard backlight events are only visible via the event-tap path). 3) DisplayServices is private; symbols could vanish in an update — dlsym-probe and disable brightness HUD gracefully. 4) External displays: DisplayServices only drives the internal panel; DDC/CI for externals is MonitorControl-scale scope — explicitly out of scope for v1. 5) Event-tap variant: TCC grant evaporates on every ad-hoc re-sign, and a stalled callback gets the tap disabled by timeout. 6) If the app crashes while OSDUIHelper is SIGSTOPped, the user has no system bezel until launchd kickstarts it — mitigate with a crash-safe restore (spawn-on-start check that SIGCONTs any stopped OSDUIHelper).

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP03 — vpn-status

**Live VPN status with session timer**

- **Wave:** W1 · **Feasibility:** yes · **Effort:** M (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Live VPN status in the island: event-driven up/down detection (utun/SCDynamicStore) with connected-session timer

### Approach

Fully feasible with public APIs, no TCC prompts, no entitlements. Do NOT use NEVPNManager: it only observes VPN configurations owned by your own app and requires the restricted com.apple.developer.networking.networkextension entitlement (provisioning-profile gated, unavailable to a dev-signed accessory app) — and it can't see Tailscale/WireGuard/Viscosity anyway. The right mechanism is SystemConfiguration's SCDynamicStore, which is what menu-bar network indicators (iStat Menus-style) and Mullvad's own daemon use: create a store with SCDynamicStoreCreate, call SCDynamicStoreSetNotificationKeys with regex patterns ["State:/Network/Interface/utun[0-9]+/IPv4", "State:/Network/Interface/utun[0-9]+/IPv6", "State:/Network/Service/.*/PPP", "State:/Network/Service/.*/IPSec", "State:/Network/Global/IPv4"], attach via SCDynamicStoreCreateRunLoopSource to a CFRunLoop on one dedicated parked thread. The callback fires only when addresses/interfaces change — zero polling. In Rust, use the Mullvad-maintained `system-configuration` crate (safe SCDynamicStore + callback bindings), so no hand-rolled CF FFI is needed; a pure-libc fallback is a PF_ROUTE routing socket (socket(PF_ROUTE, SOCK_RAW, 0) + blocking read for RTM_NEWADDR/RTM_DELADDR/RTM_IFINFO), which needs no frameworks at all.

On each event (and once at startup), classify with getifaddrs(): a utun*/ipsec*/ppp* interface counts as an active VPN when it is UP|RUNNING and holds a routable (non-fe80::) address — this filters out the 3-4 system utuns macOS always creates, which carry only IPv6 link-local addresses. (iCloud Private Relay does not create a utun, so no false positive there.) To get a human-readable name, on transition only, read the dynamic store keys Setup:/Network/Service/<id> (UserDefinedName) matched to the utun via State:/Network/Service/<id>/IPv4 InterfaceName — this names IKEv2/personal-VPN and NETunnelProvider services (Tailscale, WireGuard app); shelling out to `scutil --nc list` and grepping "(Connected)" is an acceptable transition-time fallback. Raw-utun clients (Tunnelblick/OpenVPN) get a generic "VPN · utunX" label. Session timer: the monitor records SystemTime at each down→up transition; if the VPN is already up when openNook launches there is no reliable connect timestamp (ifi_lastchange is not populated on macOS), so show "Connected" and start counting from first observation, optionally probing `scutil --nc status` extended dict which carries a connect time for NE/PPP services. Comparable notch apps (boring.notch, NotchNook, Droppy) do not ship VPN status; this is standard menu-bar-utility territory (SCDynamicStore or NWPathMonitor's usesInterfaceType(.other), which is a simpler but nameless alternative signal).

### APIs

- SystemConfiguration.framework: SCDynamicStoreCreate / SCDynamicStoreSetNotificationKeys / SCDynamicStoreCreateRunLoopSource / SCDynamicStoreCopyValue (public)
- Rust crate `system-configuration` (Mullvad) for the above; `libc` getifaddrs()/IFF_UP|IFF_RUNNING for classification
- PF_ROUTE routing socket (RTM_NEWADDR/RTM_DELADDR/RTM_IFINFO) as a framework-free alternative (public)
- `scutil --nc list` / `scutil --nc status <service>` subprocess, transition-time only, for service display name and NE connect time (public CLI)
- Network.framework nw_path_monitor + nw_path_uses_interface_type(.other) — simpler boolean alternative, no interface name (public)
- NOT NEVPNManager/NEVPNStatusDidChangeNotification — own-app configs only, requires restricted networkextension entitlement (rejected)

### Permissions / TCC

None. SCDynamicStore, getifaddrs, PF_ROUTE sockets, and scutil are unrestricted for any process — no TCC category, no entitlements, no Full Disk Access, no Accessibility. Works identically for an unsigned/dev-signed accessory app on macOS 26.

### Integration map (files to touch)

- NEW crates/nook-core/src/vpn.rs — monitor thread parking a CFRunLoop with the SCDynamicStore source; owns VpnSnapshot { connected, service_name, interface, since: Option<SystemTime> }; expose snapshot() plus a tokio watch::Receiver (nook_core::runtime() already exists) so the UI awaits changes instead of polling; register module in crates/nook-core/src/lib.rs
- crates/nook-core/Cargo.toml — add system-configuration = "0.6" (and libc if not already present)
- crates/nook/src/island/mod.rs — in spawn_loops(), add a task that awaits the vpn watch channel (no timer loop, unlike the agents/observe pollers); store VpnSnapshot on Island; on transition, trigger the brief compact reveal the same way other mode changes do
- crates/nook/src/island/mod.rs — add CompactMode::Vpn to the enum (~line 34) and to the mode-priority list around line 805; compact face shows a shield/lock glyph left and 'name · H:MM:SS' right, surfaced for a few seconds on connect/disconnect and otherwise available in rotation while connected
- crates/nook/src/island/compact.rs — arms for CompactMode::Vpn in compact_left/compact_right
- NEW crates/nook/src/widgets/vpn.rs (register in crates/nook/src/widgets/mod.rs) — expanded card modeled on speed.rs: status dot, service name, interface, live elapsed timer
- crates/nook-core/src/widgets.rs — add the VPN card to the widget catalog/registry
- crates/nook-core/src/settings.rs + crates/nook/src/island/settings.rs — enable/disable toggle and 'show timer on compact face' option

### Battery requirements

Near-zero idle by construction: one thread parked in CFRunLoopRun that wakes only on kernel-pushed network-state changes (or a blocked read() on a PF_ROUTE socket — both cost nothing while idle). getifaddrs classification and the optional scutil subprocess run only on those events, typically a handful per day. The 1 Hz elapsed-time re-render must run only while a VPN element is actually visible (compact face showing CompactMode::Vpn or the expanded card open) — gate it exactly like the existing timer face; when hidden, store only `since` and compute elapsed on next render. Avoid NWPathMonitor + a polling loop hybrid; the single SCDynamicStore source is sufficient.

### Risks & honest blockers

1) Heuristic false negatives/positives: the 'utun with routable address' rule is the industry heuristic but not a contract — some exotic setups (utun-based Private Relay-like services, virtualization helpers, some corporate ZTNA agents that proxy without a tun) can misclassify; ship an ignore-list setting for interface names. 2) Session timer honesty: if the VPN predates app launch the real connect time is unknowable except for NE/PPP services via scutil's extended status; decide whether to show '≥ elapsed-since-seen' or hide the timer in that case. 3) Naming raw-utun clients (Tunnelblick/OpenVPN CLI, wireguard-go without NE) is best-effort — they have no SCNetworkService entry, so the label degrades to the interface name. 4) The system-configuration crate's callback API requires the run loop thread to be kept alive for the process lifetime; leaking it is fine but make the watch channel robust to a monitor-thread panic (fall back to a slow 60 s snapshot poll only if the event source dies). 5) Multiple simultaneous VPNs (Tailscale + corporate IKEv2) need a UI decision — show the most recent transition, count of tunnels in the expanded card.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP04 — calendar-nl-entry

**Natural-language calendar & reminder entry**

- **Wave:** W1 · **Feasibility:** yes · **Effort:** M (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Natural-language event/reminder entry ("lunch with Sam tomorrow 12:30") from the island into EventKit

### Approach

Use NSDataDetector (public Foundation API, zero permissions) as the date/time parser rather than a Rust chrono-style NL crate. Create one cached detector via `NSDataDetector::dataDetectorWithTypes_error(NSTextCheckingType::Date)` (optionally OR'd with `.Address` for locations) in a `OnceLock`, run `matchesInString:options:range:` on each debounced keystroke, and read `NSTextCheckingResult.date`, `.duration` (populated for ranges like "3-5pm"), `.timeZone`, and `.range`. The `.range` is the key win over Rust crates: strip the matched date phrase (plus dangling "on/at/from" prepositions) from the input and the remainder is the event title. This handles "tomorrow 12:30", "next tue 3pm", "aug 30 9-10am", locale-aware, exactly like Apple Mail/Notes do. Fantastical uses a proprietary parser; NSDataDetector is the standard public-API route smaller apps take. Layer thin Rust heuristics on top: default 60-min duration when `.duration` is 0; all-day when the match has no time component (detect by checking if the resolved time is 12:00 noon default — NSDataDetector's convention — or by regex for absence of digits+am/pm/: in the matched substring); route to Reminder instead of Event when the text starts with "remind/todo/task" or the user toggles a segmented control; "at <place>" after the date range becomes location. Fallback for the rare miss: the `interim` crate (maintained chrono-english fork, pure Rust) — but I'd ship NSDataDetector-only first since this is macOS-only code anyway.

UI: a single-line "quick add" input row at the top of the existing calendar/reminders expanded card, reusing the `EntityInputHandler` + `Window::handle_input` plumbing already proven in widgets/notes_editor.rs (extract a generic one-line TextInput from it). Below the field, render a live preview chip ("→ Lunch with Sam · Tue Aug 25, 12:30–1:30 PM") that updates as the detector re-parses; Enter commits via the existing `nook_core::calendar::create_event` / `create_reminder` async fns (both already implemented and working), Esc clears. On save, clear the field, invalidate the 30s cache (already done inside create_*), and flash a brief confirmation state on the chip. Since the island window already accepts key input for the notes editor, no new window/focus work is needed.

One modernization worth doing while in here: calendar.rs still calls the deprecated `requestAccessToEntityType:completion:`; on macOS 14+ (and Tahoe/26) the correct calls are `requestFullAccessToEventsWithCompletion:` / `requestFullAccessToRemindersWithCompletion:` (or `requestWriteOnlyAccessToEvents` if you only wrote), with `NSCalendarsFullAccessUsageDescription` / `NSRemindersFullAccessUsageDescription` Info.plist keys. The app evidently works today, but the deprecated path is the most likely thing to break on a future macOS.

### APIs

- NSDataDetector.dataDetectorWithTypes:error: with NSTextCheckingType::Date (public, Foundation, via objc2-foundation — enable NSDataDetector/NSRegularExpression/NSTextCheckingResult features)
- NSTextCheckingResult.date / .duration / .timeZone / .range (public)
- NSDataDetector NSTextCheckingType::Address for location extraction (public, optional)
- EKEventStore / EKEvent.eventWithEventStore: / EKReminder.reminderWithEventStore: / saveEvent:span:commit:error: / saveReminder:commit:error: (public, objc2-event-kit — already used in nook-core/calendar.rs)
- EKEventStore.requestFullAccessToEventsWithCompletion: and requestFullAccessToRemindersWithCompletion: (public, macOS 14+ replacement for the deprecated requestAccessToEntityType: the code currently uses)
- EKEventStoreChangedNotification (public, optional — event-driven cache refresh instead of 30s TTL)
- GPUI EntityInputHandler + Window::handle_input (existing pattern in widgets/notes_editor.rs)
- interim crate (pure-Rust chrono-english successor, optional fallback only)

### Permissions / TCC

Calendar and Reminders TCC (kTCCServiceCalendar / kTCCServiceReminders) — already requested and working in this app for the display widgets, so no NEW prompts for this feature. NSDataDetector itself needs no permission, entitlement, or signing. Caveat: on macOS 14+ full-access requires NSCalendarsFullAccessUsageDescription / NSRemindersFullAccessUsageDescription Info.plist keys and the requestFullAccessTo* APIs; the current code uses the deprecated requestAccessToEntityType:completion:, which works today but is the fragile point. No Full Disk Access, no Accessibility, no private entitlements — fine for an unsigned/dev-signed accessory app.

### Integration map (files to touch)

- NEW crates/nook-core/src/nl_parse.rs — NSDataDetector wrapper (OnceLock-cached detector) + heuristics: title extraction via matched-range stripping, default duration, all-day detection, reminder-vs-event keyword routing, location split; returns a ParsedEntry {kind, title, start, end, all_day, location} struct; expose in lib.rs
- crates/nook-core/src/calendar.rs — no new EventKit code needed (create_event/create_reminder exist at ~line 533/599); optionally migrate requestAccessToEntityType_completion (~lines 156/192) to requestFullAccessTo* and make create_* return the created item so the UI can optimistically insert
- NEW crates/nook/src/widgets/quick_add.rs — single-line GPUI input (EntityInputHandler pattern lifted from widgets/notes_editor.rs) + live parse-preview chip + Enter/Esc handling + async save call
- crates/nook/src/widgets/calendar.rs and crates/nook/src/widgets/reminders.rs — mount the quick_add row at the top of each card (or once in the shared calendar card header in island/expanded.rs)
- crates/nook-core/Cargo.toml — add objc2-foundation features: NSDataDetector, NSRegularExpression, NSTextCheckingResult
- UI placement: expanded card only (calendar/reminders widget header); no compact-face or HUD work needed beyond an optional saved-confirmation flash on the chip

### Battery requirements

Effectively zero idle cost by construction: parsing runs only on user keystrokes (debounce ~100–150ms; a single NSDataDetector match on a short string is well under 1ms), the detector object is created once and cached, and saving is a one-shot EKEventStore call. No polling, no timers, no observers required. Free adjacent win: subscribe to EKEventStoreChangedNotification via NSNotificationCenter to invalidate the events/reminders caches event-driven, which would let you lengthen or drop the current 30s cache-TTL refresh cadence in calendar.rs.

### Risks & honest blockers

Parsing-quality edge cases, not platform blockers: (1) NSDataDetector is date-only — it won't split title/location/attendees for you, so the heuristic layer determines perceived quality ("lunch with Sam at Blue Bottle tomorrow" needs the 'at Blue Bottle' vs 'at 12:30' disambiguation); ship conservative defaults and always show the preview chip so the user sees exactly what will be created before Enter. (2) All-day vs timed detection from NSDataDetector requires inspecting the matched substring for a time token — the API doesn't flag it directly. (3) NSDataDetector is locale-aware but English-biased for phrases like 'next tue'; German input ('morgen 12:30') mostly works since it uses system locale, but test it given the user's likely locale. (4) The deprecated requestAccessToEntityType call in calendar.rs could stop granting access on a future macOS — migrate while touching this area. (5) GPUI key focus inside the notch panel is already solved by notes_editor, but verify the quick-add field can take focus while the expanded island is frontmost-without-activation (accessory app); if the window isn't key, keystrokes won't arrive — notes editor proves the pattern works today.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP05 — battery-alerts-lpm

**Battery alerts + one-tap Low Power Mode**

- **Wave:** W1 · **Feasibility:** yes · **Effort:** M (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Battery alerts with one-tap Low Power Mode: IOKit power-source notifications, low-battery alert in the island, LPM toggle button

### Approach

Battery monitoring is fully feasible, event-driven, permission-free, and uses only public API. Add a nook-core power module that subscribes to the Darwin power notifications via libnotify (notify_register_dispatch from notify.h — plain C in libSystem, no new crate): kIOPSNotifyPowerSource ("com.apple.system.powersources.source", fires on plug/unplug), kIOPSNotifyLowBattery ("com.apple.system.powersources.lowbattery", fires when the OS crosses its own warning thresholds), and optionally kIOPSNotifyTimeRemaining ("com.apple.system.powersources.timeremaining", fires on roughly every percent/estimate change — chatty, so debounce or use it only while discharging below a threshold). Alternative with identical cost: IOPSNotificationCreateRunLoopSource on the main CFRunLoop, matching the extern "C" callback patterns already in crates/nook/src/platform.rs; notify_register_dispatch is simpler because nook-core owns no run loop. On each event, read state once via IOPSCopyPowerSourcesInfo + IOPSCopyPowerSourcesList + IOPSGetPowerSourceDescription (keys kIOPSCurrentCapacityKey, kIOPSIsChargingKey, kIOPSPowerSourceStateKey, kIOPSTimeToEmptyKey) plus IOPSGetBatteryWarningLevel, and publish a PowerSnapshot through a tokio::sync::watch channel that an island spawn loop awaits — no timer polling, unlike the existing observe/media loops. LPM state is public: NSProcessInfo.processInfo.lowPowerModeEnabled (macOS 12+) plus NSProcessInfoPowerStateDidChangeNotification for changes (objc2-foundation is already a dependency of nook-core).

Toggling LPM is the honest hard part: it is a root-owned power preference (pmset -a lowpowermode 1) with no public unprivileged API. Ship a tiered strategy. Tier 1 (default, zero-admin, what most indie notch/menubar apps do): macOS 13+ Shortcuts ships a native "Set Low Power Mode" / "Toggle Low Power Mode" action that runs without admin because the Shortcuts runner holds the entitlement; bundle a .shortcut file, open it once for one-click import (user confirms in Shortcuts.app — this cannot be silent), then the island button runs `/usr/bin/shortcuts run "Toggle Low Power Mode"` (~200ms, no prompt ever again). Tier 2 (fallback when the shortcut is missing): `osascript -e 'do shell script "pmset -a lowpowermode 1" with administrator privileges'` — works everywhere but shows the admin password dialog on every tap. Tier 3 (best UX, most work, defer): an SMAppService-registered privileged LaunchDaemon (macOS 13+) that receives an XPC message and calls pmset/IOPMSetSystemPowerSetting as root — one-time approval in System Settings > Login Items; this is how AlDente-class apps do it, but XPC peer validation is weakened for an ad-hoc/dev-signed bundle and registration is finicky outside /Applications, so it is not worth it for v1. Do NOT chase the private LowPowerMode.framework (_PMLowPowerMode setPowerMode:fromSource:): powerd gates the write behind a private entitlement an unsigned app cannot hold, so it fails silently on modern macOS. Detect the LPM result via the NSProcessInfo notification rather than trusting the command's exit code. Comparable apps: boring.notch's battery HUD uses IOPSNotificationCreateRunLoopSource for plug/unplug + percent; NotchNook shows charge HUDs the same way; AlDente uses a privileged helper for pmset-class writes; Raycast's community "Toggle Low Power Mode" uses the osascript-admin route.

### APIs

- IOKit/ps/IOPowerSources.h: IOPSCopyPowerSourcesInfo, IOPSCopyPowerSourcesList, IOPSGetPowerSourceDescription, IOPSGetBatteryWarningLevel, IOPSGetTimeRemainingEstimate (public, link IOKit framework)
- notify.h (libSystem): notify_register_dispatch / notify_cancel with kIOPSNotifyPowerSource = com.apple.system.powersources.source, kIOPSNotifyLowBattery = com.apple.system.powersources.lowbattery, kIOPSNotifyTimeRemaining = com.apple.system.powersources.timeremaining (public)
- IOPSNotificationCreateRunLoopSource (public alternative to libnotify)
- NSProcessInfo.lowPowerModeEnabled + NSProcessInfoPowerStateDidChangeNotification (public, macOS 12+) for LPM state
- /usr/bin/shortcuts run <name> — Shortcuts 'Set Low Power Mode' system action, macOS 13+ (public CLI, no admin)
- osascript 'do shell script "pmset -a lowpowermode 1" with administrator privileges' (public, admin password prompt per use)
- SMAppService.daemon + XPC privileged helper calling pmset/IOPMSetSystemPowerSetting (public, macOS 13+, one-time Login Items approval; deferred to later)
- IOPMSetSystemPowerSetting / IOPMSetPMPreferences (public headers but require root — only usable inside the helper)
- LowPowerMode.framework _PMLowPowerMode setPowerMode:fromSource: (private) — NOT viable: powerd requires a private entitlement unsigned apps cannot hold

### Permissions / TCC

Reading battery state, warning level, and LPM state: no TCC prompt, no entitlement, works unsigned. Shortcuts route: no TCC; the only friction is a one-time user confirmation in Shortcuts.app when importing the bundled shortcut (cannot be silent), and `shortcuts run` itself needs no approval afterward. osascript-admin fallback: standard macOS administrator-password dialog on every invocation (not TCC — it's Authorization Services); no Automation/Accessibility prompt because it targets no other app. SMAppService helper (if built later): one-time user approval under System Settings > General > Login Items, runs as root; works dev-signed but XPC codesign validation is weak for ad-hoc signatures. No Full Disk Access, no Accessibility, no private entitlements required for the shipped tiers.

### Integration map (files to touch)

- NEW crates/nook-core/src/power.rs — IOKit FFI (unsafe extern "C" + #[link(name = "IOKit", kind = "framework")], same style as platform.rs blocks), PowerSnapshot {percent, is_charging, on_ac, time_to_empty_min, warning_level, low_power_mode}, notify_register_dispatch subscriptions feeding a tokio::sync::watch::Sender, plus toggle_low_power_mode() implementing the shortcuts-then-osascript tiers; register module in crates/nook-core/src/lib.rs
- crates/nook-core/src/settings.rs — add show_battery: bool (default true), battery_alert_threshold: u8 (default 20), lpm_shortcut_name: Option<String>, next to the existing show_* flags around lines 126-146/224-232
- crates/nook/src/island/mod.rs — add CompactMode::Battery to the enum (~line 34); in spawn_loops (~line 287) add a loop that awaits the watch channel (no timer); on discharging-below-threshold or OS lowbattery warning set self.preferred = Some(CompactMode::Battery) + nook_core::haptics::trigger(None), mirroring the observe outage pattern at ~line 604-611; auto-clear when charging resumes; include Battery in available_modes() (~line 793) only while the alert condition holds (like Observe)
- crates/nook/src/island/compact.rs — compact_left: battery icon (lucide "battery-low"/"battery-charging") tinted red/orange; compact_right: percent label; optional brief plug-in/unplug flash reusing the preferred-mode mechanism
- NEW crates/nook/src/widgets/battery.rs + register in crates/nook/src/widgets/mod.rs — expanded card: percent, time-remaining, charging state, and the LPM toggle button wired to nook_core::power::toggle_low_power_mode() on a background executor; button reflects live LPM state from the snapshot
- crates/nook/src/island/expanded.rs — mount the battery card alongside the other widget cards
- crates/nook/src/island/settings.rs — settings rows: enable toggle, threshold, and a 'Install Low Power Mode shortcut' button that opens the bundled .shortcut (embed via rust-embed, write to scratch, /usr/bin/open it)
- crates/nook-core/Cargo.toml — no new deps needed (objc2/objc2-foundation/block2 already present; libnotify + IOKit via #[link])

### Battery requirements

Near-zero idle cost by construction: notify_register_dispatch delivers kernel-posted Darwin notifications onto a dispatch queue, so the process does literally nothing between power events; no timers, no polling loop. Subscribe only to powersources.source and lowbattery by default; add timeremaining (per-percent granularity) only while the island is expanded on the battery card or while discharging below ~30%, and unsubscribe otherwise — timeremaining is the only chatty channel. Each event triggers exactly one IOPSCopyPowerSourcesInfo read (microseconds). LPM state via NSProcessInfoPowerStateDidChangeNotification, also push-based. Coalesce bursts (plug/unplug fires 2-3 notifications) with a 250ms debounce before publishing to the watch channel so GPUI re-renders once. This is strictly cheaper than every existing loop in spawn_loops.

### Risks & honest blockers

1) The LPM toggle UX depends on which tier the user lands in: the Shortcuts route is genuinely one-tap but requires a one-time manual import the app cannot skip, and the shortcut name must match settings (store it in lpm_shortcut_name; verify with `shortcuts list` before running). If Apple ever renames/removes the "Set Low Power Mode" action the tier silently breaks — always fall back to osascript and confirm success via NSProcessInfoPowerStateDidChangeNotification with a ~3s timeout, showing a failure state on the button otherwise. 2) `pmset -a lowpowermode` semantics: on some macOS versions the setting is per-power-source (`-b`/`-c`); use `-a` and read back with `pmset -g | grep lowpowermode` only as a debug aid, trusting NSProcessInfo as ground truth. 3) Desktop Macs (no battery): IOPSCopyPowerSourcesList returns an empty list — the module must degrade to hiding the widget, and LPM still exists on Apple Silicon desktops, so keep the toggle available in the expanded card even without a battery gauge. 4) kIOPSNotifyLowBattery only fires at Apple's own thresholds (~10%/Final), so the user-configurable threshold alert must come from the percent read on other notifications — while on battery, timeremaining must be subscribed for percent granularity, which slightly raises event frequency (still trivial). 5) Do not attempt the private _PMLowPowerMode route: it will no-op without the entitlement and waste a day of debugging.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP06 — focus-tools

**High Alert (keep-awake) + Pomodoro**

- **Wave:** W1 · **Feasibility:** yes · **Effort:** M (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** High Alert (keep-awake via IOPM assertions) + Pomodoro (work/break cycles on the existing island timers, optional Focus hook)

### Approach

(a) HIGH ALERT — feasibility: YES, fully public API, no TCC, works dev-signed. Call IOPMAssertionCreateWithName (or better, IOPMAssertionCreateWithProperties) from IOKit.framework with assertion type kIOPMAssertionTypePreventUserIdleDisplaySleep (keeps display+system awake, the Amphetamine/KeepingYouAwake behavior) or kIOPMAssertionTypePreventUserIdleSystemSleep (system awake, display may dim — offer as a settings choice). Level 255 (kIOPMAssertionLevelOn), a human-readable CFString name ('openNook High Alert' — shows up in `pmset -g assertions` and Activity Monitor's Energy tab, so name it well). Release with IOPMAssertionRelease(id). TIMED VARIANTS: pass kIOPMAssertionTimeoutKey (seconds as CFNumber) + kIOPMAssertionTimeoutActionKey = kIOPMAssertionTimeoutActionRelease in the properties dict — powerd itself releases the assertion at the deadline, so the app needs zero timers/wakeups for expiry; the UI countdown can just render from a stored deadline whenever the island happens to repaint. Rust side: raw `#[link(name = "IOKit", kind = "framework")] unsafe extern "C"` block exactly like the CoreGraphics/AVFoundation blocks already in platform.rs:953/1791 — no new crate needed if you build the CFString/CFDictionary via CFStringCreateWithCString/CFDictionaryCreate externs, or add the small `objc2-core-foundation` crate (already on objc2 0.6 ecosystem). Assertion ID is a u32; keep it in a `static AtomicU32` in a new nook-core module so it is process-global and idempotent. This is precisely how KeepingYouAwake (open source, github.com/newmarcel/KeepingYouAwake) and Amphetamine do it; Raycast's Coffee extension instead shells out to `/usr/bin/caffeinate -di -t N` — a valid zero-linking fallback (kill the child to release) but a needless subprocess here. Optional polish, all event-driven: auto-release below a battery threshold using IOPSNotificationCreateRunLoopSource (register only while an assertion is active); NSWorkspaceWillSleepNotification to clear stale UI state if the user force-sleeps. (b) POMODORO — feasibility: YES for the timer itself; it is almost entirely an extension of existing code. Add to the Timer struct (island/mod.rs:51) a `kind: TimerKind` where `Pomodoro { phase: Work|ShortBreak|LongBreak, cycle: u8, work_secs, break_secs, long_break_secs, cycles_per_long: u8, auto_advance: bool }`. The existing 1 Hz tick at mod.rs:413-428 already decrements and fires haptics on zero — extend that branch: when a pomodoro phase hits 0, haptic + advance phase, reset `remaining`, keep `running` if auto_advance. Compact face: CompactMode::Timer + widgets/timers.rs `timer_ring` already render live progress in the notch — just tint the ring by phase (work=accent, break=green) and show phase label + cycle dots in `featured_timer`. Add a pomodoro preset row to the existing timer composer (`timer_composer` flag, timers.rs:151). Persist defaults in AppSettings (nook-core/settings.rs:122). FOCUS INTEGRATION — feasibility: PARTIAL, be honest: macOS has NO public API to set a Focus mode. Three routes: (1) RECOMMENDED: `/usr/bin/shortcuts run "<user-named shortcut>"` (macOS 12+) on phase start/end — the user creates a 1-action 'Set Focus' shortcut once; openNook just stores the shortcut name in settings and execs it (reuse the async Command pattern from nook-core/browser_media.rs:244). Reliable, Apple-supported, ~100-300 ms per invocation, on-demand only. (2) Private hack: write ~/Library/DoNotDisturb/DB/Assertions.json — version-fragile (format churned across Ventura/Sonoma/Sequoia), reads/writes of that folder generally require Full Disk Access; do not ship. (3) FocusStatus/DoNotDisturb private frameworks — gated behind restricted entitlements (com.apple.developer.focus-status) that dev/ad-hoc signing cannot carry; not viable for this distribution model. Ship (1) as an optional 'Run shortcut on work start / break start' setting, plus an optional High Alert tie-in (enable keep-awake during work phases — nice synergy between the two features).

### APIs

- IOPMAssertionCreateWithName / IOPMAssertionCreateWithProperties / IOPMAssertionRelease (IOKit.framework, public C)
- kIOPMAssertionTypePreventUserIdleDisplaySleep, kIOPMAssertionTypePreventUserIdleSystemSleep, kIOPMAssertionLevelOn
- kIOPMAssertionTimeoutKey + kIOPMAssertionTimeoutActionRelease (powerd-side timed expiry, zero app wakeups)
- CFStringCreateWithCString / CFDictionaryCreate (CoreFoundation) or objc2-core-foundation crate
- IOPSNotificationCreateRunLoopSource + IOPSCopyPowerSourcesInfo (optional low-battery auto-release, event-driven)
- /usr/bin/caffeinate -di -t N (fallback alternative, subprocess)
- /usr/bin/shortcuts run <name> (macOS 12+, Focus hook via user-authored shortcut)
- NSWorkspaceWillSleepNotification (optional stale-state cleanup)
- ~/Library/DoNotDisturb/DB/Assertions.json (private Focus write hack — documented but NOT recommended, fragile + Full Disk Access)
- FocusStatus.framework (private, entitlement-gated com.apple.developer.focus-status — not viable dev-signed)

### Permissions / TCC

None for IOPM assertions (no TCC, works unsigned/dev-signed). `shortcuts run` needs no TCC either, though the shortcut's own actions may prompt once inside Shortcuts. The rejected Assertions.json route would need Full Disk Access — another reason to skip it. No new entitlements, no bundled binaries (caffeinate and shortcuts ship with macOS).

### Integration map (files to touch)

- NEW crates/nook-core/src/power.rs — IOPM assertion wrapper: enable(kind, timeout: Option<Duration>) / disable() / is_active(), static AtomicU32 assertion id, #[link(name = "IOKit", kind = "framework")] extern block (mirror pattern at crates/nook/src/platform.rs:953 and :1791); register in nook-core/src/lib.rs
- crates/nook/src/island/mod.rs — Island state: `awake_deadline: Option<Instant>` + assertion-active flag; extend Timer struct (line 51) with pomodoro kind/phase/cycle fields; extend the 1 Hz tick branch (lines 413-428) to advance pomodoro phases, fire haptics, and optionally toggle power::enable/disable and the Focus shortcut on phase edges
- crates/nook/src/widgets/timers.rs — pomodoro preset in the composer (near line 151), phase color + cycle dots in featured_timer (line 185) and timer_ring (line 63); compact face already routes through face_timer()/CompactMode::Timer — no new compact mode needed
- NEW crates/nook/src/widgets/power.rs (or a row in the expanded widgets grid) — High Alert card: on/off toggle + 15m/30m/1h/until-off chips + live remaining readout; register in widgets/mod.rs and the expanded.rs grid
- Compact face indicator — tiny sun/bolt glyph on the idle face chrome while an assertion is active (island/render.rs or chrome.rs), so the user never forgets it is on
- crates/nook-core/src/settings.rs — AppSettings additions: high_alert_default_duration, high_alert_kind (display vs system sleep), low_battery_release_pct, pomodoro work/short-break/long-break/cycles defaults, focus_shortcut_work / focus_shortcut_break (Option<String>); surface in crates/nook/src/island/settings.rs
- NEW (small) crates/nook-core/src/focus.rs — async `shortcuts run` exec reusing the Command pattern from crates/nook-core/src/browser_media.rs:244, plus a `shortcuts list` call to populate a settings picker

### Battery requirements

Zero idle cost by construction: an IOPM assertion is a state row inside powerd, not a process activity — creating one costs one IPC and nothing thereafter; timed expiry via kIOPMAssertionTimeoutKey is handled entirely by powerd so the app schedules no timers and takes no wakeups for it. Pomodoro rides the already-running 1 Hz tick in the existing poll loop (mod.rs:413) — zero new loops, zero marginal cost. Focus shortcut exec is a one-shot subprocess on phase edges only. The low-battery auto-release should use the IOPS run-loop notification source registered only while an assertion is live (event-driven, unregister on release) — never poll battery. The honest cost is the feature's purpose: preventing idle sleep/display sleep drains battery while active, so the always-visible face indicator + a default timeout (never default to 'forever') are the real battery mitigations. Compute pomodoro deadlines against wall-clock (SystemTime/Date) not Instant: Rust's Instant on macOS uses a clock that stops during system sleep, so a break phase spanning a lid-close would silently stall — irrelevant while High Alert is on, wrong otherwise.

### Risks & honest blockers

1) Lid-close (clamshell) sleep is NOT preventable by any user-space assertion — that needs `sudo pmset disablesleep 1`. Amphetamine handles it with a separately-installed privileged helper; out of scope here. Say so in the UI rather than implying the Mac stays awake with the lid shut. 2) Focus mode write access has no public API, full stop: the Shortcuts-CLI route requires a one-time user-authored shortcut (small onboarding friction), the Assertions.json hack breaks across macOS releases and needs Full Disk Access, and the private-framework route is entitlement-blocked for a dev-signed app — deep 'openNook flips Focus natively' is a NO; the shortcut hook is the honest ceiling. 3) Reading current Focus state (to show it in the notch) has the same wall — skip it or accept Full Disk Access. 4) Assertion leak on crash is harmless (powerd drops assertions when the owning process dies), but do release on quit and on settings-window-driven restarts. 5) `shortcuts run` latency (~100-300 ms) means phase-edge Focus flips are near-instant but not synchronous — fire-and-forget on the tokio runtime, never block the tick. 6) If two features grab assertions (High Alert manual + Pomodoro work-phase auto), refcount or single-owner semantics in power.rs must be explicit or toggling one will kill the other's assertion.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP07 — weather

**Weather on the shelf**

- **Wave:** W1 · **Feasibility:** yes · **Effort:** M (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Weather — live weather on the shelf (compact face + expanded card)

### Approach

Sub-feature verdicts: (1) WeatherKit native framework: NO for an unsigned/dev-signed app — the com.apple.developer.weatherkit entitlement must live in a provisioning profile issued by a paid Apple Developer Program account ($99/yr) and Apple validates the app's team ID server-side; ad-hoc signatures cannot carry a provisioning profile. (2) WeatherKit REST API: PARTIAL technically (JWT signed with a .p8 AuthKey from a paid account works regardless of app signature) but unusable here — an open-source app distributed unsigned would have to bundle the private key, leaking it and the 500k calls/month quota. Rule it out. (3) Open-Meteo REST: YES — free, keyless, global; this is the recommended provider. GET https://api.open-meteo.com/v1/forecast?latitude=..&longitude=..&current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m,is_day&daily=temperature_2m_max,temperature_2m_min,weather_code,precipitation_probability_max&hourly=temperature_2m,weather_code&forecast_days=5&temperature_unit=celsius|fahrenheit. Map WMO weather_code (0-99) to icon glyph + label with a small static table (well documented; ~28 distinct codes). Manual location via https://geocoding-api.open-meteo.com/v1/search?name=<city>&count=5 (also keyless). NWS api.weather.gov is a free fallback but US-only and needs a User-Agent header — not worth a second code path. (4) CoreLocation: PARTIAL — CLLocationManager works in an accessory app and will show the TCC prompt, but the macOS location grant is keyed to the code signature; ad-hoc re-signed builds get a new CDHash and the grant resets per build (same problem the app already lives with for EventKit). Implementation: default to MANUAL location — settings text field -> Open-Meteo geocoder -> store name+lat/lon in AppSettings, zero TCC. Offer 'Use system location' as opt-in: one-shot CLLocationManager.requestLocation() with kCLLocationAccuracyReduced (city-level is plenty for weather, and reduced accuracy avoids the scarier full-accuracy prompt) via the objc2-core-location crate (v0.3, matches the existing objc2 0.6 / objc2-foundation 0.3 stack), delegate implemented with objc2's define_class! following the ObjC patterns already in platform.rs / nook-core. Optionally add an IP-geolocation first-run guess (ip-api.com, free non-commercial, keyless) so the widget shows something before configuration — mark it clearly as approximate. HTTP client: reqwest is already a workspace dependency in nook-core (used by the speed test), so no new heavyweight dep and no bundled binaries (no ffmpeg-class payloads at all). Comparable apps: none of Droppy/Ice/Rectangle/Maccy/LocalSend/Alfred ship weather; the real comps are notch apps — NotchNook (paid, signed, uses WeatherKit) and boring.notch-style open-source notch apps which use free REST providers (Open-Meteo) precisely because of the entitlement wall. That is strong precedent for the Open-Meteo route.

### APIs

- Open-Meteo forecast REST: https://api.open-meteo.com/v1/forecast (keyless, free non-commercial, CC-BY 4.0 attribution required, ~10k calls/day limit)
- Open-Meteo geocoding REST: https://geocoding-api.open-meteo.com/v1/search (keyless, for manual city entry)
- CoreLocation: CLLocationManager.requestLocation / requestWhenInUseAuthorization, kCLLocationAccuracyReduced, CLLocationManagerDelegate via objc2-core-location 0.3 (opt-in path only)
- NSWorkspace.didWakeNotification + existing screen/occupancy signals to trigger staleness refresh on wake (already-used AppKit surface)
- reqwest (already in nook-core workspace deps) for both endpoints
- REJECTED: WeatherKit framework (needs com.apple.developer.weatherkit entitlement in a paid-account provisioning profile; impossible unsigned)
- REJECTED: WeatherKit REST (needs bundled .p8 signing key from paid account -> key leakage in open-source unsigned distribution)
- FALLBACK ONLY: NWS api.weather.gov (free, US-only, requires User-Agent header)

### Permissions / TCC

Manual-location default path: NO TCC permissions at all. Opt-in system location: kTCCServiceLiveConversation-unrelated Location Services prompt driven by NSLocationUsageDescription (macOS also honors NSLocationWhenInUseUsageDescription) in the app's Info.plist; grant is per-code-signature, so ad-hoc rebuilds reset it — surface that caveat in Settings copy. Network access needs no TCC (app is not sandboxed).

### Integration map (files to touch)

- NEW crates/nook-core/src/weather.rs — Open-Meteo client (reqwest), WeatherSnapshot {temp, feels_like, wmo_code, is_day, hi/lo, hourly strip, precip prob, fetched_at}, WMO-code -> glyph/label table, geocoding search, TTL-gated fetch() that returns cached snapshot when fresh; register in nook-core/src/lib.rs
- NEW (opt-in) crates/nook-core/src/location.rs — one-shot CLLocationManager wrapper with objc2 define_class! delegate; add objc2-core-location = "0.3" to crates/nook-core/Cargo.toml
- crates/nook-core/src/settings.rs — add WidgetModule::Weather = 10 to the enum, ALL array (becomes [Self; 11]), default_cells (3), min_cells (2), max_cells, default_widget_order; extend AppSettings with weather: {enabled, units C/F, location_mode Manual{name,lat,lon}|System, show_on_compact_face}
- NEW crates/nook/src/widgets/weather.rs — expanded card: current temp + condition glyph + hi/lo + small hourly strip; follow widgets/speed.rs cx.spawn + background executor pattern; register in crates/nook/src/widgets/mod.rs and wire into island/expanded.rs card row
- crates/nook/src/island/mod.rs — add a weather arm to spawn_loops() modeled on the agents/observe loops (background timer, ~30 min cadence, staleness check before any network call); do NOT add a new CompactMode (weather is ambient, not eventful) — instead render temp+glyph as an idle-face adornment
- crates/nook/src/island/compact.rs — optional idle-face element: small SF-style glyph + temperature next to the notch when CompactMode::Idle and setting enabled
- crates/nook/src/island/settings.rs — Weather section: enable toggle, unit picker, city search field (async geocode, pick from up to 5 results), 'Use system location' opt-in button that triggers the CoreLocation one-shot, attribution line 'Weather data by Open-Meteo.com' (CC-BY requirement)
- Info.plist / bundle scripts — add NSLocationUsageDescription string (only needed for the opt-in path)

### Battery requirements

Zero-idle-cost design: no persistent connection, no daemon. Single ~5 KB HTTPS GET per refresh. Cache snapshot with fetched_at and a 30 min TTL (Open-Meteo model updates hourly; anything faster is wasted). Fetch triggers are event-driven, not a hot loop: (a) lazy fetch when the expanded weather card or compact adornment is about to render AND the cache is stale; (b) a coarse background timer in spawn_loops (30 min) that first checks staleness and whether the island/screen is active (reuse the existing occupancy/visibility signals) and otherwise does nothing — an idle tick is a clock compare, zero radio; (c) refresh-on-wake via NSWorkspaceDidWakeNotification so the value is fresh after sleep without polling through it. Location: one-shot requestLocation with reduced accuracy, result cached in settings; never startUpdatingLocation, never significant-change monitoring (weather does not need it and location hardware is the real battery cost). Manual-location mode has literally zero sensor cost. Skip fetches on network-unreachable errors with exponential backoff (cap 30 min) instead of tight retries.

### Risks & honest blockers

1) WeatherKit is a genuine dead end for this distribution model — do not burn time on it; if the project ever gets a paid Developer ID cert, native WeatherKit still requires the entitlement/profile, so the decision does not change with mere Developer-ID signing. 2) Open-Meteo free tier is licensed for non-commercial use with CC-BY attribution; openNook is free/open-source so it qualifies today, but any future paid tier of the app would need Open-Meteo's paid API or another provider — and the attribution line in Settings is mandatory, not cosmetic. 3) It is a third-party service with a fair-use limit (~10k calls/day per IP): fine at 48 calls/day/user, but hard-code the TTL guard so a bug cannot spin-fetch. 4) CoreLocation TCC grant resets on every ad-hoc re-sign (existing known pain with EventKit) — this is why manual city must be the default, or users will report 'location broke after update'. Also, if Location Services is off system-wide, requestLocation fails silently to kCLErrorDenied; the UI needs a clean fallback to manual. 5) IP-geolocation first-run guess sends the user's IP to a third party (ip-api.com) — make it opt-in or skip it to stay privacy-clean. 6) objc2-core-location delegate plumbing is the fiddliest part (async one-shot around a delegate callback); budget the extra half-day or ship manual-location-only first (S/M) and add CoreLocation in a follow-up. 7) WMO-code icon mapping needs a night-variant pass (is_day flag) or the face looks wrong at night.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP08 — system-stats

**System Stats on the shelf**

- **Wave:** W1 · **Feasibility:** yes · **Effort:** M (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** System Stats — live CPU, memory, network up/down, disk on the shelf

### Approach

All four sub-features are plain public syscalls with no TCC prompts, no entitlements, no shelling out, no bundled binaries. Per sub-feature: (1) CPU: yes — host_statistics64(mach_host_self(), HOST_CPU_LOAD_INFO) for aggregate, or host_processor_info(PROCESSOR_CPU_LOAD_INFO) for per-core; compute usage as delta of (user+system+nice) ticks over delta of total ticks between two samples. In Rust use the `mach2` crate (or the already-present sysinfo 0.37: System::new() + refresh_cpu_usage(), respecting sysinfo::MINIMUM_CPU_UPDATE_INTERVAL). If using raw host_processor_info, the returned processor_info_array_t MUST be freed with vm_deallocate or it leaks every sample. (2) Memory: yes — host_statistics64(HOST_VM_INFO64) -> vm_statistics64_data_t; Activity-Monitor-style used = (active + wired + compressed) * page_size, total via sysctl HW_MEMSIZE; sysinfo's used_memory()/total_memory() wraps exactly this. (3) Network: yes — getifaddrs() filtering AF_LINK entries, sum ifa_data->if_data ifi_ibytes/ifi_obytes over non-loopback ('en*', skip 'lo0'/'utun*' per user pref), delta/elapsed -> bytes/s. Caveat: getifaddrs exposes 32-bit counters that wrap at 4 GiB; either handle wrap (assume single wrap on decrease) or use sysctl(CTL_NET, PF_ROUTE, 0, 0, NET_RT_IFLIST2, 0) and read if_msghdr2.ifm_data (if_data64, 64-bit) — this is what the open-source Stats app (github.com/exelban/stats, the best comparable; iStat Menus does the same) uses. sysinfo::Networks also works and handles wrap. (4) Disk: capacity/free is trivial via libc::statfs("/") or sysinfo::Disks (yes). Disk read/write throughput needs IOKit registry walking (IOBlockStorageDriver -> kIOBlockStorageDriverStatisticsKey bytes-read/written properties, delta over time) — feasible (Stats/iStat do it) but it is the only sub-feature worth deferring; ship capacity first, IO throughput as a follow-up. Recommended architecture: a stateful sampler, not stateless polls, because CPU/network are delta-based — struct SysSampler { prev_cpu_ticks, prev_net: (rx,tx), prev_at: Instant } with fn sample(&mut self) -> SysSnapshot { cpu_pct, per_core: Vec<f32>, mem_used/total, net_up_bps, net_down_bps, disk_used/total }. Each sample is a handful of syscalls, microseconds of work, fully synchronous — no tokio needed. Two integration options with observe.rs: (a) lightweight — a sibling module that reuses observe's ChartPoint/record_history_at(persist=false) and the existing sparkline renderer in widgets/observe.rs; (b) deeper — add ObserveSourceKind::LocalSystem and virtual metric keys ("cpu", "mem", "net_up", "net_down", "disk"), keeping the sampler in a static Mutex so observe::poll() can dispatch to it; that buys pins, chart-kind cycling, ObserveWindow history, and alert_above/compact-takeover for free via the existing apply_user_alerts path. Option (b) matches the codebase's grain best, but keep sampling gated on visibility (see battery notes) rather than riding the always-on 15 s observe loop.

### APIs

- host_statistics64(HOST_CPU_LOAD_INFO) — aggregate CPU tick deltas (mach2 crate)
- host_processor_info(PROCESSOR_CPU_LOAD_INFO) — per-core CPU; must vm_deallocate the returned array
- host_statistics64(HOST_VM_INFO64) -> vm_statistics64_data_t — memory (active+wired+compressed)
- sysctl HW_MEMSIZE — total RAM; sysctl NET_RT_IFLIST2 / if_msghdr2.ifm_data — 64-bit per-interface byte counters
- libc::getifaddrs + AF_LINK if_data ifi_ibytes/ifi_obytes — network deltas (32-bit, wrap-aware)
- libc::statfs("/") — disk capacity/free
- IOKit IOServiceMatching("IOBlockStorageDriver") + kIOBlockStorageDriverStatisticsKey — disk read/write throughput (optional follow-up)
- sysinfo 0.37 (already a nook-core dependency, used by agents.rs/audio.rs) — wraps CPU/mem/network/disks if raw mach is not wanted

### Permissions / TCC

None. No TCC prompts, no entitlements, no private frameworks — all listed calls are public and work for an unsigned/dev-signed accessory app. (IOKit block-storage statistics are also public and unrestricted.)

### Integration map (files to touch)

- New module crates/nook-core/src/sysstats.rs: SysSampler (delta state) + SysSnapshot; register in nook-core/src/lib.rs. Pure sync, no tokio runtime needed.
- crates/nook-core/src/observe.rs: add ObserveSourceKind::LocalSystem (enum at line 41) and route poll() to the local sampler for metric keys cpu/mem/net_up/net_down/disk; extend is_counter_query/format_chart_sample for %/bytes-per-second formatting; call record_history_at with persist=false so 1-2 s samples never hit the observe_samples SQLite table.
- crates/nook-core/src/settings.rs: add WidgetModule::SysStats = 10 (extend ALL to 11) + show_sysstats flag and per-stat toggles.
- crates/nook/src/widgets/sysstats.rs: new expanded card modeled on widgets/observe.rs — reuse its sparkline/bars renderers and hover machinery (ObserveHover); rows: CPU %, memory used/total bar, net up/down with arrows, disk gauge.
- crates/nook/src/island/expanded.rs: add WidgetModule::SysStats arm next to the Observe arm (~line 123).
- crates/nook/src/island/mod.rs: do NOT ride the always-on 15 s observe loop (spawned ~line 591); instead spawn a sampling loop only while this.expanded && show_sysstats (same lifecycle pattern as the mirror/camera widget), 1-2 s interval, break on collapse. Keep last counter values in the sampler so the first frame after re-expand shows a real delta.
- crates/nook/src/island/settings.rs: settings rows for enable + interface filter + optional compact alert thresholds (reuses observe alert_above -> apply_user_alerts -> CompactMode::Observe takeover for e.g. CPU > 90%).
- Compact face: nothing by default; alert-threshold takeover only, via the existing has_outage()/preferred = CompactMode::Observe path in mod.rs (~line 607).

### Battery requirements

Zero idle cost is achievable and should be the acceptance test: no timer exists while the island is collapsed — the sampler loop is spawned on expand (gated on this.expanded + widgets tab + show_sysstats) and breaks on collapse, so idle draw is exactly zero syscalls. Each sample is ~5 syscalls totalling microseconds; at 1-2 Hz while expanded this is negligible. Never persist these samples to SQLite (pass persist=false through record_history_at; keep an in-memory ring buffer only, accepting that stats history resets on relaunch). Do not use sysinfo's process refresh for this (agents.rs already notes it is heavy) — only refresh_cpu_usage/refresh_memory/Networks, or raw mach calls. If the user opts into a compact-face alert (CPU > x%), that needs background sampling: run it at a slow 30-60 s cadence piggybacked on an existing wakeup, and make it opt-in per stat, default off. vm_deallocate every host_processor_info buffer or the app leaks ~1 KB/sample.

### Risks & honest blockers

Honest blockers/caveats: (1) getifaddrs if_data counters are u32 and wrap at 4 GiB — on a fast link that is minutes; use NET_RT_IFLIST2 (64-bit) or wrap-aware deltas, and expect counter resets when interfaces bounce (VPN utun churn creates/destroys interfaces mid-delta — sum only stable en* by default). (2) First sample after expand has no baseline — either show '—' for one tick or keep prev counters across collapse (recommended; a stale multi-hour delta must be discarded, mirror observe.rs RATE_GAP_MS logic). (3) Aggregate CPU% on Apple Silicon blends E- and P-cores, so the number reads lower than perceived load; per-core bars via host_processor_info are the honest display but cost the vm_deallocate footgun. (4) 'Memory used' definitions vary — pick active+wired+compressed to match Activity Monitor or users will report it as wrong. (5) Disk I/O throughput (IOBlockStorageDriver) is real extra work — per-drive registry iteration and CF property parsing via objc2/core-foundation; scope it out of v1 or effort becomes L. (6) If integrating as ObserveSourceKind::LocalSystem, observe::poll() is stateless async — the sampler must live in a static Mutex, and the existing always-on 15 s observe loop must not be the thing driving it, or the zero-idle-cost goal is silently lost.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP09 — obsidian-shelf

**Obsidian vault on the shelf**

- **Wave:** W1 · **Feasibility:** yes · **Effort:** M (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Obsidian — vault live on the shelf (read/write vault notes, FSEvents watcher, daily-note quick capture, obsidian:// deep links, no-plugin vs companion plugin)

### Approach

All four sub-features are YES with no plugin required; the companion plugin is optional and should be deferred.

(1) Read/write vault (YES): openNook is unsandboxed, so a user-chosen vault folder is plain std::fs access — no security-scoped bookmarks. Folder selection via an NSOpenPanel helper (canChooseDirectories=YES, runModal) added to platform.rs using the existing objc2 msg_send style; store the path in settings. Auto-discover vaults by parsing ~/Library/Application Support/obsidian/obsidian.json (maps vault-id → {path, open}), so the picker can offer known vaults with zero prompts. Maintain a lightweight in-memory index (relative path + mtime, no bodies) built by walkdir over *.md, skipping .obsidian/ and .trash/; load a note body only when its row is opened. Render with the already-present pulldown-cmark pipeline from widgets/notes.rs; edit with the existing NotesEditor. Writes: temp-file + rename in the vault dir; Obsidian reloads external changes automatically.

(2) FSEvents watcher (YES): use the notify crate (FSEvents backend on macOS; fsevent-sys is already in Cargo.lock via GPUI, so near-zero dependency cost) plus notify-debouncer-mini with ~1s debounce to coalesce Obsidian autosaves. Recursive watch on the vault root, filter .obsidian/ and .trash/ events; on event, patch the index (stat only the reported paths) and cx.notify() if the card is visible, else just mark dirty. Raw FSEventStream via fsevent-sys is the fallback if notify's runtime footprint offends.

(3) Daily-note quick capture (YES): compute today's note path from <vault>/.obsidian/daily-notes.json ({folder, format, template}); format is a Moment.js pattern — implement a small token translator (YYYY, YY, MM, MMM, DD, ddd, dddd; nested folders like YYYY/MM-MMMM/YYYY-MM-DD are common) with YYYY-MM-DD fallback when the file is absent. Create from the template file if configured, else empty with a # H1. Capture = append under a configurable heading (settings option) or at EOF. Optionally read .obsidian/plugins/periodic-notes/data.json for Periodic Notes users. UI: a one-line capture field on the card (Enter appends and clears — same input machinery as notes_editor.rs), plus a global option later.

(4) Deep links (YES): build obsidian://open?vault=<name>&file=<relpath-no-ext> (percent-encoded) and open via the already-depended-on `open` crate or the existing NSWorkspace code in platform.rs. obsidian://open?path=<abs> also works and skips vault-name resolution. obsidian://new?vault=&file=&content=&append=true is an alternate capture path that avoids write races but foregrounds Obsidian — use it only as an opt-in. obsidian://search?vault=&query= for a search affordance. Vault name = folder basename; match against obsidian.json to know whether Obsidian has the vault open.

(5) No plugin vs companion plugin: the no-plugin FS approach covers everything above and is exactly how Raycast's Obsidian extension works (reads .obsidian/*.json, writes markdown directly, obsidian:// for navigation) — it is the proven pattern. A companion plugin (TypeScript, Obsidian plugin API) would add only: current-open-note awareness, cursor-position insertion, and command invocation; the existing community "Local REST API" plugin (HTTPS on 127.0.0.1:27124, API-key auth) already exposes those, so if ever needed, detect-and-integrate with that rather than authoring/distributing our own. Verdict: ship no-plugin v1; plugin is not a blocker for anything requested.

Codebase mapping: new crates/nook-core/src/obsidian.rs (vault discovery, index, daily-note path/format logic, capture append, URL builders — unit-testable pure functions); new crates/nook/src/widgets/obsidian.rs (expanded card: recent-notes list sorted by mtime, capture field, open-in-Obsidian buttons); extend nook-core/src/settings.rs (WidgetModule::Obsidian variant + default_widget_order + is_enabled, fields show_obsidian, obsidian_vault: Option<PathBuf>, obsidian_capture_heading: Option<String>); extend crates/nook/src/island/expanded.rs (match arm calling obsidian_card via cell_pane), crates/nook/src/island/mod.rs (Island state: note index, dirty flag, debouncer handle; start/stop watcher on enable/disable — event-driven, not another poll loop), crates/nook/src/island/settings.rs (vault picker row: known-vaults dropdown + Choose Folder button), crates/nook/src/platform.rs (NSOpenPanel directory-picker fn). Cargo: add notify + notify-debouncer-mini to nook-core macOS deps.

### APIs

- notify crate (FSEvents backend; fsevent-sys already in Cargo.lock via GPUI) + notify-debouncer-mini, ~1s debounce
- std::fs / walkdir for vault index and note IO (unsandboxed, no bookmarks needed)
- ~/Library/Application Support/obsidian/obsidian.json — vault-id → path/open registry for auto-discovery
- <vault>/.obsidian/daily-notes.json, app.json, plugins/periodic-notes/data.json — daily-note folder/format/template (Moment.js token subset)
- obsidian:// URL scheme: open?vault=&file=, open?path=, new?vault=&file=&content=&append=true, search?vault=&query= — via `open` crate or NSWorkspace openURL (platform.rs)
- NSOpenPanel (objc2 msg_send, canChooseDirectories) for the vault picker
- pulldown-cmark (already a dep) for note preview rendering
- Optional later: Obsidian Local REST API community plugin (127.0.0.1:27124, API key) instead of authoring a companion plugin

### Permissions / TCC

No TCC prompt for vaults in ~/ generally; vaults under Documents/Desktop/Downloads or ~/Library/Mobile Documents (iCloud Drive) trigger the one-time per-folder-category TCC consent (kTCCServiceSystemPolicy*Folder) on first access — add NSDocumentsFolderUsageDescription / NSDesktopFolderUsageDescription / NSDownloadsFolderUsageDescription strings to Info.plist for a decent prompt. Caveat for this distribution model: ad-hoc/dev signatures that change between builds reset those TCC grants, so users with Documents-hosted vaults get re-prompted after updates. No entitlements, no private frameworks, no bundled binaries needed.

### Integration map (files to touch)

- NEW crates/nook-core/src/obsidian.rs — vault discovery, note index, daily-note config/format, capture append, obsidian:// builders
- NEW crates/nook/src/widgets/obsidian.rs — expanded card (recent notes, capture field, deep-link buttons)
- crates/nook-core/src/settings.rs — WidgetModule::Obsidian, show_obsidian, obsidian_vault path, capture-heading option
- crates/nook/src/island/expanded.rs — card match arm (mirrors Notes at line ~149)
- crates/nook/src/island/mod.rs — index/dirty state + watcher lifecycle (event-driven; NOT a new poll loop)
- crates/nook/src/island/settings.rs — vault picker row (known-vaults dropdown + Choose Folder)
- crates/nook/src/platform.rs — NSOpenPanel directory chooser helper
- crates/nook-core/Cargo.toml — notify, notify-debouncer-mini (macOS target deps)
- UI landing: expanded card primarily; capture field on the card for v1; settings row for vault choice; no compact-face presence needed (optionally a brief HUD flash on successful capture)

### Battery requirements

Zero-idle by construction: FSEvents is push-based kernel notification — no polling ever; debouncer at ~1s coalesces Obsidian autosave bursts. Index stores only relative paths + mtimes (no note bodies); change events stat only the reported paths; full walkdir scan happens once at vault selection and on watcher (re)start, not periodically. When the expanded card is hidden, events just flip a dirty flag (no re-render, no IO); bodies load lazily on click. Watcher is torn down when the widget is disabled or no vault is set. Skip .obsidian/ and .trash/ subtrees to avoid workspace.json churn (Obsidian rewrites workspace state constantly — filtering it is the single most important battery detail). Do not follow the existing observe/agents poll-loop pattern in island/mod.rs for this feature.

### Risks & honest blockers

(1) .obsidian/workspace.json churn: Obsidian rewrites it every few seconds while active; failing to filter it turns the watcher into a constant wake source. (2) Moment.js format strings: users have exotic daily-note formats (nested-folder patterns, locale tokens); the token-subset translator must fail soft to YYYY-MM-DD and let the user override in settings. (3) Write race on the daily note: if the exact note is open in Obsidian with unsaved buffer changes, an external append can conflict — modern Obsidian merges external edits on reload, but a rare clobber is possible; appending (never rewriting) minimizes it, and the obsidian://new?append=true opt-in eliminates it at the cost of foregrounding Obsidian. (4) iCloud Drive vaults: files can be dataless/evicted — a body read may block on download; keep reads off the UI thread (background_executor, matching existing patterns) and tolerate latency; FSEvents does fire on materialization. (5) TCC re-prompts after re-signing (ad-hoc signature identity changes) for Documents-hosted vaults — distribution-model issue, not code. (6) obsidian.json location differs for the Flatpak/Store variants on other OSes — macOS path is stable, but guard for its absence (fresh Obsidian install or user without Obsidian: everything still works as a plain-Markdown-folder widget, which is a feature, not a bug). No hard blockers.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP10 — lyrics

**Time-synced lyrics**

- **Wave:** W1 · **Feasibility:** yes · **Effort:** M (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Lyrics — time-synced lyrics beside the media widget for the current Apple Music/Spotify/any track

### Approach

RECOMMENDED SOURCE — LRCLIB (lrclib.net). Free, keyless, CORS-open REST API serving crowdsourced LRC files: GET https://lrclib.net/api/get?artist_name=&track_name=&album_name=&duration= returns JSON with `syncedLyrics` (line-level LRC: "[mm:ss.xx] text") and `plainLyrics`; fall back to /api/search (fuzzy) when the exact get 404s. Duration matching tolerance is ~±2s, and openNook already has duration from mediaremote-adapter, so hit rate is high for both Apple Music and Spotify tracks (the source is player-agnostic — it matches on metadata, not on which app is playing, so it also works for the browser/YouTube Music path). LRCLIB asks clients to send an identifying User-Agent/Lrclib-Client header ("openNook/0.4 (github url)"). This is exactly how the closest comparable app, Lyric Fever (open-source macOS menu-bar lyrics for Apple Music), works: LRCLIB primary, NetEase Cloud Music as CJK fallback, optional user-supplied Spotify token. NOT RECOMMENDED SOURCES: (1) Apple Music private lyrics API — amp-api.music.apple.com/v1/catalog/{storefront}/songs/{id}/lyrics returns rich word-level TTML but requires an active subscription, a developer JWT scraped from Music.app/web player, and the user's media-user-token cookie; public MusicKit deliberately omits lyrics; AppleScript `lyrics of current track` only returns the (almost always empty, unsynced) user-editable lyrics field. Feasibility: partial, fragile, clear ToS breach — skip. (2) Spotify lyrics endpoint — spclient.wg.spotify.com/color-lyrics/v2/track/{id} needs a token minted from the user's sp_dc cookie, is unofficial (Spicetify-style), breaks regularly, and the lyrics are Musixmatch-licensed to Spotify, not to you — skip; LRCLIB covers the same tracks. Optional later fallback: Musixmatch's unofficial community API or NetEase for CJK, both gray-zone. SYNC: no new polling. The existing now-playing poll (400ms while playing, mod.rs ~line 467) already delivers elapsed_time (adapter's elapsedTimeNow). On each merge, store an anchor (elapsed_time, std::time::Instant::now(), is_playing); current position = anchor_elapsed + wall-clock delta while playing, re-anchored every poll (also catches seeks — a jump >1.5s from prediction re-anchors instantly). Parse LRC into a sorted Vec<(time_ms, line)>; active line = binary search (partition_point). Drive the UI by scheduling a single background timer for the NEXT line's timestamp (cx.background_executor().timer), not per-frame ticks. CACHING: new table in the existing opennook.db via database.rs migrate(): lyrics(artist, title, album, duration_s, synced TEXT NULL, plain TEXT NULL, source TEXT, fetched_at INTEGER, not_found INTEGER) — positive entries permanent, negative entries retried after ~7 days so a track without lyrics never re-hits the network each play. Fetch fires once per (title, artist) change, only when the lyrics toggle is on, off the UI thread on nook_core::runtime(). No new TCC permissions (outgoing HTTPS only, app is unsandboxed), no bundled binaries, reqwest + rusqlite + serde_json are already workspace deps. UI: new lyrics pane in the expanded Nook media view — a 3-line window (previous/current/next, current highlighted, ease-out scroll) beside the artwork in nook_media_pane, plus optional single-line current lyric on the compact face reusing marquee.rs (default off). Settings: `show_lyrics: bool` in nook-core settings + a row in island/settings.rs. Word-level karaoke is out of scope (LRCLIB is line-level only; word-level exists only via Apple's private TTML). LICENSING: lyrics text is copyrighted; LRCLIB serves it without publisher licenses. Practical risk for a free OSS app that fetches at runtime (never bundles lyrics), is opt-in, attributes "Lyrics from LRCLIB", and can be disabled, is low but nonzero — same posture as Lyric Fever, Feishin, YesPlayMusic. Scraping Apple/Spotify credentials would raise that risk sharply, another reason to stay LRCLIB-only.

### APIs

- LRCLIB REST: GET https://lrclib.net/api/get?artist_name=&track_name=&album_name=&duration= (fields: syncedLyrics, plainLyrics, instrumental)
- LRCLIB REST fallback: GET https://lrclib.net/api/search?q= / ?track_name=&artist_name=
- Existing mediaremote-adapter (ungive/mediaremote-adapter via /usr/bin/perl) — elapsedTimeNow/duration already consumed in nook-core/src/mediaremote.rs, no new API needed for sync
- NOT recommended: Apple AMP API https://amp-api.music.apple.com/v1/catalog/{storefront}/songs/{id}/lyrics (private; needs subscription + scraped developer JWT + media-user-token; TTML word-level)
- NOT recommended: Spotify https://spclient.wg.spotify.com/color-lyrics/v2/track/{id} (private; needs sp_dc cookie token; Musixmatch-sourced)
- Optional CJK fallback: NetEase Cloud Music /api/song/lyric or unofficial Musixmatch community API (both gray-zone)
- rusqlite (already a dep) for the lyrics cache table in opennook.db; reqwest (already a dep) for HTTP

### Permissions / TCC

None new. Outgoing HTTPS to lrclib.net only (app is unsandboxed, no App Transport Security issue for HTTPS). No TCC prompt, no entitlements, no private frameworks beyond the MediaRemote adapter already shipped.

### Integration map (files to touch)

- NEW crates/nook-core/src/lyrics.rs — LRC parser (sorted Vec<LyricLine{time_ms,text}>, binary-search active_line(pos)), LRCLIB client (reqwest, User-Agent header), cache read/write, negative-result TTL; register in nook-core/src/lib.rs
- crates/nook-core/src/database.rs — add lyrics table to migrate()
- crates/nook-core/src/models.rs — SyncedLyrics/LyricLine structs (serde)
- crates/nook-core/src/settings.rs — show_lyrics toggle
- crates/nook/src/island/mod.rs — hold Option<Arc<SyncedLyrics>> + position anchor (elapsed, Instant, is_playing) in Island; kick off fetch task in the now-playing merge (~line 477 change detection) on (title,artist) change when enabled; schedule next-line timer
- crates/nook/src/island/media.rs — lyrics pane inside nook_media_pane (expanded) next to album_chip/artwork; 3-line highlight window
- crates/nook/src/island/compact.rs + marquee.rs — optional compact-face current-line marquee (default off)
- crates/nook/src/island/settings.rs — settings row for the toggle

### Battery requirements

Zero idle cost by construction: no new poll loop — sync piggybacks on the existing 400ms now-playing poll and interpolates position from a monotonic-clock anchor between polls. Network: exactly one HTTPS request per new track, and only when the lyrics toggle is on; SQLite cache (positive forever, negative 7-day TTL) makes replays free. UI redraws are event-driven: one timer armed for the next lyric line's timestamp, disarmed when paused, when the pane is hidden, or when the island is collapsed (if the compact marquee is off). Instrumental/no-lyrics tracks cost nothing after the first (cached) miss. When nothing is playing the feature is completely dormant.

### Risks & honest blockers

1) Copyright: lyrics are licensed works and LRCLIB has no publisher deals — low practical risk for a free OSS app fetching at runtime, but keep it opt-in, attribute the source, never bundle or re-ship lyrics text, and be ready to remove the feature on complaint. 2) Match quality: LRCLIB is crowdsourced — remixes, live versions, and non-Western metadata mismatch; mitigate with duration-tolerant matching and the /api/search fallback; some tracks simply have no synced lyrics (show plain lyrics or hide the pane gracefully). 3) Sync accuracy is bounded by the adapter's elapsedTimeNow (subprocess snapshot every 400ms) — line-level highlighting is fine, word-level karaoke is not achievable with this pipeline. 4) The AppleScript fallback path (pre-15.4 or adapter missing) reports coarser positions; lyrics should tolerate ±1s jitter there. 5) Apple/Spotify private endpoints were evaluated and rejected: credential scraping (media-user-token / sp_dc) is fragile, a ToS breach, and converts the copyright posture from gray to clearly infringing. 6) lrclib.net is a single free community service — handle outages silently (cache-first, fail quiet, no retry storm).

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP11 — airplay-output-picker

**Audio output / AirPlay picker**

- **Wave:** W2 · **Feasibility:** partial · **Effort:** M (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Native AirPlay / audio-output picker in the island: list output devices (speakers, AirPods, AirPlay targets) and switch the default output

### Approach

Core of the feature is fully public and reliable: use the CoreAudio HAL. Enumerate devices with AudioObjectGetPropertyData on kAudioObjectSystemObject / kAudioHardwarePropertyDevices, keep only output-capable ones (kAudioDevicePropertyStreamConfiguration in the output scope has >0 channels), read names (kAudioDevicePropertyDeviceNameCFString) and transport type (kAudioDevicePropertyTransportType: BuiltIn / Bluetooth / USB / AirPlay / DisplayPort) for per-row icons. Read/switch the default with kAudioHardwarePropertyDefaultOutputDevice via AudioObjectSetPropertyData (optionally also kAudioHardwarePropertyDefaultSystemOutputDevice for alert sounds — mirror what the option-click Sound menu does). Per-device volume via kAudioHardwareServiceDeviceProperty_VirtualMainVolume. No TCC prompt of any kind for output enumeration or switching. This covers built-in speakers, wired/USB/DisplayPort outputs, any currently-connected AirPods/Bluetooth headset, and an AirPlay device that is already the active route. Bind via the coreaudio-sys crate (bindgen over AudioHardware.h) or ~10 hand-written extern "C" fns in the existing platform-interop style; this is how boring.notch and MediaMate-class apps do it (Swift equivalents wrap the same HAL calls, e.g. SimplyCoreAudio).

The honest blocker is *initiating* a system-wide route to a not-yet-connected AirPlay target (HomePod, Apple TV on the network). Those do NOT appear as CoreAudio devices until the route is active. The private path is MediaRemote routing (MRAVRoutingDiscoverySession / MRAVOutputDevice pick calls), which since macOS 15.4 is gated behind com.apple.private.mediaremote.* entitlements a dev-signed app cannot hold — the same breakage the project already works around for now-playing; the perl mediaremote-adapter does not expose routing. AVKit's AVRoutePickerView is public but on macOS only routes a specific AVPlayer/AVSampleBufferAudioRenderer, not the system default output, so it does not solve this. Two pragmatic mitigations: (a) list discoverable AirPlay targets read-only via Bonjour (NWBrowser on _airplay._tcp / _raop._tcp — triggers the Local Network TCC prompt on macOS 15+) and, on click, deep-link the user to the system picker; (b) for paired-but-disconnected AirPods, call IOBluetoothDevice pairedDevices + openConnection (Bluetooth TCC prompt), after which the device appears in the HAL and can be set default — the ToothFairy/AirBuddy trick. A last-resort AirPlay switch exists via Accessibility UI-scripting of Control Center's Sound module, but it is brittle across macOS releases; ship it only as an experimental toggle if at all.

Recommended scope: ship phase 1 (CoreAudio picker + default switching + AirPods reconnect) which matches what NotchNook/boring.notch actually deliver, and present unreachable AirPlay targets greyed-out or omit Bonjour discovery entirely. Everything is event-driven: AudioObjectAddPropertyListenerBlock on kAudioHardwarePropertyDevices and kAudioHardwarePropertyDefaultOutputDevice fires callbacks on any change, so the device list and the "current output" glyph update with zero polling.

### APIs

- CoreAudio HAL: AudioObjectGetPropertyData / AudioObjectSetPropertyData on kAudioObjectSystemObject (public)
- kAudioHardwarePropertyDevices, kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDefaultSystemOutputDevice (public)
- kAudioDevicePropertyStreamConfiguration (output scope), kAudioDevicePropertyDeviceNameCFString, kAudioDevicePropertyTransportType (public)
- AudioObjectAddPropertyListenerBlock / AudioObjectRemovePropertyListenerBlock for event-driven updates (public)
- kAudioHardwareServiceDeviceProperty_VirtualMainVolume via AudioHardwareServiceSet/GetPropertyData for per-device volume (public, soft-deprecated, still works)
- coreaudio-sys crate (bindgen bindings for AudioHardware.h) or hand-written extern "C" declarations linking framework CoreAudio
- IOBluetooth: IOBluetoothDevice pairedDevices / openConnection to reconnect AirPods so they surface in the HAL (public, triggers Bluetooth TCC)
- Network.framework NWBrowser (or NSNetServiceBrowser) on _airplay._tcp / _raop._tcp for read-only AirPlay target discovery (public, triggers Local Network TCC)
- AVKit AVRoutePickerView — public but macOS variant routes only an AVPlayer, NOT system output; not usable here (documenting to preempt the dead end)
- MediaRemote MRAVRoutingDiscoverySession / MRAVOutputDevice (private) — the real system AirPlay switch; BLOCKED since macOS 15.4 by com.apple.private.mediaremote entitlements, unavailable to unsigned/dev-signed apps, and not exposed by the ungive/mediaremote-adapter perl shim the app already uses
- Accessibility UI-scripting of Control Center Sound module (kTCCServiceAccessibility) — brittle fallback for true AirPlay selection, not recommended for default-on

### Permissions / TCC

Phase 1 (CoreAudio enumerate/switch/volume): zero TCC prompts, no entitlements — output-side HAL access is unrestricted (mic TCC applies only to capturing input, not enumeration). Optional AirPods reconnect via IOBluetooth: Bluetooth TCC prompt (kTCCServiceBluetoothAlways; needs NSBluetoothAlwaysUsageDescription in Info.plist) — works fine for a dev-signed non-App-Store app. Optional Bonjour AirPlay listing: Local Network TCC prompt (macOS 15+; NSLocalNetworkUsageDescription + NSBonjourServices keys). True system AirPlay route switching requires com.apple.private.mediaremote.* private entitlements that only Apple-signed binaries can hold since macOS 15.4 — not obtainable for this app, full stop. The Accessibility UI-scripting fallback needs kTCCServiceAccessibility.

### Integration map (files to touch)

- NEW crates/nook-core/src/audio_devices.rs — HAL bindings, OutputDevice{id,name,transport,is_default} model, snapshot() -> Vec<OutputDevice>, set_default_output(id), property-listener registration writing into a OnceLock<Mutex<..>> snapshot + AtomicBool dirty flag (same pattern as audio.rs statics)
- crates/nook-core/Cargo.toml — add coreaudio-sys (or link CoreAudio via build.rs / #[link(name = "CoreAudio", kind = "framework")])
- crates/nook-core/src/lib.rs — export audio_devices; register listeners once at startup next to init_audio_state()
- crates/nook-core/src/models.rs — add OutputDevice / OutputTransport types if shared with UI layer
- crates/nook/src/island/media.rs — add the AirPlay-glyph button on the expanded media card that toggles an output-device list; rows call audio_devices::set_default_output
- crates/nook/src/island/expanded.rs — host the picker submenu/overlay state (which panel is open) alongside existing expanded-card plumbing
- crates/nook/src/island/mod.rs — in the existing spawn_loops tick, check the dirty flag (no new timer) and cx.notify(); optionally trigger a brief compact-face HUD ('AirPods Pro connected') on default-output change
- OPTIONAL phase 2: crates/nook-core/src/bluetooth.rs (IOBluetooth reconnect via objc2 msg_send, matching platform.rs style) and Bonjour browsing started only while the picker is open

### Battery requirements

Near-zero idle cost is natural here: AudioObjectAddPropertyListenerBlock delivers device-list and default-output changes as callbacks — no polling at all. Enumerate the full device list only (a) once at startup, (b) when a listener fires, (c) when the user opens the picker. The callback just rebuilds a small snapshot into a static and flips an AtomicBool that the already-running island tick consumes, so no new timers or wakeups are added. If Bonjour AirPlay discovery ships, start NWBrowser only while the picker panel is visible and cancel it on close — continuous mDNS browsing keeps radios busy and is the one real battery trap in this feature. IOBluetooth work happens only on explicit click.

### Risks & honest blockers

Main risk is expectation mismatch: users will read 'AirPlay picker' as 'send audio to my HomePod', and that specific action is entitlement-blocked on current macOS — the UI must be honest (show AirPlay targets only when routable, or label discovered-but-unroutable ones). kAudioHardwareServiceDeviceProperty_VirtualMainVolume is deprecated API surface Apple could remove. IOBluetooth openConnection is old API and connection results for AirPods are sometimes flaky (AirPods auto-route to iPhone). The Control Center UI-scripting fallback breaks on nearly every macOS release and Tahoe's Liquid Glass Control Center makes it worse — avoid shipping it as a supported path. Also note switching default output mid-playback can cause a brief audio glitch in some apps; that is system behavior, not fixable. Bindgen/coreaudio-sys adds a build dependency; hand-rolled extern declarations avoid it if build hygiene matters.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP12 — audio-per-app

**Per-app volume mixer**

- **Wave:** W2 · **Feasibility:** yes · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Audio Control — per-app volume mixer (slider for every app producing sound)

### Approach

FEASIBILITY PER SUB-FEATURE: (1) Per-process volume via Core Audio process taps: YES on macOS 14.4+ — this is the only sane public-API route and what modern mixers use. (2) Per-app volume WITHOUT the audio-capture TCC prompt: NO with public APIs — process/tap AudioObjects expose no settable volume property; the tap IS a capture object and creating one triggers the consent prompt even if you only want mute. (3) Older undocumented approaches: HAL AudioServerPlugIn virtual device (Background Music/eqMac style) works without the audio-capture prompt but needs an admin installer into /Library/Audio/Plug-Ins/HAL + coreaudiod restart — wrong fit for a notch overlay app; code-injection era approaches are dead under SIP/hardened runtime. (4) No-permission fallback for a subset: AppleScript 'set sound volume' works for scriptable players only (Music, Spotify, TV, VLC) via Apple Events (per-app Automation prompts) — could ship as a degraded mode below macOS 14.4.

MECHANISM (the AudioCap pattern, per insidegui/AudioCap and Rogue Amoeba's SoundSource on modern macOS): For each app whose slider ≠ 100%: (a) translate PID → process AudioObjectID via kAudioHardwarePropertyTranslatePIDToProcessObject; (b) build a CATapDescription (ObjC class, CoreAudio.framework) over that process with muteBehavior = CATapMutedWhenTapped (the app's direct output is silenced while tapped) and isPrivate = YES; (c) AudioHardwareCreateProcessTap → tap AudioObjectID; (d) create ONE private aggregate device via AudioHardwareCreateAggregateDevice whose kAudioAggregateDeviceSubDeviceListKey contains the current default output device UID and whose kAudioAggregateDeviceTapListKey lists all active taps ({kAudioSubTapUIDKey, kAudioSubTapDriftCompensationKey: 1}); (e) AudioDeviceCreateIOProcID + AudioDeviceStart — in the realtime callback the input AudioBufferLists carry each tap's audio; multiply samples by that app's gain (f32, allow >1.0 with a soft limiter) and sum into the output buffers. Sliders write an atomic gain map read by the IOProc; VU meters come free from the same samples. Enumerate 'apps currently playing' event-driven: AudioObjectAddPropertyListener on kAudioHardwarePropertyProcessObjectList and per-process kAudioProcessPropertyIsRunningOutput; identify apps via kAudioProcessPropertyBundleID/kAudioProcessPropertyPID. Group helper processes (com.apple.WebKit.GPU → Safari, Chrome Helper → Chrome) by responsible PID — private responsibility_get_pid_responsible_for_pid(libsystem) or heuristic bundle-ID prefix matching like AudioCap. Listen on kAudioHardwarePropertyDefaultOutputDevice and rebuild the aggregate when the user switches to AirPods/speakers.

RUST: the project already uses objc2 0.6, so add objc2-core-audio 0.3 (features AudioHardware, AudioHardwareTapping, CATapDescription) + objc2-core-audio-types — it binds AudioHardwareCreateProcessTap/DestroyProcessTap, the aggregate-device APIs, and the CATapDescription class; alternatively cidre has full tap support, or hand-rolled extern "C" + msg_send consistent with platform.rs. No external binaries to bundle (no ffmpeg — everything is in-process CoreAudio). Comparable apps: SoundSource (formerly ACE HAL engine, taps now), Background Music (GPLv2 HAL driver — license makes code reuse viral, pattern reference only), eqMac (driver), AudioCap (MIT, the canonical taps sample). Droppy/Ice/Rectangle/Maccy/LocalSend/Alfred do nothing comparable; boring.notch and NotchNook don't ship a mixer, so this is a differentiator.

Sources: [objc2-core-audio docs](https://docs.rs/objc2-core-audio/latest/objc2_core_audio/), [objc2-core-audio feature flags](https://lib.rs/crates/objc2-core-audio/features), [insidegui/AudioCap](https://github.com/insidegui/AudioCap).

### APIs

- CATapDescription (CoreAudio.framework, ObjC, macOS 14.4+): initStereoMixdownOfProcesses:, muteBehavior=CATapMutedWhenTapped, setPrivate:
- AudioHardwareCreateProcessTap / AudioHardwareDestroyProcessTap (AudioHardwareTapping.h)
- kAudioHardwarePropertyTranslatePIDToProcessObject, kAudioHardwarePropertyProcessObjectList, kAudioProcessPropertyBundleID, kAudioProcessPropertyPID, kAudioProcessPropertyIsRunningOutput
- AudioHardwareCreateAggregateDevice / AudioHardwareDestroyAggregateDevice with kAudioAggregateDeviceTapListKey, kAudioSubTapUIDKey, kAudioSubTapDriftCompensationKey, kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceSubDeviceListKey
- AudioDeviceCreateIOProcID / AudioDeviceStart / AudioDeviceStop; kAudioTapPropertyFormat, kAudioTapPropertyUID; kAudioDevicePropertyBufferFrameSize
- AudioObjectAddPropertyListener on kAudioHardwarePropertyProcessObjectList and kAudioHardwarePropertyDefaultOutputDevice (event-driven, no polling)
- Private (optional, helper-process grouping): responsibility_get_pid_responsible_for_pid
- Fallback (pre-14.4, scriptable players only): Apple Events 'set sound volume' on Music/Spotify/TV/VLC
- Rust crates: objc2-core-audio 0.3 + objc2-core-audio-types (or cidre); no bundled binaries

### Permissions / TCC

TCC kTCCServiceAudioCapture ("<app> would like to record this computer's audio") — fires on first tap creation; requires NSAudioCaptureUsageDescription in Info.plist. On macOS 15+ it appears under Privacy & Security > Screen & System Audio Recording, and Control Center shows the system-audio-recording indicator while any tap is live. Grant is keyed to the code-signing designated requirement: openNook's stable dev signature is fine, but ad-hoc re-signing per build would re-prompt every rebuild. No permission needed to merely LIST audio-producing processes (process object enumeration is prompt-free) — so the mixer card can show apps + meters-off state with zero TCC, prompting only when the user first drags a slider.

### Integration map (files to touch)

- NEW crates/nook-core/src/mixer.rs — engine: process-list watcher (property listeners), tap/aggregate lifecycle, atomic gain map, per-bundle-id persistence; register in crates/nook-core/src/lib.rs
- crates/nook-core/src/settings.rs — add WidgetModule::Mixer to the enum, default_widget_order(), default_cells(), is_enabled(); persist saved gains (or via database.rs)
- NEW crates/nook/src/widgets/mixer.rs — expanded card: rows of (app icon, name, slider, mute) for apps with IsRunningOutput=true; wire into crates/nook/src/island/expanded.rs alongside speed_card/timer_card and crates/nook/src/widgets/mod.rs
- crates/nook/src/island/settings.rs — toggle + 'reset all volumes' + permission status row
- Optional compact-face gesture: scroll over the notch adjusts the frontmost/now-playing app's gain (hooks in crates/nook/src/island/mod.rs where scroll/gesture handling lives), HUD-style slider flash in render.rs
- Packaging: NSAudioCaptureUsageDescription added to the app bundle Info.plist in the installer/build script; runtime gate on macOS 14.4 (hide module below it)

### Battery requirements

Zero-idle is achievable and must be designed in: with all sliders at 100% there are NO taps, NO aggregate device, NO IOProc — nothing runs. Property listeners (process list, default device) are push-based coreaudiod callbacks with negligible cost; only register them while the mixer card is open or at least one saved gain ≠ 1.0 exists for a running app. When active, cost is a realtime IOProc waking at the buffer rate — raise kAudioDevicePropertyBufferFrameSize to 2048-4096 frames (~43-93 ms at 48 kHz) to minimize wakeups for music; note big buffers add audible A/V latency for video apps, so 512-1024 is the safer default (~1-2% of one efficiency core). Tear the whole pipeline down the moment the last gain returns to 1.0 or the tapped app stops producing audio (IsRunningOutput listener). VU meters render only while the card is expanded. Never poll the process list.

### Risks & honest blockers

(1) macOS 14.4+ hard floor for taps — earlier systems get no mixer (or AppleScript-only degraded mode). (2) The TCC prompt says "record this computer's audio" and macOS shows a recording indicator whenever a slider is active — users may read a volume mixer as surveillance; needs an explanatory pre-prompt in the UI. (3) Re-rendered audio adds latency and a failure mode: if openNook crashes while taps are live with CATapMutedWhenTapped, the tapped app goes silent until tap destruction — need robust teardown (Drop guards + relaunch cleanup of orphaned private aggregates). (4) Helper-process attribution (Safari/Chrome audio comes from GPU/renderer helpers) needs the private responsible-PID call or heuristics; imperfect grouping shows 'WebKit GPU' instead of 'Safari'. (5) Device switches (AirPods connect, AirPlay) require aggregate rebuild with a brief glitch; drift compensation must be enabled per tap. (6) Some output bypasses per-process taps (system sounds daemon, apps using their own aggregate devices). (7) objc2-core-audio bindings are thin/unsafe-heavy; realtime IOProc code in Rust must be allocation-free and panic-free (catch_unwind at the FFI boundary). (8) Background Music is GPLv2 — reference architecture only, never copy code. Effort is L for tap+aggregate+card happy path; edge cases (device switching, grouping, crash-safe teardown, boost limiter) realistically push toward XL.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP13 — automation-entry

**Automation entry points (Alfred / URL scheme / Finder / shell)**

- **Wave:** W2 · **Feasibility:** yes · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Automation entry points: (a) Alfred/URL-scheme + CLI, (b) Finder "Send to openNook" service, (c) Termi-Notch quick shell commands

### Approach

(a) URL scheme + CLI — YES, and cheaper than expected because gpui 0.2.2 already ships the plumbing: `Application::on_open_urls` (gpui-0.2.2/src/app.rs:188) installs the `application:openURLs:` path, and `App::register_url_scheme` (app.rs:1088) does the LSSetDefaultHandlerForURLScheme call. Steps: 1) add CFBundleURLTypes (CFBundleURLName + CFBundleURLSchemes=["opennook"]) to /Users/jonasvogel/openNook/Info.plist (copied into the bundle by scripts/bundle.sh:21). 2) In main.rs, before `.run()`, call `.on_open_urls(...)`; the callback pushes parsed commands into an mpsc/smol channel; inside `run`, spawn a foreground-executor task that awaits the channel and calls a new `Island::ingest_external_paths(Vec<PathBuf>)` refactored out of the existing `ingest_paths` (island/mod.rs:1234 — it already resolves, dedupes, saves via nook_core::files::save_file_tray, and pops the Files tab open). LaunchServices routes URL opens to the already-running instance automatically, so single-instance behavior is free. URL grammar: `opennook://tray/add?path=<percent-encoded>&path=...` (repeatable), plus cheap extras like `opennook://tray/clear`, `opennook://timer/start?seconds=300`, `opennook://expand`. CLI: no socket/daemon needed — add a ~50-line bin target `crates/nook/src/bin/nook.rs` that percent-encodes argv paths (canonicalized first) and execs `/usr/bin/open -g "opennook://tray/add?..."`; bundle.sh copies it into Contents/MacOS and installer.sh symlinks it to /usr/local/bin/nook. The Alfred workflow itself is then a trivial File Action → Run Script: `open -g "opennook://tray/add?path=$(python3 -c 'import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))' "$1")"` — the same pattern Rectangle uses for its `rectangle://execute-action?name=...` Alfred/Raycast integrations. Caveat: the scheme only routes after LaunchServices has seen the .app bundle (run `lsregister -f openNook.app` in bundle.sh/installer.sh); a bare `cargo run` binary will not receive URLs. Unsigned/ad-hoc apps register URL schemes fine — no entitlement involved. SECURITY RULE: never map any URL to shell execution (browsers can invoke custom schemes from web pages = remote code execution); tray/add must validate the path exists and is a regular file/dir, exactly as add_dropped_path already does. (b) Finder Services — YES via NSServices; skip Finder Sync. Add an NSServices array to Info.plist: NSMenuItem/default="Send to openNook", NSMessage="sendToOpenNook", NSPortName="openNook" (must match CFBundleName for the pbs port), NSSendFileTypes=["public.item"] — using NSSendFileTypes (10.6+ key) makes the item eligible for Finder's context menu, not only the Services submenu. At launch, `install_services_provider()` in platform.rs builds an NSObject subclass with ClassBuilder exposing `sendToOpenNook:userData:error:` and calls `[NSApp setServicesProvider:obj]` — the codebase already builds runtime classes exactly this way (NookFileDragSource at platform.rs:486, NookAppTarget at platform.rs:781), so this is a known pattern, and gpui's own app delegate is untouched (servicesProvider is a separate object). The handler reads file URLs from the passed NSPasteboard (`readObjectsForClasses:@[NSURL] options:{NSPasteboardURLReadingFileURLsOnlyKey:YES}`) and feeds the same channel as (a). This works for LSUIElement accessory apps (pbs launches the app if it is not running) and does NOT require code signing — ad-hoc is fine; pbs registers services from any bundle LaunchServices knows. Call NSUpdateDynamicServices() after registration; documentation should note users may need `/System/Library/CoreServices/pbs -update` or a Finder relaunch once after install (a classic NSServices annoyance). The Finder Sync (FIFinderSync) alternative is a genuine .appex extension: it must be properly signed to load on modern macOS, must be enabled by the user in System Settings > Extensions, and only decorates 'monitored' folders — wrong tool for an unsigned accessory app (this is why LocalSend's macOS share integration requires their signed build); do not pursue. (c) Termi-Notch — YES for the sensible scope (one-shot commands with captured output), PARTIAL for full interactive TTY apps. MVP: new nook-core/src/shell.rs spawning `$SHELL -lc "<cmd>"` (login shell so PATH/rbenv/nvm match the user's terminal) via std::process::Command with Stdio::piped stdout+stderr, run in its own process group (setsid) so cancel = kill(-pgid); a background thread streams lines over a channel with a hard output cap (~256 KB) and a configurable timeout; a minimal ANSI-SGR parser (strip escapes, optionally map 16 colors) feeds a mono-font expanded card. Many CLIs disable color when !isatty, which actually simplifies rendering. Full-TTY tier (htop, interactive prompts, 256-color): allocate a PTY with the `portable-pty` crate (pure Rust/MIT, no bundled binaries) with TERM=xterm-256color, and parse VT output into a cell grid with `alacritty_terminal` (Apache-2.0) rendered in GPUI — literally the architecture of Zed's terminal on the same framework — but that is a week+ project on its own; ship the pipe MVP first. Security: command execution must be reachable ONLY from typing inside the island UI — never from opennook:// URLs, the CLI, Services, or Alfred; no sudo (no TTY for the password prompt in MVP); command history opt-in and stored via the existing settings/database layer, not a plaintext log. Effort split: (a) S for URL handler alone, M with CLI + Alfred workflow packaging; (b) M; (c) M for pipe MVP, XL for full PTY terminal.

### APIs

- gpui 0.2.2 Application::on_open_urls (src/app.rs:188) — installs application:openURLs: handling
- gpui 0.2.2 App::register_url_scheme (src/app.rs:1088) — LSSetDefaultHandlerForURLScheme wrapper
- Info.plist CFBundleURLTypes (CFBundleURLSchemes=["opennook"])
- lsregister -f (LaunchServices) to register the bundle/scheme after install
- /usr/bin/open -g <url> from the CLI shim and Alfred Run Script
- Info.plist NSServices (NSMessage, NSPortName, NSMenuItem, NSSendFileTypes=["public.item"])
- -[NSApplication setServicesProvider:] + sendToOpenNook:userData:error: via objc2 ClassBuilder
- NSPasteboard readObjectsForClasses:@[NSURL] with NSPasteboardURLReadingFileURLsOnlyKey
- NSUpdateDynamicServices() / pbs -update for services cache refresh
- std::process::Command + libc setsid/killpg for one-shot shell commands
- portable-pty crate (MIT) + alacritty_terminal crate (Apache-2.0) for the optional full-TTY tier — no external binaries to bundle
- (rejected) FIFinderSync app extension — requires real signing + user enablement; not viable for an unsigned accessory app

### Permissions / TCC

No new TCC permissions for (a) or (b): URL schemes and NSServices need no entitlements and work unsigned/ad-hoc signed. (c) runs children under openNook's TCC responsibility, so a command touching protected folders (Desktop/Documents/Downloads) raises Files-and-Folders prompts attributed to openNook — acceptable, but consider adding NSDesktopFolderUsageDescription/NSDocumentsFolderUsageDescription strings so the prompts read sanely; no Full Disk Access required or requested.

### Integration map (files to touch)

- /Users/jonasvogel/openNook/Info.plist — add CFBundleURLTypes and NSServices dictionaries
- /Users/jonasvogel/openNook/crates/nook/src/main.rs — .on_open_urls(...) before .run(); spawn foreground task draining the command channel into the island entity
- /Users/jonasvogel/openNook/crates/nook/src/island/mod.rs:1234 — refactor ingest_paths into ingest_external_paths(Vec<PathBuf>) shared by drag-drop, URL scheme, and Services
- /Users/jonasvogel/openNook/crates/nook/src/platform.rs — new install_services_provider() using the existing ClassBuilder pattern (cf. NookFileDragSource :486, NookAppTarget :781); call it from platform::install()
- /Users/jonasvogel/openNook/crates/nook/src/bin/nook.rs — new ~50-line CLI shim (percent-encode paths, exec open -g)
- /Users/jonasvogel/openNook/scripts/bundle.sh and scripts/installer.sh — copy CLI into Contents/MacOS, symlink /usr/local/bin/nook, run lsregister -f
- /Users/jonasvogel/openNook/crates/nook-core/src/shell.rs — new: command spawn/stream/kill with output cap and timeout
- /Users/jonasvogel/openNook/crates/nook/src/widgets/terminal.rs — new expanded-card UI: input row + mono scrollback + exit-status chip; entry via a new Tab alongside Files/Notes
- Settings (crates/nook/src/island/settings.rs + nook-core/src/settings.rs) — toggles: enable Termi-Notch, shell path, timeout, history opt-in
- UI placement: (a)/(b) reuse the existing Files tray card and its 'file added' expand behavior; (c) is a new expanded card, with a compact-face spinner while a command runs and an exit-status HUD flash on completion

### Battery requirements

All three are inherently zero-idle-cost if built as described. (a)/(b): purely event-driven — LaunchServices/pbs deliver Apple Events into the existing run loop; no polling, no socket server, no helper daemon (the CLI shim is a dead process except during invocation; using `open` instead of a persistent IPC channel is the whole trick). Wake the island via a channel + foreground-executor task, not by adding anything to the poll loops in island/mod.rs. (c): the shell process exists only for the duration of a command; kill the process group when the card closes or the app quits; do not keep a warm shell at idle (at most, keep one only while the terminal card is visibly open); the streaming reader thread parks on a blocking read and dies with the child, so a collapsed island costs nothing.

### Risks & honest blockers

Honest blockers/sharp edges: 1) URL scheme registration requires the .app bundle to be seen by LaunchServices — dev `cargo run` binaries never receive opennook:// URLs; must document and run lsregister in the install path. Another app could claim the scheme; register_url_scheme reasserts default-handler status at launch. 2) opennook:// is invokable from web pages — any action reachable via URL must be side-effect-safe (add file to tray, start timer); command execution must never be URL-reachable, and tray/add should stat-validate paths. 3) NSServices registration is cached by pbs and notoriously laggy: after first install the menu item may not appear until pbs -update / re-login; also NSPortName must match the bundle's CFBundleName or the service silently fails — the most common NSServices bug. Renaming the app breaks it. 4) Services hand you file URLs without security-scoped bookmarks; fine here (no sandbox), but if the app is ever sandboxed this design needs rework. 5) Termi-Notch is a self-inflicted arbitrary-code-execution surface: keep it opt-in (settings toggle, default off), user-typed only, no history persisted by default, and kill children reliably (orphaned process groups on crash are the classic bug — use setsid + killpg and reap on relaunch). 6) Full interactive TTY support is an XL sub-project (PTY + VT grid rendering); scoping Termi-Notch to one-shot piped commands is the honest v1. 7) `$SHELL -lc` runs the user's login rc files — a slow .zprofile makes every command feel laggy; consider caching a resolved PATH after first run.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP14 — lan-sharing

**LocalSend LAN sharing + drop-a-link**

- **Wave:** W2 · **Feasibility:** yes · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** LAN sharing: (a) LocalSend-protocol send from island drops; (b) drop-a-file-get-a-link uploads (S3/WebDAV/0x0.st)

### Approach

(a) LocalSend, feasibility YES (send-only is clean; always-on receive is feasible but should be opt-in). The protocol (v2.1, documented at https://github.com/localsend/protocol) is small: UDP multicast announce on 224.0.0.167:53317 with a JSON body {alias, version, deviceModel, fingerprint, port, protocol, download, announce}, plus an HTTP(S) REST API: POST /api/localsend/v2/register (unicast fallback discovery), /prepare-upload (returns per-file tokens, may require ?pin=), /upload?sessionId&fileId&token (raw body PUT-style POST), /cancel. TLS is self-signed per device, identified by SHA-256 certificate fingerprint — the client must use reqwest's danger_accept_invalid_certs(true) (or rustls with a custom verifier that pins the fingerprint from the announce). Existing crates: [localsend-rs (CrossCopy)](https://github.com/CrossCopy/localsend-rs) is the most complete (v2 protocol, multicast discovery, TLS with protocol-compliant fingerprinting, streaming uploads, library + CLI, on [crates.io](https://crates.io/crates/localsend-rs)); [localsend](https://crates.io/crates/localsend) / [wylited/localsend](https://github.com/wylited/localsend) and [notjedi/localsend-rs](https://github.com/notjedi/localsend-rs) are WIP/CLI-grade. Honest take: the crate ecosystem is immature and mostly CLI-oriented; recommend vendoring the protocol directly (~600-900 lines) using deps the workspace mostly has: tokio (UdpSocket::join_multicast_v4 for discovery), reqwest (client uploads), rcgen (self-signed cert, only needed for receive mode or HTTPS identity), sha2 (fingerprint), uuid. Send flow, fully on-demand: user drops file on a new "LocalSend" target next to the existing AirDrop target in the tray zone → fire one announce + listen ~3s for register responses → show device-picker pane → POST prepare-upload → stream files with progress → done, all sockets closed. Optional receive mode (settings toggle, default off): a small hyper/axum server on 53317 + persistent multicast listener; incoming prepare-upload raises an accept/decline prompt on the island — this is exactly AirDrop-for-everything and is the flagship half of LocalSend, but ship it as phase 2. (b) Get-a-link, feasibility YES. Three backends behind one trait (fn upload(path) -> Result<Url>): 0x0.st = multipart POST field "file", 512 MiB cap, formula-based retention 30-365 days, keep the returned X-Token to allow delete (https://0x0.st/); WebDAV = reqwest PUT with Basic auth to {base_url}/{filename} — note plain WebDAV gives you a link only if the server serves files publicly (Nextcloud public links need its OCS share API, worth a dedicated sub-mode later); S3 = either rust-s3 (small, MIT) or hand-rolled SigV4 over reqwest — presigned GET URLs cap at 7 days, so permanent links require a public-read bucket/CloudFront, surface that in settings copy. On success, copy the URL via GPUI's cx.write_to_clipboard(ClipboardItem::new_string(url)) (pattern already at crates/nook/src/widgets/notes_editor.rs:455) and flash a HUD confirmation. Comparables: LocalSend itself is the reference implementation; Droppy's shelf is share-target-based (NSSharingService) with no LAN protocol; AirDrop via NSSharingServiceNameSendViaAirDrop is already shipped in this repo (airdrop_target in island/files.rs), which validates the drop-target UI pattern to clone. Sources: https://crates.io/crates/localsend-rs, https://github.com/CrossCopy/localsend-rs, https://github.com/localsend/protocol, https://crates.io/crates/localsend, https://github.com/notjedi/localsend-rs, https://crates.io/crates/localsnd, https://0x0.st/, https://dibi.dev/TIL/0x0-st/, https://filepost.dev/blog/transfer-sh-alternative

### APIs

- LocalSend protocol v2.1: UDP multicast 224.0.0.167:53317 announce + REST /api/localsend/v2/{register,prepare-upload,upload,cancel} (https://github.com/localsend/protocol)
- tokio::net::UdpSocket::join_multicast_v4 (discovery; already a workspace dep)
- reqwest 0.12 (already a workspace dep) with danger_accept_invalid_certs or rustls custom verifier pinning the peer's SHA-256 cert fingerprint
- rcgen (self-signed cert for receive mode / HTTPS identity), sha2, uuid — new small deps
- hyper or axum (ONLY if opt-in receive server ships)
- Optional shortcut: localsend-rs crate (CrossCopy) as library instead of vendoring
- 0x0.st: multipart POST, 512MiB, X-Token delete header
- WebDAV: HTTP PUT + Basic auth via reqwest
- S3: rust-s3 crate (MIT) or hand-rolled SigV4; presigned GET max 7 days — permanent links need public bucket
- GPUI ClipboardItem::new_string + cx.write_to_clipboard for the link
- NSLocalNetworkUsageDescription in /Users/jonasvogel/openNook/Info.plist (macOS 15+ Local Network TCC prompt); NO multicast entitlement needed on macOS (com.apple.developer.networking.multicast is iOS-only)

### Permissions / TCC

macOS Local Network TCC prompt (macOS 15+, needs NSLocalNetworkUsageDescription in Info.plist; no entitlement on macOS). Application Firewall incoming-connection prompt only if the opt-in receive server ships. No other TCC. Keychain (via existing security-framework dep) recommended for backend credentials.

### Integration map (files to touch)

- NEW crates/nook-core/src/share/mod.rs + share/localsend.rs — discovery (announce/collect ~3s), prepare-upload/upload client, streamed with progress callback; runs on the existing shared tokio runtime (nook-core/src/lib.rs::runtime())
- NEW crates/nook-core/src/share/upload.rs — LinkBackend trait + ZeroXZero/WebDav/S3 impls; reuse reqwest patterns from observe.rs/utils.rs
- crates/nook-core/src/settings.rs — extend AppSettings with ShareSettings { device_alias, localsend_receive: bool, link_backend: enum + credentials }; persists via existing settings DB plumbing (update_app_settings/tweak_app_settings)
- crates/nook/src/island/files.rs — clone the airdrop_target() drop-target (line ~155) into localsend_target() and get_link_target(); reuse ExternalPaths can_drop/on_drop wiring; also add per-file-card context actions
- crates/nook/src/island/expanded.rs (or new crates/nook/src/widgets/share.rs) — device-picker pane listing discovered LocalSend peers with transfer progress bars; progress state on Island like speed.rs (apply_speed_sample pattern)
- crates/nook/src/island/compact.rs — tiny progress indicator on the compact face during an active transfer/upload; HUD flash 'Link copied' on completion
- crates/nook/src/island/settings.rs — settings section: alias, receive toggle, backend picker + credential fields (clipboard-paste already supported there, line 328/1243)
- /Users/jonasvogel/openNook/Info.plist — add NSLocalNetworkUsageDescription
- Cargo.toml workspace — add rcgen, sha2, uuid (and rust-s3 or hyper only if those sub-features ship)

### Battery requirements

Zero idle cost is achievable for everything recommended. LocalSend send-only: no socket exists until the user drops a file on the target; discovery is one multicast announce + a 3s listen window, then all sockets close — no polling, no background task. Link uploads: pure on-demand reqwest calls, nothing idle. The only standing cost would be the optional receive mode: a bound TCP listener on 53317 plus a joined multicast socket — both are kernel-event-driven (0% CPU idle) but keep the network stack registered and can cause wakeups on chatty LANs; ship it default-OFF behind a settings toggle and tear the sockets down when toggled off. Do not add any poll loop to island/mod.rs for this feature: transfer progress should push into the Island entity via the existing channel/notify pattern used by the speed test, and the device list is a one-shot fetch per pane open, not a subscription.

### Risks & honest blockers

1) Crate maturity: none of the localsend crates is battle-tested as a library; CrossCopy/localsend-rs is the best but young — budget for vendoring the protocol yourselves (it is small and fully documented). 2) macOS 15 Sequoia Local Network privacy: first multicast send triggers a TCC prompt; a dev-signed app with unstable signature can lose the grant on rebuild (same class of problem you already have with other TCC perms), and if the user denies it discovery silently finds nothing — detect the empty-result case and deep-link to System Settings > Privacy > Local Network. 3) Receive mode additionally triggers the Application Firewall accept-incoming-connections prompt per unstable signature. 4) TLS: official LocalSend clients use self-signed HTTPS; accepting invalid certs without pinning the announced fingerprint is a MITM hole — pin it. Some receivers require a PIN (?pin= param) — handle 401. 5) iPhones only receive while the LocalSend app is open in the foreground — set user expectations; this is not AirDrop-to-locked-phone. 6) 0x0.st is a free community service: files are public-by-URL, retention is 30-365 days, uploads can be rate-limited or the service can vanish — make it clearly labeled and never the silent default. 7) S3 permanent links require a public bucket (presigned GET caps at 7 days) — a footgun users should opt into knowingly. 8) Plain WebDAV PUT does not by itself yield a shareable public link on Nextcloud/ownCloud (needs their OCS API); v1 should document that the configured base URL must be publicly readable. 9) Secrets (S3 keys, WebDAV passwords) currently have nowhere safe to live — settings DB is plaintext SQLite; use the already-present security-framework dep to store them in the Keychain instead.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP15 — input-feel

**Keyboard sounds + smooth scrolling (Mechey / LiquidMouse)**

- **Wave:** W2 · **Feasibility:** yes · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Input-event features: (a) Mechey — mechanical keyboard sounds per keystroke; (b) LiquidMouse — smooth scrolling for external mice + per-device scroll direction reverse

### Approach

MECHEY (feasible: yes). Event source: a listen-only CGEventTap (CGEventTapCreate at kCGSessionEventTap, kCGEventTapOptionListenOnly, mask = keyDown|keyUp|flagsChanged) on a dedicated CFRunLoop thread. Prefer this over NSEvent addGlobalMonitorForEventsMatchingMask: — both require the same Input Monitoring TCC since Catalina, but the tap delivers keyUp reliably, sees repeats via kCGKeyboardEventAutorepeat (skip or soften repeats), and keeps work off the main thread. Check/request permission with IOHIDCheckAccess/IOHIDRequestAccess(kIOHIDRequestTypeListenEvent). Playback: pure-Rust path recommended — cpal output stream + a tiny lock-free one-shot mixer (or the kira crate), with all samples decoded to PCM at pack load (symphonia for OGG, MPL-2.0, pure Rust — no ffmpeg or any external binary needed). Alternative: AVAudioEngine + AVAudioPlayerNode pool with preloaded AVAudioPCMBuffers via objc2-av-foundation msg_send, which is what Klack (the best-known Mac keyboard-sound app) uses; cpal avoids that ObjC plumbing. Per-key mapping: distinct samples for space/enter/backspace/modifiers, separate down/up sounds, ±3-5% random pitch/volume for realism. Sound packs: adopt the Mechvibes community pack format (config.json + one OGG sprite sheet with per-key offsets, keyed by JS keycodes — write a CGKeyCode→JS-keycode table); bundle 2-3 verified CC0/CC-BY packs (~1-3 MB each) and let users drop Mechvibes packs into ~/Library/Application Support/openNook/soundpacks (many community packs are unlicensed keyboard rips — only bundle vetted ones). Secure input (password fields, Terminal Secure Keyboard Entry) silently suppresses events — correct behavior; optionally detect via IsSecureEventInputEnabled() to show state. LIQUIDMOUSE (smooth scroll + mouse-only reverse: yes; true per-individual-device config: partial, needs private API or a correlation hack). Active CGEventTap (kCGEventTapOptionDefault) at kCGSessionEventTap on kCGEventScrollWheel — this is exactly how Mos (GPL, github.com/Caldis/Mos), LinearMouse (MIT), and Mac Mouse Fix do it; LinearMouse/MMF are the license-safe references. In the callback: read kCGScrollWheelEventIsContinuous — 1 means trackpad/Magic Mouse (also nonzero kCGScrollWheelEventScrollPhase/MomentumPhase): pass through untouched; 0 means wheel mouse: swallow the event (return NULL), feed the line delta into a velocity/exponential-decay interpolator, and emit synthetic continuous scroll events from a CVDisplayLink-driven loop via CGEventCreateScrollWheelEvent with IsContinuous=1, pixel deltas in kCGScrollWheelEventPointDeltaAxis1/2, and correct Phase/MomentumPhase sequencing (began/changed/ended) so apps render elastic overscroll. Tag synthetic events with a magic kCGEventSourceUserData value so the tap ignores its own output. Reverse-for-mouse-only is then a trivial delta negation on the wheel path — no device identification needed (this alone replicates Scroll Reverser). True per-device settings (two different mice, different behavior): CGEvents carry no public device identity; option A (private, acceptable for this app): CGEventCopyIOHIDEvent + IOHIDEventGetSenderID to get the sending device's registry ID, matched against an IOHIDManager device list (vendor/product/name) — the Mac Mouse Fix approach; option B (public): IOHIDManager input-value callbacks correlated by timestamp with the tap stream — LinearMouse's approach, fiddly and racy. Ship mouse-vs-trackpad first, per-device as a follow-up. Must-handle edge cases: re-enable tap on kCGEventTapDisabledByTimeout/ByUserInput; per-app exclusion list (games, VMs, remote desktops) via frontmost bundle id; Shift+scroll → horizontal; coexistence with Mos/LinearMouse/Logi Options+ (detect and warn); when the tap is disabled events pass through unmodified, so failure degrades gracefully.

### APIs

- CGEventTapCreate / CGEventTapEnable / CFMachPortCreateRunLoopSource (CoreGraphics, public) — via objc2-core-graphics + objc2-core-foundation crates, or the servo core-graphics crate's EventTap wrapper
- CGEventGetIntegerValueField / CGEventSetIntegerValueField: kCGScrollWheelEventIsContinuous, kCGScrollWheelEventDeltaAxis1, kCGScrollWheelEventPointDeltaAxis1, kCGScrollWheelEventScrollPhase, kCGScrollWheelEventMomentumPhase, kCGKeyboardEventKeycode, kCGKeyboardEventAutorepeat, kCGEventSourceUserData (public)
- CGEventCreateScrollWheelEvent + CGEventPost(kCGSessionEventTap) for momentum synthesis (public)
- IOHIDCheckAccess / IOHIDRequestAccess(kIOHIDRequestTypeListenEvent) — Input Monitoring prompt (public, IOKit/hid)
- AXIsProcessTrustedWithOptions(kAXTrustedCheckOptionPrompt) — Accessibility prompt (public, ApplicationServices)
- IOHIDManagerCreate + device matching (usage page 0x01, usages Mouse/Keyboard) for the per-device list in settings (public IOKit)
- CGEventCopyIOHIDEvent + IOHIDEventGetSenderID (PRIVATE, CoreGraphics/IOKit SPI) — per-device identity on scroll events; acceptable given openNook ships outside the App Store
- IsSecureEventInputEnabled (Carbon HIToolbox, public) — optional secure-input state display
- cpal (Rust, Apache/MIT) + symphonia (MPL-2.0) for sample playback — or AVAudioEngine/AVAudioPlayerNode/AVAudioPCMBuffer via objc2-av-foundation as the native alternative
- CVDisplayLink (or a thread parked on a 120 Hz timer only while animating) to clock momentum frames
- No external binaries to bundle — no ffmpeg; sound packs are 1-3 MB OGG/WAV assets via rust-embed or Application Support

### Permissions / TCC

Mechey: Input Monitoring (kTCCServiceListenEvent), one-time system prompt via IOHIDRequestAccess, user flips toggle in System Settings > Privacy & Security > Input Monitoring. LiquidMouse: Accessibility (kTCCServiceAccessibility) because the tap modifies/swallows events — prompt via AXIsProcessTrustedWithOptions. So the app ends up requesting BOTH if both features enabled. Critical distribution caveat: TCC grants key on the code-signing designated requirement — ad-hoc signatures change every build, forcing users to re-grant (and sometimes to remove+re-add the stale entry) after each update. A stable Developer ID, or at minimum a persistent self-signed identity used across releases, is effectively a prerequisite for shipping these two features; also gate each feature behind an explicit opt-in in settings with a permission-status row (green/red) like the existing calendar handling.

### Integration map (files to touch)

- crates/nook-core/src/eventtap.rs (NEW, shared): CFRunLoop thread owning both taps, creation/teardown, automatic re-enable on kCGEventTapDisabledByTimeout/ByUserInput, TCC check/request helpers; taps are created only when a feature is enabled and fully invalidated (CFMachPortInvalidate) when disabled
- crates/nook-core/src/keysounds.rs (NEW): sample bank loader (Mechvibes config.json parser + CGKeyCode mapping), cpal mixer with idle auto-close, keyDown/keyUp handlers
- crates/nook-core/src/scroll.rs (NEW): wheel interceptor, velocity model + phase-correct momentum synthesizer, CVDisplayLink lifecycle, per-app exclusion, reverse toggle
- crates/nook-core/src/hiddevices.rs (NEW, phase 2): IOHIDManager enumeration for the per-device settings list + senderID lookup
- crates/nook-core/src/settings.rs: extend AppSettings — keysounds_enabled, keysound_pack, keysound_volume, smooth_scroll_enabled, scroll_speed/duration, reverse_mouse_scroll, scroll_excluded_apps, per_device overrides (follows existing #[serde(default)] pattern at line ~122)
- crates/nook/src/island/settings.rs: two new sidebar sections (Keyboard Sounds: pack picker, volume slider, test button, permission row; Scrolling: enable, speed curve, reverse-mouse toggle, exclusion list, device list) — no compact-face or expanded-card presence needed; optional transient HUD on toggle
- crates/nook/src/assets + rust-embed: bundled CC0 sound packs; user packs in ~/Library/Application Support/openNook/soundpacks
- crates/nook-core/src/mouse.rs: untouched initially, but its NSEvent mouseLocation polling could later migrate onto the shared tap thread (mouseMoved events) — a free battery win once eventtap.rs exists

### Battery requirements

Both features are inherently event-driven — near-ideal for the battery budget. Zero-idle design: (1) taps exist only while their feature is enabled; disabled = tap invalidated, thread parked or joined, literally zero cost. (2) Mechey: the tap callback wakes the process only on actual keystrokes (negligible at human typing rates, sub-ms work per event). The one real drain is the audio output: a running CoreAudio output stream keeps the audio device powered even when silent, so open the cpal stream lazily on the first keystroke and close it after ~15-30 s of keyboard idle (reopen is a few ms; at worst the first click of a burst starts ~5 ms late — inaudible). All samples pre-decoded to PCM in memory (~2-10 MB), no per-key decode. (3) LiquidMouse: callback fires only on scroll events; the CVDisplayLink/animation thread starts on the first wheel tick and stops the moment synthesized velocity decays below epsilon (typically <1.5 s) — never runs while idle. Never poll; never keep a timer armed between gestures. (4) Keep tap callbacks under ~100 µs (just enqueue to the worker) — a slow callback both burns power and gets the tap force-disabled by WindowServer.

### Risks & honest blockers

1) TCC + unsigned distribution is the biggest blocker-shaped risk: without a stable signing identity, every release invalidates the Input Monitoring and Accessibility grants — users will experience the features silently dying after updates. 2) Per-individual-device scroll settings need private API (CGEventCopyIOHIDEvent/IOHIDEventGetSenderID) or fragile timestamp correlation — ship mouse-vs-trackpad distinction (public, robust via IsContinuous flag) first and label per-device as best-effort; feasibility for that sub-piece alone is 'partial'. 3) Active scroll tap makes openNook part of every app's scroll path: bugs manifest as system-wide janky scrolling, and conflicts with Mos/LinearMouse/Logi Options+/Scroll Reverser users already run are likely — detect known bundle ids and warn. Failure mode is graceful (a timed-out tap passes events through) but momentum can visibly stutter under CPU pressure. 4) Momentum synthesis has a long tail of app-specific quirks (line-delta-only apps like some Java/VM software, games needing raw input, horizontal scroll, Launchpad/Mission Control gestures) — the 3-5 day estimate covers the core plus an exclusion list, not exhaustive app compatibility. 5) Sound pack licensing: most Mechvibes community packs are unlicensed recordings of commercial switches; bundle only vetted CC0/CC-BY packs, treat user-imported packs as user responsibility. 6) Secure-input contexts mute Mechey by OS design — document it so users don't file 'sounds randomly stop' bugs. Effort split behind the single L rating: Mechey M (1-2 days incl. pack import), LiquidMouse L (3-5 days for smooth scroll + mouse-only reverse); add 2-3 more days (XL overall) if true per-device settings are in scope.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP16 — voice-memos

**Voice recordings with live transcription**

- **Wave:** W2 · **Feasibility:** yes · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Voice recordings from the island with live Speech transcription (record button, streaming transcript, recordings list with duration)

### Approach

This needs only public frameworks: AVFAudio for capture and Speech for transcription, both callable from Rust via objc2 framework crates the project already uses idiomatically (see the block2 completion-handler pattern in crates/nook-core/src/calendar.rs). Build a new nook-core module `recorder.rs` around one AVAudioEngine: on start, request mic access (AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio, or AVAudioApplication requestRecordPermission on macOS 14+) and Speech authorization (SFSpeechRecognizer requestAuthorization:), then installTapOnBus:0 on engine.inputNode. Each tap callback (block2 closure receiving AVAudioPCMBuffer) does two things: writeFromBuffer: into an AVAudioFile (write .caf/AAC into ~/Library/Application Support/openNook/recordings/) and appendAudioPCMBuffer: into an SFSpeechAudioBufferRecognitionRequest with shouldReportPartialResults=YES and requiresOnDeviceRecognition=YES (check supportsOnDeviceRecognition first; if false, either allow the server path — which requires Siri/Dictation enabled — or degrade to record-only). The recognitionTaskWithRequest:resultHandler: block streams bestTranscription.formattedString partials over a tokio watch/mpsc channel into the Island entity, which cx.notify()s — same flow as the existing spawn_loops in crates/nook/src/island/mod.rs. On stop: endAudio the request, remove the tap, stop the engine, finalize the file, persist {path, created_at, duration_ms, transcript} via the existing rusqlite database.rs. Playback of a listed recording via AVAudioPlayer (also in AVFAudio). Note macOS 26's newer SpeechAnalyzer/SpeechTranscriber API is Swift-concurrency-only and unreachable from objc2 — SFSpeechRecognizer remains available and working on Tahoe, and this is what menu-bar recorders and Voice Memos clones actually ship with; whisper.cpp (MacWhisper-style) is the heavy fallback if Apple's on-device model quality disappoints, but skip it initially for battery reasons. Because the app is dev-signed outside the App Store, no entitlements are needed (com.apple.security.device.audio-input only matters for sandboxed apps); the TCC prompts work fine for ad-hoc/dev-signed apps with a stable bundle ID, with the same caveat the app already lives with for Calendar: a signature change invalidates prior grants.

### APIs

- AVFAudio: AVAudioEngine, inputNode, installTapOnBus:bufferSize:format:block:, AVAudioPCMBuffer, AVAudioFile (writeFromBuffer:), AVAudioPlayer (via objc2-avf-audio crate)
- AVFoundation: AVCaptureDevice authorizationStatusForMediaType: / requestAccessForMediaType:completionHandler: with AVMediaTypeAudio (via objc2-av-foundation); alternatively AVAudioApplication requestRecordPermissionWithCompletionHandler: (macOS 14+, in AVFAudio)
- Speech: SFSpeechRecognizer (+requestAuthorization:, supportsOnDeviceRecognition), SFSpeechAudioBufferRecognitionRequest (appendAudioPCMBuffer:, shouldReportPartialResults, requiresOnDeviceRecognition), recognitionTaskWithRequest:resultHandler:, SFSpeechRecognitionResult.bestTranscription.formattedString (via objc2-speech)
- block2 for tap and result-handler closures (already a dependency)
- NOT usable: macOS 26 SpeechAnalyzer/SpeechTranscriber — Swift-only, no ObjC surface; would need a small Swift helper dylib if ever wanted
- No private APIs required

### Permissions / TCC

Two TCC prompts, both fine for a dev-signed non-App-Store app: Microphone (kTCCServiceMicrophone, needs NSMicrophoneUsageDescription) and Speech Recognition (kTCCServiceSpeechRecognition, needs NSSpeechRecognitionUsageDescription). No entitlements needed since the app is unsandboxed. Caveats: server-based Speech recognition requires Siri/Dictation enabled system-wide — force requiresOnDeviceRecognition=YES and gate on supportsOnDeviceRecognition (the on-device dictation model may need to be downloaded for the locale); re-signing with a different identity resets both TCC grants (same as the app's existing Calendar/Reminders situation).

### Integration map (files to touch)

- crates/nook-core/Cargo.toml — add objc2-avf-audio, objc2-speech, objc2-av-foundation to the macOS dependency block
- crates/nook-core/src/recorder.rs (NEW) — engine lifecycle, permission requests, tap → file + recognition request, partial-transcript channel, list/delete/play recordings; export from lib.rs
- crates/nook-core/src/database.rs — add a recordings table (path, created_at, duration_ms, transcript)
- Info.plist — add NSMicrophoneUsageDescription and NSSpeechRecognitionUsageDescription strings
- crates/nook/src/island/mod.rs — Island state: recording bool, elapsed secs, live transcript String, Vec<RecordingItem>; add CompactMode::Recording; wire the transcript channel into the existing spawn pattern
- crates/nook/src/island/compact.rs — compact face while recording: red dot + elapsed timer (like the Timer compact mode)
- crates/nook/src/widgets/recorder.rs (NEW) — expanded widget card: record/stop button, input level meter (derived from tap buffers), scrolling partial transcript, recordings list with duration + play/delete; register in crates/nook/src/widgets/mod.rs
- crates/nook-core/src/widgets.rs and crates/nook/src/island/settings.rs — widget enable toggle alongside existing widget config

### Battery requirements

Near-zero idle by construction: this feature needs NO polling and NO background listeners — the AVAudioEngine, the tap, and the SFSpeechRecognitionTask exist only between record-start and record-stop, so idle cost is literally zero. While recording, on-device recognition costs roughly a sustained fraction of one core (ANE/CPU); use a 0.5–1s tap buffer size rather than tiny buffers, derive the level meter from buffers the tap already delivers (no extra metering timer), throttle transcript-driven cx.notify() to ~4 Hz, and drive the elapsed-time display off the existing island tick rather than a new timer. Make record-only mode (skip the recognition request) available for long recordings to halve active cost. Persist transcript once at stop, not per partial.

### Risks & honest blockers

Main risk is objc2 plumbing fiddliness, not feasibility: installTapOnBus's block runs on a realtime audio thread — the closure must be non-blocking (push buffers into a channel; do file writes and appendAudioPCMBuffer off that thread if writes ever stall). SFSpeechRecognizer is Apple's legacy path on macOS 26 — still functional, but accuracy/latency is below the Swift-only SpeechAnalyzer; if quality matters later, a ~100-line Swift dylib shim is the upgrade path. On-device model availability varies by locale and may be absent until the user enables Dictation once, so ship graceful degradation to record-without-transcript. One-minute recognition-task limits from iOS lore don't formally apply on macOS but long sessions should restart the recognition request every ~60s and stitch transcripts. Finally, verify AVAudioFile AAC (.m4a) writing from the input format — if the converter path is painful, write CAF/PCM first and transcode is unnecessary since files are local-only.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP17 — live-motion-art

**Live motion art (animated album covers)**

- **Wave:** W2 · **Feasibility:** yes · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Live motion art in the shelf: Apple Music animated album covers (editorialVideo HLS loops) plus a subtle animated dominant-color visual behind the media card

### Approach

Two layers, ship both. (1) True animated covers: MediaRemote gives you only a static JPEG/PNG (`artworkData`) — animated covers are NOT in the now-playing pipeline. They live in the Apple Music catalog as the undocumented `editorialVideo` attribute on album resources: short looping HLS videos (`motionSquareVideo1x1`/`motionDetailSquare`, each with a `video` .m3u8 URL and a `previewFrame`). The pipeline every Droppy/NotchNook-style app uses (boring.notch has an open implementation of this; Cider does it via MusicKit): on track change, take title+artist+album from your existing AdapterTrack → resolve the album's adamId via the keyless public iTunes Search API (`https://itunes.apple.com/search?entity=album`) → call `https://amp-api.music.apple.com/v1/catalog/{storefront}/albums/{id}?extend=editorialVideo` with the anonymous bearer token scraped from music.apple.com's JS bundle (fetch the album page or `index.js`, regex the `eyJ…` JWT; cache it, refetch on 401) plus `Origin: https://music.apple.com` → if `editorialVideo` exists, get the m3u8. Play it with AVFoundation: `AVQueuePlayer` + `AVPlayerItem` + `AVPlayerLooper`, rendered by an `AVPlayerLayer` added as a sublayer of the island window's `contentView` layer (platform.rs already manipulates these layers with objc2 `msg_send`), positioned over the artwork rect with `cornerRadius` set, `isMuted=true`, `preventsDisplaySleepDuringVideoPlayback=false`. GPUI keeps drawing the static artwork underneath as the poster; the video layer sits above it, so a failed/removed layer degrades gracefully. Cache the resolved m3u8 (or download the small loop to ~/Library/Caches) keyed by album, and negative-cache albums without motion art so you hit the network at most once per album.

(2) The always-works fallback (most tracks have no editorialVideo, and Spotify matches are fuzzy): an animated "aura" behind the media card rendered purely in GPUI — two or three large blurred blobs in the 2–3 dominant artwork colors (extend the existing `sample_dominant_color` in media.rs to a small palette), drifting on slow sinusoidal keyframes. Drive it from the existing frame-request loop in island/mod.rs, capped at ~24–30fps, and only while the expanded media card is visible AND playback is active. This is what MediaMate/boring.notch actually ship as their 'animated' media look, and it needs zero network and zero new permissions.

Honest blockers: the amp-api token scrape is unofficial and ToS-gray — it can rotate or break (build it to fail silent to static art). Coverage is limited to albums Apple produced motion art for. AVPlayerLayer z-ordering must coexist with the fragile glass-mask code in platform.rs (comments at ~line 1421/1555 warn that layer churn flashes the glass) — add the video layer once per track, never per frame. No TCC prompts, no entitlements, no Full Disk Access, no new private frameworks: plain outbound HTTPS from an unsandboxed app plus public AVFoundation.

### APIs

- MediaRemoteAdapter (already integrated, private MediaRemote via com.apple.perl entitlement) — track identity + static artworkData only; no motion art here
- iTunes Search API — public, keyless: https://itunes.apple.com/search?entity=album to resolve adamId
- Apple Music AMP API (private/undocumented): https://amp-api.music.apple.com/v1/catalog/{storefront}/albums/{id}?extend=editorialVideo with anonymous web bearer JWT scraped from music.apple.com (fragile, ToS-gray)
- AVFoundation (public): AVQueuePlayer, AVPlayerItem, AVPlayerLooper, AVPlayerLayer (via objc2 msg_send or objc2-av-foundation crate; link AVFoundation + CoreMedia)
- Core Animation (public): CALayer addSublayer/setFrame/setCornerRadius on the window contentView, same pattern as platform.rs glass code
- reqwest (already a workspace dep) for token scrape + catalog calls + caching the loop file
- GPUI frame loop + motion.rs springs/keyframes for the fallback dominant-color aura (no new APIs)

### Permissions / TCC

None. No TCC category applies (network access is unprompted on macOS for a non-sandboxed app), no new entitlements, no Accessibility/Full Disk Access. AVFoundation and Core Animation are public frameworks. The only 'permission-like' risk is the unofficial amp-api bearer token, which is a ToS/fragility issue, not an OS-permission one; the already-shipped perl-based MediaRemote adapter is unchanged.

### Integration map (files to touch)

- crates/nook-core/src/motion_artwork.rs (NEW): token scrape/cache, iTunes search, editorialVideo fetch, on-disk cache + negative cache keyed by (artist, album); async fn lookup(track) -> Option<MotionArtwork { m3u8_url, preview_frame }>
- crates/nook-core/src/models.rs: add motion_artwork_url: Option<String> to NowPlayingData
- crates/nook/src/platform.rs: NEW video-layer block — attach_motion_art_layer(window, url, rect, radius), update_motion_art_frame, set_paused, remove; AVQueuePlayer/AVPlayerLooper/AVPlayerLayer via msg_send, inserted above the GPUI Metal layer, created once per track
- crates/nook/src/island/mod.rs: in the existing media poll loop (~line 466), on album change spawn the motion_artwork lookup; pause/remove the layer when island collapses, hides, or playback stops
- crates/nook/src/island/media.rs: in nook_media_pane/expanded card, reserve the artwork rect and report its screen frame to platform.rs when motion art is active (static art stays as poster); extend sample_dominant_color (~line 796) to a 2-3 color palette for the aura
- crates/nook/src/island/expanded.rs + motion.rs: fallback animated aura behind the media card — slow keyframe blob drift using palette colors, frame requests only while visible+playing
- crates/nook-core/src/settings.rs + crates/nook/src/island/settings.rs: two toggles — 'Animated album art (Apple Music)' (network, default off) and 'Ambient art glow' (local, default on)

### Battery requirements

Near-zero idle by construction: no new polling at all — the catalog lookup fires only on album change inside the existing media poll, with positive and negative caching so each album costs at most one search + one catalog call ever. The HLS loop is hardware-decoded (VideoToolbox) via AVPlayerLayer at ~84-240pt, a few percent GPU while visible; pause the AVPlayer and hide the layer the moment the island collapses, the screen sleeps, or playback pauses (hook the same state transitions that already gate the visualizer). Optionally auto-disable on battery via IOPSGetTimeRemainingEstimate/NSProcessInfo.isLowPowerModeEnabled check at layer-attach time. The GPUI aura must be gated the same way: request frames only while expanded+playing, cap at 24-30fps by skipping frames in the existing frame loop, and let it settle to a static gradient when paused — zero frames requested when idle.

### Risks & honest blockers

1) The music.apple.com anonymous token and amp-api endpoint are unofficial — they rotate occasionally and could break or be rate-limited; design for silent fallback to static art and cache the token. 2) Catalog matching from Spotify/browser metadata is fuzzy — verify artist+album string match before accepting an adamId or you'll loop the wrong album's video. 3) Coverage: only a minority of albums (major editorial releases) have editorialVideo, so the aura fallback is what users see most of the time — set expectations in the settings copy. 4) AVPlayerLayer compositing over GPUI's Metal layer must not disturb the glass-effect view stack platform.rs warns about (re-masking flashes the glass); attach once per track, move via setFrame only, and test corner clipping against the notch chrome. 5) GPUI has no video element, so if the sublayer approach fights the renderer the fallback is AVPlayerItemVideoOutput frame-pumping into gpui::Image at 15-24fps — works but costs CPU, so treat it as plan B. 6) ToS-gray scraping in a distributed app: keep the feature opt-in and clearly labeled.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP18 — media-queue

**Full media player with Playing Next queue**

- **Wave:** W2 · **Feasibility:** partial · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Full media player in the expanded island with a Playing Next queue (Apple Music/Spotify): current track, artwork, scrubber, and upcoming queue with per-item play

### Approach

The "full player" half is already shipped: crates/nook-core/src/mediaremote.rs wraps ungive/mediaremote-adapter (private MediaRemote called from an Apple-signed /usr/bin/perl helper, the standard post-macOS-15.4 workaround also used by boring.notch), exposing get_now_playing (title/artist/album/artwork/duration/elapsed), send(MraCommand) and seek_seconds — so current track, artwork, play/pause/next/prev and a scrubber all work today (see nook_media_pane/media_card in crates/nook/src/island/media.rs, which already render a seekable progress bar). Remaining player polish is UI-only: a draggable scrubber thumb instead of the 24 discrete seek-hit zones, and elapsed-time interpolation between polls.

The queue is the hard part and is per-service. Spotify: neither AppleScript nor MediaRemote exposes its queue; the only real path is the Spotify Web API — OAuth 2.0 PKCE (loopback redirect on 127.0.0.1, user-read-playback-state + user-modify-playback-state scopes), then GET /v1/me/player/queue (returns currently_playing plus ~20 upcoming items with names/artists/artwork URLs). Per-item play has no direct endpoint: implement "jump to item N" as N sequential POST /v1/me/player/next calls (what third-party Spotify controllers do), or PUT /v1/me/player/play with context_uri+offset when the item belongs to the current album/playlist context. Playback-control endpoints require Spotify Premium and a developer-app client ID. Apple Music: the true "Playing Next" queue is NOT readable — Music.app's AppleScript dictionary omits it, and the private MediaRemote queue path (MRPlaybackQueueRequest/MRContentItem) is undocumented, not implemented by mediaremote-adapter, and entitlement-gated on modern macOS; treat it as a dead end. Ship a pseudo-queue instead: via AppleScript read `current playlist` and the current track's index, list the following tracks, and per-item play with `play track i of current playlist`. This is correct when playing an album/playlist unshuffled, wrong under shuffle and blind to manually queued Play Next items — label the section "Up Next in playlist" and hide it when shuffle is on (`shuffle enabled` is readable). Comparable apps (NotchNook, boring.notch, MediaMate) show now-playing via the same adapter trick but none render an Apple Music queue, which corroborates the blocker.

### APIs

- mediaremote-adapter (ungive) driving private MediaRemote.framework: MRMediaRemoteGetNowPlayingInfo, MRMediaRemoteSendCommand, MRMediaRemoteSetElapsedTime (private, via /usr/bin/perl helper — already integrated)
- MRPlaybackQueueRequest / MRContentItem (private MediaRemote queue API — entitlement-gated, unimplemented in adapter; documented here as a rejected path)
- Spotify Web API: GET /v1/me/player/queue, POST /v1/me/player/next, PUT /v1/me/player/play (context_uri+offset), OAuth 2.0 PKCE with scopes user-read-playback-state, user-modify-playback-state
- AppleScript via /usr/bin/osascript to Music.app: current playlist, index of current track, tracks of current playlist, play track N of current playlist, shuffle enabled, player position (already partially used in audio.rs)
- AppleScript to Spotify.app: play/pause, player position, play track "spotify:track:..." (fallback control only, no queue)
- DistributedNotificationCenter (NSDistributedNotificationCenter via objc2): com.spotify.client.PlaybackStateChanged and com.apple.Music.playerInfo / com.apple.iTunes.playerInfo — event-driven track/state change triggers
- Security.framework Keychain (SecItemAdd/SecItemCopyMatching) for the Spotify refresh token
- reqwest (already a workspace dep) for the Web API + queue artwork fetches

### Permissions / TCC

Automation TCC (kTCCServiceAppleEvents) prompts once per target app (Music.app, Spotify.app) for the AppleScript paths — needs NSAppleEventsUsageDescription in Info.plist (likely already present since audio.rs uses osascript today; verify). The mediaremote-adapter path needs no TCC and works unsigned because the entitled process is Apple's perl. Spotify Web API needs no macOS permission, only network plus a Spotify developer client ID (PKCE, no client secret — safe to embed, but shipping one binds all users to your app's API quota and Spotify ToS; Premium required for control endpoints). No Full Disk Access, no Accessibility, no private entitlements the unsigned app would have to hold itself.

### Integration map (files to touch)

- crates/nook-core/src/spotify.rs (new): PKCE OAuth — open browser to accounts.spotify.com/authorize, one-shot TcpListener on 127.0.0.1 for the redirect, token refresh; queue fetch (GET /me/player/queue) and jump-to-item (skip-N / context+offset); refresh token in Keychain
- crates/nook-core/src/queue.rs (new): unified QueueItem {title, artist, artwork_url/b64, source, jump handle} + QueueSource enum; Apple Music pseudo-queue via one osascript snapshot of current playlist (window of ~10 tracks after current index, skip when shuffle enabled); dispatch per-item play to spotify.rs or osascript
- crates/nook-core/src/models.rs: add QueueItem and Option<Vec<QueueItem>> alongside NowPlayingData (keep queue out of the hot now-playing struct; separate fetch)
- crates/nook-core/src/audio.rs: expose track-change signal the queue layer keys off (is_track_changed already exists); add media_jump_to_queue_item entry point mirroring media_seek/media_next_track
- crates/nook/src/island/media.rs: extend media_card (expanded card — this is where the feature lands; compact face and album_chip stay unchanged) with a scrollable 'Up Next' list: 40px rows, thumb + title/artist + play-on-click; reuse artwork_element/decode_artwork for thumbs
- crates/nook/src/island/mod.rs: fetch queue only when expanded AND media card visible — trigger on expand and on track change inside the existing now-playing poll loop (lines ~467-516), never on its own timer; optionally register DistributedNotificationCenter observers here (via platform.rs objc2 helpers) to replace the 400ms poll with event-driven wakeups
- crates/nook-core/src/settings.rs + crates/nook/src/island/settings.rs: 'Connect Spotify' button, connection status, disconnect; persist client ID override
- crates/nook/src/platform.rs: small objc2 helper to add NSDistributedNotificationCenter observers with a Rust callback
- Info.plist / bundling: confirm NSAppleEventsUsageDescription; no new bundle resources needed (adapter already bundled)

### Battery requirements

Near-zero idle is achievable: fetch the queue only when the expanded media card is actually visible, refreshing on (a) expand and (b) track change — never on a standalone interval. Track change is already detected by the existing now-playing loop; better, subscribe to DistributedNotificationCenter (com.spotify.client.PlaybackStateChanged, com.apple.Music.playerInfo) so the 400ms/2s poll in island/mod.rs can back off to multi-second cadence and wake instantly on real changes — this also improves the app overall, since each current poll spawns a perl process. Scrubber progress between updates should be interpolated locally from elapsed_time + wallclock (the adapter already returns elapsedTimeNow), not polled faster. Queue artwork: Spotify returns URLs — fetch the 64px size lazily per row, cache by track ID; Apple Music pseudo-queue rows can skip artwork or fetch on hover, since per-track AppleScript artwork extraction is expensive. One osascript snapshot per track change, one HTTPS call per queue refresh: effectively zero cost when the island is closed.

### Risks & honest blockers

Honest blockers: (1) Apple Music's real Playing Next queue is unreadable — the AppleScript pseudo-queue misses manually queued items and breaks under shuffle/autoplay-radio; the private MediaRemote queue API is entitlement-gated and not worth chasing. Set UI expectations ("Up Next in playlist") or ship Apple Music queue read-only-when-unshuffled. (2) Spotify per-item play is a hack: skip-N is racy (queue can change mid-jump), counts as skips in Spotify's metrics, and the queue endpoint caps at ~20 items with no remove/reorder API; control endpoints require Premium, and a free-tier user gets 403s you must surface gracefully. (3) Shipping an embedded Spotify client ID puts all users on one API quota and requires the app to be registered in Spotify's dev dashboard (extension-quota approval for >25 users); alternative is asking users for their own client ID, which is hostile UX. (4) The Automation TCC prompt for Music/Spotify can be denied, silently killing the AppleScript pseudo-queue — detect -1743 errors and show a settings hint. (5) mediaremote-adapter itself is a private-API workaround Apple could close in a future macOS; the queue design should keep AppleScript/Web API paths functional without it.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP19 — meetings

**Meeting controls (Zoom / Teams / Meet)**

- **Wave:** W2 · **Feasibility:** partial · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Meetings — detect active Zoom/Teams/Google Meet calls and offer mute/unmute + leave from the island

### Approach

DETECTION (all three: YES, fully event-driven).
1. App presence: NSWorkspace notificationCenter observers for NSWorkspaceDidLaunchApplicationNotification / NSWorkspaceDidTerminateApplicationNotification, filtering bundle IDs us.zoom.xos, com.microsoft.teams2 (new Teams), and the browser IDs already enumerated in nook-core/src/browser_media.rs. Zero cost until a candidate app launches.
2. Mic-in-use: CoreAudio. Baseline (all macOS): AudioObjectAddPropertyListener on the default input device for kAudioDevicePropertyDeviceIsRunningSomewhere (plus kAudioHardwarePropertyDefaultInputDevice to re-arm on device switch) — push, not poll, and needs NO TCC permission (device state, not audio content). Attribution (macOS 14+): the public process-object API in AudioHardware.h — kAudioHardwarePropertyProcessObjectList, then per-object kAudioProcessPropertyPID / kAudioProcessPropertyBundleID / kAudioProcessPropertyIsRunningInput — tells you WHICH process (zoom.us, MSTeams, Google Chrome Helper) holds the live input; also usable as a listener on the property for event-driven attribution. Bind via the coreaudio-sys crate (MIT, bindgen-only, no binary) or ~40 lines of raw extern "C" declarations with #[link(name="CoreAudio", kind="framework")] — no new bundled binaries.
3. Meeting confirmation: Zoom — AXUIElementCreateApplication(pid) and check the menu bar for the "Meeting" menu (only exists in-meeting); this is how the Lostdomain Stream Deck Zoom plugin and MuteDeck do it. Meet — reuse the existing AppleScript tab-URL reader in browser_media.rs and match meet.google.com/xxx-xxxx-xxx. Teams — mic attribution + app running is the practical signal (post-API-retirement there is no state channel). Avoid CGWindowListCopyWindowInfo for window titles: titles require Screen Recording TCC; AX needs only Accessibility, which we need anyway.

CONTROL.
- Zoom (YES, the solid one): with Accessibility permission, click menu items in zoom.us's menu bar without focusing it — AX traversal to menu item "Mute audio"/"Unmute audio" in menu "Meeting", then AXPress (or equivalently System Events AppleScript 'click menu item ... of menu "Meeting" of menu bar 1 of process "zoom.us"' via the existing run_osascript). The toggling menu-item title doubles as MUTE-STATE READBACK — poll it only while the meeting face is visible. Leave: CGEventPostToPid Cmd+W to zoom.us, then AXPress the "Leave meeting"/"End" button on the confirmation sheet via AX. Do menu lookup by AX with fallback to English titles; titles are localized, so prefer position/identifier heuristics or read the user's Zoom locale. Do NOT rely on Zoom's optional "global shortcut" checkbox (user config dependent).
- Teams (PARTIAL — honest blocker): the proper channel, the local third-party WebSocket API at ws://localhost:8124 that MuteDeck/Stream Deck used, was RETIRED by Microsoft on June 30, 2026 (MC1266901); Elgato pulled its plugin in Dec 2025. What remains: inject the in-app shortcuts Cmd+Shift+M (mute) and Cmd+Shift+H (hang up) with CGEventPostToPid targeted at the Teams pid — works without stealing focus in most cases (fails if the meeting window is minimized), requires Accessibility. NO reliable mute-state readback: Teams mutes in software and keeps the capture stream open, so CoreAudio can't distinguish muted/unmuted. Ship the buttons as fire-and-forget toggles with an "unverified state" UI treatment.
- Google Meet (PARTIAL): detection is easy (tab URL + mic attribution to the browser process). Control options, in order of honesty: (a) focus the Meet tab (AppleScript 'set active tab', already have per-browser Automation TCC from browser_media) then post Cmd+D / Cmd+E keystrokes — reliable but focus-stealing; (b) Chrome/Safari 'execute javascript' via Apple Events to click the [data-is-muted] button without focus — blocked by default, user must enable "Allow JavaScript from Apple Events" (Chrome View>Developer, Safari Develop menu), so offer as opt-in; (c) a bundled WebExtension talking to the app over a localhost WebSocket — what MuteDeck actually ships for Meet, correct but XL scope, defer. Recommend (a) as default, (b) as power-user opt-in.

STATE OF THE ART: NotchNook does not ship meeting controls; the real comparables are MuteDeck and Mutesync, and even MuteDeck needs a browser extension for Meet and lost Teams state sync to the API retirement — so "Zoom full, Teams blind toggle, Meet best-effort" is genuinely the ceiling without shipping an extension.

Sources: [MC1266901 Teams API retirement](https://mc.merill.net/message/MC1266901), [MakeUseOf on the Teams integration removal](https://www.makeuseof.com/microsoft-killed-teams-integration-without-telling-anyone/), [MuteDeck Teams API support](https://mutedeck.com/blog/new-microsoft-teams-api-support-new-actions-touch-portal-plugin-and-more/), [MuteDeck Google Meet extension](https://chromewebstore.google.com/detail/mutedeck/egphpgddoenbpakmaojmnjpjoflmknjk?hl=en), [Lostdomain Stream Deck Zoom plugin (AppleScript menu technique)](https://lostdomain.org/2020/06/17/introducing-the-stream-deck-plugin-for-zoom), [teams-mac-hotkeys (keystroke injection)](https://github.com/RobvH/teams-mac-hotkeys), [keeping Stream Deck working post-retirement via shortcuts](https://teams.handsontek.net/2025/11/28/keep-using-elgato-stream-deck-microsoft-teams-api-deprecation/)

### APIs

- CoreAudio: AudioObjectAddPropertyListener + kAudioDevicePropertyDeviceIsRunningSomewhere, kAudioHardwarePropertyDefaultInputDevice (all macOS, no TCC)
- CoreAudio (macOS 14+): kAudioHardwarePropertyProcessObjectList, kAudioProcessPropertyPID, kAudioProcessPropertyBundleID, kAudioProcessPropertyIsRunningInput — per-process mic attribution
- AppKit: NSWorkspace didLaunch/didTerminateApplicationNotification (event-driven app watch)
- ApplicationServices AX: AXUIElementCreateApplication, AXUIElementCopyAttributeValue (menu traversal), AXUIElementPerformAction(kAXPressAction), AXIsProcessTrustedWithOptions
- CoreGraphics: CGEventCreateKeyboardEvent + CGEventPostToPid (focus-free keystrokes to Zoom/Teams)
- AppleScript via existing run_osascript: System Events menu clicks (Zoom fallback), browser tab URL/activation, optional 'execute javascript' in Chrome/Safari for Meet
- Retired/blocked: Teams local WebSocket API ws://localhost:8124 (killed 2026-06-30); no public Zoom or Meet control API without OAuth/cloud

### Permissions / TCC

Accessibility (kTCCServiceAccessibility) — required for AX menu clicking and CGEventPostToPid; works fine unsigned/dev-signed, user grants once in System Settings; gate with AXIsProcessTrustedWithOptions prompt. Automation (kTCCServiceAppleEvents) per target — System Events (new) and per-browser (already granted for browser_media). No Microphone TCC needed: kAudioDevicePropertyDeviceIsRunningSomewhere and process objects are device state, not audio capture. Avoid Screen Recording by never reading window titles via CGWindowList.

### Integration map (files to touch)

- NEW crates/nook-core/src/meetings.rs — MeetingState machine (Idle -> AppRunning -> MicLive(app) -> InMeeting{app, muted: Option<bool>}), CoreAudio listener FFI, mic->pid attribution, per-app control dispatch
- crates/nook/src/platform.rs — add AX helpers (create app element, walk menu bar, press), CGEventPostToPid keystroke sender, AXIsProcessTrustedWithOptions check, NSWorkspace launch/terminate observers (extends existing objc2 msg_send patterns at lines ~565/1054/1333)
- crates/nook-core/src/browser_media.rs — reuse applescript_app() browser table; add find/activate Meet tab and optional execute-javascript path
- crates/nook/src/island/mod.rs spawn_loops() — no new poll loop; subscribe to meetings events, and only while a meeting face is shown run a 1-2s Zoom menu-title readback tick (pattern mirrors the agents loop at line ~561)
- crates/nook/src/island/compact.rs — compact face: mic glyph + app icon, green/orange for live/muted (Zoom only shows true state; Teams/Meet show neutral)
- NEW crates/nook/src/widgets/meeting.rs — expanded card: app icon, elapsed time, Mute/Unmute + Leave buttons, register in widgets/mod.rs like observe/speed cards
- crates/nook/src/island/render.rs + motion.rs — brief HUD confirmation flash on mute toggle (reuse existing reveal spring)
- crates/nook-core/src/settings.rs + crates/nook/src/island/settings.rs — per-app enable toggles, Meet control mode (focus-tab vs Apple-Events-JS), Accessibility permission status row

### Battery requirements

Zero idle cost by construction: NSWorkspace notifications (push) arm the feature only while a meeting-capable app is running; the CoreAudio kAudioDevicePropertyDeviceIsRunningSomewhere listener is a kernel-driven callback, not a poll, and costs nothing between events; AX/AppleScript reads happen only (a) once on mic-live transition to confirm a meeting and (b) on a short interval strictly while the meeting compact face/card is visible — stop the readback tick the moment the meeting face hides or mic goes idle. No osascript processes at rest, no new threads at rest, no CGWindowList sweeps ever. Process-object attribution is a one-shot property read per mic transition, not a stream/tap (never call AudioHardwareCreateProcessTap — that would capture audio and trigger the mic indicator).

### Risks & honest blockers

1) Teams is the big blocker: the localhost:8124 API is retired (June 2026), so Teams gets blind keystroke toggles with no mute-state readback and breakage if the meeting window is minimized; UI must not claim a state it cannot verify. 2) Zoom AX menu titles are localized and change across Zoom releases ("Mute audio" vs "Unmute audio"); need locale-aware or structural matching and a test matrix per Zoom update. 3) Google Meet without a bundled extension is either focus-stealing (activate tab + Cmd+D) or gated on a hidden browser developer flag; a proper extension is the only clean fix and is XL extra scope plus Chrome Web Store distribution. 4) Accessibility TCC on an unsigned/dev-signed app resets whenever the binary's signature changes between builds — annoying during development and across ad-hoc-signed updates. 5) kAudioDevicePropertyDeviceIsRunningSomewhere fires for any capture (Siri, voice memos, browser mic tests), so meeting confirmation must always be app+mic joint evidence, or the face will false-positive. 6) Zoom leave flow depends on a confirmation-dialog AXPress that Zoom occasionally redesigns.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP20 — file-processing

**File-processing suite (Convert / Compress / BG-removal / OCR)**

- **Wave:** W3 · **Feasibility:** yes · **Effort:** XL (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Local file-processing suite (Converter, Video Target Size, PDF Compress, AI Background Removal, OCR) for files dropped on the island

### Approach

All five sub-features are feasible on-device with system frameworks; only exotic codec output (mkv/webm/mp3/opus) needs an optional bundled ffmpeg. Per sub-feature:

(a) CONVERTER — feasibility: yes.
- Images: two paths. Pure-Rust via the `image` crate already in nook's deps (png/jpeg/gif/webp/tiff/bmp encode+decode, zero new deps) — covers 90% of drops. For HEIC (decode+encode) and RAW decode, use ImageIO (C API: CGImageSourceCreateWithURL + CGImageDestinationCreateWithURL with UTType `public.heic`, macOS 10.13+; hardware HEVC on Apple Silicon). ImageIO is a C framework, so declare a small extern block or use the objc2-core-graphics crate; note ImageIO can DECODE webp/avif (macOS 11/13+) but cannot ENCODE webp/avif — the `image` crate covers webp encode (lossless), avif encode is realistically out of scope.
- Audio: AVFoundation. AVAssetExportSession with AVAssetExportPresetAppleM4A for aac/alac→m4a; ExtAudioFile/AVAudioConverter (or AVAssetReader/Writer with AVAudioSettings) for wav/aiff/caf/aac/alac in any direction. Hard limit: Apple frameworks decode MP3 but do NOT encode MP3, and no Opus/Ogg — those need ffmpeg (with LGPL-compatible libmp3lame/libopus).
- Video: AVAssetReader + AVAssetWriter (not just AVAssetExportSession — writer gives codec/bitrate control) with AVVideoCodecTypeH264 / AVVideoCodecTypeHEVC; VideoToolbox hardware encode is automatic. Containers limited to mp4/mov/m4v. mkv INPUT is not readable by AVFoundation at all; webm/vp9/av1 encode unavailable (AV1 is decode-only on M3+). Video→GIF via CGImageDestination with kCGImagePropertyGIFDelayTime.
- ffmpeg strategy: ship WITHOUT it by default; offer an opt-in "extended formats" toggle in Settings that downloads (or bundles) an LGPL-configured ffmpeg — built with h264_videotoolbox/hevc_videotoolbox (no GPL x264/x265), native aac, libmp3lame (LGPL), libopus (BSD), libvpx (BSD): that single LGPL binary adds mkv/webm/mp3/opus in AND out. ~25–40 MB per arch (~60–75 MB universal). Invoke as a subprocess (no linking → no license contamination; LGPL obligation is just shipping the ffmpeg source offer/notice, same pattern as the already-bundled MediaRemoteAdapter LICENSE in scripts/bundle.sh). Comparable apps: Dropover/CleanShot use system frameworks; Permute/Downie bundle or side-load ffmpeg; Raycast converts via system frameworks; Alfred workflows shell out to a user-installed ffmpeg.

(b) VIDEO TARGET SIZE — feasibility: yes.
- Compute video bitrate = (target_bytes×8 − audio_bytes×8 − container overhead ~1.5%) / duration_s; get duration/tracks via AVAsset. Encode with AVAssetWriter, AVVideoCompressionPropertiesKey { AVVideoAverageBitRateKey } (H.264) — VideoToolbox ABR lands within ~5–10%. Aim 4% under target, stat the output, and re-encode once with a corrected bitrate if over (rarely needed). AVAssetExportSession.fileLengthLimit exists as a simpler advisory API but is not guaranteed; the writer route is the reliable one. Audio: fixed 96–128 kbps AAC. Same approach every "8MB for Discord" utility uses; ffmpeg optional here only for two-pass x264 (GPL — skip it).

(c) PDF COMPRESS — feasibility: partial (quality control is the caveat, not possibility).
- Primary: PDFKit (Quartz.framework) `-[PDFDocument writeToURL:withOptions:]` with macOS 13+ write options PDFDocumentSaveImagesAsJPEGOption and PDFDocumentOptimizeImagesForScreenOption — recompresses embedded images while keeping the text layer. On macOS 12: fall back to applying the system QuartzFilter "Reduce File Size" (Quartz.framework QuartzFilter API, public but ancient; output quality notoriously aggressive).
- Aggressive mode for a target size: render each page via PDFPage/CGPDFDocument to a bitmap at chosen DPI/JPEG quality and rebuild a PDF with CGPDFContext — loses selectable text (offer OCR re-layer via (e) as a bonus). Avoid Ghostscript (AGPL); qpdf (Apache-2.0) only restructures, doesn't recompress images — not worth bundling.

(d) AI BACKGROUND REMOVAL — feasibility: yes.
- macOS 14+: Vision VNGenerateForegroundInstanceMaskRequest (the system "lift subject" model, any salient object, runs on the Neural Engine); call generateMaskedImageOfInstances:fromRequestHandler:croppedToInstancesExtent: → CVPixelBuffer with alpha → write PNG/HEIC via ImageIO or CIContext.
- macOS 12–13 fallback: VNGeneratePersonSegmentationRequest (people only, qualityLevel accurate) + CIBlendWithMask (CoreImage) to apply the mask.
- No TCC, no bundled model, fully offline. This is exactly what CleanShot X and Raycast's "Remove Background" do. Rust binding: `objc2-vision` + `objc2-core-image` crates exist in the same madsmtm family as the objc2-event-kit already used; otherwise raw msg_send! per the agents.rs pattern.

(e) OCR — feasibility: yes.
- Images: Vision VNRecognizeTextRequest (macOS 10.15+; VNRequestTextRecognitionLevelAccurate, usesLanguageCorrection, automaticallyDetectsLanguage on 13+). Result → NSPasteboard general.
- PDFs: first try PDFKit `PDFDocument.string` (embedded text layer, instant, no ML); if empty, render pages to CGImage (PDFPage thumbnailOfSize or CGContextDrawPDFPage at 2x) and run VNRecognizeTextRequest per page.
- Screen region: spawn `/usr/sbin/screencapture -i -x -t png <scratch.png>` (system region-picker UI for free, same trick TextSniper/Raycast use), OCR the file, copy text, delete the temp file. Requires the Screen Recording TCC grant attributed to the app (one prompt; macOS 15 re-approval nag applies, reduced in 15.1+). ScreenCaptureKit/SCScreenshotManager is the "proper" API but needs the same permission and you'd have to build your own region picker — use screencapture.

WHAT TO BUNDLE vs SYSTEM: default build bundles NOTHING new — ImageIO/CoreGraphics, AVFoundation/VideoToolbox, Quartz(PDFKit), Vision, CoreImage are all system. Optional LGPL ffmpeg (subprocess, Contents/Resources or ~/Library/Application Support download-on-first-use) is the only binary, gated behind a Settings toggle.

### APIs

- ImageIO: CGImageSourceCreateWithURL / CGImageDestinationCreateWithURL (HEIC encode/decode, RAW/webp/avif decode, animated GIF write)
- Rust `image` crate (already a dependency): png/jpeg/gif/webp/tiff encode-decode
- AVFoundation: AVAsset, AVAssetReader/AVAssetReaderTrackOutput, AVAssetWriter/AVAssetWriterInput with AVVideoCompressionPropertiesKey + AVVideoAverageBitRateKey, AVAssetExportSession (+ .fileLengthLimit, AVAssetExportPresetAppleM4A), AVAudioConverter/ExtAudioFile
- VideoToolbox (implicit hardware H.264/HEVC encode via AVAssetWriter)
- Quartz/PDFKit: PDFDocument writeToURL:withOptions: with PDFDocumentSaveImagesAsJPEGOption + PDFDocumentOptimizeImagesForScreenOption (macOS 13+), PDFDocument.string, PDFPage rendering; QuartzFilter 'Reduce File Size' fallback; CGPDFContext for rebuild
- Vision: VNGenerateForegroundInstanceMaskRequest (macOS 14+), VNGeneratePersonSegmentationRequest (macOS 12+ fallback), VNRecognizeTextRequest (accurate level)
- CoreImage: CIBlendWithMask, CIContext writePNGRepresentation
- /usr/sbin/screencapture -i -x (system region picker for OCR)
- NSPasteboard generalPasteboard (OCR result copy)
- Optional subprocess: LGPL-built ffmpeg with h264_videotoolbox/hevc_videotoolbox, libmp3lame, libopus, libvpx (~25–40 MB/arch)
- Rust crates: objc2-vision, objc2-av-foundation, objc2-core-image, objc2-core-graphics (same objc2 0.6 family as existing objc2-event-kit), or raw msg_send! per crates/nook-core/src/agents.rs pattern

### Permissions / TCC

None for (a)-(d): the app is non-sandboxed and files arrive by explicit drag, so plain POSIX file access suffices; Vision/AVFoundation/PDFKit local processing has no TCC gate. (e) OCR screen-region capture requires the Screen Recording TCC grant (System Settings > Privacy > Screen & System Audio Recording) — prompt fires automatically on first screencapture/ScreenCaptureKit use, no Info.plist usage-description key required; macOS 15.x shows periodic re-approval reminders. OCR of dropped images/PDFs needs no permission. No new Info.plist keys needed (existing /Users/jonasvogel/openNook/Info.plist already handles camera/calendar).

### Integration map (files to touch)

- crates/nook-core/src/process/ — NEW module tree: mod.rs (job queue: ProcessJob{id, kind, input, output, progress: Arc<AtomicU8>, status}), imageconv.rs (image crate + ImageIO externs), avconv.rs (AVAssetReader/Writer audio+video, target-size bitrate math), pdf.rs (PDFKit write-options + rasterize path), vision.rs (foreground mask, person segmentation fallback, OCR incl. PDF page loop), ffmpeg.rs (optional subprocess wrapper, presence detection)
- crates/nook-core/src/files.rs — extend mime_from_path consumers with per-type capability lookup (which actions a FileTrayItem supports); add output-file insertion back into the tray via add_dropped_path
- crates/nook/src/island/files.rs — action affordances on file_card (hover/right-click menu: Convert, Target size, Compress PDF, Remove BG, Copy Text); reuse the airdrop_target pattern (line ~155) to show Convert/OCR drop chips next to AirDrop while file_drag is hot; processed outputs appear as new tiles
- crates/nook/src/widgets/ — NEW process.rs expanded card: job list with progress bars, format/size pickers; follow widgets/speed.rs (line 120-160) cx.spawn + background_executor pattern for off-main-thread work
- crates/nook/src/island/compact.rs + mod.rs — compact Live Activity face while a job runs (thumbnail + progress ring, like media playback); completion flash/HUD 'Saved clip-8MB.mp4' and 'Copied 214 chars' for OCR
- crates/nook/src/island/settings.rs — new File Actions section: default output formats, output folder (default: alongside source or ~/Downloads), JPEG quality, PDF preset, ffmpeg extended-formats toggle + download/license notice
- crates/nook-core/src/settings.rs — persist the above
- scripts/bundle.sh + Info.plist — only if ffmpeg is bundled rather than downloaded: copy binary + LICENSE into Contents/Resources (same pattern as MediaRemoteAdapter at bundle.sh:39-43) and codesign it
- Cargo.toml (nook-core): add objc2-vision / objc2-av-foundation / objc2-core-image (macOS target only)

### Battery requirements

Zero idle cost by construction: every sub-feature is strictly user-triggered (drop + explicit action), no polling loops, no resident daemons, no models loaded at launch. Run jobs on cx.background_executor (speed.rs pattern); Vision/CoreML models load lazily on first use and are released after the request completes. Video encode uses VideoToolbox hardware (efficient, but a long transcode is inherently power-hungry — show progress in compact face so users expect it; consider ProcessInfo beginActivityWithOptions:NSActivityUserInitiated around jobs so App Nap doesn't throttle them, ended immediately after). Progress UI: drive repaints from the writer callback/progress atomic only while a job is live — never a timer when the queue is empty. ffmpeg subprocess is spawned per job and exits. OCR screen capture is one screencapture invocation, no persistent SCStream. Net idle drain: zero.

### Risks & honest blockers

1) Codec gaps are real: no MP3/Opus encode, no mkv read, no webm/av1 write without ffmpeg — decide up front whether 'Converter' means 'Apple-ecosystem formats' (zero bundle) or 'everything' (+60-75 MB universal ffmpeg + LGPL source-offer obligation; GPL builds like evermeet.cx statics must be avoided or the whole distribution inherits GPL). 2) Target-size is ABR-approximate: VideoToolbox has no true two-pass; guarantee requires an occasional second encode pass (doubles time on misses) — set expectations at ±5%. 3) PDF compression quality: the good API (SaveImagesAsJPEG/OptimizeImagesForScreen) is macOS 13+; the QuartzFilter fallback produces visibly muddy output, and no system API hits an exact target size without rasterizing (which kills text selection). 4) VNGenerateForegroundInstanceMaskRequest is macOS 14+ — on 12/13 background removal silently degrades to persons-only; gate the menu item by OS version. 5) Screen Recording TCC for region-OCR is the scariest prompt in macOS and triggers Sequoia's recurring re-approval nag; unsigned/dev-signed distribution makes TCC grants reset on every re-sign with a different identity (ad-hoc signatures lack a stable designated requirement) — Developer ID signing strongly recommended before shipping this. 6) objc2 framework crates (Vision/AVFoundation) are auto-generated and partially unsafe/untested; block-callback plumbing (completion handlers, sample-buffer loops) in Rust is verbose — budget extra time vs a Swift codebase; the calendar.rs/agents.rs msg_send precedent shows it works but AVAssetWriter's pull-model (requestMediaDataWhenReadyOnQueue) is the hairiest interop in the group. 7) Long transcodes vs app lifecycle: killing the island mid-encode must clean up partial output files.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP21 — search-clipboard

**Universal search + clipboard history (Thunderstorm)**

- **Wave:** W3 · **Feasibility:** yes · **Effort:** XL (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Thunderstorm — universal search (Spotlight) + clipboard history behind one global hotkey, with a text field in the island

### Approach

All four sub-features are feasible; none require private frameworks or bundled binaries. (1) GLOBAL HOTKEY (feasible: yes, effort S): use Carbon RegisterEventHotKey + InstallEventHandler — still fully functional on current macOS, zero TCC prompts, zero idle cost (kernel-delivered event, no polling). Easiest via the `global-hotkey` crate (Tauri project, MIT/Apache-2.0, wraps exactly this; runs its handler on the main thread run loop). This is how Rectangle (via MASShortcut) and Alfred do it. Do NOT use CGEventTap or NSEvent.addGlobalMonitorForEvents — both demand Accessibility/Input Monitoring TCC and cost more. On hotkey: record the current frontmost app (NSWorkspace.frontmostApplication), call the existing `platform::activate_app()` (activateIgnoringOtherApps:), expand the island into search mode, focus the query field; on dismiss, re-activate the remembered NSRunningApplication so the user's app gets focus back. GPUI note: the island window must be able to become key — the notes editor already receives KeyDownEvents and IME input via ElementInputHandler, so this path is proven in-app; reuse that editor pattern (single-line variant) for the query field. (2) SPOTLIGHT SEARCH (yes, effort M): use the C MDQuery API from CoreServices (MDQueryCreate with a query string like `(kMDItemDisplayName == "*term*"cd) || (kMDItemTextContent == "term*"cd)`, MDQuerySetMaxCount(~40), MDQueryExecute(kMDQuerySynchronous) on a background thread, MDQueryGetResultAtIndex → MDItemRef → MDItemCopyAttribute for kMDItemPath/kMDItemDisplayName/kMDItemContentType). MDQuery is far simpler to drive from Rust/objc2 than NSMetadataQuery (which needs a delegate + run loop + NSNotification plumbing); declare the CoreServices externs by hand (~10 functions) or via the `core-foundation` crate types. Alfred and Raycast both sit on the same metadata index (equivalent to `mdfind`). Rank apps first (filter kMDItemContentTypeTree == com.apple.application-bundle, boost prefix matches), then files. Launch results with NSWorkspace openURL/openApplicationAtURL — no read access to the file needed, so no TCC prompt on launch. Debounce keystrokes 150–250 ms, cancel/release the previous MDQueryRef before issuing the next, synchronous queries on a background executor thread. Optionally shell out to `mdfind` for a first prototype, but ship in-process MDQuery (no process-spawn cost per keystroke). (3) CLIPBOARD HISTORY (yes, effort M): NSPasteboard.generalPasteboard has no change notification — polling changeCount is the only mechanism, exactly what Maccy does (0.5 s Timer). changeCount is a single cheap XPC read from pboardd; poll at 1 Hz (configurable 0.5–2 s) from an existing spawn_loops cadence in island/mod.rs — the codebase already polls the drag pasteboard this way in nook-core/src/files.rs, so copy that msg_send pattern against pasteboardWithName:NSPasteboardNameGeneral. On changeCount delta, read pasteboardItems: capture public.utf8-plain-text always; optionally public.png/public.tiff thumbnails (cap ~1 MB, downscale) and public.file-url. PRIVACY: skip any item whose types contain org.nspasteboard.TransientType, org.nspasteboard.ConcealedType, or org.nspasteboard.AutoGeneratedType (the nspasteboard.org convention all password managers follow), plus Apple's com.apple.pasteboard.promised-file-content-type oddities; also offer a per-app exclusion list keyed on NSWorkspace.frontmostApplication.bundleIdentifier at copy time (best-effort — the copier isn't directly knowable, frontmost is the standard heuristic Maccy uses). Persist to the existing SQLite db (new clipboard_items table: id, kind, text, image BLOB, app_bundle_id, copied_at, pinned), FIFO-cap at ~500 items, dedupe consecutive identicals. Paste-back: writeObjects: to the general pasteboard, then either let the user Cmd-V themselves (zero-permission) or offer optional auto-paste via a synthesized Cmd-V CGEvent — that one path requires Accessibility TCC, so gate it behind a setting exactly like Maccy's "paste automatically" toggle. (4) ISLAND SEARCH UI (yes, effort M–L): one unified query field; results list merges Spotlight hits and clipboard matches (prefix `;` or a tab toggle to filter clipboard-only, Raycast-style). Rendering is a new expanded-mode card; arrow keys/Enter handled through the same on_key_down listener pattern as notes_editor.rs. MVP sequencing: hotkey (S) → clipboard capture+list (M) → Spotlight (M) → ranking/polish/settings (M). Full group is a week-plus of work; a hotkey+clipboard-only MVP is ~2 days.

### APIs

- Carbon RegisterEventHotKey / UnregisterEventHotKey / InstallEventHandler (via `global-hotkey` crate ~0.7, MIT/Apache-2.0) — global summon, no TCC
- CoreServices MDQueryCreate / MDQuerySetMaxCount / MDQueryExecute / MDQueryGetResultAtIndex / MDItemCopyAttribute (kMDItemPath, kMDItemDisplayName, kMDItemContentTypeTree, kMDItemLastUsedDate) — Spotlight index queries
- NSMetadataQuery (alternative to MDQuery; rejected — delegate/run-loop plumbing is heavier from objc2)
- NSPasteboard: pasteboardWithName:NSPasteboardNameGeneral, changeCount, pasteboardItems, types, stringForType:, dataForType:, clearContents/writeObjects: — clipboard capture and paste-back
- nspasteboard.org marker types: org.nspasteboard.TransientType / ConcealedType / AutoGeneratedType — privacy filtering
- NSWorkspace: frontmostApplication (source-app heuristic + focus restore), openURL:/openApplicationAtURL:configuration: (launch results)
- NSRunningApplication activateWithOptions: — restore focus to the previously frontmost app on dismiss
- CGEventCreateKeyboardEvent + CGEventPost (Cmd-V auto-paste ONLY, optional, needs Accessibility TCC)
- rusqlite (already a dependency) — clipboard_items persistence
- NSDistributedNotificationCenter com.apple.screenIsLocked/Unlocked or NSWorkspace screensDidSleep — pause clipboard polling

### Permissions / TCC

Core feature set: NO TCC permissions at all. Carbon hotkeys, MDQuery/Spotlight, and NSPasteboard reads are unrestricted for an unsandboxed app. Two opt-in edges: (a) auto-paste (synthesized Cmd-V) requires Accessibility (kTCCServiceAccessibility) — ship it off by default with the same "grant in System Settings" flow other TCC features presumably use; (b) reading FILE CONTENTS of Spotlight results under ~/Desktop, ~/Documents, ~/Downloads would trigger per-folder TCC prompts — avoid by never opening result files for preview; metadata from the index and NSWorkspace launching prompt nothing. Note MDQuery returns metadata for TCC-protected folders regardless (index access isn't TCC-gated for unsandboxed apps).

### Integration map (files to touch)

- crates/nook-core/src/clipboard.rs (NEW): general-pasteboard watcher (changeCount snapshot fn called from the UI poll loop, mirroring the drag-pasteboard pattern in nook-core/src/files.rs:152-200), privacy-type filtering, SQLite read/write, search over history
- crates/nook-core/src/spotlight.rs (NEW): MDQuery FFI externs + safe query fn (query string in, Vec<SearchHit> out), app-vs-file ranking
- crates/nook-core/src/database.rs: add clipboard_items table in migrate()
- crates/nook/src/platform.rs: register/unregister global hotkey (or init `global-hotkey` crate on the main run loop in main.rs), remember/restore frontmost NSRunningApplication; `activate_app()` at platform.rs:423 already exists for taking key focus
- crates/nook/src/island/mod.rs: new IslandState fields (search_open, query, results, clipboard page), hotkey → expand+focus wiring in spawn_loops/event path; hook clipboard changeCount check onto the existing 400ms/2s background cadence (mod.rs:466-516) — do not add a new wake source
- crates/nook/src/island/search.rs (NEW) or widgets/search.rs: expanded-card UI — single-line query editor cloned from widgets/notes_editor.rs (FocusHandle + ElementInputHandler + on_key_down, lines 199-798), result rows with icon (NSWorkspace iconForFile → CGImage), keyboard navigation
- crates/nook/src/island/expanded.rs + render.rs: mount the search card as an expanded mode; compact face unchanged (search is summon-only via hotkey, optional magnifier affordance)
- crates/nook/src/island/settings.rs + nook-core/src/settings.rs: hotkey recorder, clipboard history on/off (default OFF for privacy), history size, per-app exclusions, auto-paste toggle (Accessibility-gated)
- crates/nook/Cargo.toml / nook-core/Cargo.toml: add `global-hotkey`; MDQuery externs need only existing objc2/core-foundation linkage plus `#[link(name = "CoreServices", kind = "framework")]`

### Battery requirements

Hotkey: literally zero idle cost — Carbon hot keys are event-driven kernel/WindowServer dispatch, no thread, no poll. Spotlight: strictly on-demand — MDQuery runs only per debounced keystroke while the search card is open; cancel+release the prior MDQueryRef; use synchronous one-shot queries (no live-update MDQueryEnableUpdates, which keeps the query resident). Clipboard: the one unavoidable poll — changeCount is a single integer over XPC, ~microseconds; at 1 Hz the cost is negligible but nonzero, so (a) make history opt-in (feature off = no poll at all), (b) piggyback the check on the existing 400 ms/2 s loop in island/mod.rs rather than a new timer/wake source, reading changeCount at most once per second, (c) suspend on screen lock/display sleep via com.apple.screenIsLocked distributed notifications, (d) only touch pasteboardItems (the expensive read) when changeCount actually moved. Image capture: downscale/thumbnail on the background executor, cap stored blob size. SQLite writes only on actual clipboard changes. Focus restore and UI work happen only during an active summon.

### Risks & honest blockers

Honest blockers and sharp edges: (1) Key-window focus is the real integration risk, not any API: the island is an accessory-app overlay; summoning must make it key without visually disrupting the user's space. `activateIgnoringOtherApps:` (already used) steals app focus, so restoring the previous frontmost app on dismiss is mandatory or the feature feels broken; a nonactivating-panel approach (GPUI WindowKind::PopUp / NSNonactivatingPanelMask) types without deactivating the front app but has Cmd-shortcut routing quirks — prototype this first. (2) Pasteboard polling is genuinely the only mechanism (no public notification exists; Maccy, Paste, Raycast all poll) — accept it, but 1 Hz means a copy can appear up to 1 s late in history; that's fine, but auto-capture racing a quick copy-copy sequence loses the first item unless you read on every count delta. (3) Privacy is reputational, not technical: apps that don't mark ConcealedType (older/broken apps, some terminals with passwords) WILL leak secrets into history — ship default-off, plain-text SQLite should at least be 0600 and ideally offer "exclude marked-secure + exclusion list" prominently; consider not persisting across reboots by default. (4) SecureEventInput sessions (password fields) can swallow some hotkey paths; Carbon hotkeys mostly survive, event-tap approaches don't — another reason to stay on RegisterEventHotKey, which is deprecated-but-stable (Apple has kept it working through macOS 26; every launcher depends on it). (5) Spotlight results are only as good as the index: mdutil-disabled volumes, fresh reindexing, and iCloud-dataless files return nothing or non-local paths — show an honest empty state, don't fall back to a custom crawler (battery). (6) MDQuery C externs are hand-written FFI; wrong CFRelease discipline = leaks/crashes — keep the wrapper tiny and leak-test. (7) Auto-paste needs Accessibility TCC and breaks in secure-input fields — keep default 'copy to pasteboard, user pastes'. (8) GPUI single-line text input doesn't exist as a stock control — budget real time to extract a single-line editor from notes_editor.rs (IME/marked-text handling included).

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP22 — window-management

**Window Snap + menu-bar hiding (Thaw)**

- **Wave:** W3 · **Feasibility:** partial · **Effort:** XL (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Window Snap (Rectangle-style halves/quarters via hotkeys + drag-to-edge) and Thaw (Ice/Bartender-style menu bar item hiding)

### Approach

== (a) WINDOW SNAP — feasibility: YES ==
Mechanism (exactly what Rectangle, MIT-licensed, does):
1. Window manipulation: the public Accessibility API in ApplicationServices/HIServices. AXUIElementCreateApplication(pid) for the frontmost app (from NSWorkspace.shared.frontmostApplication), read kAXFocusedWindowAttribute / kAXWindowsAttribute, then AXUIElementSetAttributeValue with kAXPositionAttribute / kAXSizeAttribute using AXValueCreate(kAXValueCGPointType / kAXValueCGSizeType). Two known quirks Rectangle handles: (i) set size, then position, then size again (some apps clamp); (ii) temporarily set kAXEnhancedUserInterface=false on the app element before moving (Electron/VoiceOver apps animate or mis-place otherwise). AX coordinates are top-left-origin global; NSScreen.visibleFrame is bottom-left — flip via primary screen height. No Rust crate needed: declare AXUIElement* fns via extern "C" against ApplicationServices, consistent with the codebase's raw-FFI style (or use the `accessibility-sys` crate).
2. Global hotkeys: Carbon RegisterEventHotKey + InstallEventHandler (still fully supported on macOS 26). This is what Rectangle uses. It is purely event-driven, consumes the keystroke, and requires NO TCC permission. The `global-hotkey` crate (tauri-apps, MIT/Apache) wraps exactly this on macOS. Do NOT use CGEventTap or NSEvent.addGlobalMonitorForEvents for hotkeys — both need Accessibility/Input Monitoring and the NSEvent monitor can't consume events.
3. Drag-to-edge: needs global drag tracking. Cleanest: a listen-only CGEventTap (CGEventTapCreate, kCGEventLeftMouseDragged|kCGEventLeftMouseUp, kCGEventTapOptionListenOnly) — callback-driven, zero idle cost, enabled only when the user turns the feature on; requires the same Accessibility grant the AX calls already need. Cheaper alternative for this codebase: nook-core/src/mouse.rs already polls NSEvent.mouseLocation and exposes drag_active(); when a drag is active, do one AX hit-test (is the frontmost window being moved, i.e. its kAXPosition changes with the cursor — Rectangle's exact heuristic) and only then track edges. Snap preview: a second borderless transparent NSWindow tinted at the target half/quarter (reuse style_island_window plumbing in platform.rs). On mouse-up inside an edge zone, apply the AX set.
4. Geometry: halves/quarters from NSScreen.visibleFrame per-screen; respect multiple displays by picking the screen under the cursor.
Blockers (honest): TCC Accessibility is mandatory and, for an unsigned/ad-hoc-signed app, the grant is keyed to the code signature — every rebuild with a new ad-hoc signature silently invalidates trust (users re-toggle in System Settings). Sign with a stable Developer ID or stable self-signed cert. Windows in native full-screen/tiled Spaces can't be snapped; some apps mark size non-settable; macOS 15+ has built-in window tiling whose drag-to-edge zones will fight yours (offer a note in settings to disable system tiling or use modifier-key-only drag snapping).

== (b) THAW — feasibility: PARTIAL (hiding: yes, cheap; rendering hidden items in the notch: possible but permission-heavy and fragile) ==
The real mechanism, stated plainly: you CANNOT touch another app's NSStatusItem — no API, public or private, lets you set another process's status item length or visibility. Every menu-bar manager uses the same layout trick:
1. The separator trick (Dozer, Hidden Bar, Ice, Bartender all build on this): create your OWN NSStatusItem as a separator. macOS lays status items out right-to-left in user-arranged order (users cmd-drag items relative to your separator). To hide, set your separator item's length to a huge value (~10000pt): everything to its LEFT is pushed past the screen's left edge and macOS simply doesn't draw it. To show, restore length to a few points. 100% public API (NSStatusItem.length), zero permissions, zero idle cost. Limitations: hidden items are genuinely off-screen and unclickable while hidden; user must arrange items via cmd-drag; on notched MacBooks the usable menu bar is short, which is exactly why this pairs well with a notch app.
2. Rendering hidden items elsewhere (Ice's "Ice Bar", Bartender 5's bar — this is how you'd show them inside the expanded island): enumerate menu-bar item windows via CGWindowListCopyWindowInfo — each status item is a window owned by its app at the status window level (layer 25); filter by layer and exclude own pid, giving frame + owner + CGWindowID. Capture each item's image on-demand with ScreenCaptureKit (SCScreenshotManager.captureImage with an SCContentFilter for that window; Ice does this) — requires the Screen Recording TCC permission, and on Sonoma+ shows the purple "screen is being observed" indicator while capturing, so capture ONLY on island expansion, never continuously. Forwarding a click: temporarily shrink the separator (un-hide), wait one runloop for relayout, re-read the item's frame, then synthesize the click with CGEventPost (CGEventCreateMouseEvent pair) at that point — needs Accessibility (already granted for window snap) — then re-hide. This is genuinely how Ice does it (open source, MIT: jordanbaird/Ice — read MenuBarItemManager + ScreenCapture code for the exact dance).
Blockers (honest): Screen Recording prompt is scary and, like Accessibility, signature-keyed for unsigned builds; the CGWindowList layer-25 heuristic and item-window ownership details have shifted across macOS releases (Ice carries per-OS workarounds); click-forwarding has a visible ~100ms un-hide flicker; Bartender-style "always show on trigger" is not achievable without the capture permission. Recommendation: ship tier 1 (separator hide/show toggle — solid, cheap, permissionless) first; treat the in-island hidden-item bar as a follow-up flag.
No external binaries needed for either feature. Comparable-app licenses: Rectangle (MIT) and Ice (MIT) are both readable references; Bartender is closed-source.

### APIs

- AXUIElementCreateApplication / AXUIElementCopyAttributeValue / AXUIElementSetAttributeValue / AXValueCreate (ApplicationServices, public)
- kAXFocusedWindowAttribute, kAXPositionAttribute, kAXSizeAttribute, kAXEnhancedUserInterface
- AXIsProcessTrustedWithOptions + kAXTrustedCheckOptionPrompt (permission prompt)
- Carbon RegisterEventHotKey / InstallEventHandler (public, no TCC) — or the global-hotkey crate wrapping it
- CGEventTapCreate listen-only for LeftMouseDragged/LeftMouseUp (drag-to-edge; optional — can reuse existing mouse.rs poll + drag_active instead)
- NSScreen.visibleFrame / NSWorkspace.frontmostApplication (objc2 msg_send, matching existing platform.rs style)
- NSStatusItem.length manipulation on OWN separator item (the only real hiding mechanism; public)
- CGWindowListCopyWindowInfo filtered to status-window layer 25 (enumerate other apps' menu bar item windows)
- ScreenCaptureKit SCScreenshotManager.captureImage + SCContentFilter(desktopIndependentWindow:) (render hidden items in island, on-demand only)
- CGEventCreateMouseEvent + CGEventPost (forward clicks to temporarily un-hidden items)
- _AXUIElementGetWindow (private, optional: map AXUIElement to CGWindowID; not required for MVP)

### Permissions / TCC

Window Snap: Accessibility (TCC kTCCServiceAccessibility) — mandatory for AX window moves and for CGEventPost click synthesis; Carbon hotkeys need NO permission. Thaw tier 1 (separator hide/show): NO permissions at all. Thaw tier 2 (show hidden items inside the island): Screen Recording (kTCCServiceScreenCapture) for ScreenCaptureKit item snapshots, plus the already-granted Accessibility for click forwarding. Critical caveat: TCC grants are keyed to the code signature — the current unsigned/dev-signed distribution means ad-hoc re-signs invalidate grants on every update; adopt a stable signing identity before shipping either feature.

### Integration map (files to touch)

- NEW crates/nook-core/src/window_snap.rs — AX FFI wrappers, snap geometry (halves/quarters from NSScreen.visibleFrame, top-left coord flip), kAXEnhancedUserInterface workaround, drag-edge state machine
- NEW crates/nook-core/src/hotkeys.rs — Carbon RegisterEventHotKey registration + dispatch (or global-hotkey crate); shortcuts stored in nook-core/src/settings.rs AppSettings
- NEW crates/nook-core/src/menubar.rs — own separator NSStatusItem create/expand/collapse (extend the STATUS_ITEM AtomicPtr pattern already in nook/src/platform.rs), CGWindowList layer-25 enumeration, optional SCK capture + CGEventPost click forwarding
- EXTEND crates/nook/src/platform.rs — AXIsProcessTrustedWithOptions prompt helper; second borderless NSWindow for the snap-preview overlay (reuse style_island_window plumbing)
- EXTEND crates/nook-core/src/mouse.rs — surface drag_active() transitions as events so window_snap can do AX drag-detection only during live drags (avoids a CGEventTap entirely)
- UI compact face: brief snap-confirmation flash; menu-bar hide/show toggle button
- UI expanded: NEW crates/nook/src/widgets/snap_grid.rs — 2x2/halves click-to-snap card for the frontmost window; later a menubar.rs widget card rendering captured hidden items (tier 2)
- UI settings: crates/nook/src/island/settings.rs — shortcut recorder rows, drag-to-edge toggle, Thaw enable + separator arrangement hint; persisted via nook-core/src/settings.rs
- Wire-up: crates/nook/src/island/mod.rs — hotkey/drag event ingestion into island state; no new poll loops

### Battery requirements

Both features can be built with strictly zero idle cost. Hotkeys: Carbon RegisterEventHotKey is callback-only — no polling, no wakeups. Snap execution: AX calls run only on hotkey/drag-release. Drag-to-edge: prefer gating on the EXISTING mouse.rs poll (drag_active transitions) so no new tap or loop is added; if a CGEventTap is used, make it listen-only, register only mouse-dragged/up, and create it only when the toggle is on — taps are kernel-callback-driven, zero cost between events. Thaw tier 1: NSStatusItem.length changes are one-shot AppKit calls, nothing runs while hidden. Thaw tier 2: CGWindowList enumeration + ScreenCaptureKit snapshots ONLY at the moment the island expands (single captureImage per item, no SCStream), never a persistent capture session — this also keeps the Sonoma purple capture indicator from being permanently lit. No timers, no new threads that spin.

### Risks & honest blockers

1) TCC-vs-unsigned-distribution is the biggest real blocker: Accessibility and Screen Recording grants break on every ad-hoc re-sign; needs a stable signing identity or users re-grant per update. 2) macOS 15+ native window tiling conflicts with drag-to-edge zones (mitigate: modifier-key drag snapping or in-settings guidance). 3) AX can't move windows in full-screen Spaces and some apps reject size writes; the kAXEnhancedUserInterface Electron quirk must be handled or snaps land wrong. 4) Menu-bar hiding fundamentals: other apps' NSStatusItems are untouchable — the separator trick is the only mechanism, hidden items are unclickable while hidden, and users must cmd-drag to arrange items around the separator (UX education needed). 5) Ice-style in-island rendering rests on undocumented behavior (layer-25 window enumeration, item window ownership) that has changed across macOS releases and carries a scary Screen Recording prompt plus capture-indicator optics; click forwarding has a visible un-hide flicker. 6) Effort split: snap hotkeys M, +drag-to-edge and preview HUD = L total; Thaw tier 1 M, tier 2 XL — full group XL. Recommend shipping snap-via-hotkeys + Thaw tier 1 first (both together ~L) and gating drag-to-edge and the in-island menu-bar viewer behind follow-ups.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP23 — messaging-replies

**iMessage / WhatsApp replies**

- **Wave:** W3 · **Feasibility:** partial · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Instant reply to iMessage and WhatsApp from the island: surface incoming messages and send replies inline

### Approach

iMessage is fully doable and is the BlueBubbles-proven recipe: READ by opening ~/Library/Messages/chat.db read-only with rusqlite (already a workspace dep) and querying `message` JOIN `handle` JOIN `chat_message_join` for ROWID > last_seen; on modern macOS the body often lives in the `attributedBody` typedstream blob with `text` NULL, so pull in the `imessage-database` crate (ReagentX, powers imessage-exporter, maintained through current macOS) to decode bodies, tapbacks, and group-chat names instead of hand-rolling it. Detect new messages event-driven by watching `chat.db-wal` with FSEvents/kqueue (the `notify` crate, 300ms debounce) — never poll. SEND via osascript exactly like audio.rs already does for media: `tell application "Messages" to send theText to chat id "..."` using the chat GUID read from chat.db (`iMessage;-;+49...` form); sending to existing 1:1s and existing groups works, creating brand-new group chats does not, and there is no AppleScript path for tapbacks/typing indicators/read receipts (BlueBubbles needs a SIP-disabled dylib injected into Messages for those — declare out of scope). Permissions: Full Disk Access for chat.db (no prompt API — detect the open() failure and deep-link the user to x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles) and an Automation/AppleEvents prompt on first send.

WhatsApp is the honest 'partial': the Mac app (now Catalyst) has no AppleScript dictionary and its local store is encrypted, and personal accounts have no public API. Realistic tier-1: READ incoming via the macOS notification store `~/Library/Group Containers/group.com.apple.usernoted/db2/db` (same FDA grant, also kqueue-watchable) to show sender+snippet in the island; 'REPLY' opens `whatsapp://send?phone=<E164>&text=<prefill>` which prefills but cannot auto-send (automating the Enter keypress via CGEvent/AX is possible but fragile — offer it behind an 'experimental' toggle needing Accessibility TCC). True in-island WhatsApp send requires linking a companion device via the reverse-engineered multi-device protocol (whatsmeow/Go or Baileys/Node as a sidecar process, as Texts.com/Beeper do) — it works but carries real account-ban ToS risk and a week+ of sidecar plumbing; recommend shipping iMessage-complete + WhatsApp-notify/prefill first. Note the UNUserNotificationCenter API cannot read other apps' notifications at all, and AppleScript lost its incoming-message handlers years ago — chat.db and the usernoted db are the only supported-ish read paths. NotchNook/boring.notch/Droppy do not ship messaging reply, so this is a differentiator.

### APIs

- rusqlite read-only open of ~/Library/Messages/chat.db (tables: message, handle, chat, chat_message_join; attributedBody typedstream blob)
- imessage-database crate (typedstream body decoding, chat GUIDs) — third-party, public
- notify crate / FSEvents + kqueue file watch on chat.db-wal and usernoted db2 (event-driven wake)
- osascript via /usr/bin/osascript: tell application "Messages" to send ... to chat id "iMessage;-;<handle>" (public scripting; Automation TCC kTCCServiceAppleEvents, needs NSAppleEventsUsageDescription in Info.plist)
- ~/Library/Group Containers/group.com.apple.usernoted/db2/db notification store (private/undocumented schema, FDA required) for WhatsApp incoming
- whatsapp:// URL scheme via NSWorkspace openURL (public, prefill-only, no auto-send)
- Optional experimental: CGEventPost / AXUIElement to press Return in WhatsApp (Accessibility TCC kTCCServiceAccessibility) — fragile (private-ish behavior)
- Out of scope: Messages private frameworks (IMCore) for tapbacks/typing — require SIP disabled + injection (BlueBubbles approach)
- whatsmeow (Go) or Baileys sidecar for real WhatsApp send — reverse-engineered protocol, ToS/ban risk (private)

### Permissions / TCC

Full Disk Access (kTCCServiceSystemPolicyAllFiles) for chat.db and the usernoted db — no programmatic prompt exists, the app must detect failure and walk the user to System Settings; grant is keyed to the code-signing identity, so keep the current stable dev-signing cert (an ad-hoc re-signed rebuild silently loses the grant — worst UX pitfall here). Automation (kTCCServiceAppleEvents → Messages) prompts on first send; add NSAppleEventsUsageDescription to Info.plist. Accessibility (kTCCServiceAccessibility) only for the optional WhatsApp Enter-keypress hack. No entitlements are needed that an unsigned/dev-signed app cannot hold; only tapbacks/typing indicators would need SIP-off injection, which should stay out of scope.

### Integration map (files to touch)

- /Users/jonasvogel/openNook/crates/nook-core/src/messages.rs (NEW): chat.db reader (snapshot() returning recent conversations + unread deltas keyed by ROWID watermark), FDA probe, WAL file-watcher task, send_imessage(chat_guid, text) via osascript, WhatsApp notification-db reader + whatsapp:// prefill helper
- /Users/jonasvogel/openNook/crates/nook-core/src/audio.rs: extract run_osascript() (line 666) into utils.rs so messages.rs reuses it instead of duplicating
- /Users/jonasvogel/openNook/crates/nook-core/Cargo.toml: add imessage-database, notify
- /Users/jonasvogel/openNook/crates/nook/src/island/mod.rs: new spawn in spawn_loops() mirroring the agents snapshot loop (~line 561), but woken by a channel fed from the notify watcher rather than a timer; island state gains messages: Vec<Conversation> + incoming peek state
- /Users/jonasvogel/openNook/crates/nook/src/widgets/messages.rs (NEW): expanded card — recent conversation list, inline reply text field, send button; WhatsApp rows get 'open with prefilled reply' action
- /Users/jonasvogel/openNook/crates/nook/src/island/compact.rs: transient incoming-message peek on the compact face (sender + snippet, reuse marquee.rs), tap to expand into the messages card
- /Users/jonasvogel/openNook/crates/nook/src/island/settings.rs + nook-core/src/settings.rs: enable toggle, FDA status row with deep link to Privacy_AllFiles pane, experimental WhatsApp auto-send toggle
- /Users/jonasvogel/openNook/crates/nook-core/src/database.rs: small table for per-conversation last-seen ROWID watermarks

### Battery requirements

Near-zero idle is achievable: no polling anywhere. FSEvents/kqueue watch on ~/Library/Messages/chat.db-wal wakes the reader only when Messages actually writes; debounce 300ms and run one indexed query (WHERE ROWID > watermark) per burst. Same pattern on the usernoted db2 file for WhatsApp. osascript spawns only on user-initiated send. Keep the sqlite connection closed between wakes (open is cheap, avoids holding a file handle that blocks Messages' WAL checkpointing). The only steady cost is the two dormant file watchers, which is effectively free.

### Risks & honest blockers

1) FDA is the whole feature: users who refuse it get nothing for reading, and a re-signed build drops the grant. 2) chat.db schema and attributedBody encoding shift across macOS releases — mitigated by riding imessage-database, but a macOS point release can break reads until that crate updates. 3) AppleScript send to Messages has been slowly degraded by Apple (buddy-based send broke around Sonoma; chat-id send still works on current macOS but is unsupported and could vanish in any release); no new-group creation, no tapbacks. 4) WhatsApp cannot truly send without a reverse-engineered sidecar that risks account bans — the shipped tier is notify + prefill, which users may perceive as half a feature; set expectations in the UI. 5) usernoted db schema is undocumented and only contains notifications that were actually delivered (Focus/DND suppresses them). 6) Privacy optics: the app reads the user's entire message history — read only recent ROWIDs, never persist bodies to opennook.db.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP24 — notification-shelf

**Notification shelf (other apps’ notifications)**

- **Wave:** W3 · **Feasibility:** partial · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Recent notifications shelf — capture other apps' macOS notifications (Discord, etc.) and keep them in the island shelf

### Approach

Two viable capture paths, neither clean. (A) Read the system notification store. On current macOS (confirmed Tahoe 26.6.1 on this machine) it is a SQLite DB at ~/Library/Group Containers/group.com.apple.usernoted/db2/db. The relevant tables: `record` (columns include `rec_id`/rowid, `app_id`, `uuid`, `delivered_date`, and a `data` BLOB) and `app` (`app_id` -> bundle identifier). The `data` blob is a binary plist (bplist00 / NSKeyedArchiver) whose `req` dict holds `titl` (title), `subt` (subtitle), `body`, `app` (bundle id) and `date`. Records are pruned when the user clears them, so it is a rolling window, not a history. I verified this DB is TCC-protected: without Full Disk Access, both `ls` on the group container and a read-only sqlite open fail ("Operation not permitted" / "authorization denied"). So path A requires Full Disk Access (kTCCServiceSystemPolicyAllFiles) — a manual toggle in System Settings the user must add by hand (there is NO programmatic prompt/API to request it). It also has no real-time signal; you detect new rows by watching the db-wal file and reading rows past your last-seen rowid. (B) Scrape banners live via the Accessibility API. Register an AXObserver on the com.apple.notificationcenterui process pid for kAXWindowCreatedNotification / kAXCreatedNotification; when a banner window appears, walk its AXUIElement tree (AXStaticText children) to pull title/body, and map the owning app. This needs Accessibility permission (kTCCServiceAccessibility) via AXIsProcessTrustedWithOptions — which DOES show a system prompt and deep-links to Settings. Path B is fully event-driven and catches notifications in real time, but only while a banner is actually drawn (misses items delivered under Do Not Disturb/Focus, or if the user set the app's alert style to 'None'), and text scraping is locale/OS-version fragile. The dead end: private UNUserNotificationCenter does NOT help — getDeliveredNotifications only ever returns the calling app's OWN notifications; there is no public or private UN API to enumerate other apps'. Recommended senior approach: primary = AXObserver (B) for real-time capture with an in-UI Accessibility prompt; optional = DB read (A) as backfill/enrichment gated behind an explicit 'grant Full Disk Access' opt-in. Important for this app: unsigned/dev-signed is fine — Accessibility and Full Disk Access are user-granted TCC permissions, not code-signed entitlements, so no Apple-private com.apple.* entitlement is required (those would be the real blocker). Comparable apps: I could not verify any notch app (NotchNook, boring.notch, MediaMate) ships cross-app notification mirroring; Ice is a menu-bar manager and does not do this. The notification-DB-read technique is well documented historically from CLI tools that dumped the usernoted DB, but those all now need Full Disk Access on Monterey+. Be honest with the user that this is the least 'clean' feature of the set: it works, but only behind a scary manual permission and with inherent gaps.

### APIs

- AXObserverCreate / AXObserverAddNotification / AXObserverGetRunLoopSource (ApplicationServices/HIServices) — event-driven banner capture
- kAXWindowCreatedNotification, kAXCreatedNotification, kAXStaticTextRole, kAXValueAttribute, kAXChildrenAttribute (AX constants)
- AXUIElementCreateApplication(pid) targeting the com.apple.notificationcenterui process
- AXIsProcessTrustedWithOptions with kAXTrustedCheckOptionPrompt — Accessibility permission prompt (kTCCServiceAccessibility)
- SQLite read of ~/Library/Group Containers/group.com.apple.usernoted/db2/db (record + app tables) via rusqlite readonly — gated by Full Disk Access (kTCCServiceSystemPolicyAllFiles)
- Binary-plist decode of the record.data blob (bplist00 / NSKeyedArchiver) — `plist` crate; fields titl/subt/body/app/date
- FSEvents or kqueue watch on db + db-wal for change-driven reads (avoid timed polling)
- NSRunningApplication / LSCopyApplicationURLsForBundleIdentifier to resolve app name + icon from bundle id
- x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility and ?Privacy_AllFiles — deep links to grant the two permissions
- UNUserNotificationCenter.getDeliveredNotifications — NOTE: own-app only, cannot enumerate other apps (documented dead end)

### Permissions / TCC

Accessibility (kTCCServiceAccessibility) — required for the real-time AXObserver banner-scrape path; prompts via AXIsProcessTrustedWithOptions and deep-links to Settings. Full Disk Access (kTCCServiceSystemPolicyAllFiles) — required for the DB-read backfill path; confirmed necessary on 26.6.1 (readonly open of the usernoted db fails without it); CANNOT be requested programmatically — user must add the app manually in System Settings > Privacy & Security > Full Disk Access. No code-signed entitlement and no Apple-private com.apple.* entitlement are needed — both are user-granted TCC permissions an unsigned/dev-signed accessory app can legitimately hold. App is non-sandboxed already (uses NSAppleEventsUsageDescription), so no App Sandbox conflict. Add a usage-string / onboarding explainer for both grants.

### Integration map (files to touch)

- NEW crates/nook-core/src/notifications.rs — define NotificationEvent { bundle_id, app_name, title, subtitle, body, delivered_at }; two backends behind a trait: ax_observer (real-time, primary) and db_reader (Full Disk Access backfill); a bounded ring-buffer/shelf store
- crates/nook-core/src/database.rs — add a `notification_shelf` table (migrate()) to persist the recent N events, mirroring the existing file_tray/observe_samples pattern
- crates/nook/src/platform.rs — add AX permission helpers (AXIsProcessTrustedWithOptions wrapper) and a Full-Disk-Access probe (attempt readonly-open the usernoted db; classify Operation-not-permitted); reuse existing objc2 interop style
- crates/nook/src/island/mod.rs — wire an event channel from the AXObserver run-loop source into the poll/state loop (no timed poll for the AX path; slow/kqueue-driven for the DB path)
- NEW crates/nook/src/widgets/notifications.rs (or extend widgets/) — expanded-card shelf: scrollable list of app icon + title + body + relative time, swipe/click to dismiss
- crates/nook/src/island/expanded.rs — mount the notification shelf card
- crates/nook/src/island/compact.rs + render.rs — compact-face badge: latest app icon or unread count
- crates/nook/src/island/settings.rs — add enable toggle, live permission status (Accessibility / Full Disk Access), and the two x-apple.systempreferences deep-link buttons; per-app allow/block filter list
- Cargo.toml (nook-core) — add `plist` crate for bplist decode; add an AX binding path (accessibility-sys / raw extern "C" to ApplicationServices, since objc2 doesn't cover the AX C API)

### Battery requirements

Near-zero idle is achievable and should be the design constraint. Primary AXObserver path is fully event-driven: the observer installs a CFRunLoopSource on the main run loop and the callback fires ONLY when a banner window is created — no timer, zero CPU when nothing is happening. For the optional DB backfill, do NOT timed-poll; put a kqueue/FSEvents watch on db-wal (and db) and only run a query when the file changes, then read rows past the last-seen rowid and decode just those blobs. Keep the readonly sqlite connection open (or open-on-change) and cap decode work. Resolving app icons should be cached by bundle id. Net idle cost after setup is effectively nil; the only cost is a brief decode burst per delivered notification, which is inherently rate-limited by how often notifications actually arrive.

### Risks & honest blockers

See battery_notes and above; primary blockers are manual Full Disk Access grant, event-only visibility of the AX path (misses DND/Focus/'None'-style notifications), undocumented DB schema that Apple changes across OS versions, and privacy sensitivity of mirroring other apps' notification text. None require Apple-private entitlements.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

## WP25 — clock-timers

**Apple Clock timer sync**

- **Wave:** W3 · **Feasibility:** partial · **Effort:** L (S <½ day · M 1–2 d · L 3–5 d · XL week+)
- **Researched scope:** Apple Clock (macOS) timer sync: read/control system timers from the notch island, and mirror island timers into user notifications

### Approach

READ = fully possible, verified on this machine. Clock timers on macOS are owned by the user agent `mobiletimerd` (/System/Library/PrivateFrameworks/MobileTimer.framework/Executables/mobiletimerd, launchd label com.apple.mobiletimerd). Its complete timer state is readable by any user process through two stores: (1) the CFPreferences domain `com.apple.mobiletimerd` (~/Library/Preferences/com.apple.mobiletimerd.plist) whose MTTimers array carries MTTimerID (UUID), MTTimerDuration, MTTimerState, MTTimerTitle, MTTimerFiredDate/MTTimerDismissedDate, MTTimerFireTime (NSKeyedArchiver: MTTimerTimeInterval or an absolute fire date when running), and sound — no TCC involved; and (2) the authoritative CoreData store ~/Library/Group Containers/group.com.apple.mobiletimerd/local.sqlite, table ZMTCDTIMER (ZSTATE, ZDURATION, ZFIREDDATE, ZDISMISSEDDATE, ZTITLE, ZFIRETIME blob, ZTIMERURL = "x-apple-clock:timer?id=UUID") — opened read-only with sqlite `?mode=ro&immutable` semantics; both were read successfully here with zero prompts. One calibration pass is needed on day 1: start/pause a Clock timer and diff both stores to pin the MTTimerState enum values and the running-timer fire-date encoding, and to confirm the plist mirror updates live post-'MigratedToCoreData' (alarm-side modified dates show the plist is still written). Change detection is a kqueue/dispatch-source vnode watch on the plist (re-armed on atomic rename by cfprefsd) with the sqlite -wal file as fallback — pure event-driven. A running timer needs no further events: compute remaining from the absolute fire date and tick the UI locally.

CONTROL = no direct API for a non-App-Store app. The write path is the `com.apple.MobileTimer.timerserver` XPC service (MTTimerManager in private MobileTimer.framework), gated by the private entitlement `com.apple.private.mobiletimerd` (verified in Clock.app's entitlements) — AMFI rejects private entitlements on dev/ad-hoc-signed binaries, so this is dead without SIP-off. AlarmKit (the new public timer/alarm API whose ZAKCD* tables live in the same sqlite) exists on macOS 26 only under /System/iOSSupport — Catalyst-only, not linkable from a native GPUI app. The workable control path is Shortcuts: Clock ships App Intents (verified in MobileTimerSupport.framework's Metadata.appintents): INCreateTimerIntent (Start Timer), PauseTimerIntent, ResumeTimerIntent, CancelTimerIntent, GetCurrentTimerDetailsIntent, plus stopwatch/alarm intents. Ship pre-built shortcuts signed with `shortcuts sign` (e.g. "Nook Start Timer" taking duration as input), have the user import them once during onboarding, then invoke `/usr/bin/shortcuts run "Nook Pause Timer"` from the island buttons (~1–2 s latency, acceptable for taps). Detect availability with `shortcuts list`. Deep-link `open x-apple-clock:timer?id=UUID` (scheme verified in Clock's Info.plist PrivateURLSchemes) opens the timer in Clock as a fallback affordance.

NOTIFICATION MIRROR = straightforward. openNook already ships as a dev-signed .app with Info.plist (required — UNUserNotificationCenter needs a bundle identity), so use UserNotifications via the objc2-user-notifications crate: request authorization once (standard Notifications prompt, not a TCC pane), and when an island-local timer starts, schedule a UNTimeIntervalNotificationTrigger for the remaining seconds (cancel/reschedule on pause/edit). The notification is delivered by usernoted even if the overlay is occluded or the app crashed mid-countdown, and it appears in Notification Center history — this is the "at least" deliverable and is unconditionally feasible.

### APIs

- CFPreferences / NSUserDefaults domain `com.apple.mobiletimerd` — MTTimers/MTStopwatches state mirror (private data format, public read API, no TCC)
- ~/Library/Group Containers/group.com.apple.mobiletimerd/local.sqlite, table ZMTCDTIMER via rusqlite read-only URI (private schema; group-container path may hit the macOS 15+ 'access data from other apps' TCC on some setups)
- kqueue EVFILT_VNODE / DISPATCH_SOURCE_TYPE_VNODE watch on the plist and sqlite-wal for change events
- NSKeyedArchiver bplist decoding of MTTimerFireTime (classes MTTimerTimeInterval / MTTimerFireDate) via the `plist` crate
- Shortcuts CLI `/usr/bin/shortcuts run|list|sign` driving Clock App Intents: INCreateTimerIntent, PauseTimerIntent, ResumeTimerIntent, CancelTimerIntent, GetCurrentTimerDetailsIntent (public user-facing actions, verified in MobileTimerSupport.framework Metadata.appintents)
- URL scheme x-apple-clock:timer?id=UUID / clock-timer:// via NSWorkspace openURL to deep-link into Clock
- UNUserNotificationCenter + UNTimeIntervalNotificationTrigger via objc2-user-notifications for mirroring island timers (public; requires bundled app)
- BLOCKED: com.apple.MobileTimer.timerserver XPC / MTTimerManager in MobileTimer.framework (private) — requires entitlement com.apple.private.mobiletimerd (private), impossible for dev-signed apps
- BLOCKED: AlarmKit.framework (public on iOS 26) — present on macOS 26 only in /System/iOSSupport, Catalyst-only, unusable from GPUI

### Permissions / TCC

Reading the CFPreferences plist: no TCC, no prompt (verified). Reading the group container sqlite may trigger macOS 15+/26 'wants to access data from other apps' (App Data TCC) one-time prompt on some configurations — treat it as an optional fallback and prefer the plist. Notifications: standard UNUserNotificationCenter authorization prompt, requires the dev-signed .app bundle the repo already produces (no entitlement needed outside the App Store). Shortcuts control: one-time manual import of the shipped .shortcut files by the user (files must be signed via `shortcuts sign` to import); `shortcuts run` itself needs no TCC. No Full Disk Access, no Accessibility, no private entitlements — the only truly blocked path (direct MTTimerManager XPC) requires com.apple.private.mobiletimerd, which a dev-signed app cannot hold.

### Integration map (files to touch)

- NEW crates/nook-core/src/system_timers.rs — SystemTimer model {id: Uuid, title, duration, state, fire_date, deep_link}, CFPreferences reader (primary) + rusqlite group-container fallback, MTTimerState enum mapping, NSKeyedArchiver fire-time decode, kqueue vnode watcher exposing an async change stream (register in lib.rs)
- NEW crates/nook-core/src/shortcuts.rs — `shortcuts list` detection of the Nook Clock shortcuts, `shortcuts run` wrappers for start/pause/resume/cancel, x-apple-clock: deep-link open; degrade to 'Open Clock' when shortcuts not imported
- crates/nook/src/island/mod.rs — add system_timers: Vec<SystemTimer> to Island state; spawn the watcher task next to the existing poll loops (~lines 500–620); extend running_timer()/face_timer() so a running Clock timer competes for the compact face
- crates/nook/src/widgets/timers.rs — expanded Timers pane: render system timers in a 'Clock' section with a badge, remaining time from fire_date, pause/resume/cancel buttons wired to shortcuts.rs; compact face reuses the existing timer_ring canvas
- NEW crates/nook/src/notify.rs (or extend platform.rs) — UNUserNotificationCenter authorization + schedule/cancel of UNTimeIntervalNotificationTrigger for island-local timers; called from the timer start/pause/finish transitions in island/mod.rs
- Info.plist + scripts/bundle.sh — bundle resources/shortcuts/*.shortcut (pre-signed with `shortcuts sign`), onboarding row in island/settings.rs with an 'Install Clock shortcuts' button (opens the .shortcut files for one-click import)
- crates/nook/Cargo.toml — add objc2-user-notifications = "0.3"; nook-core already has rusqlite and objc2-foundation

### Battery requirements

Near-zero idle cost is achievable because nothing polls: a kqueue/dispatch-source vnode watch on ~/Library/Preferences/com.apple.mobiletimerd.plist (re-armed after cfprefsd's atomic rename-writes, with the group-container sqlite-wal as a second watch) costs nothing while idle and fires only when timers actually change. A running Clock timer stores an absolute fire date, so no external events are needed during countdown — the existing island tick loop (which already runs only while a timer is visible/running) computes remaining locally. CFPreferences reads are served from cfprefsd's cache, so the on-change re-read is microseconds. `shortcuts run` spawns a short-lived process only on explicit user taps. UNUserNotificationCenter scheduling is one XPC call per timer start/pause — the countdown itself is carried by usernoted, not by the app. Do not watch the whole ~/Library/Preferences directory with FSEvents (chatty); watch the single file.

### Risks & honest blockers

All read paths depend on private storage formats: the MTTimerState enum values and running-timer fire-time encoding must be calibrated empirically on day 1 (start/pause a Clock timer and diff), and Apple set 'MigratedToCoreData' flags — if a future macOS stops mirroring into the plist, the reader must fall back to the sqlite store, whose group-container path can raise a scary-sounding TCC prompt. App Intents identifiers (INCreateTimerIntent etc.) could be renamed in any release, silently breaking the shipped shortcuts; the shortcuts onboarding is a manual step users may skip, leaving the feature read-only; `shortcuts run` adds 1–2 s latency and can briefly activate the Shortcuts process. There is no silent programmatic create/pause path at all without Shortcuts — pure XPC control is entitlement-gated and AlarmKit is Catalyst-only on macOS 26. Notification mirroring has no meaningful risk. No comparable notch app (boring.notch, NotchNook, Droppy) reads mobiletimerd today — this is novel, which also means no prior art to lean on when macOS changes the format.

### Definition of done

- `cargo test -p nook -p nook-core` passes; new logic has unit tests where practical.
- Zero new idle wakeups: no polling loop unless the package explicitly allows one; verify with the cputime check from the ground rules.
- Feature has a Settings toggle (default per package) and degrades gracefully when its permission/API is unavailable.
- No `window.resize` or blocking AppKit calls inside `render` (see GPUI hazards).

---

