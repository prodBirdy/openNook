# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 2.0.0-alpha.3 - 2026-11-10

### Added

- A mod gallery at https://prodbirdy.github.io/openNook/gallery/: public repos with the topic
  `opennook-mod`, rebuilt hourly, with the shipped mods as examples. AGENTS.md says how to add
  one from it and how to share your own.
- A new website for 2.0, with real captures of the island (Liquid Glass over macOS's wallpaper)
  and Now Playing, Weather and Herdr as mod examples.
- The installer opens as a designed window: openNook on the left, Applications on the right,
  and an arrow between them.
- Three more shipped mods: Messages (unread beside the camera, recent conversations with photos,
  voice messages and link previews, and replies from the island; needs Full Disk Access),
  Reminders (tick off, add, see what's due; it asks for access only when its page is first
  opened) and Caffeinate (keep the Mac awake for a while or until stopped).
- Caffeinate can keep the Mac awake with the lid closed too (`pmset disablesleep`, after one
  administrator password); sleep comes back by itself when the time is up, when it's turned
  off, or when openNook quits, and the page offers to restore it if a restart got in between.
- Reminders: a new reminder can have a date, time, priority and flag, from chips under the
  field or written into the title ("Call Sam tomorrow 9:00 !!").
- A pill-shaped island for the MacBook Pro with the PROLED XDR display, whose camera sits in a
  cutout below the screen's edge: closed, a floating pill around it; open, an island rounded
  all round, without wings. `island.shape` in layout.json: `"auto"` (picks it for a cutout
  narrower than any notch), `"notch"` or `"pill"`.
- After an hour of use (counted only while someone's at the Mac), the open island asks once
  for a star on GitHub; starring or "Not now" ends it for good.
- Demo data for screenshots: `opennook shot --demo <fixture.json>` (or `OPENNOOK_DEMO` for the
  island itself) puts a track, coding agents, a music meter and programs' and websites'
  answers in place of the Mac's own. `scripts/demo/fixture.json` is the website's.

## 2.0.0-alpha.2 - 2026-11-09

### Added

- Containers, a shipped mod for Apple's [`container`](https://github.com/apple/container)
  runtime: how many are running beside the camera, and a page with each container's image,
  address, uptime, CPU and memory, and a button to start or stop it.
- Text fields for mods: `InputState` and `Input` in `nook/ui`, as in herdr's mods. The text
  lives in openNook, so it survives renders and typing never waits on the mod. Focusing one
  gives the island the keyboard without bringing openNook forward, and Escape, a click
  elsewhere or closing the island gives it back. `.on_key_down` hears the keys a field doesn't
  type (`"escape"`, `"cmd-enter"`, `"up"`).
- Scrolling in mods: `.overflow_y_scroll()` on a box with a bounded height and an id, and
  `.stick_bottom()` for a chat that opens at its end and follows it. A two-finger swipe that
  starts mostly vertical over one scrolls it, momentum included; its edges fade while there's
  more under them.
- Remote pictures: `image("https://…")` downloads and decodes the picture (AVIF and HEIC
  through macOS), and `opennook shot` waits for them.

### Changed

- More tabs than fit beside the camera scroll: a sideways swipe that starts over the tab strip
  moves it instead of changing the page, its edges fade while tabs hide past them, and the open
  page's tab slides into view whenever the page changes. Tabs under the camera can't be clicked.

## 2.0.0-alpha.1 - 2026-11-07

A rewrite. The island renders with [Rune](https://github.com/anomalyco/rune) (wgpu) instead of
GPUI, and everything it shows is a mod: a folder of JavaScript under `~/.config/opennook/mods`
that you or a coding agent edit while it runs. macOS only for now.

### Added

- Mods: `main.js` + `mod.json` per folder, run on QuickJS off the UI thread. Saved edits apply
  within a second; a broken edit keeps the last good version and writes why to
  `mods/<name>.error`. Mods can ship WGSL shaders and a colour `icon.png`.
- `layout.json` picks who wins the closed island, page order, props, theme and island size.
- `~/.config/opennook/AGENTS.md`, `nook.d.ts` and `ui.d.ts`: the guide and exact API a coding
  agent reads before changing anything. `opennook --skill` prints it as an Agent Skill.
- CLI: `opennook check <mod>` loads a mod and reports its views or its error;
  `opennook shot` renders the island or Settings to a PNG; `opennook paths`.
- Shipped mods: Now Playing (with a live music meter from a Core Audio process tap, macOS
  14.2+), Weather (an animated WGSL sky with the real moon phase), World Clock (a dot-matrix
  world lit where the sun is up), ISS (live position, orbit, altitude and speed), Timer
  (presets, ring, chime) and Herdr (your coding agents; click to jump to one).
- Settings: General (hover, display, full screen, haptics, launch at login, Liquid Glass, open
  width, Location), Mods, Permissions (System Audio Recording, Location, Automation), Make it
  yours, and About, plus a first-run welcome.
- Gestures: hover to peek, click or two-finger swipe down to open, swipe sideways for pages.
- Location: this Mac's, or a place set once in Settings; mods read it with `location()`.
- The island's look is moddable, as in Herdr: themes (built-in `graphite`, `midnight`,
  `ember`, `forest`, `rose`, or your own `themes/*.json`, picked in Settings › General),
  `island.style` in layout.json (fill or gradient, border, glow, corner radii, wings, shadow),
  and a mod's `style()` that restyles the island while it owns it. Mods can save changes with
  `run("theme.set")` and `run("layout.patch")`, and list themes with `themes()`.

### Changed

- Full-screen detection asks for the display's Space type, so the island hides correctly on
  notched displays.
- The app asks only for what it uses: System Audio Recording, Location and Automation.
- The open island's drop shadow shows again: the island clipped its own shadows away.

### Removed

- The 1.x widgets (Calendar, Reminders, Files, Notes, Speed, Mirror, Voice, Observe, Lyrics,
  floating widgets and the rest). Each can come back as a mod.

## 1.x - never released

These landed on the GPUI line after 1.0.0 and were never shipped; 2.0 replaces that app.

### Added

- Floating widgets: pop Lyrics or Observe out of the notch into a small always-on-top window that joins every Space. Throw it toward a corner and it springs there (multi-display, three-finger drag works), hover for Keep on Top / Back to Notch / Close, double-click Lyrics for a mini pill. Windows reopen where you left them.
- Observe picks its metrics from what the endpoint provides: warmUP totals, latency, per-route counts, errors and latency, and rate limits, or Prometheus metric names, with a filter and pin/unpin.
- Speed measures upload as well as download.
- Up Next shows cover art for Apple Music and other MediaRemote players, not only Spotify.

### Changed

- Media card, Up Next, AirPlay, Lyrics, Mirror and Voice matched 1:1 to the updated design: bold titles with smaller artist lines, the new Now Playing row heights, 24 pt queue covers, and artwork rings that no longer clip the image.
- Lyric changes animate like Apple Music: lines follow each other with a short stagger and the text size eases instead of snapping. Wrapped lines push the next line down, and the first frame no longer flashes.
- The Voice recording state shows the live transcript under the waveform.

### Removed

- Dropped the Obsidian, VPN, High Alert, Meetings, and Notifications widgets (deleted from the design). Saved settings that still list those modules load without failing and silently drop them.

## 1.0.0 - 2026-09-27

The first open-source 1.0 release.

### Changed

- Applied a HIG design pass across the app, including clearer permissions, keyboard access, Reduce Motion and Reduce Transparency, Increase Contrast, and Differentiate Without Color.
- Trimmed the visible product to the baseline widget set: Agents, Media, Calendar, Reminders, Timers, Weather, Battery, Files, Notes, Speed, and Mirror. Experimental widgets remain behind a Settings toggle.
- Media controls now use an optimistic interaction model, expanded cards keep a consistent width, and an empty Now Playing card still provides a player surface.
- The expanded island is 1120 pt wide and the Nook tab is one row of 17 cells, so five or six widgets fit side by side; widget sizes Small/Medium/Large set how much of the row a widget takes.
- Widgets were matched 1:1 to the design gallery: Media, Calendar, Notes, Observe, Obsidian, Timers, Reminders, Speed, Agents, Battery, Messages, Weather, VPN, Alert, Stats, Voice, Meetings, and Notify.
- The media player has new transport glyphs, an AirPlay picker, an always-on queue slot, a "Not Playing" idle state, and a compact visualizer that animates all five bars.
- Weather renders a Metal shader backdrop (12 moods) and always follows the current location.
- Observe opens a full-width chart when the card is clicked, with stacked status columns, axis ticks, and a hover crosshair and tooltip. It no longer polls without a token.
- The voice recorder has a scrubber, pause/resume, ±15 s skips, and Open in QuickTime / Show in Finder.
- Island animations step at up to 60 fps on a display-synced frame driver.
- Media view switches between Now Playing, Up Next, Lyrics, and AirPlay animate; lyrics are Apple Music style, with the current line wrapped, past and next lines dimmed, and a scroll on line advance; the compact hover chin shows the track title, and the mode dots are removed.
- The compact pill widens to fit its trailing text, Agents and Messages show their trailing labels again, and Idle shows weather.

### Removed

- Search, including clipboard history, keyboard sounds, smooth scrolling, window snap, Thaw menu-bar hiding, public-link sharing, and the menu-bar status item.

### Fixed

- Now Playing froze in the packaged app (title and cover stuck on the track playing at launch): the MediaRemote stream was started with a `--diff` flag the bundled adapter rejects, so it never ran. A dead stream now falls back to fresh one-shot reads instead of a stale snapshot.
- Spotify covers fall back to Spotify's artwork URL when MediaRemote has no artwork data.
- Settings showed every permission twice (a status row and a button row); each is one row now, with Open Settings only when access is missing.
- The packaged app asks for Accessibility at launch when it is not granted, so a reinstall prompts instead of silently failing.
- Battery showed "No battery" because the startup snapshot was dropped.
- Finder drags open the file tray before reaching the top screen edge (macOS 26 Spaces bar).
- Calendar and Notes no longer repaint on every 30-second poll when nothing changed.
- Media scrubber seeks stay optimistic: the time thumb no longer snaps back
  while MediaRemote is still catching up to the seek command.
- Term tab aborted on wheel-scroll (and when collapsing the island while
  scrolled): vt100 subtracted `rows - scrollback_offset` and overflowed
  once the viewport moved more than one screen into history.
- The LocalSend drop target is hidden when the LocalSend app is not installed,
  so the tray does not offer a share path that cannot run.
- Hovering a Finder file over the island did not open the file tray: the mouse
  thread sampled NSPasteboard off the main thread and could poison the drag
  baseline, so inbound drags never armed the dropzone.
- Term tab clipped the last lines of a login-shell TUI: the PTY claimed the
  pane's full height on top of the "login shell" header, and the island stayed
  at the widget-row size.
- Clipped or overlapping text at small widget sizes (Weather, Meeting, High
  Alert, SysStats, Obsidian, Messages, Reminders, Agents); the Observe card no
  longer shows a square hover highlight.
- The Linux build compiles again: macOS-only focus-restore code is cfg-gated and
  lyrics build on all targets, and `scripts/linux-deps.sh` exposes an unversioned
  `libstdc++.so` so rust-lld links on Debian and Ubuntu.

### Changed

- Coding-agent faces use each agent's logo as a mask over a Magic UI glyph
  matrix (`01·•+*/\<>=`), tinted with that agent's brand color. The island
  border uses the same brand on the compact Agents face and while expanded.
- Detects the Pi coding-agent harness (`pi` / `pi-coding-agent`).
- Now Playing blooms a darkened, radially faded blur of the album artwork
  behind the cover, so grayscale tracks don't turn the pane into a gray card.
- Terminal is a real login-shell PTY instead of the one-shot command field,
  so typing, control keys, and interactive programs work like the machine CLI.
- Messages is an incoming-only quick reply on the island instead of a
  conversation inbox. The pane appears when a message arrives, with a sender
  lockup and a reply field; Escape dismisses it.
- Voice recordings list as dated memos with a ringed record/stop control,
  matching a native memo list instead of a count plus mic icon.

### Packaging

- Linux GPUI artifact: `cargo build --release -p nook` on Ubuntu, uploaded as
  `openNook-0.3.0-x86_64-unknown-linux-gnu.tar.gz` from `.github/workflows/linux-release.yml`
  (`workflow_dispatch` or a `v*-linux` tag). Publishes `v0.3.0-linux`; does not
  retag or rewrite the macOS `v0.3.0` notes.
- This is the current GPUI product. It is not the Tauri `0.0.2a` AppImage.

Linux compact is the Idle housing wrap (no “openNook” title), top-center on
the real X11 `DisplayWidth` / `DisplayHeight`. Hover polling is still
stubbed — click or scroll the painted island.

Linux does not get Metal, camera-housing notch metrics, MediaRemote, Liquid
Glass, camera Mirror, AirDrop, AppKit file drag-out,
EventKit, hide-when-maximized occupancy, or `installer.dmg`. Settings / Quit:
Ctrl+, and Ctrl+Q.

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

- Native GPUI island (transparent `PopUp` window, no dock icon)
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
[1.0.0]: https://github.com/prodBirdy/openNook/compare/v0.3.0...v1.0.0
