//! The island: closed around the camera, showing the leading mod's activity, or open with the
//! mods' pages. Every part of what it shows comes from mods; this owns the shape, the states and
//! the motion between them, as 1.x drew them: a flat top that flows into the screen edge through
//! concave wings, a hover peek, a snappy morph, content that slides and blurs in, and Liquid
//! Glass under a black veil when open.

use crate::fields::{Fields, Keys};
use crate::gesture::{self, Action, Swipe};
use crate::home::{DisplayMode, HoverMode, IslandShape, Layout};
use crate::images::Images;
use crate::render::{self, Rcx, Scroller};
use crate::theme::Theme;
use nook_mods::{IslandStyle, Mods, Output};
use rune::tok::TextStyle;
use rune::{Color, Cx, El, PopupItem, PopupMenu, Spring, SpringPair, SystemMaterial, Transition, Tx, WheelEvent, col, el, icon, row, spacer, text};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
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

/// The open island's tab strip, in the shoulder left of the camera: tabs, the space between
/// them, its insets, and the fade over an edge with tabs past it. More tabs than fit scroll.
const TAB_W: f32 = 28.0;
const TAB_GAP: f32 = 4.0;
const TABS_LEFT: f32 = 14.0;
const TABS_RIGHT: f32 = 8.0;
const TABS_FADE: f32 = 18.0;

/// Liquid Glass: the black band kept at the top (the camera's), at the default tint, and the
/// rim's light at the bottom edge.
const VEIL_BAND: f32 = 48.0;
const VEIL_HOLD_CAP: f32 = 0.82;
const RIM_ALPHA: f32 = 0.38;

/// The camera's place on the island's display: a notch the island grows out of, or a cutout
/// it wraps as a pill. `w` and `h` are the closed island's own size around it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Notch {
    pub w: f32,
    pub h: f32,
    pub has: bool,
    pub screen: (f32, f32),
    /// A pill floating `top` points below the screen's edge, rounded all round, in place of a
    /// shape joined to the edge.
    pub pill: bool,
    pub top: f32,
}

/// The PROLED MacBook Pro's camera cutout (Apple's bezel: 59 x 31.5 points, 8.5 below the
/// edge), and the island's margin around it.
const PILL_CUTOUT: (f32, f32) = (59.0, 31.5);
const PILL_CUTOUT_TOP: f32 = 8.5;
const PILL_MARGIN: f32 = 3.0;
/// A cutout narrower than this is a pill's: every notch is 185 points or more.
const PILL_MAX_W: f64 = 120.0;

impl Notch {
    /// The camera housing of the display `mode` picks.
    pub fn detect(mode: DisplayMode) -> Self {
        let all = crate::platform::displays();
        match pick(mode, &all, crate::platform::pointer()) {
            Some(d) => Self::of(d),
            None => {
                let info = nook_core::notch::get_notch_info();
                let screen = (info.screen_width as f32, info.screen_height as f32);
                if shape() == IslandShape::Pill {
                    return Self::pill(info.has_notch, screen);
                }
                let (w, h) = if info.has_notch { (info.notch_width as f32, info.notch_height as f32) } else { (200.0, 32.0) };
                Self { w, h, has: info.has_notch, screen, pill: false, top: 0.0 }
            }
        }
    }

    pub fn of(d: &crate::platform::Display) -> Self {
        Self::shaped(d, shape())
    }

    fn shaped(d: &crate::platform::Display, shape: IslandShape) -> Self {
        let screen = (d.w as f32, d.h as f32);
        let pill = match shape {
            IslandShape::Pill => true,
            IslandShape::Notch => false,
            IslandShape::Auto => d.notch.is_some_and(|(w, _)| w < PILL_MAX_W),
        };
        if pill {
            return Self::pill(d.notch.is_some(), screen);
        }
        match d.notch {
            Some((w, h)) => Self { w: w as f32, h: h as f32, has: true, screen, pill: false, top: 0.0 },
            // Without a camera housing, a virtual one the size of a MacBook's.
            None => Self { w: (d.w * 0.1).clamp(180.0, 220.0) as f32, h: 32.0, has: false, screen, pill: false, top: 0.0 },
        }
    }

