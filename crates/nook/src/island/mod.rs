//! Dynamic Island entity: state, polling, springs, gestures.

mod chrome;
mod compact;
mod expanded;
mod files;
mod marquee;
mod media;
mod render;
mod settings;
pub(crate) mod ui;

pub use render::open_island;

use crate::motion::{self, SpringValue};
use crate::platform;
use crate::theme;
use gpui::{
    prelude::*, px, size, Context, Entity, ExternalPaths, Focusable, MouseDownEvent, Subscription,
    TouchPhase, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions,
};
use nook_core::agents::AgentSession;
use nook_core::calendar::{CalendarEvent, Reminder};
use nook_core::files::FileTrayItem;
use nook_core::models::NowPlayingData;
use nook_core::notch;
use nook_core::observe::{MetricHistory, ObserveSnapshot};
use nook_core::settings::AppSettings;
use nook_core::sysvol::{self, HudEvent, HudKind, HUD_TTL};
use settings::SettingsView;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct HudState {
    pub kind: HudKind,
    pub value: f32,
    pub shown_at: Instant,
    pub gen: u64,
}

impl HudState {
    pub fn display_value(self) -> f32 {
        match self.kind {
            HudKind::Mute => 0.0,
            HudKind::Volume | HudKind::Brightness => sysvol::clamp_unit(self.value),
        }
    }

