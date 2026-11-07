//! The Settings window, and the first launch's onboarding in it. Native-looking (a sidebar,
//! grouped rows, switches, AppKit menus for choices), and backed by the same files an agent
//! edits: every change is written to layout.json, which the island applies live.

use crate::home;
use nook_core::permissions::Access;
use rune::tok::TextStyle;
use rune::{Color, Cx, Drag, El, Phase, PopupMenu, Spring, TextEdit, Tx, col, el, icon, row, text, text_input};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const SIZE: (f32, f32) = (780.0, 580.0);
const SIDEBAR: f32 = 210.0;
const ROW_H: f32 = 52.0;
const SNAPPY: Spring = Spring::new(0.25, 0.0);

/// Hex-like dark palette: a black sidebar, a raised content panel, cards on it.
mod c {
    use rune::Color;
    pub fn hex(s: &str) -> Color {
        rune::color::hex(s).unwrap_or(Color::WHITE)
    }
    pub fn sidebar() -> Color { hex("#0c0c0d") }
    pub fn panel() -> Color { hex("#18181a") }
    pub fn card() -> Color { hex("#1e1e20") }
    pub fn line() -> Color { hex("#2a2a2d") }
    pub fn selected() -> Color { hex("#2a2a2d") }
    pub fn hover() -> Color { hex("#ffffff0d") }
    pub fn fg() -> Color { hex("#f2f2f3") }
    pub fn muted() -> Color { hex("#8e8e93") }
    pub fn faint() -> Color { hex("#5f5f63") }
    pub fn accent() -> Color { hex("#3d6df5") }
    pub fn off() -> Color { hex("#39393d") }
    pub fn red() -> Color { hex("#ff453a") }
    pub fn green() -> Color { hex("#30d158") }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Mods,
    Permissions,
    Yours,
    About,
}

const PAGES: [(Page, &str, &str); 5] = [
    (Page::General, "General", "settings"),
    (Page::Mods, "Mods", "layout-grid"),
    (Page::Permissions, "Permissions", "shield-check"),
    (Page::Yours, "Make it yours", "pencil"),
    (Page::About, "About", "info"),
];

#[derive(Clone, Debug)]
pub struct ModInfo {
    pub name: String,
    pub title: String,
    pub icon: Option<String>,
    /// The mod's own picture (`./icon.png`), decoded once per refresh: width, height, RGBA.
    pub picture: Option<(u32, u32, Vec<u8>)>,
    pub description: Option<String>,
    pub error: bool,
}

/// Coding agents that take a first message as an argument, by command.
const AGENTS: [(&str, &str); 5] = [("claude", "Claude Code"), ("codex", "Codex"), ("opencode", "OpenCode"), ("pi", "Pi"), ("gemini", "Gemini")];

pub struct Settings {
    pub page: Page,
    /// Onboarding step while the first launch walks through it (0 welcome, 1 make it yours).
    pub onboarding: Option<u8>,
    layout: Value,
    mods: Vec<ModInfo>,
    login: bool,
    wish: TextEdit,
    /// Installed agents (command, name) and the one picked.
    agents: Vec<(&'static str, &'static str)>,
    agent: &'static str,
    herdr: bool,
    note: Option<(String, bool)>,
    refreshed: Option<Instant>,
    /// The place being typed for Settings › General › Location, the lookup under way, and how
    /// the last one went.
    place: TextEdit,
    finding: Option<std::sync::mpsc::Receiver<Result<Vec<Value>, String>>>,
    /// Places the lookup found, while the person picks one of several.
    found: Vec<Value>,
    place_note: Option<(String, bool)>,
    audio_access: Access,
    location_access: Access,
    /// The app macOS asks on openNook's behalf (a development build's terminal).
    asked_for: Option<String>,
}

impl Settings {
    pub fn new() -> Self {
        let mut s = Self {
            page: Page::General,
            onboarding: None,
            layout: Value::Null,
            mods: Vec::new(),
            login: false,
            wish: TextEdit::new(""),
            agents: Vec::new(),
            agent: "claude",
            herdr: false,
            note: None,
            refreshed: None,
            place: TextEdit::new(""),
            finding: None,
            found: Vec::new(),
            place_note: None,
            audio_access: Access::NotAsked,
            location_access: Access::NotAsked,
            asked_for: nook_core::permissions::asked_for(),
        };
        s.refresh();
        s
    }

