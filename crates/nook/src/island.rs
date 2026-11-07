//! The island: closed around the camera, showing the leading mod's activity, or open with the
//! mods' pages. Every part of what it shows comes from mods; this owns the shape, the states and
//! the motion between them, as 1.x drew them: a flat top that flows into the screen edge through
//! concave wings, a hover peek, a snappy morph, content that slides and blurs in, and Liquid
//! Glass under a black veil when open.

use crate::gesture::{self, Action, Swipe};
use crate::home::{DisplayMode, HoverMode, Layout};
use crate::images::Images;
use crate::render::{self, Rcx};
use crate::theme::Theme;
use nook_mods::{IslandStyle, Mods, Output};
use rune::tok::TextStyle;
use rune::{Color, Cx, El, PopupItem, PopupMenu, Spring, SpringPair, SystemMaterial, Transition, Tx, WheelEvent, col, el, icon, row, spacer, text};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The overlay is a fixed canvas the island moves inside: resizing a window every frame of a
/// spring would drop frames. Room below the open island for its shadow.
pub const CANVAS_H: f32 = 460.0;
const SHADOW_ROOM: f32 = 48.0;

/// Size morph: 1.x's snappy 0.30 s spring (damping ≈ 0.65, a touch of overshoot).
const MORPH: Spring = Spring::new(0.30, 0.3);
/// Content swaps: no bounce, an overshooting fade only clips.
const CROSSFADE: Spring = Spring::new(0.25, 0.0);
/// How far content travels as it swaps: down into an opening island, sideways between pages.
const SLIDE_Y: f32 = 10.0;
const SLIDE_X: f32 = 14.0;
/// Motion blur on swapping content, in points (1.x smeared up to 7 px along the travel).
const SWAP_BLUR: f32 = 3.0;

/// The concave fillets joining the island's flat top to the screen edge: their size by default
/// and at most (the canvas leaves room for the largest, so a style change never resizes it).
const WING: f32 = 14.0;
const MAX_WING: f32 = 24.0;
const COMPACT_RADIUS: f32 = 14.0;
const EXPANDED_RADIUS: f32 = 36.0;
const MAX_EXPANDED_RADIUS: f32 = 60.0;
/// Hover peek: the closed island leans out this much, and grows a chin under the camera.
const PEEK_EXTRA: f32 = 88.0;
const PEEK_CHIN: f32 = 11.0;
const PEEK_DETAIL_CHIN: f32 = 24.0;

/// Liquid Glass: the black band kept at the top (the camera's), at the default tint, and the
/// rim's light at the bottom edge.
const VEIL_BAND: f32 = 48.0;
const VEIL_HOLD_CAP: f32 = 0.82;
const RIM_ALPHA: f32 = 0.38;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Notch {
    pub w: f32,
    pub h: f32,
    pub has: bool,
    pub screen: (f32, f32),
}

impl Notch {
    /// The camera housing of the display `mode` picks.
    pub fn detect(mode: DisplayMode) -> Self {
        let all = crate::platform::displays();
        match pick(mode, &all, crate::platform::pointer()) {
            Some(d) => Self::of(d),
            None => {
                let info = nook_core::notch::get_notch_info();
                let (w, h) = if info.has_notch { (info.notch_width as f32, info.notch_height as f32) } else { (200.0, 32.0) };
                Self { w, h, has: info.has_notch, screen: (info.screen_width as f32, info.screen_height as f32) }
            }
        }
    }

    pub fn of(d: &crate::platform::Display) -> Self {
        match d.notch {
            Some((w, h)) => Self { w: w as f32, h: h as f32, has: true, screen: (d.w as f32, d.h as f32) },
            // Without a camera housing, a virtual one the size of a MacBook's.
            None => Self { w: (d.w * 0.1).clamp(180.0, 220.0) as f32, h: 32.0, has: false, screen: (d.w as f32, d.h as f32) },
        }
    }
}

/// The display the island goes on: by mode, falling back to the main display.
pub fn pick(mode: DisplayMode, all: &[crate::platform::Display], pointer: (f64, f64)) -> Option<&crate::platform::Display> {
    let main = all.iter().find(|d| d.main).or(all.first());
    match mode {
        DisplayMode::Notch => all.iter().find(|d| d.notch.is_some()).or_else(|| all.iter().find(|d| d.builtin)).or(main),
        DisplayMode::Main => main,
        DisplayMode::Pointer => all.iter()
            .find(|d| pointer.0 >= d.x && pointer.0 < d.x + d.w && pointer.1 >= d.y && pointer.1 <= d.y + d.h)
            .or(main),
    }
}