    pub fn expired(self, now: Instant, dragging: bool) -> bool {
        !dragging && now.duration_since(self.shown_at) >= HUD_TTL
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactMode {
    Idle,
    Media,
    Agents,
    Files,
    Timer,
    Observe,
    Onboard,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Widgets,
    Files,
}

#[derive(Clone)]
pub struct Timer {
    pub id: u64,
    #[allow(dead_code)]
    pub name: String,
    pub remaining: u32,
    pub total: u32,
    pub running: bool,
}

pub struct Island {
    pub notch_width: f32,
    pub notch_height: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    pub hovered: bool,
    pub expanded: bool,
    pub preferred: Option<CompactMode>,
    pub tab: Tab,
    pub now_playing: NowPlayingData,
    pub visualizer_color: Option<gpui::Rgba>,
    pub files: Vec<FileTrayItem>,
    pub events: Vec<CalendarEvent>,
    pub reminders: Vec<Reminder>,
    pub agents: Vec<AgentSession>,
    pub notes: String,
    /// In-card markdown editor for the Notes card, created on first edit.
    pub(crate) notes_editor: Option<Entity<crate::widgets::NotesEditor>>,
    pub(crate) notes_editing: bool,
    notes_sub: Option<Subscription>,
    pub timers: Vec<Timer>,
    pub next_timer_id: u64,
    /// Index into the 7-day week strip (today − 3 … today + 3). 3 is today.
    pub calendar_day: u8,
    /// Preset chips for a new timer, matching the React add dialog.
    pub timer_composer: bool,
    pub observe: ObserveSnapshot,
    observe_history: MetricHistory,
    pub(crate) observe_hover: Option<crate::widgets::ObserveHover>,
    pub settings: AppSettings,
    pub first_run: bool,
    pub speed_mbps: Option<f64>,
    pub speed_progress: f64,
    pub speed_running: bool,
    /// Bumped on start and on Stop so an in-flight test cannot apply after it
    /// was cancelled (Stop then Run would otherwise take the old result).
    pub speed_gen: u64,
    pub last_tick: Instant,
    last_frame: Instant,
    /// Last seen `nook_core::settings::settings_generation()`; the tick loop
    /// only clones the settings struct when this moves.
    settings_gen: u64,
    /// Cursor is within approach distance of the island (from the tick loop).
    /// Render pre-grows the overlay strip on this, so the NSWindow resize
    /// happens while the island is still a static sliver — a resize that
    /// lands mid-animation shows one stretched frame.
    cursor_near: bool,
    pub settings_open: bool,
    settings_window: Option<WindowHandle<SettingsView>>,
    _settings_closed: Option<Subscription>,
    screen_gen: u64,
    /// Island size on `motion::MORPH`.
    anim_w: SpringValue,
    anim_h: SpringValue,
    /// Content crossfade after an expanded/mode/tab swap, 0..1 on
    /// `motion::CROSSFADE`.
    content_fade: SpringValue,
    /// Short context-preserving travel for the incoming content. Expansion
    /// follows the island vertically; compact modes and tabs follow their
    /// horizontal ordering.
    content_x: SpringValue,
    content_y: SpringValue,
    /// Play/pause scrim over the compact album art, 0..1 on `motion::REVEAL`.
    overlay_fade: SpringValue,
    /// Mirrors Accessibility › Display › "Reduce motion"; refreshed by the
    /// poll loop so springs collapse to a dissolve while it is on.
    reduce_motion: bool,
    /// How hard the size spring is moving right now, 0..1. Drives the motion
    /// blur in `content_stack`; exactly 0 once the spring has settled.
    blur: f32,
    last_expanded: bool,
    last_mode: CompactMode,
    last_tab: Tab,
    file_drag: bool,
    pending_file_drag: Option<PendingFileDrag>,
    /// True while a full-screen / zoomed app is covering the display and
    /// Settings asked us to hide.
    suppressed: bool,
    /// Option-drag is moving the island; mouse-up persists the new origin.
    repositioning: bool,
    reposition_grab_x: f32,
    reposition_grab_y: f32,
    /// Mirrors NSWindow.ignoresMouseEvents so we only cross into ObjC on change.
    click_through: bool,
    /// Ticks since the flag above was last pushed to AppKit. The overlay strip
    /// covers the menu bar and the top of every app, so a window left grabbing
    /// events would eat those clicks — too costly to trust a mirrored bool that AppKit or GPUI
    /// can invalidate behind our back (a restyle drops it). Re-asserted about
    /// once a second regardless of whether we think it changed.
    click_through_age: u32,
    /// Ignore extra wheel events from the same two-finger swipe / momentum.
    wheel_locked: bool,
    last_wheel_at: Instant,
    wheel_acc_x: f32,
    wheel_acc_y: f32,
    /// Origin for the working-agent Dot Matrix loader (seconds * speed).
    pixel_origin: Instant,
    pixel_t: f32,
    pub(crate) mirror_on: bool,
    mirror_gen: u64,
    pub(crate) mirror_frame: Option<std::sync::Arc<gpui::RenderImage>>,
    pub(super) hud: Option<HudState>,
    hud_fill: SpringValue,
    hud_dragging: bool,
}

struct PendingFileDrag {
    path: String,
    screen_x: f64,
    screen_y: f64,
}

impl Island {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        nook_core::init();
        let info = notch::get_notch_info();
        let settings = nook_core::settings::get_app_settings();
        let files = nook_core::files::load_file_tray().unwrap_or_default();
        let notes = nook_core::notes::load_notes().unwrap_or_default();
        let first_run = nook_core::settings::is_first_run();
        log::info!(
            "notch {}×{} has_notch={} screen={}×{}",
            info.notch_width,
            info.notch_height,
            info.has_notch,
            info.screen_width,
            info.screen_height
        );

        let mut this = Self {
            notch_width: info.notch_width as f32,
            notch_height: if info.has_notch {
                info.notch_height as f32
            } else {
                32.0
            },
            screen_width: info.screen_width as f32,
            screen_height: info.screen_height as f32,
            hovered: false,
            expanded: false,
            preferred: None,
            tab: Tab::Widgets,
            now_playing: NowPlayingData::default(),
            visualizer_color: None,
            files,
            events: Vec::new(),
            reminders: Vec::new(),
            agents: Vec::new(),
            notes,
            notes_editor: None,
            notes_editing: false,
            notes_sub: None,
            timers: Vec::new(),
            next_timer_id: 1,
            calendar_day: 3,
            timer_composer: false,
            observe: ObserveSnapshot::default(),
            observe_history: nook_core::observe::load_history(),
            observe_hover: None,
            settings,
            first_run,
            speed_mbps: None,
            speed_progress: 0.0,
            speed_running: false,
            speed_gen: 0,
            last_tick: Instant::now(),
            last_frame: Instant::now(),
            settings_gen: nook_core::settings::settings_generation(),
            cursor_near: false,
            settings_open: false,
            settings_window: None,
            _settings_closed: None,
            screen_gen: notch::screen_generation(),
            anim_w: SpringValue::at(0.0),
            anim_h: SpringValue::at(0.0),
            content_fade: SpringValue::at(1.0),
            content_x: SpringValue::at(0.0),
            content_y: SpringValue::at(0.0),
            overlay_fade: SpringValue::at(0.0),
            reduce_motion: platform::reduce_motion(),
            blur: 0.0,
            last_expanded: false,
            last_mode: CompactMode::Idle,
            last_tab: Tab::Widgets,
            file_drag: false,
            pending_file_drag: None,
            suppressed: false,
            repositioning: false,
            reposition_grab_x: 0.0,
            reposition_grab_y: 0.0,
            // NSWindow starts out grabbing events; the first poll tick corrects it.
            click_through: false,
            click_through_age: u32::MAX,
            wheel_locked: false,
            last_wheel_at: Instant::now(),
            wheel_acc_x: 0.0,
            wheel_acc_y: 0.0,
            pixel_origin: Instant::now(),
            pixel_t: 0.0,
            mirror_on: false,
            mirror_gen: 0,
            mirror_frame: None,
            hud: None,
            hud_fill: SpringValue::at(0.0),
            hud_dragging: false,
        };
        // Start at the compact idle size so the first paint isn't a jump.
        let (w, h) = this.target_size();
        this.anim_w.set(w);
        this.anim_h.set(h);

        platform::install_media_observers();
        platform::install_osd_wake_observer();
        nook_core::runtime().spawn(async {
            let _ = nook_core::calendar::request_calendar_access().await;
        });
        // Do not touch GPUI's Window handle here — HasWindowHandle RefCell-panics
        // during construction. Chrome is applied via NSApp window enumeration.
        let _ = window;
        this.spawn_loops(cx);
        Self::spawn_pin(cx);
        this
    }

    fn spawn_pin(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            // Wait until NSApplication didFinishLaunching has returned;
            // styling the NSWindow inside that extern "C" callback aborts.
            for ms in [80u64, 200, 500, 1200] {
                cx.background_executor()
                    .timer(Duration::from_millis(ms))
                    .await;
                platform::apply_island_chrome();
            }
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                if this.update(cx, |_, _| ()).is_err() {
                    break;
                }
                platform::pin_island_windows();
            }
        })
        .detach();
    }

    fn spawn_loops(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            // Adaptive cadence: 20ms while anything is live (hover, springs,
            // drags, media) or the cursor is near enough to reach the island
            // within one slow tick; 80ms otherwise. The idle tick is the
            // steady-state cost of the whole app, so it is the one to keep
            // cheap and rare.
            let mut wait_ms = 20u64;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(wait_ms))
                    .await;
                let (mx, my) = nook_core::mouse::current_mouse_logical();
                match this
                    .update(cx, |this, cx| {
                    let inside = nook_core::mouse::hit_test(mx, my);
                    let drag_capture = nook_core::mouse::hit_test_drag_capture(mx, my);
                    let on_ui = nook_core::mouse::hit_test_exact(mx, my);
                    let mut dirty = false;
                    // Settings hold strings/vecs; clone them only when the
                    // store's generation says something was actually written.
                    let settings_gen = nook_core::settings::settings_generation();
                    if this.settings_gen != settings_gen {
                        this.settings_gen = settings_gen;
                        let settings = nook_core::settings::get_app_settings();
                        if this.repositioning {
                            // Keep the in-flight drag's origin over the stored one.
                            let (x, y) = (this.settings.island_x, this.settings.island_y);
                            this.settings = settings;
                            this.settings.island_x = x;
                            this.settings.island_y = y;
                        } else {
                            this.settings = settings;
                        }
                        nook_core::osd::apply_from_settings();
                        if !this.settings.show_volume_brightness_hud {
                            this.hud = None;
                            this.hud_dragging = false;
                        }
                        dirty = true;
                    }
                    if this.repositioning {
                        this.apply_reposition(mx as f32, my as f32);
                        dirty = true;
                    }
                    if !platform::island_glass_setting_on() {
                        platform::sync_island_glass(None);
                    }
                    if platform::take_open_settings() {
                        this.open_settings(cx);
                        dirty = true;
                    }
                    let want_suppress = this.settings.hide_when_maximized
                        && !this.repositioning
                        && !this.settings_open
                        && nook_core::occupancy::frontmost_fills_display();
                    if this.suppressed != want_suppress {
                        this.suppressed = want_suppress;
                        if want_suppress {
                            this.hovered = false;
                            if this.expanded {
                                this.expanded = false;
                                this.close_notes_editor(cx);
                                this.stop_mirror(cx);
                            }
                        }
                        dirty = true;
                    }
                    // Our own press / AppKit drag-out also dirties the drag
                    // pasteboard. Do not treat that as an inbound Finder drop.
                    let dragging = nook_core::mouse::drag_active()
                        && this.pending_file_drag.is_none()
                        && !nook_core::files::outbound_drag_active();
                    if this.file_drag != dragging {
                        this.file_drag = dragging;
                        if dragging {
                            platform::register_current_file_drops();
                            if inside {
                                this.arm_dropzone(cx);
                            }
                        }
                        dirty = true;
                    }
                    if this.poll_pending_file_drag(None) {
                        dirty = true;
                    }
                    if let Some((path, dropped)) = nook_core::files::take_outbound_drag() {
                        if dropped {
                            this.remove_file(&path, cx);
                            dirty = true;
                        }
                    }
                    // The overlay strip paints over the menu bar and the top of
                    // the screen; click-through is what keeps the menu bar,
                    // Settings, and apps underneath usable.
                    // Own the cursor only over the painted island. Exception:
                    // while Finder is dragging, the window must see the cursor
                    // early (`drag_capture`) or it never gets draggingEntered.
                    // This wider region only lifts click-through; `inside` stays
                    // exact so it cannot hover or expand the island prematurely.
                    // Same while we are the drag source — AppKit needs the
                    // session's window live. Settings is a separate window and
                    // must not pin this overlay capturing.
                    let ignore = this.overlay_ignores_mouse(on_ui, drag_capture);
                    let changed = this.click_through != ignore;
                    // 20 ms per tick, so this re-asserts roughly every second.
                    if changed || this.click_through_age >= 50 {
                        this.click_through = ignore;
                        this.click_through_age = 0;
                        platform::set_click_through_current(ignore);
                        if changed {
                            log::debug!(
                                "click-through {} at ({mx:.0},{my:.0}) on_ui={on_ui} drag={} expanded={}",
                                if ignore { "on" } else { "off" },
                                this.file_drag,
                                this.expanded
                            );
                        }
                    } else {
                        this.click_through_age += 1;
                    }
                    if !this.suppressed && !this.repositioning && this.hovered != inside {
                        this.hovered = inside;
                        if inside {
                            nook_core::haptics::trigger(None);
                            if this.file_drag {
                                this.arm_dropzone(cx);
                            }
                        } else if this.expanded
                            && !this.settings_open
                            && !this.notes_editing
                            && !this.mirror_on
                            && !this.file_drag
                            && !nook_core::files::outbound_drag_active()
                        {
                            this.expanded = false;
                            this.close_notes_editor(cx);
                        }
                        dirty = true;
                    }
                    let now = Instant::now();
                    // `SpringValue::step` substeps at 120Hz internally, so a
                    // large dt is numerically fine — but it would fast-forward
                    // a freshly retargeted spring, visibly skipping the start
                    // of an animation after an idle (80ms) tick. Cap perceived
                    // time at one 30fps frame.
                    let dt = now
                        .duration_since(this.last_frame)
                        .as_secs_f32()
                        .min(1.0 / 30.0);
                    this.reduce_motion = platform::reduce_motion();
                    this.last_frame = now;
                    let elapsed_secs = now.duration_since(this.last_tick).as_secs() as u32;
                    if elapsed_secs >= 1 {
                        this.last_tick += Duration::from_secs(elapsed_secs as u64);
                        // Repaint only when a countdown actually moved — an
                        // unconditional dirty here kept the island rendering
                        // (and Metal submitting) once a second forever.
                        for t in &mut this.timers {
                            if t.running && t.remaining > 0 {
                                t.remaining = t.remaining.saturating_sub(elapsed_secs);
                                dirty = true;
                                if t.remaining == 0 {
                                    t.running = false;
                                    nook_core::haptics::trigger(Some(nook_core::haptics::HapticConfig {
                                        pattern: nook_core::haptics::HapticPattern::Success,
                                        intensity: 1.0,
                                    }));
                                }
                            }
                        }
                    }
                    let levels = nook_core::audio::get_audio_levels();
                    if this.now_playing.audio_levels.as_deref() != Some(levels.as_slice()) {
                        this.now_playing.audio_levels = Some(levels);
                        dirty = true;
                    }
                    if this.step_spring(dt) {
                        dirty = true;
                    }
                    let any_working = this.agents.iter().any(|a| a.status.is_working());
                    if any_working {
                        // Full-rate on purpose: this repaints the island every
                        // tick while an agent runs, which costs real battery,
                        // but the Dot Matrix loader is the app's signature
                        // animation and smoothness wins here.
                        this.pixel_t = now.duration_since(this.pixel_origin).as_secs_f32();
                        dirty = true;
                    }
                    if this.mirror_on {
                        if let Some((gen, bgra)) = platform::mirror_frame(this.mirror_gen) {
                            this.mirror_gen = gen;
                            if let Some(rendered) = mirror_render_image(bgra) {
                                if let Some(old) = this.mirror_frame.replace(rendered) {
                                    cx.drop_image(old, None);
                                }
                                dirty = true;
                            }
                        }
                    }
                    if let Some(hud) = this.hud {
                        if hud.expired(now, this.hud_dragging) {
                            this.hud = None;
                            dirty = true;
                        }
                    }
                    if dirty {
                        cx.notify();
                    }
                    let active = dirty
                        || this.hovered
                        || this.expanded
                        || this.file_drag
                        || this.repositioning
                        || this.pending_file_drag.is_some()
                        || this.mirror_on
                        || this.settings_open
                        || any_working
                        || this.hud_active();
                    // Media playing promotes itself through `dirty` (the
                    // visualizer levels change every frame), so it needs no
                    // term of its own here.
                    let near = nook_core::mouse::hit_test_near(mx, my);
                    if this.cursor_near != near {
                        this.cursor_near = near;
                        // One render so the overlay strip pre-grows while the
                        // island is still parked (see `sync_overlay_strip`).
                        cx.notify();
                    }
                    if active || near {
                        20
                    } else {
                        80
                    }
                    }) {
                    Ok(next_wait) => wait_ms = next_wait,
                    Err(_) => break,
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            loop {
                let playing = cx
                    .background_executor()
                    .spawn(async {
                        nook_core::runtime().block_on(nook_core::audio::get_now_playing())
                    })
                    .await;
                let alive = this.update(cx, |this, cx| {
                    let was_media = this.has_media();
                    let changed = this.now_playing.title != playing.title
                        || this.now_playing.artist != playing.artist
                        || this.now_playing.is_playing != playing.is_playing
                        || this.now_playing.elapsed_time != playing.elapsed_time
                        || this.now_playing.app_name != playing.app_name
                        || this.now_playing.bundle_id != playing.bundle_id;
                    this.now_playing.title = playing.title;
                    this.now_playing.artist = playing.artist;
                    this.now_playing.album = playing.album;
                    this.now_playing.artwork_base64 = playing.artwork_base64;
                    this.now_playing.duration = playing.duration;
                    this.now_playing.elapsed_time = playing.elapsed_time;
                    this.now_playing.is_playing = playing.is_playing;
                    this.now_playing.app_name = playing.app_name;
                    this.now_playing.bundle_id = playing.bundle_id;
                    this.visualizer_color = media::visualizer_color_from_art(
                        this.now_playing.artwork_base64.as_deref(),
                    );
                    if !was_media && this.has_media() {
                        this.preferred = Some(CompactMode::Media);
                    }
                    if changed {
                        cx.notify();
                    }
                    this.has_media() && this.now_playing.is_playing
                });
                let Ok(is_playing) = alive else {
                    break;
                };
                // Every poll spawns an osascript/perl child, so cadence is
                // the battery cost that matters: 1s while playing (the
                // elapsed display ticks in whole seconds anyway), 5s
                // otherwise. The distributed-notification observers wake the
                // wait instantly on Spotify/Music play/pause/track changes —
                // the slow cadence only gates Safari/YouTube pickup.
                let cadence = if is_playing {
                    Duration::from_secs(1)
                } else {
                    Duration::from_secs(5)
                };
                let slice = Duration::from_millis(250);
                let mut waited = Duration::ZERO;
                loop {
                    cx.background_executor().timer(slice).await;
                    waited += slice;
                    if nook_core::audio::take_media_event() || waited >= cadence {
                        break;
                    }
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| loop {
            let events = cx
                .background_executor()
                .spawn(async {
                    nook_core::runtime()
                        .block_on(nook_core::calendar::get_upcoming_events(Some(false)))
                        .unwrap_or_default()
                })
                .await;
            let reminders = cx
                .background_executor()
                .spawn(async {
                    nook_core::runtime()
                        .block_on(nook_core::calendar::get_reminders(Some(false)))
                        .unwrap_or_default()
                })
                .await;
            if this
                .update(cx, |this, cx| {
                    this.events = events;
                    this.reminders = reminders;
                    if !this.notes_editing {
                        if let Ok(notes) = nook_core::notes::load_notes() {
                            if this.notes != notes {
                                this.notes = notes;
                            }
                        }
                    }
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_secs(30))
                .await;
        })
        .detach();

        cx.spawn(async move |this, cx| loop {
            let agents = cx
                .background_executor()
                .spawn(async { nook_core::agents::snapshot() })
                .await;
            if this
                .update(cx, |this, cx| {
                    let was_empty = this.agents.is_empty();
                    let changed = this.agents != agents;
                    this.agents = agents;
                    if was_empty
                        && !this.agents.is_empty()
                        && this.preferred != Some(CompactMode::Media)
                    {
                        this.preferred = Some(CompactMode::Agents);
                    }
                    if changed {
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
            }
            cx.background_executor()
                .timer(nook_core::agents::poll_interval())
                .await;
        })
        .detach();

        cx.spawn(async move |this, cx| loop {
            let settings = nook_core::settings::get_app_settings();
            let range = settings.observe.range;
            let snapshot = if settings.show_observe {
                cx.background_executor()
                    .spawn(async move {
                        nook_core::runtime().block_on(nook_core::observe::poll(&settings.observe))
                    })
                    .await
            } else {
                ObserveSnapshot::default()
            };
            let connected = snapshot.connected;
            if this
                .update(cx, |this, cx| {
                    let was_quiet = !this.observe.has_outage();
                    this.apply_observe_snapshot(snapshot, range, cx);
                    if was_quiet && this.observe.has_outage() {
                        this.preferred = Some(CompactMode::Observe);
                        nook_core::haptics::trigger(None);
                    }
                })
                .is_err()
            {
                break;
            }
            let wait = if connected { 15 } else { 5 };
            cx.background_executor()
                .timer(Duration::from_secs(wait))
                .await;
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let mut rx = nook_core::sysvol::subscribe();
            loop {
                if rx.changed().await.is_err() {
                    break;
                }
                let event = *rx.borrow_and_update();
                if event.is_initial() {
                    continue;
                }
                let Ok(gen) = this.update(cx, |this, cx| this.apply_hud_event(event, cx)) else {
                    break;
                };
                if gen == 0 {
                    continue;
                }
                let this2 = this.clone();
                cx.spawn(async move |_, cx| {
                    cx.background_executor().timer(HUD_TTL).await;
                    this2
                        .update(cx, |this, cx| {
                            if this
                                .hud
                                .is_some_and(|hud| hud.gen == gen && !this.hud_dragging)
                            {
                                this.hud = None;
                                cx.notify();
                            }
                        })
                        .ok();
                })
                .detach();
            }
        })
        .detach();
    }

    /// Merge a fresh poll into the island: extend the local sample history and
    /// publish the snapshot. Both the periodic loop and manual refreshes
    /// (range-chip taps) go through here.
    fn apply_observe_snapshot(
        &mut self,
        snapshot: ObserveSnapshot,
        range: nook_core::observe::ObserveRange,
        cx: &mut Context<Self>,
    ) {
        let mut snapshot = snapshot;
        nook_core::observe::record_history_range(&mut self.observe_history, &mut snapshot, range);
        nook_core::observe::apply_user_alerts(&self.settings.observe, &mut snapshot);
        self.observe = snapshot;
        cx.notify();
    }

    pub(crate) fn refresh_observe(&mut self, cx: &mut Context<Self>) {
        let config = nook_core::settings::get_app_settings().observe;
        let range = config.range;
        cx.spawn(async move |this, cx| {
            let snapshot = cx
                .background_executor()
                .spawn(
                    async move { nook_core::runtime().block_on(nook_core::observe::poll(&config)) },
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                this.apply_observe_snapshot(snapshot, range, cx);
                this.settings = nook_core::settings::get_app_settings();
            });
        })
        .detach();
    }

    pub(crate) fn has_media(&self) -> bool {
        self.settings.show_media
            && (self.now_playing.is_playing
                || self.now_playing.title.is_some()
                || self.now_playing.artist.is_some())
    }

    pub(crate) fn running_timer(&self) -> Option<&Timer> {
        self.timers.iter().find(|t| t.running)
    }

    /// Compact face: a finished timer first so the ring turns red, else running, else first.
    pub(crate) fn face_timer(&self) -> Option<&Timer> {
        self.timers
            .iter()
            .find(|t| t.remaining == 0 && t.total > 0)
            .or_else(|| self.running_timer())
            .or_else(|| self.timers.first())
    }

    pub(crate) fn reset_timer(&mut self, id: u64) {
        if let Some(t) = self.timers.iter_mut().find(|t| t.id == id) {
            t.remaining = t.total;
            t.running = false;
        }
    }

    pub(crate) fn remove_timer(&mut self, id: u64) {
        self.timers.retain(|t| t.id != id);
        if self.timers.is_empty() {
            self.timer_composer = false;
        }
    }

    /// Swap the Notes card into its raw-markdown editor, ready to type.
    pub(crate) fn begin_notes_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let notes = self.notes.clone();
        let editor = cx.new(|cx| crate::widgets::NotesEditor::new(notes, cx));
        self.notes_sub = Some(cx.subscribe(
            &editor,
            |this, _, _: &crate::widgets::NotesEditorEvent, cx| {
                this.close_notes_editor(cx);
            },
        ));
        window.focus(&editor.focus_handle(cx));
        // The overlay is a nonactivating NSPanel: clicks never make it key,
        // so key events only flow after an explicit makeKeyAndOrderFront.
        // Accessory apps also need an explicit activate or the panel never
        // receives the IME key-down that drives insertText.
        window.activate_window();
        platform::activate_app();
        self.notes_editor = Some(editor);
        self.notes_editing = true;
        cx.notify();
    }

    pub(crate) fn toggle_mirror(&mut self, cx: &mut Context<Self>) {
        if self.mirror_on {
            self.stop_mirror(cx);
        } else if platform::start_mirror() {
            self.mirror_on = true;
            self.expanded = true;
        }
        cx.notify();
    }

    pub(crate) fn stop_mirror(&mut self, cx: &mut Context<Self>) {
        if !self.mirror_on && self.mirror_frame.is_none() {
            return;
        }
        platform::stop_mirror();
        self.mirror_on = false;
        self.mirror_gen = 0;
        if let Some(old) = self.mirror_frame.take() {
            cx.drop_image(old, None);
        }
    }

    /// Flush pending edits back into `self.notes` and restore the preview.
    pub(crate) fn close_notes_editor(&mut self, cx: &mut Context<Self>) {
        self.notes_sub.take();
        if let Some(editor) = self.notes_editor.take() {
            editor.update(cx, |editor, _| editor.flush());
            self.notes = editor.read(cx).text().to_string();
        }
        if self.notes_editing {
            self.notes_editing = false;
            cx.notify();
        }
    }

    pub(crate) fn refresh_calendar(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let events = cx
                .background_executor()
                .spawn(async {
                    nook_core::runtime()
                        .block_on(nook_core::calendar::get_upcoming_events(Some(true)))
                        .unwrap_or_default()
                })
                .await;
            let reminders = cx
                .background_executor()
                .spawn(async {
                    nook_core::runtime()
                        .block_on(nook_core::calendar::get_reminders(Some(true)))
                        .unwrap_or_default()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.events = events;
                this.reminders = reminders;
                cx.notify();
            });
        })
        .detach();
    }

    fn mode(&self) -> CompactMode {
        let modes = self.available_modes();
        if let Some(preferred) = self.preferred.filter(|mode| modes.contains(mode)) {
            return preferred;
        }
        modes.into_iter().next().unwrap_or(CompactMode::Idle)
    }

    fn has_agents(&self) -> bool {
        self.settings.show_agents && !self.agents.is_empty()
    }

    fn has_observe_outage(&self) -> bool {
        self.settings.show_observe && self.observe.has_outage()
    }

    fn available_modes(&self) -> Vec<CompactMode> {
        let mut modes = Vec::new();
        if self.has_observe_outage() {
            modes.push(CompactMode::Observe);
        }
        if self.has_media() {
            modes.push(CompactMode::Media);
        }
        if self.has_agents() {
            modes.push(CompactMode::Agents);
        }
        if self.settings.show_timers && self.running_timer().is_some() {
            modes.push(CompactMode::Timer);
        }
        if self.settings.show_files && !self.files.is_empty() {
            modes.push(CompactMode::Files);
        }
        if self.first_run {
            modes.push(CompactMode::Onboard);
        }
        modes.push(CompactMode::Idle);
        modes
    }

    pub(super) fn hud_enabled(&self) -> bool {
        self.settings.show_volume_brightness_hud
    }

    pub(super) fn hud_active(&self) -> bool {
        self.hud_enabled() && self.hud.is_some()
    }

    fn apply_hud_event(&mut self, event: HudEvent, cx: &mut Context<Self>) -> u64 {
        if !self.hud_enabled() {
            return 0;
        }
        let first = self.hud.is_none();
        let gen = self.hud.map(|h| h.gen.saturating_add(1)).unwrap_or(1);
        self.hud = Some(HudState {
            kind: event.kind,
            value: event.value,
            shown_at: Instant::now(),
            gen,
        });
        if first {
            if self.reduce_motion {
                self.hud_fill.set(event.display_value());
            }
            nook_core::haptics::trigger(None);
        }
        cx.notify();
        gen
    }

    pub(super) fn apply_hud_slider(&mut self, ratio: f32, cx: &mut Context<Self>) {
        let Some(kind) = self.hud.map(|h| h.kind) else {
            return;
        };
        let value = sysvol::clamp_unit(ratio);
        self.hud_dragging = true;
        match kind {
            HudKind::Volume | HudKind::Mute => {
                sysvol::set_volume(value);
                self.hud = Some(HudState {
                    kind: HudKind::Volume,
                    value,
                    shown_at: Instant::now(),
                    gen: self.hud.map(|h| h.gen.saturating_add(1)).unwrap_or(1),
                });
            }
            HudKind::Brightness => {
                nook_core::brightness::set_brightness(value);
                self.hud = Some(HudState {
                    kind: HudKind::Brightness,
                    value,
                    shown_at: Instant::now(),
                    gen: self.hud.map(|h| h.gen.saturating_add(1)).unwrap_or(1),
                });
            }
        }
        cx.notify();
    }

    pub(super) fn end_hud_drag(&mut self) {
        if !self.hud_dragging {
            return;
        }
        self.hud_dragging = false;
        if let Some(hud) = &mut self.hud {
            hud.shown_at = Instant::now();
        }
    }

    fn target_size(&self) -> (f32, f32) {
        let base_w = self.notch_width.max(180.0);
        let base_h = self.notch_height.max(32.0);
        if self.expanded {
            let w = self.expanded_width();
            let body = if self.tab == Tab::Files {
                // Tall enough for one full dropzone tile (flush preview + caption)
                // plus Clear All, so a single file is not clipped behind a scroll.
                theme::EXPANDED_PAD * 2.0 + files::files_pane_min_height(w)
            } else {
                theme::NOOK_INSET + theme::NOOK_BODY
            };
            return (w, self.notch_height.max(32.0) + body);
        }
        if self.hovered {
            return (base_w + 125.0, base_h + 15.0);
        }
        if self.hud_active() {
            return (base_w + 120.0, base_h + theme::COMPACT_HEIGHT_OVERFLOW);
        }
        if self.mode() == CompactMode::Idle {
            let h = if self.settings.non_notch_mode {
                1.0
            } else {
                self.notch_height + theme::IDLE_NOTCH_OVERFLOW + theme::COMPACT_HEIGHT_OVERFLOW
            };
            return (self.notch_width + theme::IDLE_NOTCH_OVERFLOW, h);
        }
        (base_w + 120.0, base_h + theme::COMPACT_HEIGHT_OVERFLOW)
    }

    pub(super) fn expanded_width(&self) -> f32 {
        (self.screen_width - 40.0).min(theme::EXPANDED_MAX_WIDTH)
    }

    /// Lowest screen edge the island can reach if it expands right now, over
    /// either tab. Used to pre-size the overlay strip while the cursor hovers,
    /// so the NSWindow resize happens before an expand can start instead of
    /// stuttering inside the animation.
    pub(super) fn expanded_bottom(&self) -> f32 {
        let w = self.expanded_width();
        let mut body = theme::NOOK_INSET + theme::NOOK_BODY;
        if self.settings.show_files {
            body = body.max(theme::EXPANDED_PAD * 2.0 + files::files_pane_min_height(w));
        }
        let h = self.notch_height.max(32.0) + body;
        let (_, top) = self.settings.island_origin(
            self.screen_width,
            self.screen_height,
            w.max(1.0),
            h.max(1.0),
        );
        top + h
    }

    /// Pick a short incoming offset that explains where the new context came
    /// from. The island is pinned at its top edge, so expansion follows the
    /// vertical reveal; sibling compact modes and tabs preserve horizontal
    /// ordering. This is deliberately small: continuity, not choreography.
    fn content_transition_offset(&self, mode: CompactMode) -> (f32, f32) {
        const HORIZONTAL: f32 = 14.0;
        const VERTICAL: f32 = 10.0;

        if self.expanded != self.last_expanded {
            return (0.0, if self.expanded { -VERTICAL } else { VERTICAL });
        }
        if self.expanded && self.tab != self.last_tab {
            return (
                if self.tab == Tab::Files {
                    HORIZONTAL
                } else {
                    -HORIZONTAL
                },
                0.0,
            );
        }
        if !self.expanded && mode != self.last_mode {
            let modes = self.available_modes();
            let from = modes
                .iter()
                .position(|candidate| *candidate == self.last_mode);
            let to = modes.iter().position(|candidate| *candidate == mode);
            if let (Some(from), Some(to)) = (from, to) {
                let len = modes.len() as isize;
                let mut delta = to as isize - from as isize;
                if delta.abs() > len / 2 {
                    delta -= delta.signum() * len;
                }
                return (HORIZONTAL * delta.signum() as f32, 0.0);
            }
        }
        (0.0, 0.0)
    }

    /// Advance every animated value one frame on its `motion` spring.
    /// Returns whether we still need frames.
    fn step_spring(&mut self, dt: f32) -> bool {
        let (tw, th) = self.target_size();
        let mode = self.mode();
        if self.expanded != self.last_expanded
            || mode != self.last_mode
            || self.tab != self.last_tab
        {
            let (x, y) = self.content_transition_offset(mode);
            self.content_fade.set(0.0);
            self.content_x.set(x);
            self.content_y.set(y);
            self.last_expanded = self.expanded;
            self.last_mode = mode;
            self.last_tab = self.tab;
        }

        let mut moving = false;
        if self.reduce_motion {
            // HIG › Motion: motion must be optional. Size and spatial travel
            // park instantly and the blur stays off; the crossfade below still
            // runs as a simple dissolve.
            self.anim_w.set(tw);
            self.anim_h.set(th);
            self.content_x.set(0.0);
            self.content_y.set(0.0);
        } else {
            moving |= self.anim_w.step(motion::MORPH, tw, dt, motion::REST_PX);
            moving |= self.anim_h.step(motion::MORPH, th, dt, motion::REST_PX);
            moving |= self
                .content_x
                .step(motion::CONTEXT_SHIFT, 0.0, dt, motion::REST_PX);
            moving |= self
                .content_y
                .step(motion::CONTEXT_SHIFT, 0.0, dt, motion::REST_PX);
        }
        moving |= self
            .content_fade
            .step(motion::CROSSFADE, 1.0, dt, motion::REST_ALPHA);
        let overlay = media::album_overlay_target(self.hovered);
        moving |= self
            .overlay_fade
            .step(motion::REVEAL, overlay, dt, motion::REST_ALPHA);
        let hud_target = self
            .hud
            .filter(|_| self.hud_enabled())
            .map(|h| h.display_value())
            .unwrap_or(0.0);
        if self.reduce_motion {
            self.hud_fill.set(hud_target);
        } else {
            moving |= self
                .hud_fill
                .step(motion::REVEAL, hud_target, dt, motion::REST_ALPHA);
        }

        if self.reduce_motion {
            self.blur = 0.0;
            return moving;
        }

        // Width grows from the centre out, so the content on either flank only
        // travels at half the box velocity; height grows downwards from the
        // pinned top edge, so there it tracks it 1:1. Context-shift velocity
        // makes mode and tab swaps smear along their actual travel axis. The
        // fade term keeps the blur up through a crossfade with little movement.
        const BLUR_SPEED: f32 = 900.0;
        let vx = self.anim_w.velocity * 0.5 + self.content_x.velocity;
        let vy = self.anim_h.velocity + self.content_y.velocity;
        let content_speed = vx.hypot(vy);
        self.blur = (content_speed / BLUR_SPEED)
            .min(1.0)
            .max(1.0 - self.content_fade.value);
        moving
    }

    /// Offset (px) for the motion-blur side taps, or `None` while the island is
    /// close enough to rest that the content should be a single crisp layer.
    fn blur_offset(&self) -> Option<(f32, f32)> {
        /// Below this the smear is finer than a pixel — not worth two extra
        /// passes over the content tree.
        const MIN_BLUR: f32 = 0.06;
        /// Half-width of the kernel at full speed.
        const MAX_SMEAR: f32 = 7.0;

        if self.blur < MIN_BLUR {
            return None;
        }
        let vx = (self.anim_w.velocity * 0.5 + self.content_x.velocity).abs();
        let vy = (self.anim_h.velocity + self.content_y.velocity).abs();
        let len = vx.hypot(vy);
        // A pure crossfade has no velocity to point at; the island's long axis
        // is the honest guess there.
        let (ux, uy) = if len < 1.0 {
            (1.0, 0.0)
        } else {
            (vx / len, vy / len)
        };
        let smear = self.blur * MAX_SMEAR;
        Some((ux * smear, uy * smear))
    }

    fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        if self.expanded {
            self.tab = if self.mode() == CompactMode::Files {
                Tab::Files
            } else {
                Tab::Widgets
            };
            if self.first_run {
                self.first_run = false;
                nook_core::settings::mark_onboarded();
            }
        } else {
            self.close_notes_editor(cx);
            self.stop_mirror(cx);
        }
        nook_core::haptics::trigger(None);
        cx.notify();
    }

    fn cycle_mode(&mut self, next: bool) -> bool {
        let modes = self.available_modes();
        if modes.len() <= 1 {
            return false;
        }
        let current = self.mode();
        let idx = modes.iter().position(|m| *m == current).unwrap_or(0);
        let new_idx = if next {
            (idx + 1) % modes.len()
        } else {
            (idx + modes.len() - 1) % modes.len()
        };
        self.preferred = Some(modes[new_idx]);
        nook_core::haptics::trigger(None);
        true
    }

    fn on_wheel(&mut self, event: &gpui::ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(16.0));
        if self.apply_wheel(delta.x.into(), delta.y.into(), event.touch_phase) {
            if !self.expanded {
                self.close_notes_editor(cx);
            }
            cx.notify();
        }
    }

    /// One physical two-finger swipe → one compact-mode / expand / tab change.
    fn apply_wheel(&mut self, dx: f32, dy: f32, phase: TouchPhase) -> bool {
        const THRESHOLD: f32 = 20.0;
        const IDLE: Duration = Duration::from_millis(280);

        let now = Instant::now();
        if matches!(phase, TouchPhase::Started)
            || now.saturating_duration_since(self.last_wheel_at) >= IDLE
        {
            self.wheel_locked = false;
            self.wheel_acc_x = 0.0;
            self.wheel_acc_y = 0.0;
        }
        self.last_wheel_at = now;

        if self.wheel_locked {
            return false;
        }

        self.wheel_acc_x += dx;
        self.wheel_acc_y += dy;
        let ax = self.wheel_acc_x;
        let ay = self.wheel_acc_y;

        let acted = if ax.abs() > ay.abs() {
            if ax.abs() <= THRESHOLD {
                false
            } else if self.expanded {
                self.tab = if ax > 0.0 && self.settings.show_files {
                    Tab::Files
                } else {
                    Tab::Widgets
                };
                true
            } else {
                self.cycle_mode(ax > 0.0)
            }
        } else if ay.abs() <= THRESHOLD {
            false
        } else if !self.expanded && ay > 0.0 {
            // AppKit scrollingDeltaY: two-finger swipe *down* is positive.
            self.expanded = true;
            nook_core::haptics::trigger(None);
            true
        } else if self.expanded && ay < 0.0 {
            self.expanded = false;
            nook_core::haptics::trigger(None);
            true
        } else {
            false
        };

        if acted {
            self.wheel_locked = true;
            self.wheel_acc_x = 0.0;
            self.wheel_acc_y = 0.0;
        }
        acted
    }

    /// Whether the overlay NSWindow should `ignoresMouseEvents`.
    ///
    /// `on_ui` is a hit against the painted island. `drag_capture` is the wider
    /// capture-only region used so an inbound Finder drag meets the window before
    /// `draggingEntered`. Settings must not appear here: it used
    /// to force the overlay live, which ate every click in the window — the
    /// top of the screen when the overlay was ~280px, the whole display now.
    ///
    /// After we start an AppKit drag-out, the session is global. Keeping the
    /// full-screen overlay live made *us* the drop target, so Finder never
    /// saw the file. Click-through off the painted island; stay live on it.
    fn overlay_ignores_mouse(&self, on_ui: bool, drag_capture: bool) -> bool {
        if self.suppressed {
            return true;
        }
        if self.repositioning {
            return false;
        }
        if nook_core::files::outbound_drag_active() {
            return !on_ui;
        }
        !(on_ui || (self.file_drag && drag_capture) || self.pending_file_drag.is_some())
    }

    fn on_island_press(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if event.modifiers.alt {
            self.begin_reposition(event);
            cx.notify();
            return;
        }
        self.toggle_expanded(cx);
    }

    fn begin_reposition(&mut self, event: &MouseDownEvent) {
        let (left, top) = self.island_body_origin();
        self.reposition_grab_x = f32::from(event.position.x) - left;
        self.reposition_grab_y = f32::from(event.position.y) - top;
        self.repositioning = true;
    }

    fn island_body_origin(&self) -> (f32, f32) {
        self.settings.island_origin(
            self.screen_width,
            self.screen_height,
            self.anim_w.value.max(1.0),
            self.anim_h.value.max(1.0),
        )
    }

    fn apply_reposition(&mut self, mx: f32, my: f32) {
        let tw = self.anim_w.value.max(1.0);
        let th = self.anim_h.value.max(1.0);
        let left = (mx - self.reposition_grab_x).clamp(0.0, (self.screen_width - tw).max(0.0));
        let top = (my - self.reposition_grab_y).clamp(0.0, (self.screen_height - th).max(0.0));
        self.settings
            .set_island_origin(left, top, self.screen_width, self.screen_height, tw);
    }

    fn finish_reposition(&mut self) -> bool {
        if !self.repositioning {
            return false;
        }
        self.repositioning = false;
        nook_core::settings::update_app_settings(self.settings.clone());
        true
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.settings_window = None;
        }
        self.settings_open = true;
        platform::set_accessory(false);
        platform::activate_app();
        let (w, h) = settings::SETTINGS_SIZE;
        let (min_w, min_h) = settings::SETTINGS_MIN;
        let bounds = gpui::Bounds::centered(None, size(px(w), px(h)), cx);
        let Ok(handle) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Settings".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                kind: WindowKind::Normal,
                is_resizable: true,
                focus: true,
                show: true,
                window_background: WindowBackgroundAppearance::Blurred,
                window_min_size: Some(size(px(min_w), px(min_h))),
                ..Default::default()
            },
            |_, cx| cx.new(SettingsView::new),
        ) else {
            self.settings_open = false;
            platform::set_accessory(true);
            log::error!("failed to open settings window");
            return;
        };
        if let Ok(entity) = handle.entity(cx) {
            self._settings_closed = Some(cx.observe_release(&entity, |this, _, cx| {
                this.settings_open = false;
                this.settings_window = None;
                platform::set_accessory(true);
                cx.notify();
            }));
        }
        self.settings_window = Some(handle);
        cx.notify();
    }

    pub(super) fn airdrop_paths(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let files: Vec<std::path::PathBuf> = paths.paths().iter().cloned().collect();
        if files.is_empty() {
            return;
        }
        nook_core::haptics::trigger(None);
        platform::share_via_airdrop(&files);
        cx.notify();
    }

    fn ingest_paths(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let mut added = false;
        for path in paths.paths() {
            let raw = path.to_string_lossy().into_owned();
            let resolved = nook_core::files::resolve_path(raw.clone()).unwrap_or(raw);
            if self.files.iter().any(|f| f.path == resolved) {
                continue;
            }
            if let Ok(item) = nook_core::files::add_dropped_path(&resolved) {
                self.files.push(item);
                added = true;
            }
        }
        if added {
            let _ = nook_core::files::save_file_tray(self.files.clone());
            self.preferred = Some(CompactMode::Files);
            self.tab = Tab::Files;
            self.expanded = true;
            nook_core::haptics::trigger(None);
            cx.notify();
        }
    }

    pub(crate) fn add_timer(&mut self, seconds: u32) {
        self.timers.push(Timer {
            id: self.next_timer_id,
            name: String::new(),
            remaining: seconds,
            total: seconds,
            running: true,
        });
        self.next_timer_id += 1;
        self.timer_composer = false;
        self.preferred = Some(CompactMode::Timer);
    }

    pub(crate) fn arm_file_drag(&mut self, path: String) {
        let (screen_x, screen_y) = nook_core::mouse::current_mouse_logical();
        self.pending_file_drag = Some(PendingFileDrag {
            path,
            screen_x,
            screen_y,
        });
    }

    /// Start an OS drag after the pointer moves a few points (click still opens).
    pub(crate) fn poll_pending_file_drag(&mut self, window: Option<&Window>) -> bool {
        let Some(pending) = self.pending_file_drag.as_ref() else {
            return false;
        };
        let (mx, my) = nook_core::mouse::current_mouse_logical();
        let dx = mx - pending.screen_x;
        let dy = my - pending.screen_y;
        if dx * dx + dy * dy < 16.0 {
            return false;
        }
        let path = self.pending_file_drag.take().unwrap().path;
        if self.forget_missing_tray_path(&path) {
            return true;
        }
        nook_core::haptics::trigger(None);
        platform::start_file_drag(&path, window);
        true
    }

    pub(crate) fn finish_file_press(&mut self) -> bool {
        let Some(pending) = self.pending_file_drag.take() else {
            return false;
        };
        if self.forget_missing_tray_path(&pending.path) {
            return true;
        }
        let _ = nook_core::files::open_file(pending.path);
        false
    }

    /// Drop a tray entry whose file is gone. Returns true if the tray changed.
    fn forget_missing_tray_path(&mut self, path: &str) -> bool {
        if std::path::Path::new(path).exists() {
            return false;
        }
        log::warn!("drag-out skipped; missing {path}");
        let n = self.files.len();
        self.files.retain(|f| f.path != path);
        if self.files.len() == n {
            return true;
        }
        let _ = nook_core::files::save_file_tray(self.files.clone());
        true
    }
}

