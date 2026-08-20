use crate::icons::lucide;
use crate::platform;
use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, relative, rgb, rgba, size, AnyElement, App, Bounds,
    Context, CursorStyle, ExternalPaths, FocusHandle, FontFallbacks, FontWeight, KeyDownEvent,
    MouseButton, MouseDownEvent, PathBuilder, ScrollWheelEvent, SharedString, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};
use nook_core::calendar::{CalendarEvent, Reminder};
use nook_core::files::FileTrayItem;
use nook_core::models::NowPlayingData;
use nook_core::notch;
use nook_core::observe::ObserveSnapshot;
use nook_core::settings::AppSettings;
use std::any::Any;
use std::time::{Duration, Instant};

const WING: f32 = 6.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompactMode {
    Idle,
    Media,
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
    pub name: String,
    pub remaining: u32,
    pub total: u32,
    pub running: bool,
}

pub struct Island {
    pub notch_width: f32,
    pub notch_height: f32,
    pub screen_width: f32,
    pub hovered: bool,
    pub expanded: bool,
    pub preferred: Option<CompactMode>,
    pub tab: Tab,
    pub now_playing: NowPlayingData,
    pub files: Vec<FileTrayItem>,
    pub events: Vec<CalendarEvent>,
    pub reminders: Vec<Reminder>,
    pub notes: String,
    pub notes_dirty: bool,
    pub timers: Vec<Timer>,
    pub next_timer_id: u64,
    pub settings: AppSettings,
    pub first_run: bool,
    pub speed_mbps: Option<f64>,
    pub speed_progress: f64,
    pub speed_running: bool,
    pub observe: ObserveSnapshot,
    pub last_tick: Instant,
    pub settings_open: bool,
    chrome_applied: bool,
    anim_w: f32,
    anim_h: f32,
    anim_vw: f32,
    anim_vh: f32,
    content_opacity: f32,
    last_expanded: bool,
    last_mode: CompactMode,
    file_drag: bool,
}