    /// Reads the files again (layout, mods, agents): the agent may have changed them.
    pub fn refresh(&mut self) {
        let dir = home::app_dir();
        self.layout = std::fs::read_to_string(dir.join("layout.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}));
        let mods = dir.join("mods");
        self.mods = nook_mods::files::scan(&mods).into_iter().map(|f| {
            let m = nook_mods::files::read_manifest(&f.dir).unwrap_or_default();
            let picture = m.icon.as_deref().filter(|i| crate::icons::is_picture(i)).and_then(|i| picture(&f.dir.join(i)));
            ModInfo { error: mods.join(format!("{}.error", f.name)).exists(), title: m.title.unwrap_or_else(|| f.name.clone()), icon: m.icon, picture, description: m.description, name: f.name }
        }).collect();
        self.login = nook_core::login_item::is_enabled();
        let path = nook_mods::user_path(&home::home());
        self.agents = AGENTS.iter().copied().filter(|(cmd, _)| path.split(':').any(|d| std::path::Path::new(d).join(cmd).is_file())).collect();
        if !self.agents.iter().any(|(c, _)| *c == self.agent) {
            self.agent = self.agents.first().map_or("claude", |a| a.0);
        }
        self.audio_access = nook_core::permissions::audio_capture();
        self.location_access = nook_core::permissions::location();
        self.herdr = std::path::Path::new("/Applications/Herdr.app").exists() || path.split(':').any(|d| std::path::Path::new(d).join("herdr-app").is_file());
        self.refreshed = Some(Instant::now());
    }

    fn island(&self, key: &str) -> Option<&Value> {
        self.layout.get("island").and_then(|i| i.get(key))
    }

    fn hidden(&self, name: &str) -> bool {
        self.layout.get("hidden").and_then(Value::as_array).is_some_and(|h| h.iter().any(|v| v.as_str() == Some(name)))
    }

    /// Changes layout.json (read fresh, so an agent's edit in between isn't lost).
    fn edit_layout(&mut self, f: impl FnOnce(&mut Value)) {
        self.layout = home::edit_layout(f);
    }

    fn set_island(&mut self, key: &str, value: Value) {
        let key = key.to_owned();
        self.edit_layout(move |v| {
            if !v["island"].is_object() {
                v["island"] = json!({});
            }
            v["island"][key] = value;
        });
    }

    fn set_location_mode(&mut self, mode: &str) {
        let mode = mode.to_owned();
        self.edit_layout(move |v| {
            if !v["location"].is_object() {
                v["location"] = json!({});
            }
            v["location"]["mode"] = Value::from(mode);
        });
    }

    /// Looks the typed place up (Open-Meteo's geocoder, as the mods do) off the main thread;
    /// [`poll`] saves what it finds.
    fn find_place(&mut self) {
        let query = self.place.text.trim().to_owned();
        if query.is_empty() || self.finding.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.finding = Some(rx);
        self.place_note = Some((format!("Looking up {query}…"), true));
        std::thread::spawn(move || {
            let _ = tx.send(geocode(&query));
        });
    }

    fn toggle_hidden(&mut self, name: &str) {
        let name = name.to_owned();
        self.edit_layout(move |v| {
            let mut hidden: Vec<Value> = v.get("hidden").and_then(Value::as_array).cloned().unwrap_or_default();
            if let Some(i) = hidden.iter().position(|h| h.as_str() == Some(&name)) {
                hidden.remove(i);
            } else {
                hidden.push(Value::from(name));
            }
            v["hidden"] = Value::Array(hidden);
        });
    }

    fn prompt(&self) -> String {
        let wish = self.wish.text.trim();
        let wish = if wish.is_empty() { "Ask me what I'd like in my notch, then build it." } else { wish };
        format!(
            "You're customizing openNook, the island in my MacBook's notch. Everything it shows is a mod in \
             ~/.config/opennook, and ~/.config/opennook/AGENTS.md explains all of it: read it first. Then: {wish}\n\n\
             Check every mod you write with `{exe} check <mod folder>` and look at the island with `{exe} shot` \
             (add --mod <name> for its page) before you call it done.",
            exe = exe().display()
        )
    }

    fn start_in_herdr(&mut self) {
        let args = json!({ "name": "openNook", "folder": home::app_dir(), "argv": [self.agent], "task": self.prompt() });
        let result = std::process::Command::new("herdr-app").env("PATH", nook_mods::user_path(&home::home()))
            .args(["do", "space.new", &args.to_string()]).output();
        self.note = Some(match result {
            Ok(o) if o.status.success() => {
                let _ = open::that_detached("/Applications/Herdr.app");
                ("Started in Herdr. Your agent has the brief.".into(), true)
            }
            Ok(o) => (format!("Herdr: {}", String::from_utf8_lossy(&o.stderr).trim()), false),
            Err(e) => (format!("Herdr didn't start: {e}"), false),
        });
    }

    fn finish_onboarding(&mut self) {
        self.onboarding = None;
        self.page = Page::General;
        let _ = std::fs::write(onboarded_marker(), "");
    }
}

pub fn onboarded_marker() -> PathBuf {
    home::app_dir().join("data").join("onboarded")
}

pub fn first_launch() -> bool {
    !onboarded_marker().exists()
}

pub fn exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| "opennook".into())
}