/// What the display watcher needs from the layout, read off the UI thread.
pub static DISPLAY_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
pub static HIDE_IN_FULL_SCREEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn display_mode() -> DisplayMode {
    match DISPLAY_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => DisplayMode::Main,
        2 => DisplayMode::Pointer,
        _ => DisplayMode::Notch,
    }
}

fn share_options(o: &crate::home::IslandOptions) {
    use std::sync::atomic::Ordering;
    let mode = match o.display {
        DisplayMode::Notch => 0,
        DisplayMode::Main => 1,
        DisplayMode::Pointer => 2,
    };
    DISPLAY_MODE.store(mode, Ordering::Relaxed);
    HIDE_IN_FULL_SCREEN.store(o.hide_in_full_screen, Ordering::Relaxed);
}

pub struct Island {
    pub mods: Option<Mods>,
    pub outputs: BTreeMap<String, Output>,
    pub layout: Layout,
    pub theme: Theme,
    pub images: Option<Images>,
    pub notch: Notch,
    pub expanded: bool,
    pub selected: Option<String>,
    pub hover: bool,
    hover_at: Instant,
    /// When the pointer last left; None until it has been over the island.
    left_at: Option<Instant>,
    peek_until: Option<Instant>,
    /// Which way swapped content travels: -1 from the left, 1 from the right.
    page_dir: f32,
    active_dir: f32,
    last_active: Option<String>,
    /// A mod chosen for the closed island by swiping sideways, while it has something to show.
    compact_choice: Option<String>,
    swipe: Swipe,
    /// System Reduce Transparency: no glass, as the HIG asks.
    pub reduce_transparency: bool,
    /// Shown once in the open island (edited shipped mods set aside, a broken layout).
    pub notes: Vec<String>,
    pub quit: bool,
    /// Opens the settings window (set once it exists).
    pub open_settings: Option<Box<dyn Fn()>>,
}

impl Island {
    pub fn new(notch: Notch, layout: Layout) -> Self {
        share_options(&layout.island);
        let (theme, problem) = Theme::resolve(&layout.theme, &crate::home::app_dir());
        Self {
            mods: None,
            outputs: BTreeMap::new(),
            layout,
            theme,
            images: None,
            notch,
            expanded: false,
            selected: None,
            hover: false,
            hover_at: Instant::now(),
            left_at: None,
            peek_until: None,
            page_dir: 1.0,
            active_dir: 1.0,
            last_active: None,
            compact_choice: None,
            swipe: Swipe::default(),
            reduce_transparency: crate::platform::reduce_transparency(),
            notes: problem.into_iter().collect(),
            quit: false,
            open_settings: None,
        }
    }

    pub fn canvas_w(&self) -> f32 {
        self.open_w() + 2.0 * (SHADOW_ROOM + MAX_WING)
    }

    fn open_w(&self) -> f32 {
        self.layout.island.width.max(self.notch.w + 200.0)
    }

    /// Content width of an open page.
    pub fn page_w(&self) -> f32 {
        self.open_w() - 32.0
    }

    fn glass(&self) -> bool {
        self.layout.island.glass && !self.reduce_transparency
    }

    fn hidden(&self, name: &str) -> bool {
        self.layout.hidden.iter().any(|h| h == name)
    }

    fn compact_rank(&self, name: &str) -> usize {
        self.layout.compact.iter().position(|n| n == name).unwrap_or(usize::MAX)
    }

    /// The mod that owns the closed island: the first listed in layout.compact with an activity,
    /// then by priority.
    pub fn active(&self) -> Option<&Output> {
        let candidates = self.candidates();
        self.compact_choice.as_deref().and_then(|c| candidates.iter().copied().find(|o| o.name == c))
            .or_else(|| candidates.first().copied())
    }

    /// Mods with something to show closed, best first.
    fn candidates(&self) -> Vec<&Output> {
        let mut c: Vec<&Output> = self.outputs.values().filter(|o| o.activity.is_some() && !self.hidden(&o.name)).collect();
        c.sort_by(|a, b| {
            self.compact_rank(&a.name).cmp(&self.compact_rank(&b.name))
                .then_with(|| b.activity.as_ref().unwrap().priority.total_cmp(&a.activity.as_ref().unwrap().priority))
                .then_with(|| a.name.cmp(&b.name))
        });
        c
    }

    /// A sideways swipe on the closed island: the next mod with something to show.
    fn cycle_compact(&mut self, forward: bool) -> bool {
        let names: Vec<String> = self.candidates().iter().map(|o| o.name.clone()).collect();
        if names.len() < 2 {
            return false;
        }
        let at = self.active().and_then(|a| names.iter().position(|n| *n == a.name)).unwrap_or(0);
        let next = if forward { (at + 1) % names.len() } else { (at + names.len() - 1) % names.len() };
        self.compact_choice = Some(names[next].clone());
        self.track_active();
        self.active_dir = if forward { 1.0 } else { -1.0 };
        self.publish();
        true
    }