impl Island {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        nook_core::init();
        let info = notch::get_notch_info();
        let settings = nook_core::settings::get_app_settings();
        let files = nook_core::files::load_file_tray().unwrap_or_default();
        let notes = nook_core::notes::load_notes().unwrap_or_default();
        let first_run = nook_core::database::get_setting("app_settings").is_none();
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
            hovered: false,
            expanded: false,
            preferred: None,
            tab: Tab::Widgets,
            now_playing: NowPlayingData::default(),
            files,
            events: Vec::new(),
            reminders: Vec::new(),
            notes,
            notes_dirty: false,
            timers: Vec::new(),
            next_timer_id: 1,
            settings,
            first_run,
            speed_mbps: None,
            speed_progress: 0.0,
            speed_running: false,
            observe: ObserveSnapshot::default(),
            last_tick: Instant::now(),
            settings_open: false,
            chrome_applied: false,
            anim_w: 0.0,
            anim_h: 0.0,
            anim_vw: 0.0,
            anim_vh: 0.0,
            content_opacity: 1.0,
            last_expanded: false,
            last_mode: CompactMode::Idle,
            file_drag: false,
        };
        // Start at the compact idle size so the first paint isn't a jump.
        let (w, h) = this.target_size();
        this.anim_w = w;
        this.anim_h = h;

        nook_core::runtime().spawn(async {
            let _ = nook_core::calendar::request_calendar_access().await;
        });
        // Do not touch GPUI's Window handle here — HasWindowHandle RefCell-panics
        // during construction. Chrome is applied via NSApp window enumeration.
        let _ = window;
        this.chrome_applied = true;
        this.spawn_loops(cx);
        Self::spawn_pin(cx);
        this
    }

    fn spawn_pin(cx: &mut Context<Self>) {
        cx.spawn(async move |_, cx| {
            // Wait until NSApplication didFinishLaunching has returned;
            // styling the NSWindow inside that extern "C" callback aborts.
            for ms in [80u64, 200, 500, 1200] {
                cx.background_executor()
                    .timer(Duration::from_millis(ms))
                    .await;
                platform::apply_island_chrome();
            }
        })
        .detach();
    }

    fn spawn_loops(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(20))
                    .await;
                let (mx, my) = nook_core::mouse::current_mouse_logical();
                let inside = nook_core::mouse::hit_test(mx, my);
                this.update(cx, |this, cx| {
                    this.settings = nook_core::settings::get_app_settings();
                    let mut dirty = false;
                    let dragging = nook_core::files::file_drag_active();
                    if this.file_drag != dragging {
                        this.file_drag = dragging;
                        if dragging {
                            // Lift click-through now, not on the next paint —
                            // Finder must see the window before the cursor enters.
                            platform::set_click_through_current(false);
                            if inside {
                                this.arm_dropzone(cx);
                            }
                        }
                        dirty = true;
                    }
                    if this.hovered != inside {
                        this.hovered = inside;
                        if inside {
                            nook_core::haptics::trigger(None);
                            if this.file_drag {
                                this.arm_dropzone(cx);
                            }
                        } else if this.expanded && !this.settings_open && !this.file_drag {
                            this.expanded = false;
                        }
                        dirty = true;
                    }
                    if this.step_spring(0.02) {
                        dirty = true;
                    }
                    if dirty {
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();

        cx.spawn(async move |this, cx| loop {
            let playing = cx
                .background_executor()
                .spawn(async { nook_core::runtime().block_on(nook_core::audio::get_now_playing()) })
                .await;
            this.update(cx, |this, cx| {
                let was_media = this.has_media();
                this.now_playing = playing;
                if !was_media && this.has_media() {
                    this.preferred = Some(CompactMode::Media);
                }
                let now = Instant::now();
                if now.duration_since(this.last_tick) >= Duration::from_secs(1) {
                    this.last_tick = now;
                    for t in &mut this.timers {
                        if t.running && t.remaining > 0 {
                            t.remaining -= 1;
                            if t.remaining == 0 {
                                t.running = false;
                                nook_core::haptics::trigger(Some(
                                    nook_core::haptics::HapticConfig {
                                        pattern: nook_core::haptics::HapticPattern::Success,
                                        intensity: 1.0,
                                    },
                                ));
                            }
                        }
                    }
                }
                cx.notify();
            })
            .ok();
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
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
            this.update(cx, |this, cx| {
                this.events = events;
                this.reminders = reminders;
                cx.notify();
            })
            .ok();
            cx.background_executor()
                .timer(Duration::from_secs(30))
                .await;
        })
        .detach();

        cx.spawn(async move |this, cx| loop {
            let settings = nook_core::settings::get_app_settings();
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
            this.update(cx, |this, cx| {
                let was_quiet = !this.observe.has_outage();
                this.observe = snapshot;
                if was_quiet && this.observe.has_outage() {
                    this.preferred = Some(CompactMode::Observe);
                    nook_core::haptics::trigger(None);
                }
                cx.notify();
            })
            .ok();
            let wait = if connected { 15 } else { 5 };
            cx.background_executor()
                .timer(Duration::from_secs(wait))
                .await;
        })
        .detach();
    }

    fn has_media(&self) -> bool {
        self.settings.show_media
            && (self.now_playing.is_playing
                || self.now_playing.title.is_some()
                || self.now_playing.artist.is_some())
    }

    fn running_timer(&self) -> Option<&Timer> {
        self.timers.iter().find(|t| t.running)
    }

    fn mode(&self) -> CompactMode {
        if let Some(preferred) = self.preferred {
            if self.mode_available(preferred) {
                return preferred;
            }
        }
        self.available_modes()
            .into_iter()
            .next()
            .unwrap_or(CompactMode::Idle)
    }

    fn mode_available(&self, mode: CompactMode) -> bool {
        match mode {
            CompactMode::Media => self.has_media(),
            CompactMode::Files => !self.files.is_empty(),
            CompactMode::Timer => self.running_timer().is_some(),
            CompactMode::Observe => self.settings.show_observe && self.observe.has_outage(),
            CompactMode::Onboard => self.first_run,
            CompactMode::Idle => true,
        }
    }

    fn available_modes(&self) -> Vec<CompactMode> {
        let mut modes = Vec::new();
        if self.settings.show_observe && self.observe.has_outage() {
            modes.push(CompactMode::Observe);
        }
        if self.has_media() {
            modes.push(CompactMode::Media);
        }
        if self.running_timer().is_some() {
            modes.push(CompactMode::Timer);
        }
        if !self.files.is_empty() {
            modes.push(CompactMode::Files);
        }
        if self.first_run {
            modes.push(CompactMode::Onboard);
        }
        modes.push(CompactMode::Idle);
        modes
    }

    fn target_size(&self) -> (f32, f32) {
        let base_w = self.notch_width.max(180.0);
        let base_h = self.notch_height.max(32.0);
        if self.expanded {
            return ((self.screen_width - 40.0).min(600.0), 250.0);
        }
        if self.hovered {
            return if self.mode() == CompactMode::Idle {
                (base_w + 30.0, base_h + 10.0)
            } else {
                (base_w + 125.0, base_h + 15.0)
            };
        }
        if self.mode() == CompactMode::Idle {
            let h = if self.settings.non_notch_mode {
                1.0
            } else {
                base_h
            };
            return (base_w, h);
        }
        (base_w + 120.0, base_h)
    }

    /// Critically-damped-ish spring matching the Tauri Motion values
    /// (stiffness 400, damping 30, mass 0.8). Returns whether we still need frames.
    fn step_spring(&mut self, dt: f32) -> bool {
        let (tw, th) = self.target_size();
        let mode = self.mode();
        if self.expanded != self.last_expanded || mode != self.last_mode {
            self.content_opacity = 0.0;
            self.last_expanded = self.expanded;
            self.last_mode = mode;
        }
        self.content_opacity = (self.content_opacity + dt / 0.18).min(1.0);

        const STIFFNESS: f32 = 400.0;
        const DAMPING: f32 = 30.0;
        const MASS: f32 = 0.8;
        let step = |pos: &mut f32, vel: &mut f32, target: f32| {
            let acc = ((target - *pos) * STIFFNESS - *vel * DAMPING) / MASS;
            *vel += acc * dt;
            *pos += *vel * dt;
        };
        step(&mut self.anim_w, &mut self.anim_vw, tw);
        step(&mut self.anim_h, &mut self.anim_vh, th);

        let moving = (self.anim_w - tw).abs() > 0.4
            || (self.anim_h - th).abs() > 0.4
            || self.anim_vw.abs() > 4.0
            || self.anim_vh.abs() > 4.0
            || self.content_opacity < 0.999;
        if !moving {
            self.anim_w = tw;
            self.anim_h = th;
            self.anim_vw = 0.0;
            self.anim_vh = 0.0;
            self.content_opacity = 1.0;
        }
        moving
    }

    fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        if self.expanded {
            self.tab = if self.mode() == CompactMode::Files {
                Tab::Files
            } else {
                Tab::Widgets
            };
            self.first_run = false;
        }
        nook_core::haptics::trigger(None);
        cx.notify();
    }

    fn cycle_mode(&mut self, next: bool, cx: &mut Context<Self>) {
        let modes = self.available_modes();
        if modes.len() <= 1 {
            return;
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
        cx.notify();
    }

    fn on_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(16.0));
        let dx: f32 = delta.x.into();
        let dy: f32 = delta.y.into();
        // AppKit scrollingDeltaY: two-finger swipe *down* is positive.
        if !self.expanded && dy > 20.0 {
            self.expanded = true;
            nook_core::haptics::trigger(None);
            cx.notify();
        } else if self.expanded && dy < -20.0 {
            self.expanded = false;
            nook_core::haptics::trigger(None);
            cx.notify();
        } else if self.expanded {
            if dx > 20.0 {
                self.tab = Tab::Files;
                cx.notify();
            } else if dx < -20.0 {
                self.tab = Tab::Widgets;
                cx.notify();
            }
        } else if dx.abs() > 20.0 {
            self.cycle_mode(dx > 0.0, cx);
        }
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = true;
        platform::set_accessory(false);
        platform::activate_app();
        let bounds = Bounds::centered(None, size(px(440.), px(560.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("openNook".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                kind: WindowKind::Normal,
                is_resizable: false,
                focus: true,
                show: true,
                ..Default::default()
            },
            |_, cx| cx.new(|cx| SettingsView::new(cx)),
        )
        .ok();
        cx.notify();
    }

    fn arm_dropzone(&mut self, cx: &mut Context<Self>) {
        self.expanded = true;
        self.tab = Tab::Files;
        self.preferred = Some(CompactMode::Files);
        nook_core::haptics::trigger(None);
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

    fn add_timer(&mut self, seconds: u32) {
        self.timers.push(Timer {
            id: self.next_timer_id,
            name: String::new(),
            remaining: seconds,
            total: seconds,
            running: true,
        });
        self.next_timer_id += 1;
        self.preferred = Some(CompactMode::Timer);
    }

    fn format_time(seconds: u32) -> String {
        let m = seconds / 60;
        let s = seconds % 60;
        format!("{m}:{s:02}")
    }
}

impl Render for Island {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        // File drags must see the window or Finder never delivers the drop.
        let receive_events = self.hovered || self.expanded || self.file_drag;
        if self.chrome_applied {
            platform::set_click_through_current(!receive_events);
        }

        let (tw, th) = (self.anim_w, self.anim_h);
        nook_core::mouse::update_ui_bounds(
            ((self.screen_width - tw) / 2.0) as f64,
            0.0,
            tw as f64,
            th as f64,
        );

        let island_bg = if self.settings.liquid_glass_mode {
            theme::ISLAND_GLASS
        } else {
            theme::ISLAND
        };
        let mode = self.mode();
        let expanded = self.expanded;
        let hovered = self.hovered;
        let notch_w = self.notch_width.max(180.0);
        let dropping = self.file_drag && (self.hovered || self.expanded);
        let show_wings = th > 4.0
            && !(self.settings.non_notch_mode
                && mode == CompactMode::Idle
                && !hovered
                && !expanded);

        let wing = if show_wings { WING } else { 0.0 };
        let chrome_w = tw.max(1.0) + wing * 2.0;
        let chrome_h = th.max(1.0);

        div()
            .id("island-root")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_start()
            .bg(rgba(0x00000000))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                this.on_wheel(event, cx);
            }))
            .font(gpui::Font {
                family: "SF Pro".into(),
                features: gpui::FontFeatures::default(),
                fallbacks: Some(FontFallbacks::from_fonts(vec![
                    "SF Compact".into(),
                    "SF Symbols".into(),
                    ".AppleSystemUIFont".into(),
                ])),
                weight: FontWeight::NORMAL,
                style: gpui::FontStyle::Normal,
            })
            .can_drop(|drag: &dyn Any, _, _| drag.downcast_ref::<ExternalPaths>().is_some())
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                log::info!("drop {} path(s)", paths.paths().len());
                this.ingest_paths(paths, cx);
            }))
            .child(
                div()
                    .id("island")
                    .relative()
                    .w(px(chrome_w))
                    .h(px(chrome_h))
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.toggle_expanded(cx);
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        this.on_wheel(event, cx);
                    }))
                    .child(island_chrome(tw.max(1.0), th.max(1.0), wing, island_bg))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left(px(wing))
                            .w(px(tw.max(1.0)))
                            .h(px(th.max(1.0)))
                            .overflow_hidden()
                            .when(self.settings.liquid_glass_mode || dropping, |d| {
                                d.border_1().border_color(if dropping {
                                    rgb(0xffffff)
                                } else {
                                    theme::DIVIDER
                                })
                            })
                            .child(
                                div()
                                    .size_full()
                                    .opacity(self.content_opacity.clamp(0.0, 1.0))
                                    .child(if expanded {
                                        self.render_expanded(notch_w, cx).into_any_element()
                                    } else {
                                        self.render_compact(mode, hovered, notch_w, cx)
                                            .into_any_element()
                                    }),
                            )
                            .when(dropping && !expanded, |d| d.child(drop_veil()))
                            .when(!expanded, |d| d.child(self.mode_dots(cx))),
                    ),
            )
    }
}

