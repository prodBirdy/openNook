# openNook

<!-- Written by openNook on every start. Edits here are overwritten. -->

This folder is **openNook's whole interface, as files**. openNook is the island around the
MacBook's camera: closed, it shows one thing beside the camera (a track playing, an agent
working, a dictation); open, it shows pages. Every one of those is a **mod**: a folder of
JavaScript in `mods/`. openNook redraws from these files while it runs, within a second of a
save. When someone asks for something ("show my PRs in the notch", "make the player bigger",
"put agents first", "show when my build fails"), you do it by editing these files or writing a
mod. The shipped mods (`now-playing`, `weather`, `world-clock`, `iss`, `timer`, `herdr`, `containers`) are built from
exactly the parts you have; nothing about them is special.

## The files

| Path | What it decides | Applied |
|---|---|---|
| `mods/<name>/main.js` | a mod: what it shows closed (`activity`) and open (`expanded`) | live |
| `mods/<name>/mod.json` | its title, tab icon (`"app:<bundle id>"` for an app's own icon, or `"./icon.png"`: see Tab icons), default props | live |
| `layout.json` | which mod wins the closed island, page order, hidden mods, props, theme, the island's size and look | live |
| `themes/<name>.json` | your own themes: tokens, optionally over a `"base"` theme | live |
| `nook.d.ts`, `ui.d.ts` | the API, exactly. Read them; they're the truth | — |
| `mods/<name>.error` | why that mod is broken; present only while it is | written by openNook |
| `layout.error` | why layout.json was rejected | written by openNook |
| `logs/<name>.log` | the mod's `console.log` and errors | written by openNook |
| `data/<name>.json` | the mod's `localStorage` | written by openNook |
| `shared.json` | `setShared` values every mod sees | written by openNook |

A broken edit never breaks the island: the last good version keeps running and the reason
goes to the `.error` file. **Check after every edit.**

## Shipped mods are openNook's

`now-playing`, `weather`, `world-clock`, `iss`, `timer`, `herdr` and `containers` are replaced with openNook's copy on each start. If you
edit one in place, openNook moves your version to `mods/<name>-edited` before restoring its
own, so nothing is lost, but the clean way is to copy the folder to a new name, then list the
new name in `layout.json` and add the old one to `"hidden"`.

## A mod

```js
// mods/battery/main.js
import { View, row, text, icon } from "nook/ui";
import { exec } from "nook";

export default class Battery extends View {
  init(props, cx) {
    this.percent = null;
    const refresh = async (cx) => {
      const { stdout } = await exec("pmset", ["-g", "batt"]);
      this.percent = Number(stdout.match(/(\d+)%/)?.[1] ?? NaN);
      cx.notify();
    };
    cx.spawn(refresh);
    cx.timer.every(60_000, (cx) => cx.spawn(refresh));
  }

  // Closed island: beside the camera. null = nothing to show (another mod may).
  activity() {
    if (!(this.percent < 20)) return null;
    return { leading: icon("battery-low", 16).color("red"), trailing: text(`${this.percent}%`).text_size(12), priority: 40 };
  }

  // Open island: this mod's page (a tab with mod.json's icon).
  expanded() {
    return row().gap(10).py(10).child(icon("battery", 18)).child(text(`${this.percent ?? "–"}%`).text_size(15).font_semibold());
  }
}
```

```json
// mods/battery/mod.json
{ "title": "Battery", "icon": "battery" }
```

- `export default class … extends View`. Every method is optional.
- `init(props, cx)` runs once per load; a save reloads the mod (state starts over; keep what
  must survive in `localStorage`).
- `activity(cx)` returns `{ leading, trailing, detail, priority, width }` or `null`. Leading
  sits left of the camera, trailing right; each side is about one notch-height wide unless
  `width` says. Keep it to an icon, a few characters, a dot or a tiny meter: the space is small.
  `detail` is one short line (a title, a place) shown under the camera when the closed island
  is hovered and leans out.
- `expanded(cx)` returns one element: the page. Its width is layout.json's island width less
  the margins (`island().page.width`: about 470 points at a 500-point island, less when
  narrower); height follows content. Lay it out to fit any width: let rows share space
  (`flex_1`), or break a long run of buttons into rows. Never a fixed-width row that only fits
  the default island: on a narrower one it runs off the right edge.
- `icon(cx)` returns the tab icon for now (`"app:<bundle id>"`, `"./icon.png"`, a built-in
  name or `"./file.svg"`), in place of mod.json's: `now-playing` shows the app that's playing.
  An app that isn't installed falls back to mod.json's icon.
- `unmount(cx)` runs before an unload.

### Tab icons

The tab bar is a row of app icons, as in the Dock: every mod's tab icon looks like a macOS
app icon, in colour. Never a bare line glyph beside the others.

- A mod about an app uses that app's icon: `"icon": "app:com.apple.Music"`.
- Any other mod ships its own `icon.png` (`"icon": "./icon.png"`, at least 128 × 128 px,
  256 is best) drawn as a macOS app icon:
  - Apple's icon grid: the body is a squircle (continuous corners, not a rounded rectangle)
    824/1024 of the canvas, centered, with the rest transparent, the margin an app's own icon
    has. Without it the tab looks bigger than its neighbours.
  - A full-bleed background with a soft top-to-bottom gradient in one hue (orange for a
    timer, deep blue for the sky), a gentle sheen at the top and a thin, light rim (the
    Liquid Glass edge).
  - One bold, simple glyph, mostly white, centered, with a soft shadow. No text and no fine
    detail: it is shown at 16 points, so check it at that size.
  - A soft shadow under the squircle, as app icons have.
- Built-in names (`"timer"`) and `"./file.svg"` are tinted line glyphs, for an icon you change
  while the mod runs (`icon(cx)`) or for a first draft, not for a finished mod.

openNook's own repository draws the shipped mods' icons with `scripts/mod-icons.swift`; it
is a template for new ones.

### Rendering and state

Views are descriptions. openNook re-runs `activity` and `expanded` when:

- something they **read** changed: `media()`, `agents()`, `apps()`, `appRunning()`, `island()`,
  `shared()`, `theme()`, `props()`. Reads are tracked; never subscribe or poll for these.
- the mod called `cx.notify()`, after changing its own fields (in a handler, a timer, a `tail`
  callback, or after an `await`).

Render functions must be quick and pure: no `exec`, no `await`, no timers per render. Do slow
work in `init` or a timer with `cx.spawn(async (cx) => { …; cx.notify(); })`. A call into a mod
gets 500 ms (3 s for `init`); past that it's stopped and the `.error` file says so.

Give elements that change an `.id("…")`: then a change *springs* (size, color, position) instead
of replacing the element. Ids only need to be unique within the mod.

### Elements (`nook/ui`, full list in `ui.d.ts`)

`row()` (side by side, vertically centered), `col()` / `div()` (stacked), `text(s)`,
`icon(name, size)`, `image(src)`, `shader(src, params)`, `spacer()`. Methods chain like gpui / Tailwind:
`.w(n) .h(n) .size(n) .w_full() .flex_1() .p(n) .px(n) .py(n) .gap(n) .items_center()
.justify_between() .bg(c) .rounded(n) .rounded_full() .border(w, c) .opacity(o) .clip()
.hover_bg(c) .gradient(top, bottom) .text_size(n) .text_color(c) .font_semibold() .font_mono() .tabular_nums()
.truncate() .line_clamp(n) .color(c)`, motion: `.spring(duration, bounce) .enter("fade" |
"scale" | "blur" | "slide") .pulse(period) .spin(period) .scale(k) .offset(x, y)`, input:
`.on_click((event, cx) => …) .on_hover((hovered, cx) => …) .on_key_down((key, cx) => …)`,
layering: `.pin(l, t, r, b) .absolute_fill()` (over the parent; earlier children draw under
later ones), and `.when(cond, el => …)`.

### Scrolling

A page grows with its content, up to the screen. For a long list, give a box a bounded height
and let it scroll:

```js
col().id("chats").max_h(260).gap(4).overflow_y_scroll().children(rows)   // a list
col().id("thread").max_h(240).gap(4).stick_bottom().children(bubbles)     // a chat
```

- `.overflow_y_scroll()` (or herdr's `.overflow_y_scrollbar()`) on a box with `.h(n)` or
  `.max_h(n)` and an `.id()` (without one, the render fails: the id keeps its place).
- `.stick_bottom()`: opens at its end and follows it as content arrives.
- A two-finger swipe that starts mostly vertical over a box that has somewhere to go scrolls
  it, momentum included; any other swipe changes the page or closes the island as before.
  Its edges fade while there's more under them. There's no visible scrollbar.

### Text fields

As in herdr's mods: a field's state is made once, in `init` (never in a render, which throws),
and an `Input` element shows it. The text lives in openNook, so it survives every render and
typing never waits on the mod.

```js
import { View, row, InputState, Input } from "nook/ui";

init(props, cx) {
  this.field = InputState.new({ placeholder: "New reminder" });
  this.field.on("submit", (_, cx) => { this.add(this.field.value()); this.field.set_value(""); cx.notify(); });
}
expanded() {
  return row().gap(8).child(Input.new(this.field).flex_1().h(32).px(10).rounded(8).bg("surface").text_size(13));
}
```

- `InputState.new({ placeholder, value })`; `.value()`, `.set_value(s)` (no `change`
  follows), `.focus()`, `.blur()`, `.is_focused()`, `.set_placeholder(s)`,
  `.set_masked(bool)` (a password), `.release()`.
- `.on("change" | "submit" | "focus" | "blur", (event, cx) => …)`: `change` on every edit
  (`event.value`), `submit` on Return, `blur` on Escape, a click in another app, or the
  island closing.
- `Input.new(state)` (or `input(state)`) is a single-line field: style its text with
  `text_size`/`text_color`, its box with `bg`/`rounded`/`border`/`px`/`h`, and the
  placeholder with `.placeholder_color(c)`.
- `.on_key_down((key, cx) => …)` on the field or a parent hears the keys the field doesn't
  type itself: `{ key, keystroke, key_char, modifiers }`, spelled like `"escape"`,
  `"cmd-enter"`, `"up"`.

Focusing a field (a click, or `focus()`) gives the island the keyboard without bringing
openNook forward, and keeps the island open while someone types; the keyboard goes back to
the app it came from on Escape, a click elsewhere, or when the island closes.

Colors are hex (`"#ff9f0a"`, `"#ffffff80"`) or theme tokens: `island` (the island's own
body), `fg`, `muted`, `faint`, `bg`, `surface`, `raised`, `border`, `accent`, `green`,
`yellow`, `orange`, `red`, `blue`, `purple`. Themes are all dark; design for dark. Prefer
tokens, so a theme change restyles every mod.

Images: `image("https://…")`, an absolute path, `"./cover.png"` (the mod's folder),
`"data:image/png;base64,…"`, `"app:<bundle id>"` (that app's icon), or `media().artwork`.
Give images a size.

Icons: built in (lucide, plus agents' logos), or `"./file.svg"` from the mod's folder.
Built in: {{ICONS}}.

### Shaders

`shader("./sky.wgsl", [a, b, …])` draws a WGSL fragment function from the mod's folder,
animated, inside Rune's own frame (clipped, rounded and faded like any element; nothing is
copied back to the CPU). The file defines:

```wgsl
fn scene(frag: vec2f, res: vec2f, t: f32) -> vec4f {
  // frag: device pixels from the element's top-left; res: its size; t: seconds.
  // P(0)…P(7): the params array. Return straight-alpha sRGB.
  let uv = frag / res;
  return vec4f(uv.x, 0.3 + 0.2 * sin(t + P(0)), uv.y, 1.0);
}
```

`LEVEL(0)`…`LEVEL(4)` is how loud the Mac is playing right now, 0–1, in five bands: bass,
low mids, mids, high mids, treble (the `now-playing` meter is built on it). It is the real
sound on its way to the speakers, read from a Core Audio tap that runs only while a shader
that calls `LEVEL` is on screen; the first one asks the person for System Audio Recording,
and without it every level is 0. Give such a shader `.fps(60)` so it keeps up with the beat.

Helper functions and constants may sit beside it; don't name anything `nook_*`, `NookSlot`,
`P` or `LEVEL`. It's a box like any other: size it (`.h(120)`; width defaults to full), round it,
layer it with `.absolute_fill()` under content. `.fps(0)` draws one still frame. A shader
that doesn't compile draws nothing; `opennook check` prints the compiler's error with the
line. The `weather` mod's `scene.wgsl` is a full example (twelve skies).

### The island's look

The island itself can change too, not just what's in it: its color or gradient, a border, a
glow, how round its corners are, the wings that join it to the screen edge, its shadow. Three
ways, from lasting to momentary:

- **Theme** (`layout.json` `"theme"`): every color token. Built in: `default`, `graphite`,
  `midnight`, `ember`, `forest`, `rose`; add your own as `themes/<name>.json`
  (`{ "base": "midnight", "accent": "#c792ea" }`). `"theme": "ember"` picks one;
  `"theme": { "base": "ember", "accent": "#ffffff" }` overrides tokens over it.
- **Look** (`layout.json` `island.style`): the island's shape and paint for every mod.
- **A mod's `style()`**: the same fields, while that mod owns the island (its activity is
  the closed island's, or its page is open), over `island.style`. A CI mod can turn the
  closed island red with an orange glow while a build fails, and return `null` once it's fixed.

```js
style() {
  return this.failing ? { gradient: ["#000000", "#3a0a0a"], border: { width: 1, color: "red" }, glow: { color: "#ff453a88", radius: 22 } } : null;
}
```

Fields (`IslandStyle` in `ui.d.ts`): `fill`, `gradient: [top, bottom]`, `border: { width,
color }`, `glow: { color, radius }`, `compactRadius` (closed corners, at most the camera's
height), `expandedRadius` (open, up to 60), `wings` (0–24, 0 for none), `shadow` (the open
island's drop shadow). The top edge stays flat against the screen's, and the camera housing
is black whatever the island's color, so keep `island` dark enough to sit around it. A border runs down
the sides and round the bottom, not along the screen edge. Liquid Glass ignores `fill` and
`gradient`.

A mod can also change these for good with `run("theme.set", { name, colors })` and
`run("layout.patch", { patch })` (below); `themes()` lists every theme with its colors.
Change the look when someone asks for it, not on your own.

### Data and commands (`nook`, full list in `nook.d.ts`)

- `media()`: the playing track or null. `elapsed` is as of `sampledAt`; advance it yourself
  while `playing` (a 1 s timer that calls `cx.notify()`). `colors` are three from the cover,
  most prominent first (`now-playing` tints its level bars with them). Anything that
  publishes to macOS's Now Playing shows here (Spotify, Music, browsers, …); an app that
  doesn't, doesn't.
- `agents()`: coding-agent sessions (Claude, Codex, OpenCode, Cursor, Gemini, Grok, Aider, Pi),
  each with `status` `"working"` or `"waiting"` and an `icon` for its logo.
- `apps()`, `appRunning(bundleId)`: running apps, menu-bar apps included.
- `island()`: open or closed, which page, which mod owns the closed island, notch size, the
  page's width.
- `location()`: where the person is, `{ latitude, longitude, name, timezone }`: the place set
  in Settings › General › Location, else the Mac's location. `null` while it's found,
  `{ error }` when it can't be (the Mac's location asks permission on the first read and
  needs openNook.app). Use it for "here" rather than a city prop of your own.
- `exec(cmd, args, { cwd })` → `{ code, stdout, stderr }`, with the user's PATH (Homebrew,
  `~/.local/bin`, …). 30 s and 8 MiB limits.
- `fetch(url, { method, headers, body })` → `{ status, ok, text(), json() }`.
- `readTextFile(path)`, `writeTextFile(path, text)` (writes only under this folder).
  `~/` is home; relative paths are the mod's folder.
- `tail(path, (line, cx) => …, { fromEnd })`: follow a log as it grows (rotation included).
- `watchFile(path, (text, cx) => …)`: the file's contents now and on every change.
- `run(command, args)`: `island.expand`, `island.collapse`, `island.toggle`,
  `island.select {mod}`, `island.peek {mod?, ms?}` (open briefly, e.g. when something
  finishes), `media.toggle/next/previous/seek {seconds}/shuffle/repeat`, `agents.focus {pid}`,
  `agents.reveal {cwd}`, `open {target}` (URL, file or folder), `theme.set {name?, colors?}`
  (pick a theme, override tokens; null drops one), `layout.patch {patch}` (a JSON merge patch
  into layout.json: objects merge, null deletes). Both are saved and apply within a second; a
  refused one is written to `logs/<name>.log`.
- `props()`: layout.json's `mods.<name>.props`, else mod.json's `props`.
- `shared(key)`, `setShared(key, value)`: values every mod sees (setting re-renders readers).
- Globals: `localStorage` (per mod), `console.log` (to `logs/<name>.log`), `fetch`.

Mods are trusted code: `exec` and `fetch` are unrestricted. Never put secrets in a mod;
read them from a file or the keychain (`exec("security", [...])`) at run time.

## Integrating another program

Most apps say what they're doing somewhere a mod can see:

- **A log or state file**: `tail` it (JSON lines are ideal) or `watchFile` it.
- **A CLI**: `exec` it on a timer (`gh pr status --json …`, `docker ps --format json`). The
  `herdr` mod reads `hc ls --json` this way and acts with `herdr-app do agent.focus`.
- **A local HTTP API**: `fetch("http://127.0.0.1:…")`; read its port and token from its config
  file with `readTextFile`.
- **Whether it's running**: `appRunning("com.example.app")` (bundle ids: `apps()`).

Show nothing (`activity()` → `null`) while the program isn't running or has nothing to say:
the closed island belongs to whatever is happening now.

## layout.json

```json
{
  "island": { "width": 560, "hover": "peek", "display": "notch", "hideInFullScreen": true, "glass": true, "glassTint": 0.5, "haptics": true },
  "compact": ["herdr", "now-playing", "weather"],
  "expanded": ["now-playing", "weather", "herdr"],
  "hidden": [],
  "mods": { "herdr": { "props": { "peekOnBlocked": false } } },
  "theme": { "base": "midnight", "accent": "#ff9f0a" }
}
```

- `compact`: who wins the closed island when several have an activity, first listed first;
  unlisted mods follow by their `priority`.
- `expanded`: page (tab) order; unlisted mods with a page follow by name.
- `hidden`: loaded, never shown.
- `island.hover`: `"peek"` (hovering leans the closed island out and shows the activity's
  `detail`; a click opens it), `"open"` (opens after `hoverDelayMs`), or `"none"`.
- `island.display`: `"notch"` (the MacBook's display, else the main one), `"main"` (the one with
  the menu bar), or `"pointer"` (follows the pointer between displays). On a display without a
  camera housing the resting island is invisible; it shows when hovered or when a mod has an
  activity. `island().notch.hasNotch` says which kind the island is on.
- `island.hideInFullScreen`: out of the way while an app is full screen on its display.
- `island.haptics`: a tap when a swipe opens the island.
- `island.glass`: Liquid Glass under the open island (always off with Reduce Transparency).
  `glassTint` is how far the black veil reaches down from the camera: 0 bare glass, 0.5 the
  camera band, 1 twice that.
- `island.style`: the island's look for every mod (see The island's look):
  `{ "border": { "width": 1, "color": "border" }, "expandedRadius": 24, "wings": 0 }`.
- `theme`: a theme's name, or `{ "base": "<name>", "<token>": "<color>" }`.

## Check before you finish

```sh
opennook check ~/.config/opennook/mods/<name>
```

Loads the mod against live data, lets it settle (1.5 s; `--wait ms`), and prints what it shows
or every problem (syntax, a throw, a bad element, an unknown icon), exiting nonzero. Never call
a mod done without a passing check.

```sh
opennook shot island.png            # the closed island, as it would look now
opennook shot open.png --mod <name> # the open island on that mod's page
opennook shot peek.png --hover      # the closed island as hovered
```

Renders the island with live mods to a PNG and prints its path (glass shows as a plain grey
there; the real one needs the screen behind it). Look at it: spacing, sizes,
truncation, contrast. `opennook paths` prints where everything is.

## Seeing it live

openNook is running if the notch responds to hover. People open it with a click or a
two-finger swipe down (it stretches first, then opens), swipe sideways to change the page
(or, closed, which mod's activity shows), and swipe up to close it. Right-click the island for this folder,
this guide, and Quit. Run `opennook` to start it (`target/release/opennook` in the repo, or
`/Applications/openNook.app`).