    /// A sideways swipe on the open island: the next page.
    fn cycle_page(&mut self, forward: bool) -> bool {
        let names: Vec<String> = self.pages().iter().map(|o| o.name.clone()).collect();
        if names.len() < 2 {
            return false;
        }
        let at = self.page().and_then(|p| names.iter().position(|n| *n == p.name)).unwrap_or(0);
        let next = if forward { (at + 1) % names.len() } else { (at + names.len() - 1) % names.len() };
        self.select(&names[next]);
        self.page_dir = if forward { 1.0 } else { -1.0 };
        true
    }

    fn wheel(&mut self, e: WheelEvent, tx: &mut Tx) -> bool {
        self.scroll(e.dx, e.dy, e.phase, e.momentum, tx)
    }

    /// A two-finger swipe step, from Rune (headless) or the panel's event monitor (live).
    pub fn scroll(&mut self, dx: f32, dy: f32, phase: rune::GesturePhase, momentum: bool, tx: &mut Tx) -> bool {
        use nook_core::haptics::{HapticConfig, HapticPattern, trigger};
        // Momentum after the fingers lift reports as a new swipe; it isn't one.
        if momentum {
            return true;
        }
        let action = self.swipe.wheel(dx, dy, phase, self.expanded, Instant::now());
        tx.wake_after(gesture::IDLE);
        // One tap, when it opens: the stretch and the other swipes stay quiet.
        match action {
            Action::Open => {
                self.set_expanded(true);
                if self.layout.island.haptics {
                    trigger(Some(HapticConfig { pattern: HapticPattern::Alignment }));
                }
            }
            Action::Close => self.set_expanded(false),
            Action::Cycle(forward) => {
                if self.expanded {
                    self.cycle_page(forward);
                } else {
                    self.cycle_compact(forward);
                }
            }
            _ => {}
        }
        true
    }