impl Island {
    fn render_compact(
        &self,
        mode: CompactMode,
        hovered: bool,
        notch_w: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let side = ((self.target_size().0 - notch_w) / 2.0).max(8.0);
        div()
            .flex()
            .items_center()
            .justify_between()
            .size_full()
            .px_3()
            .child(
                div()
                    .w(px(side))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_start()
                    .child(self.compact_left(mode, hovered, cx)),
            )
            .child(div().w(px(notch_w)).h_full())
            .child(
                div()
                    .w(px(side))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(self.compact_right(mode, hovered, cx)),
            )
    }

    fn compact_left(
        &self,
        mode: CompactMode,
        _hovered: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match mode {
            CompactMode::Media => album_chip(&self.now_playing, cx).into_any_element(),
            CompactMode::Files => lucide("image", 16.0).into_any_element(),
            CompactMode::Timer => self.compact_timer_left(cx),
            CompactMode::Observe => lucide("triangle-alert", 16.0).into_any_element(),
            CompactMode::Onboard => label("openNook", 12.0, true).into_any_element(),
            CompactMode::Idle => div().into_any_element(),
        }
    }

    fn compact_timer_left(&self, cx: &mut Context<Self>) -> AnyElement {
        let timer = self.running_timer().or_else(|| self.timers.first());
        let Some(timer) = timer else {
            return lucide("clock", 18.0).into_any_element();
        };
        let total = timer.total.max(1);
        let progress = 1.0 - timer.remaining as f32 / total as f32;
        let id = timer.id;
        div()
            .id("timer-ring")
            .size(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(t) = this.timers.iter_mut().find(|t| t.id == id) {
                        t.running = !t.running;
                    }
                    cx.notify();
                }),
            )
            .child(timer_ring(progress.clamp(0.0, 1.0), 22.0))
            .into_any_element()
    }

    fn compact_right(
        &self,
        mode: CompactMode,
        hovered: bool,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        match mode {
            CompactMode::Media => visualizer(
                self.now_playing
                    .audio_levels
                    .as_deref()
                    .unwrap_or(&[0.2; 6]),
                self.now_playing.is_playing,
            )
            .into_any_element(),
            CompactMode::Files => {
                label(self.files.len().to_string(), 13.0, true).into_any_element()
            }
            CompactMode::Timer => {
                let text = self
                    .running_timer()
                    .or_else(|| self.timers.first())
                    .map(|t| Self::format_time(t.remaining))
                    .unwrap_or_else(|| "0:00".into());
                label(text, 13.0, true).into_any_element()
            }
            CompactMode::Observe => {
                label(self.observe.firing_count().to_string(), 13.0, true).into_any_element()
            }
            CompactMode::Onboard if hovered => lucide("github", 16.0).into_any_element(),
            _ => div().into_any_element(),
        }
    }

    fn render_expanded(&mut self, notch_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab;
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(self.render_topbar(notch_w, cx))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .px_5()
                    .pb_4()
                    .overflow_hidden()
                    .child(if tab == Tab::Widgets {
                        self.render_widgets(cx).into_any_element()
                    } else {
                        self.render_files(cx).into_any_element()
                    }),
            )
    }

    fn render_topbar(&self, notch_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let widgets_active = self.tab == Tab::Widgets;
        div()
            .w_full()
            .h(px(self.notch_height.max(32.0)))
            .flex()
            .items_center()
            .justify_between()
            .px_5()
            .child(tab_switch(widgets_active, cx))
            .child(div().w(px(notch_w)))
            .child(
                div()
                    .id("settings-btn")
                    .size(px(self.notch_height.max(28.0) - 4.0))
                    .rounded_full()
                    .bg(theme::SURFACE)
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(theme::SURFACE_HOVER))
                    .active(|s| s.opacity(0.85))
                    .cursor(CursorStyle::PointingHand)
                    .child(lucide("settings", 16.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_settings(cx);
                        }),
                    ),
            )
    }

    fn render_widgets(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div()
            .id("widgets-row")
            .flex()
            .flex_row()
            .gap_4()
            .h_full()
            .overflow_x_scroll();

        if self.settings.show_media && self.has_media() {
            row = row.child(media_card(&self.now_playing, cx));
        }
        if self.settings.show_calendar {
            row = row.child(calendar_card(&self.events, cx));
        }
        if self.settings.show_reminders {
            row = row.child(reminders_card(&self.reminders, cx));
        }
        if self.settings.show_observe {
            row = row.child(observe_card(&self.observe, &self.settings, cx));
        }
        row = row.child(timer_card(&self.timers, cx));
        row = row.child(notes_card(&self.notes, cx));
        row = row.child(speed_card(
            self.speed_mbps,
            self.speed_progress,
            self.speed_running,
            cx,
        ));
        row
    }

    fn mode_dots(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let modes = self.available_modes();
        let current = self.mode();
        if modes.len() <= 1 {
            return div().into_any_element();
        }
        let mut row = div()
            .absolute()
            .bottom(px(3.))
            .flex()
            .gap(px(3.))
            .items_center()
            .justify_center();
        for mode in modes {
            let active = mode == current;
            let name = match mode {
                CompactMode::Idle => "idle",
                CompactMode::Media => "media",
                CompactMode::Files => "files",
                CompactMode::Timer => "timer",
                CompactMode::Observe => "observe",
                CompactMode::Onboard => "onboard",
            };
            row = row.child(
                div()
                    .id(SharedString::from(format!("dot-{name}")))
                    .h(px(4.))
                    .w(px(if active { 8. } else { 4. }))
                    .rounded_full()
                    .bg(if active {
                        rgba(0xffffffE6)
                    } else {
                        rgba(0xffffff4D)
                    })
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.preferred = Some(mode);
                            nook_core::haptics::trigger(None);
                            cx.notify();
                        }),
                    ),
            );
        }
        row.into_any_element()
    }

    fn render_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.files.is_empty() {
            let hot = self.file_drag;
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_1()
                .mx_4()
                .rounded(px(theme::INNER_RADIUS))
                .when(hot, |d| d.border_1().border_color(rgb(0xffffff)))
                .child(lucide(if hot { "plus" } else { "upload" }, 22.0))
                .child(label(
                    if hot {
                        "Release to add"
                    } else {
                        "Drop files onto the island"
                    },
                    13.0,
                    true,
                ))
                .child(label(
                    "They stay here until you open or clear them.",
                    11.0,
                    false,
                ))
                .into_any_element();
        }
        let mut list = div()
            .id("files-list")
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .overflow_y_scroll();
        for file in &self.files {
            let path = file.path.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("file-{}", file.path)))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .rounded(px(theme::INNER_RADIUS))
                    .bg(theme::SURFACE)
                    .hover(|s| s.bg(theme::SURFACE_HOVER))
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let _ = nook_core::files::open_file(path.clone());
                        }),
                    )
                    .child(label(file.name.clone(), 12.0, true))
                    .child(label(format_size(file.size), 11.0, false)),
            );
        }
        list.into_any_element()
    }
}