    /// The closed island around the PROLED MacBook Pro's cutout.
    fn pill(has: bool, screen: (f32, f32)) -> Self {
        let (w, h) = PILL_CUTOUT;
        Self { w: w + 2.0 * PILL_MARGIN, h: h + 2.0 * PILL_MARGIN, has, screen, pill: true, top: PILL_CUTOUT_TOP - PILL_MARGIN }
    }

    /// The island's corners for `radius`: all four on a pill, the bottom two under a notch.
    pub fn corners(&self, radius: f32) -> [f32; 4] {
        if self.pill { [radius; 4] } else { [0.0, 0.0, radius, radius] }
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
pub static SHAPE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn shape() -> IslandShape {
    match SHAPE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => IslandShape::Notch,
        2 => IslandShape::Pill,
        _ => IslandShape::Auto,
    }
}

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
    SHAPE.store(match o.shape { IslandShape::Auto => 0, IslandShape::Notch => 1, IslandShape::Pill => 2 }, Ordering::Relaxed);
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
    /// The one-time "star it on GitHub" card, after an hour of use (`star`).
    pub star_prompt: bool,
    pub quit: bool,
    /// Opens the settings window (set once it exists).
    pub open_settings: Option<Box<dyn Fn()>>,
    /// Mods' text fields.
    pub fields: Fields,
    /// Takes the keyboard for a focused field, and gives it back to the app it came from (set
    /// once the panel exists; a still from `shot` has neither).
    pub request_keys: Option<Box<dyn Fn()>>,
    pub release_keys: Option<Box<dyn Fn()>>,
    /// Mods' scroll viewports: those the last view drew, where they are and how far each goes,
    /// and how far each is scrolled. The overlay hears no wheel of its own (the panel's monitor
    /// takes every scroll for swipes), so the island scrolls them.
    scrollers_drawn: RefCell<Vec<Scroller>>,
    scroll_geometry: RefCell<BTreeMap<String, ScrollGeometry>>,
    scroll_offsets: RefCell<BTreeMap<String, f32>>,
    /// Who the swipe under way belongs to, decided by its first movement.
    scroll_lock: Option<(ScrollLock, Instant)>,
    /// The body's top-left in window points, for wheel events Rune reports in its own.
    body_origin: Cell<(f32, f32)>,
    /// How far the tab strip is scrolled, and where it was drawn (window points).
    tabs_offset: f32,
    tabs_rect: Cell<Option<[f32; 4]>>,
}

#[derive(Clone, Copy, Debug)]
struct ScrollGeometry {
    /// `[x, y, w, h]` in window points.
    rect: [f32; 4],
    /// How far it scrolls: content less viewport.
    max: f32,
    stick_bottom: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum ScrollLock {
    /// No movement yet (a trackpad's first event often has none).
    Pending,
    Swipe,
    Scroller(String),
    Tabs,
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
            star_prompt: false,
            quit: false,
            open_settings: None,
            fields: Fields::default(),
            request_keys: None,
            release_keys: None,
            scrollers_drawn: RefCell::default(),
            scroll_geometry: RefCell::default(),
            scroll_offsets: RefCell::default(),
            scroll_lock: None,
            body_origin: Cell::new((0.0, 0.0)),
            tabs_offset: 0.0,
            tabs_rect: Cell::new(None),
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
        let (x, y) = self.body_origin.get();
        self.scroll(e.dx, e.dy, e.phase, e.momentum, Some((x + e.x, y + e.y)), tx)
    }