    /// Mods with a page, in layout.expanded order, then by name.
    pub fn pages(&self) -> Vec<&Output> {
        let rank = |o: &Output| self.layout.expanded.iter().position(|n| *n == o.name).unwrap_or(usize::MAX);
        let mut pages: Vec<&Output> = self.outputs.values().filter(|o| o.has_expanded && !self.hidden(&o.name)).collect();
        pages.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name)));
        pages
    }

    fn page(&self) -> Option<&Output> {
        let pages = self.pages();
        self.selected.as_deref().and_then(|s| pages.iter().copied().find(|o| o.name == s))
            .or_else(|| self.active().filter(|a| a.has_expanded))
            .or_else(|| pages.first().copied())
    }

    /// Selects a page; content slides in from the side it sits on.
    pub fn select(&mut self, name: &str) {
        let pages: Vec<String> = self.pages().iter().map(|o| o.name.clone()).collect();
        let from = self.page().and_then(|p| pages.iter().position(|n| *n == p.name));
        let to = pages.iter().position(|n| n == name);
        if let (Some(from), Some(to)) = (from, to) {
            self.page_dir = if to >= from { 1.0 } else { -1.0 };
        }
        self.selected = Some(name.to_owned());
        self.publish();
    }

    /// Notes which way the closed island's owner changed, for its content's slide.
    fn track_active(&mut self) {
        let now = self.active().map(|o| o.name.clone());
        if now != self.last_active {
            let rank = |n: &Option<String>| n.as_deref().map_or(usize::MAX, |n| self.compact_rank(n));
            self.active_dir = if rank(&now) <= rank(&self.last_active) { -1.0 } else { 1.0 };
            self.last_active = now;
        }
    }

    pub fn apply(&mut self, out: Output) {
        self.outputs.insert(out.name.clone(), out);
        self.track_active();
        self.publish();
    }

    pub fn remove(&mut self, name: &str) {
        self.outputs.remove(name);
        self.track_active();
        self.publish();
    }

    /// The island moved to another display (or the display changed shape).
    pub fn set_notch(&mut self, notch: Notch) {
        if self.notch != notch {
            self.notch = notch;
            self.publish();
        }
    }

    pub fn set_layout(&mut self, layout: Layout, raw: Value) {
        share_options(&layout.island);
        let dir = crate::home::app_dir();
        let (theme, problem) = Theme::resolve(&layout.theme, &dir);
        if let Some(p) = problem {
            log::warn!("{p}");
            if !self.notes.contains(&p) {
                self.notes.push(p);
            }
        }
        self.theme = theme;
        if let Some(m) = &self.mods {
            m.publish("layout", raw);
            m.publish("theme", self.theme.to_json());
            m.publish("themes", crate::theme::list(&dir, &layout.theme));
        }
        self.layout = layout;
        self.reduce_transparency = crate::platform::reduce_transparency();
        self.publish();
    }

    /// The look now: the style of the mod that owns what's shown (the closed island's activity,
    /// or the open page) over layout.json's, with the defaults under both.
    pub fn look(&self) -> Look {
        let owner = if self.expanded { self.page() } else { self.active() };
        let base = &self.layout.island.style;
        let style = owner.and_then(|o| o.style.as_ref()).map_or_else(|| base.clone(), |s| s.over(base));
        Look::of(&style, &self.theme, self.notch.h)
    }

    /// The island's state for mods (`island()`).
    pub fn publish(&self) {
        let Some(m) = &self.mods else { return };
        m.publish("island", self.state_json());
    }

    pub fn state_json(&self) -> Value {
        json!({
            "expanded": self.expanded,
            "selected": self.page().map(|o| o.name.clone()),
            "active": self.active().map(|o| o.name.clone()),
            "notch": { "width": self.notch.w, "height": self.notch.h, "hasNotch": self.notch.has },
            "screen": { "width": self.notch.screen.0, "height": self.notch.screen.1 },
            "page": { "width": self.page_w() },
        })
    }

    fn set_expanded(&mut self, open: bool) {
        if self.expanded != open {
            self.expanded = open;
            if !open {
                self.peek_until = None;
                self.left_at = None;
                self.notes.clear();
            }
            self.publish();
        }
    }

    /// `run(command, args)` from a mod, for the island; media and the rest go to services.
    pub fn command(&mut self, from: &str, command: &str, args: &Value, tx: &mut Tx) {
        let arg = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_owned);
        match command {
            "island.expand" => self.set_expanded(true),
            "island.collapse" => self.set_expanded(false),
            "island.toggle" => self.set_expanded(!self.expanded),
            "island.select" => {
                self.select(&arg("mod").unwrap_or_else(|| from.to_owned()));
                self.set_expanded(true);
            }
            "island.peek" => {
                let ms = args.get("ms").and_then(Value::as_u64).unwrap_or(3000).clamp(500, 30_000);
                self.select(&arg("mod").unwrap_or_else(|| from.to_owned()));
                if !self.expanded || self.peek_until.is_some() {
                    self.peek_until = Some(Instant::now() + Duration::from_millis(ms));
                    tx.wake_after(Duration::from_millis(ms));
                }
                self.set_expanded(true);
            }
            "theme.set" => {
                let dir = crate::home::app_dir();
                let name = arg("name");
                if let Some(n) = name.as_deref().filter(|n| !crate::theme::exists(&dir, n)) {
                    refused(from, &format!("theme.set: no theme named {n:?} (themes: {})", crate::theme::names(&dir).join(", ")));
                    return;
                }
                let colors = args.get("colors").and_then(Value::as_object);
                crate::home::edit_layout(|l| {
                    let next = crate::theme::set(&l["theme"], name.as_deref(), colors);
                    match next {
                        Value::Null => {
                            l.as_object_mut().map(|m| m.remove("theme"));
                        }
                        v => l["theme"] = v,
                    }
                });
            }
            "layout.patch" => {
                if let Err(e) = crate::home::patch_layout(args.get("patch").unwrap_or(&Value::Null)) {
                    refused(from, &e);
                }
            }
            "app.quit" => {
                self.quit = true;
                tx.quit();
            }
            _ => {
                if !crate::services::command(command, args) {
                    log::warn!("{from}: unknown command {command}");
                }
            }
        }
    }

    fn hovered(&mut self, entered: bool, tx: &mut Tx) {
        self.hover = entered;
        if entered {
            self.hover_at = Instant::now();
            if self.layout.island.hover == HoverMode::Open {
                tx.wake_after(Duration::from_millis(self.layout.island.hover_delay_ms));
            }
        } else {
            self.left_at = Some(Instant::now());
            tx.wake_after(Duration::from_millis(self.layout.island.collapse_delay_ms));
        }
    }
}

/// A command a mod asked for that can't be done: in the app's log and the mod's, where its
/// author looks.
fn refused(from: &str, why: &str) {
    log::warn!("{from}: {why}");
    nook_mods::files::log(&crate::home::config().logs, from, "error", why);
}