/// One filled silhouette: flat top, concave 6px wings, rounded bottom.
/// GPUI's per-corner radius was turning the compact island into a capsule.
fn island_chrome(body_w: f32, body_h: f32, wing: f32, color: gpui::Rgba) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            let g = wing;
            let w = body_w;
            let h = body_h;
            let r = theme::COMPACT_RADIUS.min(h * 0.5);
            let k = 0.552_284_75;
            let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
            let cubic = |path: &mut PathBuilder, to: (f32, f32), c1: (f32, f32), c2: (f32, f32)| {
                path.cubic_bezier_to(p(to.0, to.1), p(c1.0, c1.1), p(c2.0, c2.1));
            };

            let mut path = PathBuilder::fill();
            path.move_to(p(0.0, 0.0));
            path.line_to(p(g + w + g, 0.0));
            if g > 0.5 {
                let kk = k * g;
                cubic(
                    &mut path,
                    (g + w, g),
                    (g + w + g - kk, 0.0),
                    (g + w, g - kk),
                );
            }
            path.line_to(p(g + w, h - r));
            let rk = k * r;
            cubic(
                &mut path,
                (g + w - r, h),
                (g + w, h - r + rk),
                (g + w - r + rk, h),
            );
            path.line_to(p(g + r, h));
            cubic(&mut path, (g, h - r), (g + r - rk, h), (g, h - r + rk));
            path.line_to(p(g, g.max(0.0)));
            if g > 0.5 {
                let kk = k * g;
                cubic(&mut path, (0.0, 0.0), (g, g - kk), (kk, 0.0));
            } else {
                path.line_to(p(0.0, 0.0));
            }
            path.close();
            match path.build() {
                Ok(built) => window.paint_path(built, color),
                Err(err) => log::warn!("island path: {err}"),
            }
        },
    )
    .w(px(body_w + wing * 2.0))
    .h(px(body_h))
}