    /// A two-finger swipe step, from Rune (headless) or the panel's event monitor (live); `at`
    /// is the pointer in window points. One that starts mostly vertical over a mod's scroll
    /// viewport scrolls it, momentum included; one that starts mostly sideways over a tab strip
    /// with more tabs than fit scrolls the strip; any other swipes.
    pub fn scroll(&mut self, dx: f32, dy: f32, phase: rune::GesturePhase, momentum: bool, at: Option<(f32, f32)>, tx: &mut Tx) -> bool {
        use nook_core::haptics::{HapticConfig, HapticPattern, trigger};
        let now = Instant::now();
        let stale = self.scroll_lock.as_ref().is_none_or(|(_, t)| now.duration_since(*t) >= gesture::IDLE);
        if !momentum && (phase == rune::GesturePhase::Began || stale) {
            self.scroll_lock = Some((ScrollLock::Pending, now));
        }
        if self.scroll_lock.as_ref().is_some_and(|(l, _)| *l == ScrollLock::Pending) && (dx != 0.0 || dy != 0.0) {
            let lock = match at {
                Some(p) if self.expanded && dy.abs() >= dx.abs() => self.scroller_at(p).map(ScrollLock::Scroller),
                Some(p) if self.expanded && self.over_tabs(p) && self.tabs_max() > 0.5 => Some(ScrollLock::Tabs),
                _ => None,
            };
            self.scroll_lock = Some((lock.unwrap_or(ScrollLock::Swipe), now));
        }
        if let Some((lock, t)) = &mut self.scroll_lock {
            *t = now;
            match lock.clone() {
                ScrollLock::Scroller(key) => {
                    self.scroll_by(&key, dy, tx);
                    return true;
                }
                ScrollLock::Tabs => {
                    // The strip follows the fingers, not a spring behind them.
                    tx.instant();
                    let max = self.tabs_max();
                    self.tabs_offset = (self.tabs_offset.min(max) - dx).clamp(0.0, max);
                    return true;
                }
                _ => {}
            }
        }
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
        self.reveal_tab();
        self.publish();
    }

    /// The tab strip's width: the shoulder left of the camera.
    fn tabs_w(&self) -> f32 {
        (self.open_w() - self.notch.w) / 2.0
    }

    /// How far the tab strip scrolls: its tabs' width less the strip's.
    fn tabs_max(&self) -> f32 {
        let n = self.pages().len() as f32;
        (TABS_LEFT + n * TAB_W + (n - 1.0).max(0.0) * TAB_GAP + TABS_RIGHT - self.tabs_w()).max(0.0)
    }