/// The island's look, resolved: colors from the theme, sizes within what the canvas holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fill: Color,
    pub gradient: Option<(Color, Color)>,
    pub border: Option<(f32, Color)>,
    pub glow: Option<(f32, Color)>,
    pub compact_radius: f32,
    pub expanded_radius: f32,
    pub wings: f32,
    pub shadow: bool,
}

impl Look {
    fn of(s: &IslandStyle, t: &Theme, notch_h: f32) -> Self {
        Self {
            fill: t.color(s.fill.as_deref().unwrap_or("island")),
            gradient: s.gradient.as_ref().map(|(top, bottom)| (t.color(top), t.color(bottom))),
            border: s.border.as_ref().filter(|b| b.width > 0.0).map(|b| (b.width.min(6.0), t.color(&b.color))),
            // The glow can't reach past the room left for the open island's shadow.
            glow: s.glow.as_ref().filter(|g| g.radius > 0.0).map(|g| (g.radius.min(SHADOW_ROOM), t.color(&g.color))),
            // A closed island's corners can't be rounder than it is tall.
            compact_radius: s.compact_radius.unwrap_or(COMPACT_RADIUS).clamp(0.0, notch_h),
            expanded_radius: s.expanded_radius.unwrap_or(EXPANDED_RADIUS).clamp(0.0, MAX_EXPANDED_RADIUS),
            wings: s.wings.unwrap_or(WING).clamp(0.0, MAX_WING),
            shadow: s.shadow.unwrap_or(true),
        }
    }

    /// The color along the top edge, where the wings meet the body.
    fn top(&self) -> Color {
        self.gradient.map_or(self.fill, |(top, _)| top)
    }
}

impl Island {
    /// The island's menu (right-click), at `at` in window points.
    pub fn menu(&mut self, at: (f32, f32), tx: &mut Tx) {
        let menu = PopupMenu::new()
            .item(PopupItem::new("settings", "Settings…"))
            .item(PopupItem::new("folder", "Open Mods Folder"))
            .item(PopupItem::new("guide", "Open AGENTS.md"))
            .separator()
            .item(PopupItem::new("quit", "Quit openNook"));
        tx.popup_menu(menu, at, |s: &mut Island, picked, tx| match picked {
            Some("settings") => {
                if let Some(open) = &s.open_settings {
                    open();
                }
            }
            Some("folder") => {
                let _ = open::that_detached(crate::home::app_dir().join("mods"));
            }
            Some("guide") => {
                let _ = open::that_detached(crate::home::app_dir().join("AGENTS.md"));
            }
            Some("quit") => {
                s.quit = true;
                tx.quit();
            }
            _ => {}
        });
    }
}

#[cfg(test)]
mod display_tests {
    use super::*;
    use crate::platform::Display;

    fn d(id: u32, x: f64, notch: bool, main: bool) -> Display {
        Display { id, x, y: 0.0, w: 1800.0, h: 1169.0, notch: notch.then_some((220.0, 38.0)), builtin: notch, main }
    }

    #[test]
    fn each_mode_picks_its_display() {
        // A MacBook beside an external display that holds the menu bar.
        let all = [d(1, 0.0, true, false), d(2, 1800.0, false, true)];
        assert_eq!(pick(DisplayMode::Notch, &all, (0.0, 0.0)).unwrap().id, 1);
        assert_eq!(pick(DisplayMode::Main, &all, (0.0, 0.0)).unwrap().id, 2);
        assert_eq!(pick(DisplayMode::Pointer, &all, (2500.0, 600.0)).unwrap().id, 2);
        assert_eq!(pick(DisplayMode::Pointer, &all, (100.0, 600.0)).unwrap().id, 1);
        // No notch anywhere (an iMac, a closed MacBook): the main display.
        let flat = [d(3, 0.0, false, true)];
        assert_eq!(pick(DisplayMode::Notch, &flat, (0.0, 0.0)).unwrap().id, 3);
        assert!(!Notch::of(&flat[0]).has);
    }
}

pub fn poll(s: &mut Island, _t: f64, _tx: &mut Tx) -> bool {
    let now = Instant::now();
    if s.swipe.stale(now) {
        return true;
    }
    let opts = &s.layout.island;
    if !s.expanded && s.hover && opts.hover == HoverMode::Open && now.duration_since(s.hover_at) >= Duration::from_millis(opts.hover_delay_ms) {
        s.set_expanded(true);
        return true;
    }
    if s.expanded && !s.hover {
        let peeking = s.peek_until.is_some_and(|t| now < t);
        let left = s.left_at.is_some_and(|t| now.duration_since(t) >= Duration::from_millis(opts.collapse_delay_ms));
        if !peeking && (left || s.peek_until.is_some()) {
            s.set_expanded(false);
            return true;
        }
    }
    false
}