/// The Agent Skill (`opennook --skill`, and what "Set up my agents" installs for Claude Code).
pub fn skill() -> String {
    format!(
        "---\nname: opennook\ndescription: Customize openNook, the island in the MacBook notch: write or change its mods \
         (JavaScript), its layout and theme. Use when someone asks to change what the notch shows or how it looks or moves.\n---\n\n\
         # openNook\n\nopenNook's whole interface is files in `~/.config/opennook`. Read `~/.config/opennook/AGENTS.md` before \
         changing anything: it covers mods, the `nook` and `nook/ui` APIs (`nook.d.ts`, `ui.d.ts`), layout.json and shaders.\n\n\
         - Check a mod: `{exe} check ~/.config/opennook/mods/<name>` (never call it done without a pass).\n\
         - Look at it: `{exe} shot island.png` (`--mod <name>` for its page, `--hover` for the peek).\n\
         - Edits apply live; a broken one keeps the last good version and writes `mods/<name>.error`.\n",
        exe = exe().display()
    )
}

/// Installs the skill for Claude Code and puts `opennook` on PATH (~/.local/bin).
fn set_up_agents() -> Result<String, String> {
    let home = home::home();
    let dir = home.join(".claude/skills/opennook");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    nook_mods::files::write_if_changed(&dir.join("SKILL.md"), skill()).map_err(|e| e.to_string())?;
    let bin = home.join(".local/bin");
    let _ = std::fs::create_dir_all(&bin);
    let link = bin.join("opennook");
    let ok_link = match std::fs::read_link(&link) {
        Ok(target) if target == exe() => true,
        Ok(_) | Err(_) => {
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(exe(), &link).is_ok()
        }
    };
    Ok(if ok_link { "Skill installed for Claude Code, and `opennook` is on your PATH.".into() } else { "Skill installed for Claude Code.".into() })
}

/// The places Open-Meteo's geocoder finds for `query` (up to five: a name like Sulzberg is a
/// village in more than one country), each as layout.json's `location` would hold it.
fn geocode(query: &str) -> Result<Vec<Value>, String> {
    let url = reqwest::Url::parse_with_params("https://geocoding-api.open-meteo.com/v1/search", &[("count", "5"), ("name", query)]).map_err(|e| e.to_string())?;
    let body: Value = reqwest::blocking::get(url).and_then(|r| r.json()).map_err(|e| format!("couldn't look it up: {e}"))?;
    let hits = body.get("results").and_then(Value::as_array).filter(|h| !h.is_empty()).ok_or_else(|| format!("no place called {query}"))?;
    Ok(hits.iter().map(|hit| {
        // Where it is, to tell same-named places apart; not saved.
        let region = [hit.get("admin1"), hit.get("country")].iter().filter_map(|v| v.and_then(Value::as_str)).collect::<Vec<_>>().join(", ");
        json!({
            "mode": "set",
            "name": hit.get("name"),
            "latitude": hit.get("latitude"),
            "longitude": hit.get("longitude"),
            "timezone": hit.get("timezone"),
            "region": region,
        })
    }).collect())
}

impl Settings {
    fn use_place(&mut self, mut place: Value) {
        let name = place.get("name").and_then(Value::as_str).unwrap_or("that place").to_owned();
        if let Some(p) = place.as_object_mut() {
            p.remove("region");
        }
        self.edit_layout(move |v| v["location"] = place);
        self.place = TextEdit::new("");
        self.found.clear();
        self.place_note = Some((format!("Using {name}."), true));
    }
}

pub fn poll(s: &mut Settings, _t: f64, tx: &mut Tx) -> bool {
    tx.wake_after(Duration::from_secs(2));
    if let Some(found) = s.finding.as_ref().and_then(|rx| rx.try_recv().ok()) {
        s.finding = None;
        match found {
            Ok(mut places) if places.len() == 1 => s.use_place(places.remove(0)),
            Ok(places) => {
                // Several by that name: ask which.
                let label = |p: &Value| {
                    let name = p.get("name").and_then(Value::as_str).unwrap_or("?");
                    match p.get("region").and_then(Value::as_str).filter(|r| !r.is_empty()) {
                        Some(r) => format!("{name} — {r}"),
                        None => name.to_owned(),
                    }
                };
                let options: Vec<(String, String)> = places.iter().enumerate().map(|(i, p)| (i.to_string(), label(p))).collect();
                s.found = places;
                s.place_note = Some(("Which one?".into(), true));
                tx.popup_menu(PopupMenu::picker(options, ""), "place/set", |s: &mut Settings, picked, _| {
                    let place = picked.and_then(|i| i.parse::<usize>().ok()).and_then(|i| s.found.get(i).cloned());
                    match place {
                        Some(p) => s.use_place(p),
                        None => {
                            s.found.clear();
                            s.place_note = None;
                        }
                    }
                });
            }
            Err(e) => s.place_note = Some((format!("Couldn't use that place: {e}"), false)),
        }
        return true;
    }
    if s.finding.is_some() {
        tx.wake_after(Duration::from_millis(200));
    }
    if s.refreshed.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
        s.refresh();
        return true;
    }
    false
}