    fn over_tabs(&self, (x, y): (f32, f32)) -> bool {
        self.tabs_rect.get().is_some_and(|r| x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3])
    }

    /// Scrolls the tab strip just far enough to show the open page's tab clear of the fades.
    fn reveal_tab(&mut self) {
        let Some(at) = self.page().and_then(|p| self.pages().iter().position(|o| o.name == p.name)) else { return };
        let left = TABS_LEFT + at as f32 * (TAB_W + TAB_GAP);
        let (lo, hi) = (left + TAB_W + TABS_FADE - self.tabs_w(), left - TABS_FADE);
        self.tabs_offset = self.tabs_offset.max(lo).min(hi).clamp(0.0, self.tabs_max());
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

    /// The innermost scroll viewport under `p` that has somewhere to go.
    fn scroller_at(&self, (x, y): (f32, f32)) -> Option<String> {
        self.scroll_geometry.borrow().iter()
            .filter(|(_, g)| g.max > 0.5 && x >= g.rect[0] && x <= g.rect[0] + g.rect[2] && y >= g.rect[1] && y <= g.rect[1] + g.rect[3])
            .min_by(|a, b| (a.1.rect[2] * a.1.rect[3]).total_cmp(&(b.1.rect[2] * b.1.rect[3])))
            .map(|(k, _)| k.clone())
    }

    /// Moves viewport `key` by a swipe's `dy` (positive moves the content down).
    fn scroll_by(&mut self, key: &str, dy: f32, tx: &mut Tx) {
        let Some(g) = self.scroll_geometry.borrow().get(key).copied() else { return };
        let mut offsets = self.scroll_offsets.borrow_mut();
        // A viewport not yet scrolled is where Rune opened it: at the bottom when it sticks there.
        let now = offsets.get(key).copied().unwrap_or(if g.stick_bottom { f32::INFINITY } else { 0.0 });
        let next = (now.min(g.max) - dy).clamp(0.0, g.max);
        // At the bottom of a sticking viewport, content that arrives keeps it there: it reads as
        // "at the end", whatever the end grows to.
        offsets.insert(key.to_owned(), if g.stick_bottom && next >= g.max - 0.5 { f32::INFINITY } else { next });
        log::debug!("island: scroll {key} to {next:.0} of {:.0}", g.max);
        tx.scroll_to_instant(key.to_owned(), 0.0, next);
    }

    /// Where the viewports the view just drew are, read from the last layout.
    fn measure_scrollers(&self, cx: &mut Cx<Island>) {
        let drawn = std::mem::take(&mut *self.scrollers_drawn.borrow_mut());
        let mut geometry = BTreeMap::new();
        for s in &drawn {
            let (Some(rect), Some([_, content])) = (cx.layout(&s.key), cx.layout_size(&s.content_key())) else { continue };
            geometry.insert(s.key.clone(), ScrollGeometry { rect, max: (content + s.pad - rect[3]).max(0.0), stick_bottom: s.stick_bottom });
        }
        // A viewport that went is drawn anew (at its top, or bottom) when it comes back.
        self.scroll_offsets.borrow_mut().retain(|k, _| drawn.iter().any(|s| s.key == *k));
        *self.scroll_geometry.borrow_mut() = geometry;
        if let Some([x, y, _, _]) = cx.layout("island/body") {
            self.body_origin.set((x, y));
        }
        self.tabs_rect.set(if self.expanded { cx.layout("topbar/tabs") } else { None });
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
            if open {
                self.reveal_tab();
            } else {
                self.peek_until = None;
                self.left_at = None;
                self.notes.clear();
                if self.fields.blur_all(self.mods.as_ref()) {
                    self.give_keys_back();
                }
            }
            self.publish();
        }
    }

    /// A mod's request about one of its text fields (`Host::input`).
    pub fn input(&mut self, from: &str, id: u64, op: &Value, tx: &mut Tx) {
        match self.fields.apply(from, id, op) {
            // A field takes the keyboard only where it can be seen and typed into.
            Keys::Request if self.expanded => {
                if let Some(ask) = &self.request_keys {
                    ask();
                }
            }
            Keys::Request => {
                self.fields.blur_all(None);
            }
            Keys::Release => {
                tx.blur();
                self.field_left(tx);
            }
            Keys::None => {}
        }
    }

    /// A field gave up the keyboard (Escape, `blur()`): back to the app it came from, and the
    /// island closes as it would have, had the pointer left while someone typed.
    pub fn field_left(&mut self, tx: &mut Tx) {
        self.give_keys_back();
        if self.expanded && !self.hover {
            self.left_at = Some(Instant::now());
            tx.wake_after(Duration::from_millis(self.layout.island.collapse_delay_ms));
        }
    }

    fn give_keys_back(&self) {
        if let Some(release) = &self.release_keys {
            release();
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

    #[test]
    fn a_narrow_cutout_gets_a_pill_and_a_notch_keeps_its_shape() {
        let mut new = d(1, 0.0, true, true);
        new.notch = Some((59.0, 31.5));
        let pill = Notch::shaped(&new, IslandShape::Auto);
        assert!(pill.pill && pill.has);
        // Around the cutout, 3 points to spare: 65 x 37.5, from 5.5 below the edge.
        assert_eq!((pill.w, pill.h, pill.top), (65.0, 37.5, 5.5));
        assert_eq!(pill.corners(10.0), [10.0; 4]);
        let notch = Notch::shaped(&d(2, 0.0, true, true), IslandShape::Auto);
        assert!(!notch.pill && notch.w == 220.0 && notch.top == 0.0);
        assert_eq!(notch.corners(10.0), [0.0, 0.0, 10.0, 10.0]);
        // Asked for, either way round.
        assert!(Notch::shaped(&d(3, 0.0, false, true), IslandShape::Pill).pill);
        assert!(!Notch::shaped(&new, IslandShape::Notch).pill);
    }
}

pub fn poll(s: &mut Island, _t: f64, _tx: &mut Tx) -> bool {
    let now = Instant::now();
    if s.images.as_ref().is_some_and(|i| i.take_arrived()) {
        return true;
    }
    if s.swipe.stale(now) {
        return true;
    }
    let opts = &s.layout.island;
    if !s.expanded && s.hover && opts.hover == HoverMode::Open && now.duration_since(s.hover_at) >= Duration::from_millis(opts.hover_delay_ms) {
        s.set_expanded(true);
        return true;
    }
    // Typing holds the island open, wherever the pointer went.
    if s.expanded && !s.hover && !s.fields.typing() {
        let peeking = s.peek_until.is_some_and(|t| now < t);
        let left = s.left_at.is_some_and(|t| now.duration_since(t) >= Duration::from_millis(opts.collapse_delay_ms));
        if !peeking && (left || s.peek_until.is_some()) {
            s.set_expanded(false);
            return true;
        }
    }
    false
}

/// The panel gained or lost the keyboard. Lost (a click in another app), a field that had it
/// lets go, so the island can close.
pub fn window_focus(s: &mut Island, focused: bool, tx: &mut Tx) {
    if !focused && s.fields.typing() {
        tx.blur();
        s.fields.blur_all(s.mods.as_ref());
        if s.expanded && !s.hover {
            s.left_at = Some(Instant::now());
            tx.wake_after(Duration::from_millis(s.layout.island.collapse_delay_ms));
        }
    }
}

fn rcx<'a>(s: &'a Island, o: &'a Output) -> Rcx<'a> {
    Rcx {
        name: &o.name,
        dir: &o.dir,
        theme: &s.theme,
        images: s.images.as_ref(),
        mods: s.mods.as_ref(),
        fields: Some(&s.fields),
        request_keys: s.request_keys.as_deref(),
        scrollers: Some(&s.scrollers_drawn),
    }
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
    s.scrollers_drawn.borrow_mut().clear();
    let body = if s.expanded { expanded(s, &look) } else { compact(s, &look) };
    s.measure_scrollers(cx);
    let n = s.notch;
    // A pill is round at its ends closed; open, every corner takes the open radius.
    let radius = if s.expanded { look.expanded_radius } else if n.pill { n.h / 2.0 } else { look.compact_radius };
    let corners = n.corners(radius);
    // The island is a row: wing, body, wing. The left wing rides the row's left edge (where
    // children ride) and the right one its trailing edge, so both stay attached through every
    // spring; the body moves on the same spring as the row. Beside the island as siblings, or
    // pinned outside it, they'd move on their own and come apart.
    let mut body = body.key("island/body").motion(MORPH).corners(corners);
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
        .corners(corners).fill(Color::BLACK.alpha(0.0));
    if let Some((r, c)) = look.glow.filter(|_| !ghost) {
        shade = shade.shadow(0.0, r, c);
    }
    if s.expanded && look.shadow {
        shade = shade.shadow(14.0, 40.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 });
    }
    let mut frame = col().key("island/frame").motion(MORPH).child(shade).child(body);
    if let Some((w, c)) = look.border.filter(|_| !ghost) {
        // Over the body, and under a notch starting above the screen's edge: the flat top has
        // no line, so the outline runs down the sides and round the bottom. A pill's goes all
        // the way round.
        let top = if n.pill { 0.0 } else { -w };
        frame = frame.child(el().key("island/border").motion(MORPH).pin(Some(0.0), Some(top), Some(0.0), Some(0.0))
            .corners(corners).border(w, c));
    }
    // A pill floats clear of the screen's edge, so it has no wings to join it there.
    let wings = if n.pill { 0.0 } else { look.wings };
    let island = row().key("island").motion(MORPH).items_start()
        .child(wing(true, top, wings).key("wing/left"))
        .child(frame)
        .child(wing(false, top, wings).key("wing/right").ride_trailing());
    col().w_full().h_full().items_center().pt(n.top).child(island)
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
    let radius = if n.pill { n.h / 2.0 } else { look.compact_radius };
    let mut island = col().size(n.w + 2.0 * side, n.h + chin + s.swipe.pull).corners(n.corners(radius)).clip_rounded(0.0);
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
    let mut t = row().key(format!("tab/{}", o.name)).size(TAB_W, 24.0).radius(7.0).items_center().justify_center()
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
    let shoulder = s.tabs_w();
    // More tabs than fit beside the camera slide under a clip; an edge fades while tabs hide
    // past it. A page's own tabs list may have shrunk since the strip was scrolled.
    let (offset, max) = (s.tabs_offset.min(s.tabs_max()), s.tabs_max());
    let tabs = pages.iter().fold(row().key("topbar/tabs/row").gap(TAB_GAP).pl(TABS_LEFT).pr(TABS_RIGHT).offset(-offset, 0.0), |r, o| {
        r.child(tab(s, o, page.is_some_and(|p| p.name == o.name)))
    });
    let fade = |hidden: bool| if hidden { TABS_FADE } else { 0.0 };
    let strip = row().key("topbar/tabs").w(shoulder).h_full().items_center()
        .clip_fade(fade(offset > 0.5), 0.0, fade(offset < max - 0.5), 0.0)
        .child(tabs);
    let title = page.map(|p| p.title.clone()).unwrap_or_default();
    // The tab bar sits still while pages swap under it; it arrives with the opening island.
    let top = row().key("topbar").h(n.h).w_full().transition(swap(0.0, -SLIDE_Y))
        .child(strip)
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
    let mut island = col().w(w).corners(n.corners(look.expanded_radius)).pb(16.0)
        .clip_rounded(look.expanded_radius);
    island = island.child(top).child(col().w_full().px(16.0).pt(6.0).child(body));
    for note in &s.notes {
        island = island.child(text(note).font(TextStyle::new(11.0, 400.0)).color(s.theme.color("muted")).px(18.0).pt(8.0));
    }
    if s.star_prompt {
        island = island.child(col().w_full().px(16.0).pt(10.0).child(star_card(s)));
    }
    island
}