fn rcx<'a>(s: &'a Island, o: &'a Output) -> Rcx<'a> {
    Rcx { name: &o.name, dir: &o.dir, theme: &s.theme, images: s.images.as_ref(), mods: s.mods.as_ref() }
}

/// Content swapping in and out: it travels `(dx, dy)` and blurs as it fades, both ways on the
/// crossfade spring.
fn swap(dx: f32, dy: f32) -> Transition {
    let enter = Transition::offset(dx, dy).combined(Transition::blur(SWAP_BLUR));
    let exit = Transition::offset(-dx, -dy).combined(Transition::blur(SWAP_BLUR));
    Transition::asymmetric(enter, exit).springs(SpringPair::new(CROSSFADE, CROSSFADE))
}

/// A concave fillet beside the island's flat top (1.x: `M0 0 h14 v14 C14 6.27 7.73 0 0 0Z` on
/// the left, mirrored on the right): a square, clear inside the quarter circle at its outer
/// bottom corner. A plain rect with a gradient, so it rides the island's edge exactly while it
/// springs. Purely paint; it takes no clicks.
fn wing(left: bool, color: Color, size: f32) -> El {
    if size <= 0.0 {
        return el().size(0.0, 0.0);
    }
    let clear = color.alpha(0.0);
    let center = if left { [0.0, 1.0] } else { [1.0, 1.0] };
    // Half a point of feather on the curve.
    let edge = 1.0 - 0.5 / size;
    el().size(size, size).radial_gradient(center, size, [(0.0, clear), (edge, clear), (1.0, color)])
}

pub fn view(s: &Island, cx: &mut Cx<Island>) -> El {
    let look = s.look();
    let glass = s.expanded && s.glass();
    // Without a camera housing there's nothing to blend into: a resting island would be a
    // black block on the menu bar. It stays clear, a hover target, until it has something.
    let ghost = !s.notch.has && !s.expanded && !s.hover && s.swipe.pull == 0.0 && s.active().is_none();
    let fade = |c: Color| if ghost { c.alpha(0.0) } else { c };
    let black = fade(look.fill);
    let top = fade(look.top());
    let body = if s.expanded { expanded(s, &look) } else { compact(s, &look) };
    let radius = if s.expanded { look.expanded_radius } else { look.compact_radius };
    // The island is a row: wing, body, wing. The left wing rides the row's left edge (where
    // children ride) and the right one its trailing edge, so both stay attached through every
    // spring; the body moves on the same spring as the row. Beside the island as siblings, or
    // pinned outside it, they'd move on their own and come apart.
    let mut body = body.key("island/body").motion(MORPH).corners([0.0, 0.0, radius, radius]);
    if s.glass() {
        // The glass is the body's own: its native view follows the body's drawn rect through
        // the morph, under the veil (a separate sheet trailed it on a spring of its own).
        let height = cx.layout_size("island/body").map_or(200.0, |[_, h]| h);
        body = body.fill(black.alpha(0.0)).linear_gradient(std::f32::consts::PI, veil(s, height, black));
        if glass {
            body = body.system_material(SystemMaterial::Glass);
            if look.border.is_none() {
                body = body.border_gradient(1.0, Color::WHITE.alpha(0.0), Color::WHITE.alpha(RIM_ALPHA));
            }
        }
    } else {
        body = body.fill(black);
        if let Some((t, b)) = look.gradient {
            body = body.gradient(fade(t), fade(b));
        }
    }
    let body = body.hit()
        .on_wheel(|s: &mut Island, e, tx| s.wheel(e, tx))
        .on_hover_change(|s: &mut Island, entered, tx| s.hovered(entered, tx))
        .on_press(|s: &mut Island| {
            if !s.expanded {
                s.set_expanded(true);
            }
        })
        .on_secondary_press(|s: &mut Island, press, tx: &mut Tx| s.menu((press.x, press.y), tx));
    // The body clips to its own shape, which would clip its outer shadows too: they go on a
    // clear sheet behind it. Clear, so the glass still shows what's behind the island.
    let mut shade = el().key("island/shade").motion(MORPH).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .corners([0.0, 0.0, radius, radius]).fill(Color::BLACK.alpha(0.0));
    if let Some((r, c)) = look.glow.filter(|_| !ghost) {
        shade = shade.shadow(0.0, r, c);
    }
    if s.expanded && look.shadow {
        shade = shade.shadow(14.0, 40.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 });
    }
    let mut frame = col().key("island/frame").motion(MORPH).child(shade).child(body);
    if let Some((w, c)) = look.border.filter(|_| !ghost) {
        // Over the body, and starting above the screen's edge: the flat top has no line, so
        // the outline runs down the sides and round the bottom.
        frame = frame.child(el().key("island/border").motion(MORPH).pin(Some(0.0), Some(-w), Some(0.0), Some(0.0))
            .corners([0.0, 0.0, radius, radius]).border(w, c));
    }
    let island = row().key("island").motion(MORPH).items_start()
        .child(wing(true, top, look.wings).key("wing/left"))
        .child(frame)
        .child(wing(false, top, look.wings).key("wing/right").ride_trailing());
    col().w_full().h_full().items_center().child(island)
}