/// Original compact timer: 24px ring, progress from 12 o'clock.
fn timer_ring(progress: f32, size: f32) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let cx: f32 = bounds.center().x.into();
            let cy: f32 = bounds.center().y.into();
            let dim: f32 = bounds.size.width.into();
            let radius = dim * 0.36;
            let stroke = dim * 0.14;
            let p = |x: f32, y: f32| point(px(x), px(y));

            let mut circle = |color: gpui::Rgba, start: f32, end: f32| {
                let steps = ((end - start).abs() * 24.0).ceil().max(2.0) as i32;
                let mut path = PathBuilder::stroke(px(stroke));
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let a = start + (end - start) * t;
                    let x = cx + radius * a.cos();
                    let y = cy + radius * a.sin();
                    if i == 0 {
                        path.move_to(p(x, y));
                    } else {
                        path.line_to(p(x, y));
                    }
                }
                if let Ok(built) = path.build() {
                    window.paint_path(built, color);
                }
            };

            // SVG circles start at 3 o'clock; rotate so 0 is 12 o'clock.
            let start = -std::f32::consts::FRAC_PI_2;
            circle(rgba(0xffffff40), start, start + std::f32::consts::TAU);
            if progress > 0.01 {
                circle(
                    rgb(0xffffff),
                    start,
                    start + progress * std::f32::consts::TAU,
                );
            }
        },
    )
    .w(px(size))
    .h(px(size))
}

fn drop_veil() -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .bg(rgba(0x00000099))
        .flex()
        .items_center()
        .justify_center()
        .child(label("Drop files", 12.0, true))
}

fn tab_switch(widgets_active: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .relative()
        .flex()
        .bg(theme::SURFACE)
        .rounded_full()
        .p(px(2.))
        .w(px(80.))
        .h(px(28.))
        .child(
            div()
                .id("tab-widgets")
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .when(widgets_active, |d| d.bg(theme::SURFACE_HOVER))
                .cursor(CursorStyle::PointingHand)
                .child(lucide("layout-grid", 14.0))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.tab = Tab::Widgets;
                        cx.notify();
                    }),
                ),
        )
        .child(
            div()
                .id("tab-files")
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .when(!widgets_active, |d| d.bg(theme::SURFACE_HOVER))
                .cursor(CursorStyle::PointingHand)
                .child(lucide("files", 14.0))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.tab = Tab::Files;
                        cx.notify();
                    }),
                ),
        )
}

fn widget_shell(
    icon: &'static str,
    title: impl Into<SharedString>,
    child: impl IntoElement,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .w(px(220.))
        .h_full()
        .p_3()
        .bg(theme::SURFACE)
        .rounded(px(theme::WIDGET_RADIUS))
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .mb_2()
                .child(lucide(icon, 12.0))
                .child(
                    div()
                        .text_color(theme::TEXT_FAINT)
                        .text_size(px(10.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title.into()),
                ),
        )
        .child(child)
}

fn media_card(np: &NowPlayingData, cx: &mut Context<Island>) -> impl IntoElement {
    let title = np.title.clone().unwrap_or_else(|| "Not playing".into());
    let artist = np.artist.clone().unwrap_or_default();
    let playing = np.is_playing;
    let elapsed = np.elapsed_time.unwrap_or(0.0);
    let duration = np.duration.unwrap_or(0.0);
    let progress = if duration > 0.0 {
        (elapsed / duration) as f32
    } else {
        0.0
    };

    widget_shell(
        "music",
        "Now Playing",
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(label(title, 13.0, true))
            .child(label(artist, 11.0, false))
            .child(
                div()
                    .w_full()
                    .h(px(3.))
                    .rounded_full()
                    .bg(theme::DIVIDER)
                    .child(
                        div()
                            .h_full()
                            .w(relative(progress.clamp(0.0, 1.0)))
                            .rounded_full()
                            .bg(rgb(0xffffff)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_center()
                    .gap_3()
                    .child(icon_btn("skip-back", cx, |_, _, _| {
                        nook_core::runtime().spawn(async {
                            let _ = nook_core::audio::media_previous_track().await;
                        });
                    }))
                    .child(icon_btn(
                        if playing { "pause" } else { "play" },
                        cx,
                        |_, _, _| {
                            nook_core::runtime().spawn(async {
                                let _ = nook_core::audio::media_play_pause().await;
                            });
                        },
                    ))
                    .child(icon_btn("skip-forward", cx, |_, _, _| {
                        nook_core::runtime().spawn(async {
                            let _ = nook_core::audio::media_next_track().await;
                        });
                    })),
            ),
    )
}

fn calendar_card(events: &[CalendarEvent], cx: &mut Context<Island>) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_1();
    if events.is_empty() {
        body = body.child(label("No upcoming events", 12.0, false));
    } else {
        for event in events.iter().take(4) {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .child(label(event.title.clone(), 12.0, true))
                    .child(label(format_ts(event.start_date), 10.0, false)),
            );
        }
    }
    widget_shell(
        "calendar",
        "Calendar",
        div().flex().flex_col().gap_2().child(body).child(text_btn(
            "Open Calendar",
            cx,
            |_, _, _| {
                nook_core::runtime().spawn(async {
                    let _ = nook_core::calendar::open_calendar_app().await;
                });
            },
        )),
    )
}