// Components ----------------------------------------------------------------------------

fn font(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
}

fn label(s: impl Into<String>, size: f32, weight: f32, color: Color) -> El {
    text(s.into()).font(font(size, weight)).color(color)
}

fn glyph(name: &str, size: f32, color: Color) -> El {
    match crate::icons::get(name, std::path::Path::new("/")) {
        Some(i) => icon(i, size).color(color),
        None => el().size(size, size),
    }
}

fn switch(key: &str, on: bool) -> El {
    let (w, h, k) = (40.0, 24.0, 20.0);
    let inset = (h - k) / 2.0;
    el().key(format!("{key}/switch")).size(w, h).radius(h / 2.0).fill(if on { c::accent() } else { c::off() }).motion(SNAPPY)
        .child(el().key(format!("{key}/knob")).size(k, k).absolute(if on { w - k - inset } else { inset }, inset).radius(k / 2.0)
            .fill(Color::WHITE).shadow(1.0, 3.0, Color::BLACK.alpha(0.3)))
}

/// A row: icon, title (and a line under it), and whatever sits at its end.
fn setting(key: &str, glyph_name: &str, title: &str, detail: Option<&str>, end: El) -> El {
    let mut words = col().gap(2.0).grow(1.0).shrink().child(label(title, 14.5, 500.0, c::fg()));
    if let Some(d) = detail {
        words = words.child(label(d, 12.5, 400.0, c::muted()).truncate());
    }
    row().key(key.to_owned()).min_h(ROW_H).w_full().px(16.0).py(10.0).gap(12.0).items_center()
        .child(row().w(22.0).justify_center().child(glyph(glyph_name, 18.0, c::muted())))
        .child(words)
        .child(end)
}

fn toggle(key: &str, glyph_name: &str, title: &str, detail: Option<&str>, on: bool, flip: impl Fn(&mut Settings) + 'static) -> El {
    setting(key, glyph_name, title, detail, switch(key, on)).hover(c::hover()).on_press(flip)
}

/// A value and a chevron; pressing it opens a native menu of choices.
fn choice(key: &'static str, glyph_name: &str, title: &str, options: &'static [(&'static str, &'static str)], current: &str, pick: fn(&mut Settings, &str)) -> El {
    let shown = options.iter().find(|(id, _)| *id == current).map_or(current, |(_, l)| l).to_owned();
    let value_key = format!("{key}/value");
    let current = current.to_owned();
    let end = row().key(value_key.clone()).gap(6.0).items_center()
        .child(label(shown, 14.5, 600.0, c::fg()))
        .child(glyph("chevron-right", 14.0, c::muted()));
    setting(key, glyph_name, title, None, end).hover(c::hover()).on_press_tx(move |_: &mut Settings, tx: &mut Tx| {
        let menu = PopupMenu::picker(options.iter().copied(), &current);
        tx.popup_menu(menu, value_key.as_str(), move |s: &mut Settings, picked, _| {
            if let Some(id) = picked {
                pick(s, id);
            }
        });
    })
}

fn slider(key: &'static str, value: f32, set: fn(&mut Settings, f32, bool)) -> El {
    let (w, h, k, bar) = (170.0, 22.0, 18.0, 4.0);
    let travel = w - k;
    let to_value = move |x: f32| ((x - k / 2.0) / travel).clamp(0.0, 1.0);
    el().key(format!("{key}/slider")).size(w, h).motion(SNAPPY)
        .on_drag(move |s: &mut Settings, d: &mut Drag, tx| match d.phase {
            Phase::Down => set(s, to_value(d.x), false),
            Phase::Move => {
                set(s, to_value(d.x), false);
                tx.instant();
            }
            Phase::Up => set(s, to_value(d.x), true),
        })
        .child(el().size(w, bar).absolute(0.0, (h - bar) / 2.0).radius(bar / 2.0).fill(c::off()))
        .child(el().key(format!("{key}/fill")).size(k / 2.0 + value * travel, bar).absolute(0.0, (h - bar) / 2.0).radius(bar / 2.0).fill(c::accent()))
        .child(el().key(format!("{key}/knob")).size(k, k).absolute(value * travel, (h - k) / 2.0).radius(k / 2.0).fill(Color::WHITE).shadow(1.0, 3.0, Color::BLACK.alpha(0.3)))
}