/// The veil over the glass, top to bottom: it holds black over the camera band and falls to
/// clear. Closed it is black all the way down, so as the island opens its stops spring apart
/// and the glass appears from the bottom up, on the island's own motion.
fn veil(s: &Island, height: f32, black: Color) -> [(f32, Color); 3] {
    if !s.expanded {
        return [(0.0, black), (0.5, black), (1.0, black)];
    }
    let band = VEIL_BAND * (s.layout.island.glass_tint.clamp(0.0, 1.0) / 0.5);
    let hold = if height > 1.0 { (band / height).clamp(0.0, VEIL_HOLD_CAP) } else { VEIL_HOLD_CAP };
    [(0.0, black), (hold, black), (1.0, black.alpha(0.0))]
}

fn compact(s: &Island, look: &Look) -> El {
    let n = s.notch;
    let peek = s.hover && s.layout.island.hover == HoverMode::Peek;
    let active = s.active();
    let activity = active.and_then(|o| o.activity.as_ref());
    let base_side = activity.map_or(0.0, |a| a.width.unwrap_or(n.h + 10.0).clamp(n.h * 0.8, 160.0));
    let side = base_side + if peek { PEEK_EXTRA / 2.0 } else { 0.0 } + s.swipe.pull_width() / 2.0;
    let detail = activity.and_then(|a| a.detail.as_ref()).filter(|_| peek);
    let chin = if !peek { 0.0 } else if detail.is_some() { PEEK_DETAIL_CHIN } else { PEEK_CHIN };
    let mut island = col().size(n.w + 2.0 * side, n.h + chin + s.swipe.pull).corners([0.0, 0.0, look.compact_radius, look.compact_radius]).clip_rounded(0.0);
    let (Some(active), Some(activity)) = (active, activity) else {
        // Idle: exactly the camera housing, so it disappears into it (until hovered).
        return island;
    };
    let r = rcx(s, active);
    let swap_in = swap(SLIDE_X * s.active_dir, 0.0);
    let cell = |content: Option<&nook_mods::Node>, key: &str, end: bool| {
        let c = row().key(format!("compact/{key}/{}", active.name)).w(side).h(n.h).items_center()
            .pl(if end { 4.0 } else { 12.0 }).pr(if end { 12.0 } else { 4.0 })
            .transition(swap_in);
        let c = if end { c.justify_end() } else { c.justify_start() };
        match content {
            Some(node) => c.child(render::node(node, &r)),
            None => c,
        }
    };
    island = island.child(
        row().h(n.h).w_full()
            .child(cell(activity.leading.as_ref(), "leading", false))
            .child(el().w(n.w).h_full())
            .child(cell(activity.trailing.as_ref(), "trailing", true)),
    );
    if let Some(d) = detail {
        island = island.child(
            row().key(format!("compact/detail/{}", active.name)).w_full().h(PEEK_DETAIL_CHIN).px(16.0).pb(4.0).justify_center().items_center()
                .transition(swap(0.0, -4.0))
                .child(render::node(d, &r)),
        );
    }
    island
}

fn tab(s: &Island, o: &Output, selected: bool) -> El {
    let name = o.name.clone();
    // The mod's icon for now, else mod.json's (an app that isn't installed has none).
    let drawable = |i: &&str| match i.strip_prefix("app:") {
        Some(bundle) => crate::platform::app_path(bundle).is_some(),
        None if crate::icons::is_picture(i) => o.dir.join(i).is_file(),
        None => crate::icons::get(i, &o.dir).is_some(),
    };
    let chosen = o.icon.as_deref().filter(drawable).or(o.manifest_icon.as_deref().filter(drawable));
    let glyph = chosen.and_then(|i| crate::icons::get(i, &o.dir));
    let mut t = row().key(format!("tab/{}", o.name)).size(28.0, 24.0).radius(7.0).items_center().justify_center()
        .hover(s.theme.color("raised"))
        .on_press(move |s: &mut Island| s.select(&name));
    if selected {
        t = t.fill(s.theme.color("surface"));
    }
    let color = s.theme.color(if selected { "fg" } else { "muted" });
    // "app:<bundle id>" is that app's own icon, in colour; so is a mod's own picture.
    let app = chosen.filter(|i| i.starts_with("app:") || crate::icons::is_picture(i)).and_then(|i| s.images.as_ref()?.get(i, &o.dir, 16.0));
    t = match (app, glyph) {
        (Some(img), _) => t.child(el().size(16.0, 16.0).pixels_fit(img.pixels(), rune::SurfaceFit::Fit).opacity(if selected { 1.0 } else { 0.7 })),
        (None, Some(g)) => t.child(icon(g, 15.0).color(color)),
        (None, None) => t.child(text(o.title.chars().next().unwrap_or('?').to_string()).font(TextStyle::new(12.0, 600.0)).color(color)),
    };
    if o.error.is_some() {
        t = t.child(el().size(5.0, 5.0).radius(3.0).fill(s.theme.color("red")).pin(None, Some(2.0), Some(2.0), None));
    }
    t
}