/// Once, after an hour of use: a star on GitHub, or not. Either answer ends it.
fn star_card(s: &Island) -> El {
    let answer = |star: bool| move |s: &mut Island| {
        s.star_prompt = false;
        crate::star::answered();
        if star {
            let _ = open::that_detached(crate::star::REPO);
        }
    };
    let github = crate::icons::get("github", std::path::Path::new("/"));
    let mut card = row().key("star").w_full().gap(10.0).padding(10.0).radius(12.0).fill(s.theme.color("surface")).items_center()
        .transition(swap(0.0, 6.0));
    if let Some(g) = github {
        card = card.child(icon(g, 18.0).color(s.theme.color("fg")));
    }
    card.child(col().grow(1.0).gap(1.0)
            .child(text("Enjoying openNook?").font(TextStyle::new(12.0, 600.0)).color(s.theme.color("fg")))
            .child(text("A star on GitHub helps others find it.").font(TextStyle::new(11.0, 400.0)).color(s.theme.color("muted")).truncate().shrink()))
        .child(row().key("star/no").px(10.0).py(5.0).radius(999.0).hover(s.theme.color("raised")).cursor(rune::Cursor::Pointer)
            .on_press(answer(false))
            .child(text("Not now").font(TextStyle::new(11.5, 500.0)).color(s.theme.color("muted"))))
        .child(row().key("star/yes").px(12.0).py(5.0).radius(999.0).fill(s.theme.color("fg")).cursor(rune::Cursor::Pointer)
            .on_press(answer(true))
            .child(text("Star on GitHub").font(TextStyle::new(11.5, 600.0)).color(s.theme.color("bg"))))
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

#[cfg(test)]
mod scroll_tests {
    use super::*;
    use rune::{Env, GesturePhase, Runtime};

    /// The open island on a page whose list of 20 rows scrolls in 200 points.
    fn island_with_list(stick_bottom: bool) -> Runtime<Island> {
        let rows: Vec<Value> = (0..20).map(|i| json!({ "t": "box", "k": format!("r{i}"), "s": { "dir": "row", "h": 30.0 } })).collect();
        let page = json!({ "t": "box", "s": { "dir": "col" }, "c": [
            { "t": "box", "k": "list", "s": { "dir": "col", "gap": 4.0, "max_h": 200.0, "scroll": true, "stick_bottom": stick_bottom }, "c": rows },
        ] });
        let notch = Notch { w: 200.0, h: 32.0, has: true, screen: (1512.0, 982.0), pill: false, top: 0.0 };
        let mut island = Island::new(notch, Layout::default());
        island.outputs.insert("t".into(), Output { name: "t".into(), title: "t".into(), has_expanded: true, expanded: Some(serde_json::from_value(page).unwrap()), ..Default::default() });
        island.expanded = true;
        island.select("t");
        let size = (island.canvas_w(), CANVAS_H);
        let mut rt = Runtime::new(island, view, size, Env::default());
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
        rt
    }

    fn row_y(rt: &Runtime<Island>, key: &str) -> f32 {
        rt.presented(key).expect(key).0[1]
    }

    fn swipe(rt: &mut Runtime<Island>, at: (f32, f32), dx: f32, dy: f32) {
        rt.update(|s, tx| {
            s.scroll(dx, dy, GesturePhase::Began, false, Some(at), tx);
            s.scroll(dx, dy, GesturePhase::Moved, false, Some(at), tx);
        });
        for _ in 0..10 {
            rt.frame(1.0 / 60.0);
        }
    }

    #[test]
    fn a_vertical_swipe_over_a_mod_list_scrolls_it_and_stops_at_its_end() {
        let mut rt = island_with_list(false);
        let at = rt.find("m/t/list").expect("list drawn");
        let top = row_y(&rt, "m/t/r0");
        swipe(&mut rt, at, 0.0, -50.0);
        assert!((top - row_y(&rt, "m/t/r0") - 100.0).abs() < 1.0, "scrolled {}", top - row_y(&rt, "m/t/r0"));
        // 20 rows of 30 with 4 between: 676 points, 476 past the viewport, and no further.
        swipe(&mut rt, at, 0.0, -1000.0);
        assert!((top - row_y(&rt, "m/t/r0") - 476.0).abs() < 1.0, "scrolled {}", top - row_y(&rt, "m/t/r0"));
        assert!(rt.state.expanded, "a swipe up over the list doesn't close the island");
    }

    #[test]
    fn a_sideways_swipe_over_a_list_still_changes_nothing_in_it() {
        let mut rt = island_with_list(false);
        let at = rt.find("m/t/list").unwrap();
        let top = row_y(&rt, "m/t/r0");
        swipe(&mut rt, at, -60.0, -5.0);
        assert_eq!(row_y(&rt, "m/t/r0"), top);
    }

    /// The open island with `n` pages, on the first.
    fn island_with_pages(n: usize) -> Runtime<Island> {
        let notch = Notch { w: 200.0, h: 32.0, has: true, screen: (1512.0, 982.0), pill: false, top: 0.0 };
        let mut island = Island::new(notch, Layout::default());
        for i in 0..n {
            let name = format!("p{i:02}");
            let page = json!({ "t": "box", "s": { "dir": "col", "h": 40.0 } });
            island.outputs.insert(name.clone(), Output { name: name.clone(), title: name, has_expanded: true, expanded: Some(serde_json::from_value(page).unwrap()), ..Default::default() });
        }
        island.expanded = true;
        island.select("p00");
        let size = (island.canvas_w(), CANVAS_H);
        let mut rt = Runtime::new(island, view, size, Env::default());
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
        rt
    }

    fn tab_x(rt: &Runtime<Island>, i: usize) -> f32 {
        rt.presented(&format!("tab/p{i:02}")).expect("tab drawn").0[0]
    }

    fn selected(rt: &Runtime<Island>) -> String {
        rt.state.page().unwrap().name.clone()
    }

    #[test]
    fn a_sideways_swipe_over_a_full_tab_strip_scrolls_it_and_keeps_the_page() {
        let mut rt = island_with_pages(12);
        let at = rt.find("topbar/tabs").expect("strip drawn");
        let first = tab_x(&rt, 0);
        swipe(&mut rt, at, -40.0, 2.0);
        assert!((first - tab_x(&rt, 0) - 80.0).abs() < 1.0, "scrolled {}", first - tab_x(&rt, 0));
        assert_eq!(selected(&rt), "p00", "the strip scrolled; the page stayed");
        // 12 tabs: 14 + 12 × 28 + 11 × 4 + 8 = 402 points in a 180-point strip, so 222 and no further.
        swipe(&mut rt, at, -1000.0, 0.0);
        assert!((first - tab_x(&rt, 0) - 222.0).abs() < 1.0, "scrolled {}", first - tab_x(&rt, 0));
        swipe(&mut rt, at, 1000.0, 0.0);
        assert!((tab_x(&rt, 0) - first).abs() < 1.0, "back at the start");
    }

    #[test]
    fn a_sideways_swipe_elsewhere_still_changes_the_page_and_its_tab_comes_into_view() {
        let mut rt = island_with_pages(12);
        let strip = rt.state.tabs_rect.get().expect("strip measured");
        let right = strip[0] + strip[2];
        assert!(tab_x(&rt, 11) > right, "the last tab starts out of sight");
        // Backwards from the first page: the last.
        let body = rt.find("island/body").expect("body drawn");
        swipe(&mut rt, body, -60.0, 0.0);
        assert_eq!(selected(&rt), "p11");
        for _ in 0..40 {
            rt.frame(1.0 / 60.0);
        }
        // At the strip's end: the last tab whole, with no fade over it (nothing hides past it).
        assert!((tab_x(&rt, 11) + TAB_W + TABS_RIGHT - right).abs() < 0.5, "last tab at {} of {right}", tab_x(&rt, 11));
        assert_eq!(rt.state.tabs_offset, rt.state.tabs_max());
    }

    #[test]
    fn a_tab_scrolled_out_of_the_strip_cant_be_clicked() {
        let mut rt = island_with_pages(12);
        let strip = rt.state.tabs_rect.get().unwrap();
        let (x, y) = (tab_x(&rt, 8) + TAB_W / 2.0, strip[1] + strip[3] / 2.0);
        assert!(x > strip[0] + strip[2], "tab 8 lies past the strip, under the camera");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(selected(&rt), "p00");
        // One in view can.
        let x = tab_x(&rt, 2) + TAB_W / 2.0;
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(selected(&rt), "p02");
    }

    #[test]
    fn the_star_card_shows_in_the_open_island_only_when_asked() {
        let mut rt = island_with_pages(2);
        assert!(rt.find("star").is_none());
        rt.update(|s, _| s.star_prompt = true);
        for _ in 0..20 {
            rt.frame(1.0 / 60.0);
        }
        assert!(rt.find("star/yes").is_some() && rt.find("star/no").is_some());
        rt.update(|s, _| s.set_expanded(false));
        for _ in 0..40 {
            rt.frame(1.0 / 60.0);
        }
        assert!(rt.find("star").is_none(), "closed, the island shows no card");
    }

    #[test]
    fn a_pill_floats_below_the_edge_and_wraps_the_camera() {
        let mut island = Island::new(Notch::pill(true, (1512.0, 993.0)), Layout::default());
        let page = json!({ "t": "box", "s": { "dir": "col", "h": 40.0 } });
        island.outputs.insert("p".into(), Output { name: "p".into(), title: "p".into(), has_expanded: true, expanded: Some(serde_json::from_value(page).unwrap()), ..Default::default() });
        let size = (island.canvas_w(), CANVAS_H);
        let mut rt = Runtime::new(island, view, size, Env::default());
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
        let ([_, y, w, h], _) = rt.presented("island/body").expect("body drawn");
        assert_eq!((y, w, h), (5.5, 65.0, 37.5), "closed: the pill around the camera");
        rt.update(|s, _| s.set_expanded(true));
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
        let ([_, y, ..], _) = rt.presented("island/body").expect("body drawn");
        assert_eq!(y, 5.5, "open, it still floats");
    }

    #[test]
    fn a_strip_that_fits_doesnt_take_the_swipe() {
        let mut rt = island_with_pages(3);
        let at = rt.find("topbar/tabs").expect("strip drawn");
        swipe(&mut rt, at, 60.0, 0.0);
        assert_eq!(selected(&rt), "p01", "with nowhere to scroll, a swipe over the tabs changes the page");
    }

    #[test]
    fn a_sticking_list_opens_at_its_end_and_scrolls_back_from_there() {
        let mut rt = island_with_list(true);
        let at = rt.find("m/t/list").unwrap();
        let last = row_y(&rt, "m/t/r19");
        swipe(&mut rt, at, 0.0, 40.0);
        assert!((row_y(&rt, "m/t/r19") - last - 80.0).abs() < 1.0, "moved {}", row_y(&rt, "m/t/r19") - last);
    }
}