fn card(rows: Vec<El>) -> El {
    let n = rows.len();
    let mut card = col().w_full().radius(12.0).fill(c::card()).border(1.0, c::line()).clip_rounded(12.0);
    for (i, r) in rows.into_iter().enumerate() {
        card = card.child(r);
        if i + 1 < n {
            card = card.child(el().h(1.0).ml(50.0).mr(16.0).fill(c::line()));
        }
    }
    card
}

fn button(key: &str, title: &str, primary: bool, press: impl Fn(&mut Settings) + 'static) -> El {
    row().key(key.to_owned()).px(14.0).h(32.0).radius(8.0).items_center()
        .fill(if primary { c::accent() } else { c::card() }).border(1.0, if primary { c::accent() } else { c::line() })
        .hover(if primary { Color::WHITE.alpha(0.08) } else { c::hover() })
        .on_press(press)
        .child(label(title, 13.5, 600.0, c::fg()))
}

/// A mod's picture at the size the Mods page shows it (22 pt at 2×).
fn picture(path: &std::path::Path) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::open(path).ok()?.resize(44, 44, image::imageops::FilterType::Lanczos3).to_rgba8();
    Some((img.width(), img.height(), img.into_raw()))
}

fn mod_icon(m: &ModInfo) -> El {
    // Mods' tab icons: an app's icon, the mod's own picture, or a built-in glyph.
    if let Some((w, h, rgba)) = &m.picture {
        let px = rune::Pixels::new();
        px.set(*w, *h, rgba.clone());
        return el().size(22.0, 22.0).pixels_fit(&px, rune::SurfaceFit::Fit);
    }
    match m.icon.as_deref() {
        Some(i) if i.starts_with("app:") => {
            let id = &i[4..];
            match crate::platform::app_icon_light(id, 44) {
                Some((w, h, rgba)) => {
                    let px = rune::Pixels::new();
                    px.set(w, h, rgba);
                    el().size(22.0, 22.0).pixels_fit(&px, rune::SurfaceFit::Fit)
                }
                None => glyph("layout-grid", 18.0, c::muted()),
            }
        }
        Some(i) => glyph(i, 18.0, c::muted()),
        None => glyph("layout-grid", 18.0, c::muted()),
    }
}

// Pages ---------------------------------------------------------------------------------

const LOCATION: [(&str, &str); 2] = [("live", "Where this Mac is"), ("set", "A place I choose")];
const HOVER: [(&str, &str); 3] = [("peek", "Peek"), ("open", "Open"), ("none", "Nothing")];
const DISPLAY: [(&str, &str); 3] = [("notch", "The MacBook's"), ("main", "Main display"), ("pointer", "Where the pointer is")];

fn general(s: &Settings) -> El {
    let hover = s.island("hover").and_then(Value::as_str).unwrap_or("peek").to_owned();
    let display = s.island("display").and_then(Value::as_str).unwrap_or("notch").to_owned();
    let hide_full = s.island("hideInFullScreen").and_then(Value::as_bool).unwrap_or(true);
    let glass = s.island("glass").and_then(Value::as_bool).unwrap_or(true);
    let haptics = s.island("haptics").and_then(Value::as_bool).unwrap_or(true);
    let tint = s.island("glassTint").and_then(Value::as_f64).unwrap_or(0.5) as f32;
    let width = s.island("width").and_then(Value::as_f64).unwrap_or(560.0) as f32;
    col().gap(18.0)
        .child(card(vec![
            choice("hover", "mouse", "When you hover the island", &HOVER, &hover, |s, id| s.set_island("hover", Value::from(id))),
            choice("display", "monitor", "Display", &DISPLAY, &display, |s, id| s.set_island("display", Value::from(id))),
            toggle("fullscreen", "eye", "Hide in full screen", Some("Out of the way while an app is full screen on its display"), hide_full, move |s| s.set_island("hideInFullScreen", Value::from(!hide_full))),
            toggle("haptics", "activity", "Haptics", Some("A tap on the trackpad when a swipe opens the island"), haptics, move |s| s.set_island("haptics", Value::from(!haptics))),
            toggle("login", "laptop", "Launch at login", None, s.login, |s| {
                let want = !s.login;
                match nook_core::login_item::set_enabled(want) {
                    Ok(()) => s.login = want,
                    Err(e) => s.note = Some((format!("Launch at login: {e}"), false)),
                }
            }),
        ]))
        .child(location(s))
        .child(label("Look", 13.0, 600.0, c::muted()).ml(4.0))
        .child(card(vec![
            theme_choice(s),
            toggle("glass", "gem", "Liquid Glass", Some("Glass under the open island (off with Reduce Transparency)"), glass, move |s| s.set_island("glass", Value::from(!glass))),
            setting("tint", "droplets", "Black at the top", Some("How far the black reaches down over the glass"), slider("tint", tint, |s, v, _| s.set_island("glassTint", json!((v * 100.0).round() / 100.0)))),
            setting("width", "grip-vertical", "Open width", Some(&format!("{} points", width.round())), slider("width", (width - 460.0) / 300.0, |s, v, _| s.set_island("width", json!((460.0 + v * 300.0).round())))),
        ]))
}