/// Paint camera pixels immediately. `img(Image)` goes through GPUI's async
/// decoder (200ms placeholder), so a new JPEG every tick looks like a reinit.
fn mirror_render_image(bgra: Vec<u8>) -> Option<std::sync::Arc<gpui::RenderImage>> {
    use image::{ImageBuffer, Rgba};
    let size = platform::MIRROR_SIZE;
    let pixels = (size as usize).checked_mul(size as usize)?.checked_mul(4)?;
    if bgra.len() != pixels {
        return None;
    }
    let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(size, size, bgra)?;
    Some(std::sync::Arc::new(gpui::RenderImage::new([
        image::Frame::new(buffer),
    ])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::island::files::{file_grid_metrics, file_tile_height, files_pane_min_height};
    use crate::island::ui::format_timer;
    use nook_core::agents::{AgentKind, AgentStatus};
    use std::collections::HashMap;
    use std::sync::{Mutex, MutexGuard};

    /// `outbound_drag_active` is process-global; overlay tests that toggle it
    /// must not overlap.
    static OVERLAY_MOUSE: Mutex<()> = Mutex::new(());

    fn lock_overlay() -> MutexGuard<'static, ()> {
        OVERLAY_MOUSE.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn test_island() -> Island {
        Island {
            notch_width: 180.0,
            notch_height: 32.0,
            screen_width: 1512.0,
            screen_height: 982.0,
            hovered: false,
            expanded: false,
            preferred: None,
            tab: Tab::Widgets,
            now_playing: NowPlayingData::default(),
            visualizer_color: None,
            files: Vec::new(),
            events: Vec::new(),
            reminders: Vec::new(),
            agents: Vec::new(),
            notes: String::new(),
            notes_editor: None,
            notes_editing: false,
            notes_sub: None,
            timers: Vec::new(),
            next_timer_id: 1,
            calendar_day: 3,
            timer_composer: false,
            observe: ObserveSnapshot::default(),
            observe_history: HashMap::new(),
            observe_hover: None,
            settings: AppSettings::default(),
            first_run: false,
            speed_mbps: None,
            speed_progress: 0.0,
            speed_running: false,
            speed_gen: 0,
            last_tick: Instant::now(),
            last_frame: Instant::now(),
            settings_gen: 0,
            cursor_near: false,
            settings_open: false,
            settings_window: None,
            _settings_closed: None,
            screen_gen: 0,
            anim_w: SpringValue::at(180.0),
            anim_h: SpringValue::at(32.0),
            content_fade: SpringValue::at(1.0),
            content_x: SpringValue::at(0.0),
            content_y: SpringValue::at(0.0),
            overlay_fade: SpringValue::at(0.0),
            reduce_motion: false,
            blur: 0.0,
            last_expanded: false,
            last_mode: CompactMode::Idle,
            last_tab: Tab::Widgets,
            file_drag: false,
            pending_file_drag: None,
            suppressed: false,
            repositioning: false,
            reposition_grab_x: 0.0,
            reposition_grab_y: 0.0,
            click_through: true,
            click_through_age: 0,
            wheel_locked: false,
            last_wheel_at: Instant::now() - Duration::from_secs(1),
            wheel_acc_x: 0.0,
            wheel_acc_y: 0.0,
            pixel_origin: Instant::now(),
            pixel_t: 0.0,
            mirror_on: false,
            mirror_gen: 0,
            mirror_frame: None,
            hud: None,
            hud_fill: SpringValue::at(0.0),
            hud_dragging: false,
        }
    }

    fn with_file(island: &mut Island) {
        island.files.push(FileTrayItem {
            name: "shot.png".into(),
            size: 12,
            path: "/tmp/shot.png".into(),
            mime_type: "image/png".into(),
            last_modified: 0,
        });
    }

    #[test]
    fn cocoa_rect_places_island_at_the_top() {
        let (x, y, w, h) = crate::platform::cocoa_rect_from_gpui(350.0, 0.0, 100.0, 40.0, 600.0);
        assert_eq!((x, y, w, h), (350.0, 560.0, 100.0, 40.0));
        let (x, y, w, h) = crate::platform::cocoa_rect_from_gpui(10.0, 20.0, 30.0, 40.0, 100.0);
        assert_eq!((x, y, w, h), (10.0, 40.0, 30.0, 40.0));
    }

    #[test]
    fn glass_underlay_grows_up_so_top_rounding_clips() {
        let island_h = 160.0;
        let radius = 36.0;
        let window_h = 1169.0;
        let (_, y, _, h) =
            crate::platform::cocoa_rect_from_gpui(200.0, 0.0, 400.0, island_h, window_h);
        let under = crate::platform::glass_underlay_height(h, radius, true);
        assert_eq!(y, window_h - island_h, "bottom of the island stays put");
        assert!(
            y + under > window_h,
            "top rounding sits past the window edge and is clipped"
        );
        let detached = crate::platform::glass_underlay_height(h, radius, false);
        assert_eq!(detached, h, "detached glass matches the island height");
    }

    #[test]
    fn sync_island_glass_is_a_noop_without_a_window() {
        assert!(
            !crate::platform::sync_island_glass(None),
            "hiding glass never reports a live material"
        );
    }

    #[test]
    fn native_glass_stays_off_unless_the_setting_is_on() {
        assert!(
            !nook_core::settings::get_app_settings().liquid_glass_mode,
            "tests start with Liquid Glass island off"
        );
        assert!(!crate::platform::island_glass_setting_on());
        assert!(
            !crate::platform::sync_island_glass(Some(crate::platform::IslandGlass {
                x: 0.0,
                y: 0.0,
                w: 180.0,
                h: 32.0,
                radius: 18.0,
                wing: 6.0,
                tint: None,
            })),
            "native glass must not attach when the setting is off"
        );
    }

    #[test]
    fn mirror_render_image_accepts_square_bgra() {
        let size = crate::platform::MIRROR_SIZE;
        let mut bgra = vec![0u8; (size * size * 4) as usize];
        bgra[0] = 40;
        bgra[1] = 80;
        bgra[2] = 160;
        bgra[3] = 255;
        let image = super::mirror_render_image(bgra).expect("valid BGRA frame");
        assert_eq!(image.frame_count(), 1);
        assert_eq!(image.size(0).width.0, size as i32);
        assert_eq!(image.size(0).height.0, size as i32);
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[40, 80, 160, 255]);
        assert!(super::mirror_render_image(vec![0u8; 16]).is_none());
    }

    #[test]
    fn agent_loader_is_deterministic_per_pid() {
        let kind = crate::dotmatrix::pick(11);
        assert_eq!(kind, crate::dotmatrix::pick(11));
        for working in [false, true] {
            let a = crate::dotmatrix::cell_opacity(kind, 1, 2, 0.3, working);
            assert!((0.0..=1.0).contains(&a), "{kind:?} -> {a}");
        }
    }

    #[test]
    fn format_timer_pads_seconds() {
        assert_eq!(format_timer(0), "0:00");
        assert_eq!(format_timer(65), "1:05");
        assert_eq!(format_timer(1500), "25:00");
        assert_eq!(format_timer(3600), "1:00");
        assert_eq!(super::ui::format_timer_compact(0), "0s");
        assert_eq!(super::ui::format_timer_compact(45), "45s");
        assert_eq!(super::ui::format_timer_compact(65), "1m05");
        assert_eq!(super::ui::format_timer_compact(3600), "1h");
        assert_eq!(super::ui::format_timer_compact(3900), "1h05");
    }

    #[test]
    fn speed_run_starts_the_readout_at_zero() {
        let mut island = test_island();
        island.speed_mbps = Some(99.0);
        island.speed_progress = 100.0;
        island.arm_speed_test();
        assert_eq!(island.speed_mbps, Some(0.0));
        assert_eq!(island.speed_progress, 0.0);
        assert!(island.speed_running);
        island.apply_speed_sample(14.2, 18.0);
        assert_eq!(island.speed_mbps, Some(14.2));
        assert_eq!(island.speed_progress, 18.0);
        island.cancel_speed_test();
        assert!(!island.speed_running);
        assert_eq!(island.speed_progress, 0.0);
        assert_eq!(island.speed_mbps, Some(14.2));
    }

    #[test]
    fn speed_stop_then_run_ignores_stale_result() {
        let mut island = test_island();
        island.speed_running = true;
        island.speed_gen = 1;
        let stale_gen = island.speed_gen;
        island.speed_gen = island.speed_gen.wrapping_add(1);
        island.speed_running = false;
        island.speed_gen = island.speed_gen.wrapping_add(1);
        if island.speed_gen == stale_gen {
            island.speed_mbps = Some(12.0);
        }
        assert!(island.speed_mbps.is_none());
        assert!(!island.speed_running);
        assert_eq!(island.speed_gen, 3);
    }

    #[test]
    fn expanded_island_shows_a_full_file_tile() {
        let mut island = test_island();
        island.expanded = true;
        island.tab = Tab::Files;
        island.notch_height = 38.0;
        island.screen_width = 1800.0;
        let (w, h) = island.target_size();
        let leftover = h - island.notch_height.max(32.0) - theme::EXPANDED_PAD * 2.0;
        let (_, tile) = file_grid_metrics(w);
        assert!(
            leftover + 0.05 >= files_pane_min_height(w),
            "h={h} leftover={leftover} need={}",
            files_pane_min_height(w)
        );
        assert!(
            leftover + 0.05 >= file_tile_height(tile),
            "leftover={leftover} tile_h={}",
            file_tile_height(tile)
        );
    }

    #[test]
    fn hidden_modules_leave_compact_modes() {
        let mut island = test_island();
        with_file(&mut island);
        island.timers.push(Timer {
            id: 1,
            name: String::new(),
            remaining: 30,
            total: 60,
            running: true,
        });
        assert!(island.available_modes().contains(&CompactMode::Files));
        assert!(island.available_modes().contains(&CompactMode::Timer));
        island.settings.show_files = false;
        island.settings.show_timers = false;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
    }

    #[test]
    fn available_modes_idle_last() {
        let island = test_island();
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
        assert_eq!(island.mode(), CompactMode::Idle);
        let (w, h) = island.target_size();
        assert!(w >= 180.0);
        assert!(h >= 1.0);
    }

    #[test]
    fn hud_takeover_expands_idle_island_and_respects_the_toggle() {
        let mut island = test_island();
        let idle = island.target_size();
        let event = HudEvent {
            kind: HudKind::Volume,
            value: 0.6,
            seq: 1,
        };
        // apply_hud_event needs a Context; drive the same fields the spawn path sets.
        island.hud = Some(HudState {
            kind: event.kind,
            value: event.value,
            shown_at: Instant::now(),
            gen: 1,
        });
        island.hud_fill.set(event.display_value());
        assert!(island.hud_active());
        let live = island.target_size();
        assert!(live.0 > idle.0, "HUD should widen the idle sliver");
        assert!(live.1 > idle.1, "HUD should grow the idle sliver");
        assert!((island.hud.unwrap().display_value() - 0.6).abs() < f32::EPSILON);

        island.settings.show_volume_brightness_hud = false;
        assert!(!island.hud_active());
        assert_eq!(island.target_size(), idle);
    }

    #[test]
    fn hud_expires_after_ttl_unless_dragging() {
        let mut island = test_island();
        island.hud = Some(HudState {
            kind: HudKind::Brightness,
            value: 0.2,
            shown_at: Instant::now() - HUD_TTL - Duration::from_millis(10),
            gen: 3,
        });
        assert!(island.hud.unwrap().expired(Instant::now(), false));
        assert!(!island.hud.unwrap().expired(Instant::now(), true));
        island.hud_dragging = true;
        island.end_hud_drag();
        assert!(!island.hud_dragging);
        assert!(
            Instant::now().duration_since(island.hud.unwrap().shown_at) < Duration::from_millis(50)
        );
    }

    #[test]
    fn collapsed_idle_wraps_the_hardware_notch_by_one_pixel() {
        let mut island = test_island();
        island.notch_width = 185.0;
        island.notch_height = 38.0;
        let (w, h) = island.target_size();
        assert_eq!(w, 185.0 + theme::IDLE_NOTCH_OVERFLOW);
        assert_eq!(
            h,
            38.0 + theme::IDLE_NOTCH_OVERFLOW + theme::COMPACT_HEIGHT_OVERFLOW
        );

        island.settings.non_notch_mode = true;
        let (w, h) = island.target_size();
        assert_eq!(w, 185.0 + theme::IDLE_NOTCH_OVERFLOW);
        assert_eq!(h, 1.0);
    }

    #[test]
    fn empty_hover_matches_live_activity_hover() {
        let mut island = test_island();
        island.notch_width = 185.0;
        island.notch_height = 38.0;
        island.hovered = true;
        assert_eq!(island.mode(), CompactMode::Idle);
        let idle_hover = island.target_size();

        island.now_playing.title = Some("Track".into());
        island.now_playing.is_playing = true;
        island.settings.show_media = true;
        assert_eq!(island.mode(), CompactMode::Media);
        assert_eq!(island.target_size(), idle_hover);
        assert_eq!(idle_hover, (185.0 + 125.0, 38.0 + 15.0));
    }

    #[test]
    fn available_modes_includes_agents() {
        let mut island = test_island();
        island.agents = vec![AgentSession {
            kind: AgentKind::Grok,
            pid: 42,
            project: "~".into(),
            cwd: "/Users/jonasvogel".into(),
            status: AgentStatus::Working,
            session_id: None,
            name: Some("GPUI circular Dot Matrix agent indicator".into()),
            model: Some("grok-4.6".into()),
        }];
        assert_eq!(
            island.available_modes(),
            vec![CompactMode::Agents, CompactMode::Idle]
        );
        assert_eq!(island.mode(), CompactMode::Agents);
        island.settings.show_agents = false;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
    }

    #[test]
    fn available_modes_observe_only_when_user_alert_fires() {
        let mut island = test_island();
        island.settings.show_observe = true;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
        island.observe.alerts = vec![nook_core::observe::FiringAlert {
            name: "5xx".into(),
            severity: "critical".into(),
            summary: "2 > 0".into(),
        }];
        assert_eq!(
            island.available_modes(),
            vec![CompactMode::Observe, CompactMode::Idle]
        );
        assert_eq!(island.mode(), CompactMode::Observe);
        island.settings.show_observe = false;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
    }

    #[test]
    fn settings_open_does_not_capture_the_overlay() {
        let _guard = lock_overlay();
        let mut island = test_island();
        island.settings_open = true;
        island.expanded = true;
        // Cursor is over the desktop / Settings, not the island.
        assert!(island.overlay_ignores_mouse(false, false));
        // Cursor over the painted island still belongs to us.
        assert!(!island.overlay_ignores_mouse(true, true));
    }

    #[test]
    fn overlay_captures_only_painted_island_and_drags() {
        let _guard = lock_overlay();
        let mut island = test_island();
        assert!(island.overlay_ignores_mouse(false, false));
        assert!(!island.overlay_ignores_mouse(true, false));

        island.file_drag = true;
        assert!(
            island.overlay_ignores_mouse(false, false),
            "outside the hover pad stays click-through"
        );
        assert!(
            !island.overlay_ignores_mouse(false, true),
            "inside the hover pad must see the inbound drag"
        );

        island.file_drag = false;
        island.arm_file_drag("/tmp/shot.png".into());
        assert!(
            !island.overlay_ignores_mouse(false, false),
            "until the AppKit session starts, mouse moves must reach us"
        );
        island.pending_file_drag = None;
        nook_core::files::begin_outbound_drag("/tmp/shot.png");
        struct ClearOutbound;
        impl Drop for ClearOutbound {
            fn drop(&mut self) {
                nook_core::files::finish_outbound_drag(false);
                let _ = nook_core::files::take_outbound_drag();
            }
        }
        let _outbound = ClearOutbound;
        assert!(
            island.overlay_ignores_mouse(false, false),
            "off the island, Finder has to be the drop target"
        );
        assert!(
            !island.overlay_ignores_mouse(true, true),
            "over the island the source window stays live"
        );
    }

    #[test]
    fn suppressed_island_is_always_click_through() {
        let _guard = lock_overlay();
        let mut island = test_island();
        island.suppressed = true;
        assert!(island.overlay_ignores_mouse(true, true));
        assert!(island.overlay_ignores_mouse(false, false));
    }

    #[test]
    fn repositioning_captures_the_overlay() {
        let _guard = lock_overlay();
        let mut island = test_island();
        island.repositioning = true;
        assert!(!island.overlay_ignores_mouse(false, false));
    }

    #[test]
    fn file_press_stays_pending_until_moved() {
        let mut island = test_island();
        island.arm_file_drag("/tmp/shot.png".into());
        assert!(!island.poll_pending_file_drag(None));
        assert!(island.pending_file_drag.is_some());
        island.finish_file_press();
        assert!(island.pending_file_drag.is_none());
    }

    #[test]
    fn missing_file_is_removed_on_drag_out() {
        let mut island = test_island();
        let path = "/tmp/nook-missing-tray-file-does-not-exist.bin";
        island.files.push(FileTrayItem {
            name: "gone.bin".into(),
            size: 1,
            path: path.into(),
            mime_type: "file".into(),
            last_modified: 0,
        });
        island.arm_file_drag(path.into());
        if let Some(pending) = island.pending_file_drag.as_mut() {
            pending.screen_x -= 100.0;
        }
        assert!(island.poll_pending_file_drag(None));
        assert!(island.files.iter().all(|f| f.path != path));
        assert!(island.pending_file_drag.is_none());
    }

    #[test]
    fn compact_swipe_cycles_once_per_gesture() {
        let mut island = test_island();
        with_file(&mut island);
        island.preferred = Some(CompactMode::Idle);
        assert_eq!(
            island.available_modes(),
            vec![CompactMode::Files, CompactMode::Idle]
        );
        assert_eq!(island.mode(), CompactMode::Idle);

        // A trackpad swipe is many events well over the threshold.
        for _ in 0..8 {
            island.last_wheel_at = Instant::now();
            assert!(
                island.apply_wheel(40.0, 0.0, TouchPhase::Moved) || island.wheel_locked,
                "first event should cycle; the rest of the gesture is locked"
            );
        }
        assert_eq!(island.mode(), CompactMode::Files);

        // A new physical gesture is allowed to take one more step.
        assert!(island.apply_wheel(40.0, 0.0, TouchPhase::Started));
        assert_eq!(island.mode(), CompactMode::Idle);
        assert!(!island.apply_wheel(40.0, 0.0, TouchPhase::Moved));
        assert_eq!(island.mode(), CompactMode::Idle);
    }

    #[test]
    fn compact_swipe_accumulates_small_deltas() {
        let mut island = test_island();
        with_file(&mut island);
        island.preferred = Some(CompactMode::Idle);

        assert!(!island.apply_wheel(8.0, 0.0, TouchPhase::Moved));
        assert!(!island.apply_wheel(8.0, 0.0, TouchPhase::Moved));
        assert_eq!(island.mode(), CompactMode::Idle);
        assert!(island.apply_wheel(8.0, 0.0, TouchPhase::Moved));
        assert_eq!(island.mode(), CompactMode::Files);
        assert!(!island.apply_wheel(8.0, 0.0, TouchPhase::Moved));
        assert_eq!(island.mode(), CompactMode::Files);
    }

    #[test]
    fn compact_swipe_rearms_after_idle() {
        let mut island = test_island();
        with_file(&mut island);
        island.preferred = Some(CompactMode::Idle);

        assert!(island.apply_wheel(-40.0, 0.0, TouchPhase::Moved));
        let after_first = island.mode();
        assert_ne!(after_first, CompactMode::Idle);

        island.last_wheel_at = Instant::now() - Duration::from_millis(400);
        assert!(island.apply_wheel(-40.0, 0.0, TouchPhase::Moved));
        assert_ne!(island.mode(), after_first);
    }

    #[test]
    fn tray_tiles_stay_at_expanded_size_during_compact_spring() {
        let island = test_island();
        let compact_w = island.notch_width + 120.0;
        let (compact_cols, compact_tile) = file_grid_metrics(compact_w);
        let (layout_cols, layout_tile) = island.file_layout();
        assert_ne!(
            (compact_cols, compact_tile.to_bits()),
            (layout_cols, layout_tile.to_bits()),
            "compact island width would pick a different tile size"
        );
        assert_eq!(
            island.file_layout(),
            file_grid_metrics(island.expanded_width())
        );
        assert!((island.expanded_width() - theme::EXPANDED_MAX_WIDTH).abs() < f32::EPSILON);
    }

    #[test]
    fn spring_settles_on_target() {
        let mut island = test_island();
        island.anim_w.set(0.0);
        island.anim_h.set(0.0);
        island.content_fade.set(0.0);
        let mut moving = true;
        for _ in 0..200 {
            moving = island.step_spring(0.016);
            if !moving {
                break;
            }
        }
        assert!(!moving);
        let (tw, th) = island.target_size();
        assert!((island.anim_w.value - tw).abs() < 0.5);
        assert!((island.anim_h.value - th).abs() < 0.5);
    }

    /// The poll loop used to cap `dt` at 50ms. Semi-implicit Euler at MORPH
    /// stiffness is already unstable around 42ms, so one hitch sent `anim_h`
    /// to ±1e6 and the island strobed open/closed forever.
    fn assert_spring_sane(island: &Island) {
        assert!(
            island.anim_w.value.is_finite() && island.anim_h.value.is_finite(),
            "spring went non-finite: {}×{}",
            island.anim_w.value,
            island.anim_h.value
        );
        assert!(
            island.anim_w.value > 0.0 && island.anim_w.value < 4000.0,
            "width exploded: {}",
            island.anim_w.value
        );
        assert!(
            island.anim_h.value > -1.0 && island.anim_h.value < 4000.0,
            "height exploded: {}",
            island.anim_h.value
        );
    }

    #[test]
    fn expansion_enters_from_the_notch() {
        let mut island = test_island();
        while island.step_spring(0.016) {}

        island.expanded = true;
        island.step_spring(0.016);

        assert_eq!(island.content_x.value, 0.0);
        assert!(
            island.content_y.value < 0.0,
            "expanded content should arrive from the pinned notch edge"
        );
    }

    #[test]
    fn compact_mode_changes_follow_horizontal_order() {
        let mut island = test_island();
        with_file(&mut island);
        island.preferred = Some(CompactMode::Idle);
        while island.step_spring(0.016) {}

        island.preferred = Some(CompactMode::Files);
        island.step_spring(0.016);

        assert!(island.content_x.value.abs() > 1.0);
        assert_eq!(island.content_y.value, 0.0);
    }

    #[test]
    fn reduce_motion_keeps_only_the_dissolve() {
        let mut island = test_island();
        island.reduce_motion = true;
        island.expanded = true;

        island.step_spring(0.016);

        assert_eq!(island.content_x.value, 0.0);
        assert_eq!(island.content_y.value, 0.0);
        assert_eq!(island.blur, 0.0);
        assert_eq!(
            (island.anim_w.value, island.anim_h.value),
            island.target_size()
        );
        assert!(island.content_fade.value > 0.0 && island.content_fade.value < 1.0);
    }

    #[test]
    fn spring_survives_50ms_hitch_while_moving() {
        let mut island = test_island();
        island.hovered = true;
        island.step_spring(0.016);
        island.step_spring(0.05);
        assert_spring_sane(&island);
        let mut moving = true;
        for _ in 0..200 {
            moving = island.step_spring(0.016);
            assert_spring_sane(&island);
            if !moving {
                break;
            }
        }
        assert!(!moving, "spring never settled after a 50ms hitch");
        let (tw, th) = island.target_size();
        assert!((island.anim_w.value - tw).abs() < 0.5);
        assert!((island.anim_h.value - th).abs() < 0.5);
    }

    #[test]
    fn spring_survives_sustained_slow_frames() {
        let mut island = test_island();
        island.hovered = true;
        island.expanded = true;
        let mut moving = true;
        for _ in 0..200 {
            moving = island.step_spring(0.05);
            assert_spring_sane(&island);
            if !moving {
                break;
            }
        }
        assert!(!moving, "spring exploded instead of settling at 50ms/frame");
        let (tw, th) = island.target_size();
        assert!((island.anim_w.value - tw).abs() < 0.5);
        assert!((island.anim_h.value - th).abs() < 0.5);
    }

    #[test]
    fn blur_peaks_mid_spring_and_clears_at_rest() {
        let mut island = test_island();
        island.hovered = true;
        with_file(&mut island);

        let mut peak = 0.0f32;
        let mut peak_smear = 0.0f32;
        let mut moving = true;
        for _ in 0..200 {
            moving = island.step_spring(0.016);
            peak = peak.max(island.blur);
            if let Some((dx, dy)) = island.blur_offset() {
                peak_smear = peak_smear.max(dx.hypot(dy));
            }
            if !moving {
                break;
            }
        }

        assert!(!moving);
        // The kernel has to actually open up mid-flight...
        assert!(peak > 0.25, "blur never built up: {peak}");
        assert!(peak_smear > 1.0, "smear never left sub-pixel: {peak_smear}");
        // ...and collapse to one crisp layer once parked.
        assert_eq!(island.blur, 0.0);
        assert!(island.blur_offset().is_none());
    }

    #[test]
    fn crossfade_blurs_without_a_resize() {
        let mut island = test_island();
        // Settle first, so the only thing in flight is the content swap.
        while island.step_spring(0.016) {}
        island.content_fade.set(0.0);

        island.step_spring(0.016);
        assert!(island.blur > 0.5, "crossfade left the content sharp");
        let (dx, dy) = island.blur_offset().expect("crossfade needs a smear");
        assert!(dx > dy, "a still island should smear along its long axis");
    }
}