fn reminders_card(reminders: &[Reminder], cx: &mut Context<Island>) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_1();
    let open: Vec<_> = reminders
        .iter()
        .filter(|r| !r.is_completed)
        .take(4)
        .collect();
    if open.is_empty() {
        body = body.child(label("Inbox zero", 12.0, false));
    } else {
        for reminder in open {
            let id = reminder.id.clone();
            body = body.child(
                div()
                    .id(SharedString::from(format!("rem-{id}")))
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let id = id.clone();
                            this.reminders.retain(|r| r.id != id);
                            nook_core::runtime().spawn(async move {
                                let _ = nook_core::calendar::complete_reminder(id).await;
                            });
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .size(px(10.))
                            .rounded_full()
                            .border_1()
                            .border_color(rgb(0xffffff)),
                    )
                    .child(label(reminder.title.clone(), 12.0, true)),
            );
        }
    }
    widget_shell("list-checks", "Reminders", body)
}

fn timer_card(timers: &[Timer], cx: &mut Context<Island>) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_2();
    if timers.is_empty() {
        body = body.child(label("No timers", 12.0, false));
    } else {
        for t in timers.iter().take(3) {
            let id = t.id;
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(Island::format_time(t.remaining), 16.0, true))
                    .child(icon_btn(
                        if t.running { "pause" } else { "play" },
                        cx,
                        move |this, _, cx| {
                            if let Some(timer) = this.timers.iter_mut().find(|x| x.id == id) {
                                timer.running = !timer.running;
                            }
                            cx.notify();
                        },
                    )),
            );
        }
    }
    body = body.child(
        div()
            .flex()
            .gap_1()
            .child(text_btn("5m", cx, |this, _, cx| {
                this.add_timer(300);
                cx.notify();
            }))
            .child(text_btn("15m", cx, |this, _, cx| {
                this.add_timer(900);
                cx.notify();
            }))
            .child(text_btn("25m", cx, |this, _, cx| {
                this.add_timer(1500);
                cx.notify();
            })),
    );
    widget_shell("clock", "Timers", body)
}

fn notes_card(notes: &str, _cx: &mut Context<Island>) -> impl IntoElement {
    let preview = if notes.trim().is_empty() {
        "Click settings to edit notes.".to_string()
    } else {
        notes.chars().take(120).collect()
    };
    widget_shell("notebook", "Notes", label(preview, 12.0, false))
}

fn speed_card(
    mbps: Option<f64>,
    progress: f64,
    running: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let status = if running {
        format!("{:.0}%", progress)
    } else if let Some(v) = mbps {
        format!("{v:.1} Mbps")
    } else {
        "Tap to test".into()
    };
    widget_shell(
        "gauge",
        "Speed",
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(label(status, 16.0, true))
            .child(text_btn(
                if running { "Testing…" } else { "Run test" },
                cx,
                |this, event, cx| {
                    cx.stop_propagation();
                    if this.speed_running {
                        return;
                    }
                    this.speed_running = true;
                    this.speed_progress = 0.0;
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        let result = cx
                            .background_executor()
                            .spawn(async {
                                nook_core::runtime()
                                    .block_on(nook_core::widgets::run_speed_test(|_| {}))
                            })
                            .await;
                        this.update(cx, |this, cx| {
                            this.speed_running = false;
                            this.speed_progress = 100.0;
                            this.speed_mbps = result.ok();
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                },
            )),
    )
}

fn observe_card(
    snap: &ObserveSnapshot,
    settings: &AppSettings,
    _cx: &mut Context<Island>,
) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_1();
    let url = settings.observe.prometheus_url.trim();
    if url.is_empty() {
        body = body.child(label("Set a Prometheus URL in Settings", 12.0, false));
    } else {
        if let Some(err) = &snap.error {
            body = body.child(label(err.clone(), 11.0, false));
        }
        if snap.has_outage() {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().size(px(8.)).rounded_full().bg(rgb(0xff453a)))
                    .child(label(format!("{} firing", snap.firing_count()), 11.0, true)),
            );
            for alert in snap.alerts.iter().take(3) {
                let line = if alert.summary.is_empty() {
                    alert.name.clone()
                } else {
                    format!("{} — {}", alert.name, alert.summary)
                };
                body = body.child(label(line, 12.0, true));
            }
        } else if snap.connected {
            body = body.child(label("No firing alerts", 11.0, false));
        }
        for reading in snap.metrics.iter().take(4) {
            let value = if let Some(err) = &reading.error {
                err.clone()
            } else if let Some(first) = reading.values.first() {
                let extra = if reading.values.len() > 1 {
                    format!(" (+{})", reading.values.len() - 1)
                } else {
                    String::new()
                };
                format!("{}{extra}", nook_core::observe::format_sample(first.value))
            } else {
                "—".into()
            };
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(reading.label.clone(), 11.0, false))
                    .child(label(value, 12.0, true)),
            );
        }
    }
    widget_shell("activity", "Observe", body)
}

fn album_chip(np: &NowPlayingData, cx: &mut Context<Island>) -> impl IntoElement {
    let playing = np.is_playing;
    div()
        .id("album")
        .size(px(28.))
        .rounded(px(8.))
        .bg(theme::SURFACE_HOVER)
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_play_pause().await;
                });
            }),
        )
        .child(lucide(if playing { "pause" } else { "music" }, 14.0))
}

fn visualizer(levels: &[f64], playing: bool) -> impl IntoElement {
    let mut row = div().flex().items_end().gap(px(1.)).h(px(22.));
    for (i, level) in levels.iter().take(6).enumerate() {
        let h = if playing {
            4.0 + (*level as f32) * 18.0
        } else {
            4.0
        };
        row = row.child(
            div()
                .id(SharedString::from(format!("bar-{i}")))
                .w(px(3.))
                .h(px(h))
                .rounded_full()
                .bg(rgb(0xffffff)),
        );
    }
    row
}

fn label(text: impl Into<SharedString>, size: f32, strong: bool) -> impl IntoElement {
    div()
        .text_color(if strong {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        })
        .text_size(px(size))
        .font_weight(if strong {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .whitespace_nowrap()
        .overflow_hidden()
        .child(text.into())
}

fn icon_btn(
    name: &'static str,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("ibtn-{name}")))
        .size(px(28.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(theme::SURFACE_HOVER))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(lucide(name, 14.0))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, event, cx);
            }),
        )
}