/// The theme layout.json names: the built-in ones and `themes/*.json`. Picking one drops
/// token overrides, as `theme.set {name}` does.
fn theme_choice(s: &Settings) -> El {
    let dir = home::app_dir();
    let current = crate::theme::current_name(s.layout.get("theme").unwrap_or(&Value::Null));
    let options: Vec<(String, String)> = crate::theme::names(&dir).into_iter().map(|n| {
        let mut title = n.replace(['-', '_'], " ");
        if let Some(first) = title.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        (n, title)
    }).collect();
    let shown = options.iter().find(|(id, _)| *id == current).map_or_else(|| current.clone(), |(_, t)| t.clone());
    let end = row().key("theme/value").gap(6.0).items_center()
        .child(label(shown, 14.5, 600.0, c::fg()))
        .child(glyph("chevron-right", 14.0, c::muted()));
    setting("theme", "palette", "Theme", Some("Colors for the island and every mod"), end).hover(c::hover())
        .on_press_tx(move |_: &mut Settings, tx: &mut Tx| {
            let menu = PopupMenu::picker(options.clone(), &current);
            tx.popup_menu(menu, "theme/value", |s: &mut Settings, picked, _| {
                if let Some(name) = picked {
                    let name = name.to_owned();
                    s.edit_layout(move |l| l["theme"] = crate::theme::set(&l["theme"], Some(&name), None));
                }
            });
        })
}

/// Where Weather, the World Clock and the ISS map put you: this Mac's location, or a place
/// typed here.
fn location(s: &Settings) -> El {
    let current = s.layout.get("location");
    let mode = current.and_then(|l| l.get("mode")).and_then(Value::as_str).unwrap_or("live").to_owned();
    let name = current.and_then(|l| l.get("name")).and_then(Value::as_str).map(str::to_owned);
    let mut rows = vec![choice("location", "map-pin", "Location", &LOCATION, &mode, |s, id| s.set_location_mode(id))];
    if mode == "set" {
        let detail = match &name {
            Some(n) => format!("Now: {n}"),
            None => "Type a city, then Set".into(),
        };
        let field = text_input(&s.place, font(14.0, 400.0)).key("place").on_edit(|s: &mut Settings| &mut s.place)
            .on_submit(|s: &mut Settings, _: &mut Tx| s.find_place())
            .placeholder("City", c::faint())
            .color(c::fg()).w(180.0).h(32.0).padding(8.0).radius(8.0).fill(c::card()).border(1.0, c::line());
        rows.push(setting("place", "search", "Place", Some(&detail),
            row().gap(8.0).items_center().child(field).child(button("place/set", "Set", true, |s| s.find_place()))));
    }
    let mut out = col().gap(8.0).child(card(rows));
    if let Some((msg, ok)) = &s.place_note {
        out = out.child(label(msg, 12.5, 500.0, if *ok { c::muted() } else { c::red() }).ml(4.0));
    }
    out
}

/// One permission: what it's for, where it stands, and a way to grant it.
fn permission(key: &str, glyph_name: &str, title: &str, why: &str, access: Option<Access>, ask: Option<fn()>, pane: &'static str) -> El {
    let (state, color) = match access {
        Some(Access::Allowed) => ("Allowed", c::green()),
        Some(Access::Denied) => ("Not allowed", c::red()),
        Some(Access::NotAsked) => ("Not asked yet", c::muted()),
        None => ("", c::muted()),
    };
    let action = match (access, ask) {
        (Some(Access::NotAsked), Some(ask)) => button(&format!("{key}/ask"), "Allow…", true, move |_| ask()),
        _ => button(&format!("{key}/open"), "Open Settings", false, move |_| { let _ = open::that_detached(pane); }),
    };
    setting(key, glyph_name, title, Some(why), row().gap(12.0).items_center().child(label(state, 13.0, 500.0, color)).child(action))
}