/// The gear at the top bar's end: Settings, one click from the open island.
fn settings_button(s: &Island) -> El {
    let gear = crate::icons::get("settings", std::path::Path::new("/"));
    let mut b = row().key("topbar/settings").size(28.0, 24.0).radius(7.0).items_center().justify_center()
        .hover(s.theme.color("raised"))
        .cursor(rune::Cursor::Pointer)
        .on_press(|s: &mut Island| {
            s.set_expanded(false);
            if let Some(open) = &s.open_settings {
                open();
            }
        });
    if let Some(g) = gear {
        b = b.child(icon(g, 15.0).color(s.theme.color("muted")));
    }
    b
}

fn expanded(s: &Island, look: &Look) -> El {
    let n = s.notch;
    let w = s.open_w();
    let pages = s.pages();
    let page = s.page();
    let shoulder = (w - n.w) / 2.0;
    let tabs = pages.iter().fold(row().gap(4.0).pl(14.0), |r, o| r.child(tab(s, o, page.is_some_and(|p| p.name == o.name))));
    let title = page.map(|p| p.title.clone()).unwrap_or_default();
    // The tab bar sits still while pages swap under it; it arrives with the opening island.
    let top = row().key("topbar").h(n.h).w_full().transition(swap(0.0, -SLIDE_Y))
        .child(row().w(shoulder).h_full().items_center().child(tabs))
        .child(el().w(n.w).h_full())
        .child(row().w(shoulder).h_full().items_center().justify_end().pr(12.0).gap(6.0)
            .child(text(title).font(TextStyle::new(12.0, 600.0)).color(s.theme.color("muted")).truncate().shrink())
            .child(settings_button(s)));
    let body = match page {
        Some(p) => {
            let content = match &p.expanded {
                Some(node) => render::node(node, &rcx(s, p)),
                None => col(),
            };
            let mut body = col().key(format!("page/{}", p.name)).w_full().transition(swap(SLIDE_X * s.page_dir, 0.0)).child(content);
            if let Some(e) = &p.error {
                body = body.child(problem(s, &p.name, e));
            }
            body
        }
        None => empty(s),
    };
    let mut island = col().w(w).corners([0.0, 0.0, look.expanded_radius, look.expanded_radius]).pb(16.0)
        .clip_rounded(look.expanded_radius);
    island = island.child(top).child(col().w_full().px(16.0).pt(6.0).child(body));
    for note in &s.notes {
        island = island.child(text(note).font(TextStyle::new(11.0, 400.0)).color(s.theme.color("muted")).px(18.0).pt(8.0));
    }
    island
}

fn problem(s: &Island, name: &str, error: &str) -> El {
    let first = error.lines().next().unwrap_or(error);
    col().key(format!("problem/{name}")).w_full().mt(8.0).padding(10.0).radius(12.0).fill(s.theme.color("red").alpha(0.14)).gap(3.0)
        .child(text(format!("{name} has a problem")).font(TextStyle::new(12.0, 600.0)).color(s.theme.color("red")))
        .child(text(first).font(TextStyle::new(11.0, 400.0).mono()).color(s.theme.color("fg")).max_lines(3))
        .child(text(format!("Details: ~/.config/opennook/mods/{name}.error")).font(TextStyle::new(10.5, 400.0)).color(s.theme.color("muted")))
}

fn empty(s: &Island) -> El {
    col().key("empty").w_full().py(18.0).gap(6.0).items_center()
        .child(text("No mods yet").font(TextStyle::new(14.0, 600.0)).color(s.theme.color("fg")))
        .child(text("Ask your agent to write one: ~/.config/opennook/AGENTS.md").font(TextStyle::new(12.0, 400.0)).color(s.theme.color("muted")))
        .child(spacer())
}