fn text_btn(
    caption: impl Into<SharedString>,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    let caption = caption.into();
    div()
        .id(caption.clone())
        .px_2()
        .py_1()
        .rounded(px(8.))
        .bg(theme::SURFACE_HOVER)
        .hover(|s| s.opacity(0.9))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(label(caption, 11.0, true))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, event, cx);
            }),
        )
}

fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn format_ts(ts: f64) -> String {
    use chrono::{Local, TimeZone};
    if let Some(dt) = Local.timestamp_opt(ts as i64, 0).single() {
        dt.format("%a %H:%M").to_string()
    } else {
        String::new()
    }
}

struct SettingsView {
    url_focus: FocusHandle,
    query_focus: FocusHandle,
    url_draft: String,
    query_draft: String,
    catalog: Vec<String>,
    catalog_error: Option<String>,
    catalog_loading: bool,
}

impl SettingsView {
    fn new(cx: &mut Context<Self>) -> Self {
        let settings = nook_core::settings::get_app_settings();
        Self {
            url_focus: cx.focus_handle(),
            query_focus: cx.focus_handle(),
            url_draft: settings.observe.prometheus_url,
            query_draft: String::new(),
            catalog: Vec::new(),
            catalog_error: None,
            catalog_loading: false,
        }
    }

    fn persist_url(&self) {
        let draft = self.url_draft.trim().to_string();
        if !draft.is_empty() && nook_core::observe::normalize_base_url(&draft).is_err() {
            return;
        }
        let mut s = nook_core::settings::get_app_settings();
        if s.observe.prometheus_url == draft {
            return;
        }
        s.observe.prometheus_url = draft;
        nook_core::settings::update_app_settings(s);
    }

    fn persist_observe(tweak: impl FnOnce(&mut AppSettings)) {
        let mut s = nook_core::settings::get_app_settings();
        tweak(&mut s);
        nook_core::settings::update_app_settings(s);
    }

    fn apply_key(draft: &mut String, event: &KeyDownEvent, cx: &Context<Self>) -> bool {
        let ks = &event.keystroke;
        if ks.modifiers.secondary() && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                *draft = text.trim().to_string();
                return true;
            }
            return false;
        }
        if ks.modifiers.platform || ks.modifiers.control {
            return false;
        }
        match ks.key.as_str() {
            "backspace" => {
                draft.pop();
                true
            }
            _ => {
                if let Some(ch) = &ks.key_char {
                    if !ch.chars().any(|c| c.is_control()) {
                        draft.push_str(ch);
                        return true;
                    }
                }
                false
            }
        }
    }

    fn browse_metrics(&mut self, cx: &mut Context<Self>) {
        if self.catalog_loading {
            return;
        }
        self.persist_url();
        self.catalog_loading = true;
        self.catalog_error = None;
        cx.notify();
        let config = nook_core::settings::get_app_settings().observe;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    nook_core::runtime().block_on(nook_core::observe::list_metric_names(&config))
                })
                .await;
            this.update(cx, |this, cx| {
                this.catalog_loading = false;
                match result {
                    Ok(names) => {
                        this.catalog = names;
                        this.catalog_error = None;
                    }
                    Err(err) => this.catalog_error = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = nook_core::settings::get_app_settings();
        let url_focused = self.url_focus.is_focused(window);
        let query_focused = self.query_focus.is_focused(window);
        div()
            .id("settings-root")
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x111111))
            .text_color(rgb(0xffffff))
            .p_6()
            .gap_3()
            .overflow_y_scroll()
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Settings"),
            )
            .child(toggle_row("Media", settings.show_media, cx, |s| {
                s.show_media = !s.show_media;
            }))
            .child(toggle_row("Calendar", settings.show_calendar, cx, |s| {
                s.show_calendar = !s.show_calendar;
            }))
            .child(toggle_row("Reminders", settings.show_reminders, cx, |s| {
                s.show_reminders = !s.show_reminders;
            }))
            .child(toggle_row(
                "Observability",
                settings.show_observe,
                cx,
                |s| {
                    s.show_observe = !s.show_observe;
                },
            ))
            .child(toggle_row(
                "Liquid glass",
                settings.liquid_glass_mode,
                cx,
                |s| s.liquid_glass_mode = !s.liquid_glass_mode,
            ))
            .child(toggle_row(
                "Non-notch mode",
                settings.non_notch_mode,
                cx,
                |s| {
                    s.non_notch_mode = !s.non_notch_mode;
                    s.window.non_notch_mode = s.non_notch_mode;
                },
            ))
            .child(self.render_observe_settings(&settings, url_focused, query_focused, cx))
            .child(
                div()
                    .mt_2()
                    .text_size(px(11.))
                    .text_color(theme::TEXT_FAINT)
                    .child("Native GPUI rewrite of openNook. Same Rust core, no WebView."),
            )
    }
}