fn permissions(s: &Settings) -> El {
    // A development build can't show macOS's prompt (its terminal has no reason to give), so
    // it only links to System Settings.
    let dev = s.asked_for.is_some();
    let rows = vec![
        permission("perm/audio", "volume-2", "System Audio Recording", "Moves the music meter with what's playing. Only loudness is read; nothing is recorded.",
            Some(s.audio_access), (!dev).then_some(nook_core::permissions::request_audio_capture as fn()), nook_core::permissions::AUDIO_CAPTURE_PANE),
        permission("perm/location", "map-pin", "Location", "Weather, the World Clock and the ISS map where you are, unless you set a place in General.",
            Some(s.location_access), (!dev).then_some(ask_location as fn()), nook_core::permissions::LOCATION_PANE),
        permission("perm/automation", "zap", "Automation", "Controls Music and Spotify when the media keys can't reach them.",
            None, None, nook_core::permissions::AUTOMATION_PANE),
    ];
    let mut page = col().gap(14.0).child(card(rows));
    if let Some(app) = &s.asked_for {
        page = page.child(label(
            format!("This is a development build, started by {app}, so macOS gives it {app}'s permissions. To try the meter or Location here, allow {app} in System Settings (System Audio Recording is under Screen & System Audio Recording). openNook.app asks for itself."),
            12.5, 400.0, c::muted()).max_lines(4).ml(4.0));
    }
    page
}

/// CoreLocation shows its prompt for the first request.
fn ask_location() {
    drop(nook_core::location::begin_request());
}

fn mods(s: &Settings) -> El {
    let rows: Vec<El> = s.mods.iter().map(|m| {
        let name = m.name.clone();
        let shown = !s.hidden(&m.name);
        let detail = if m.error { Some(format!("Has a problem: mods/{}.error", m.name)) } else { m.description.clone() };
        let mut words = col().gap(2.0).grow(1.0).shrink().child(label(&m.title, 14.5, 500.0, c::fg()));
        if let Some(d) = detail {
            words = words.child(label(d, 12.5, 400.0, if m.error { c::red() } else { c::muted() }).truncate());
        }
        row().key(format!("mod/{}", m.name)).min_h(ROW_H).w_full().px(16.0).py(10.0).gap(12.0).items_center()
            .hover(c::hover())
            .on_press(move |s: &mut Settings| s.toggle_hidden(&name))
            .child(row().w(22.0).justify_center().child(mod_icon(m)))
            .child(words)
            .child(switch(&format!("mod/{}", m.name), shown))
    }).collect();
    let mut page = col().gap(18.0);
    page = if rows.is_empty() { page.child(label("No mods yet.", 14.0, 400.0, c::muted())) } else { page.child(card(rows)) };
    page.child(row().gap(10.0)
        .child(button("mods/folder", "Open Mods Folder", false, |_| { let _ = open::that_detached(home::app_dir().join("mods")); }))
        .child(button("mods/guide", "Open AGENTS.md", false, |_| { let _ = open::that_detached(home::app_dir().join("AGENTS.md")); })))
}

fn yours(s: &Settings) -> El {
    let agent_name = s.agents.iter().find(|(c, _)| *c == s.agent).map_or("your agent", |a| a.1);
    let mut col_ = col().gap(16.0)
        .child(label("openNook is made of mods, small scripts your coding agent writes. Say what you want in your notch and hand it over.", 14.0, 400.0, c::muted()).max_lines(3))
        .child(text_input(&s.wish, font(14.0, 400.0)).key("wish").on_edit(|s: &mut Settings| &mut s.wish)
            .placeholder("e.g. My open GitHub PRs, and a pomodoro timer", c::faint())
            .color(c::fg()).w_full().h(96.0).padding(12.0).radius(10.0).fill(c::card()).border(1.0, c::line()));
    let mut actions = row().gap(10.0).items_center();
    if s.herdr && !s.agents.is_empty() {
        actions = actions.child(button("start", &format!("Start with {agent_name} in Herdr"), true, |s| s.start_in_herdr()));
    }
    actions = actions.child(button("copy", "Copy prompt", !s.herdr, |s| {
        let prompt = s.prompt();
        let copied = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn().and_then(|mut c| {
            use std::io::Write;
            c.stdin.take().unwrap().write_all(prompt.as_bytes())?;
            c.wait().map(|_| ())
        });
        s.note = Some(match copied {
            Ok(()) => ("Copied. Paste it into any coding agent.".into(), true),
            Err(e) => (format!("Couldn't copy: {e}"), false),
        });
    }));
    if s.agents.len() > 1 {
        let options: Vec<(&'static str, &'static str)> = s.agents.clone();
        let current = s.agent;
        actions = actions.child(row().key("agent").gap(4.0).px(10.0).h(32.0).radius(8.0).items_center().hover(c::hover())
            .child(label(agent_name, 13.5, 500.0, c::muted()))
            .child(glyph("chevron-right", 12.0, c::muted()))
            .on_press_tx(move |_: &mut Settings, tx: &mut Tx| {
                let menu = PopupMenu::picker(options.clone(), current);
                tx.popup_menu(menu, "agent", |s: &mut Settings, picked, _| {
                    if let Some(a) = picked.and_then(|id| s.agents.iter().find(|(c, _)| *c == id)) {
                        s.agent = a.0;
                    }
                });
            }));
    }
    col_ = col_.child(actions);
    if let Some((msg, ok)) = &s.note {
        col_ = col_.child(label(msg, 13.0, 500.0, if *ok { c::green() } else { c::red() }));
    }
    col_.child(card(vec![setting("setup", "terminal", "Set up my agents", Some("Installs the openNook skill for Claude Code and puts `opennook` on your PATH"),
        button("setup/go", "Set up", false, |s| {
            s.note = Some(match set_up_agents() { Ok(m) => (m, true), Err(e) => (format!("Setup failed: {e}"), false) });
        }))]))
}

fn about(_: &Settings) -> El {
    col().gap(18.0).child(card(vec![
        setting("version", "info", "openNook", Some(&format!("Version {}", env!("CARGO_PKG_VERSION"))), el()),
        setting("repo", "github", "Source", Some("github.com/prodBirdy/openNook"),
            button("repo/open", "Open", false, |_| { let _ = open::that_detached("https://github.com/prodBirdy/openNook"); })),
        setting("home", "folder", "Your openNook", Some("~/.config/opennook"),
            button("home/open", "Show", false, |_| { let _ = open::that_detached(home::app_dir()); })),
    ]))
}

fn sidebar(s: &Settings) -> El {
    let mut bar = col().key("sidebar").w(SIDEBAR).h_full().fill(c::sidebar()).pt(48.0).px(10.0).gap(2.0);
    for (page, title, glyph_name) in PAGES {
        let selected = s.page == page;
        let mut item = row().key(format!("nav/{title}")).h(36.0).w_full().px(10.0).gap(10.0).radius(8.0).items_center()
            .hover(c::hover())
            .on_press(move |s: &mut Settings| s.page = page)
            .child(glyph(glyph_name, 16.0, if selected { c::fg() } else { c::muted() }))
            .child(label(title, 14.5, 500.0, if selected { c::fg() } else { c::muted() }));
        if selected {
            item = item.fill(c::selected());
        }
        bar = bar.child(item);
    }
    bar.child(el().grow(1.0)).child(label(format!("Version {}", env!("CARGO_PKG_VERSION")), 12.0, 400.0, c::faint()).px(10.0).pb(16.0))
}

fn content(title: &str, body: El) -> El {
    // Shrinks below its content's width: long descriptions truncate instead of pushing the
    // panel past the window.
    col().grow(1.0).shrink().min_w(0.0).h_full().padding(8.0).child(
        col().key("panel").w_full().min_w(0.0).h_full().radius(14.0).fill(c::panel()).border(1.0, c::line()).px(28.0).pt(22.0).pb(24.0).gap(18.0)
            .child(label(title, 17.0, 600.0, c::fg()))
            // The page scrolls under its title once it outgrows the window.
            .child(col().key("scroll").w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).scroll_edge_fade(18.0)
                .child(body.key(format!("page/{title}")).pb(8.0).transition(rune::Transition::opacity()))),
    )
}

fn onboarding(s: &Settings, step: u8) -> El {
    let body = if step == 0 {
        col().gap(14.0).items_center().w(460.0)
            .child(label("Welcome to openNook", 26.0, 700.0, c::fg()))
            .child(label("Your notch, made of mods. Hover the island to peek, click or swipe down with two fingers to open it, and swipe sideways to switch pages.", 15.0, 400.0, c::muted()).max_lines(4).align(rune::Align::Center))
            .child(el().h(12.0))
            .child(button("next", "Continue", true, |s| s.onboarding = Some(1)))
    } else {
        col().gap(14.0).w(560.0)
            .child(label("Make it yours", 26.0, 700.0, c::fg()))
            .child(yours(s))
            .child(row().w_full().justify_end().pt(8.0).child(button("done", "Done", true, |s| s.finish_onboarding())))
    };
    col().key("onboarding").w_full().h_full().fill(c::panel()).items_center().justify_center().child(body.key(format!("step/{step}")).transition(rune::Transition::opacity()))
}

pub fn view(s: &Settings, _: &mut Cx<Settings>) -> El {
    if let Some(step) = s.onboarding {
        return onboarding(s, step);
    }
    let (title, body) = match s.page {
        Page::General => ("General", general(s)),
        Page::Mods => ("Mods", mods(s)),
        Page::Permissions => ("Permissions", permissions(s)),
        Page::Yours => ("Make it yours", yours(s)),
        Page::About => ("About", about(s)),
    };
    row().key("settings").w_full().h_full().fill(c::sidebar()).child(sidebar(s)).child(content(title, body))
}