impl SettingsView {
    fn render_observe_settings(
        &self,
        settings: &AppSettings,
        url_focused: bool,
        query_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut pinned = div().flex().flex_col().gap_1();
        if settings.observe.metrics.is_empty() {
            pinned = pinned.child(label("No pinned metrics", 11.0, false));
        } else {
            for metric in &settings.observe.metrics {
                let query = metric.query.clone();
                pinned = pinned.child(
                    div()
                        .id(SharedString::from(format!("pin-{}", metric.query)))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .py_1()
                        .rounded(px(8.))
                        .bg(theme::SURFACE)
                        .child(label(
                            format!("{}  {}", metric.label, metric.query),
                            11.0,
                            true,
                        ))
                        .child(
                            div()
                                .id(SharedString::from(format!("unpin-{}", metric.query)))
                                .cursor(CursorStyle::PointingHand)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |_, _, _, cx| {
                                        cx.stop_propagation();
                                        SettingsView::persist_observe(|s| {
                                            nook_core::observe::unpin_metric(
                                                &mut s.observe,
                                                &query,
                                            );
                                        });
                                        cx.notify();
                                    }),
                                )
                                .child(label("Remove", 11.0, false)),
                        ),
                );
            }
        }

        let mut catalog = div().flex().flex_col().gap_1();
        if self.catalog_loading {
            catalog = catalog.child(label("Loading metric names…", 11.0, false));
        } else if let Some(err) = &self.catalog_error {
            catalog = catalog.child(label(err.clone(), 11.0, false));
        }
        for name in self.catalog.iter().take(12) {
            let query = name.clone();
            let label_text = name.clone();
            catalog = catalog.child(
                div()
                    .id(SharedString::from(format!("cat-{name}")))
                    .px_2()
                    .py_1()
                    .rounded(px(8.))
                    .bg(theme::SURFACE)
                    .hover(|s| s.bg(theme::SURFACE_HOVER))
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            let query = query.clone();
                            let label_text = label_text.clone();
                            SettingsView::persist_observe(|s| {
                                let _ = nook_core::observe::pin_metric(
                                    &mut s.observe,
                                    &label_text,
                                    &query,
                                );
                            });
                            cx.notify();
                        }),
                    )
                    .child(label(name.clone(), 11.0, true)),
            );
        }

        let url_placeholder = self.url_draft.is_empty();
        let url_text = if url_placeholder {
            "http://127.0.0.1:9090"
        } else {
            self.url_draft.as_str()
        };
        let query_placeholder = self.query_draft.is_empty();
        let query_text = if query_placeholder {
            "PromQL, e.g. up"
        } else {
            self.query_draft.as_str()
        };

        div()
            .mt_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(label("Prometheus", 12.0, true))
            .child(label(
                "Point at Prometheus. Grafana / Alertmanager / fm-observe are later sources.",
                11.0,
                false,
            ))
            .child(
                div()
                    .id("prom-url")
                    .track_focus(&self.url_focus)
                    .px_2()
                    .py_2()
                    .rounded(px(8.))
                    .bg(theme::SURFACE)
                    .when(url_focused, |d| {
                        d.border_1().border_color(theme::ACCENT_FALLBACK)
                    })
                    .cursor(CursorStyle::IBeam)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.url_focus);
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if SettingsView::apply_key(&mut this.url_draft, event, cx) {
                            this.persist_url();
                            cx.notify();
                        } else if event.keystroke.key == "enter" {
                            this.persist_url();
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .text_color(if url_placeholder {
                                theme::TEXT_FAINT
                            } else {
                                theme::TEXT
                            })
                            .text_size(px(12.))
                            .child(SharedString::from(url_text.to_string())),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(settings_chip("Paste URL", cx, |this, _, cx| {
                        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                            this.url_draft = text.trim().to_string();
                            this.persist_url();
                            cx.notify();
                        }
                    }))
                    .child(settings_chip("Browse names", cx, |this, _, cx| {
                        this.browse_metrics(cx);
                    })),
            )
            .child(label("Pinned metrics", 12.0, true))
            .child(pinned)
            .child(
                div()
                    .id("prom-query")
                    .track_focus(&self.query_focus)
                    .px_2()
                    .py_2()
                    .rounded(px(8.))
                    .bg(theme::SURFACE)
                    .when(query_focused, |d| {
                        d.border_1().border_color(theme::ACCENT_FALLBACK)
                    })
                    .cursor(CursorStyle::IBeam)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.query_focus);
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "enter" {
                            let query = this.query_draft.trim().to_string();
                            if !query.is_empty() {
                                SettingsView::persist_observe(|s| {
                                    let _ = nook_core::observe::pin_metric(
                                        &mut s.observe,
                                        &query,
                                        &query,
                                    );
                                });
                                this.query_draft.clear();
                                cx.notify();
                            }
                        } else if SettingsView::apply_key(&mut this.query_draft, event, cx) {
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .text_color(if query_placeholder {
                                theme::TEXT_FAINT
                            } else {
                                theme::TEXT
                            })
                            .text_size(px(12.))
                            .child(SharedString::from(query_text.to_string())),
                    ),
            )
            .child(settings_chip("Pin query", cx, |this, _, cx| {
                let query = this.query_draft.trim().to_string();
                if query.is_empty() {
                    return;
                }
                SettingsView::persist_observe(|s| {
                    let _ = nook_core::observe::pin_metric(&mut s.observe, &query, &query);
                });
                this.query_draft.clear();
                cx.notify();
            }))
            .child(catalog)
    }
}

fn settings_chip(
    caption: impl Into<SharedString>,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    let caption = caption.into();
    div()
        .id(caption.clone())
        .px_2()
        .py_1()
        .rounded(px(8.))
        .bg(theme::SURFACE_HOVER)
        .hover(|s| s.opacity(0.9))
        .cursor(CursorStyle::PointingHand)
        .child(label(caption, 11.0, true))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                on_click(this, window, cx);
            }),
        )
}

fn toggle_row(
    label_text: &'static str,
    on: bool,
    cx: &mut Context<SettingsView>,
    tweak: impl Fn(&mut AppSettings) + 'static,
) -> impl IntoElement {
    div()
        .id(label_text)
        .flex()
        .items_center()
        .justify_between()
        .py_2()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                let mut s = nook_core::settings::get_app_settings();
                tweak(&mut s);
                nook_core::settings::update_app_settings(s);
                cx.notify();
            }),
        )
        .child(label(label_text, 13.0, true))
        .child(
            div()
                .w(px(36.))
                .h(px(20.))
                .rounded_full()
                .bg(if on {
                    theme::ACCENT_FALLBACK
                } else {
                    theme::SURFACE
                })
                .flex()
                .items_center()
                .when(on, |d| d.justify_end())
                .px(px(2.))
                .child(div().size(px(16.)).rounded_full().bg(rgb(0xffffff))),
        )
}

pub fn open_island(cx: &mut App) {
    let (w, h) = notch::overlay_window_size();
    let bounds = Bounds::from_corners(point(px(0.), px(0.)), point(px(w as f32), px(h as f32)));

    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            window_decorations: None,
            app_id: Some("com.jonasvogel.opennook-gpui".into()),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| Island::new(window, cx)),
    )
    .expect("open island window");
}
