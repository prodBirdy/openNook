//! Dynamic Island entity: state, polling, springs, gestures.

mod chrome;
mod compact;
mod expanded;
mod files;
mod media;
mod render;
mod settings;
pub(crate) mod ui;

pub use render::open_island;

pub(crate) use files::file_grid_metrics;
pub(crate) use ui::format_timer;

use crate::platform;
use gpui::{
    prelude::*, px, size, Context, ExternalPaths, Subscription, TouchPhase, Window, WindowBounds,
    WindowHandle, WindowKind, WindowOptions,
};
use nook_core::agents::AgentSession;
use nook_core::calendar::{CalendarEvent, Reminder};
use nook_core::files::FileTrayItem;
use nook_core::models::NowPlayingData;
use nook_core::notch;
use nook_core::settings::AppSettings;
use settings::SettingsView;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactMode {
    Idle,
    Media,
    Agents,
    Files,
    Timer,
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
    pub timers: Vec<Timer>,
    pub next_timer_id: u64,
    pub settings: AppSettings,
    pub first_run: bool,
    pub speed_mbps: Option<f64>,
    pub speed_progress: f64,
    pub speed_running: bool,
    pub last_tick: Instant,
    last_frame: Instant,
    pub settings_open: bool,
    settings_window: Option<WindowHandle<SettingsView>>,
    _settings_closed: Option<Subscription>,
    screen_gen: u64,
    anim_w: f32,
    anim_h: f32,
    anim_vw: f32,
    anim_vh: f32,
    content_opacity: f32,
    /// How hard the size spring is moving right now, 0..1. Drives the motion
    /// blur in `content_stack`; exactly 0 once the spring has settled.
    blur: f32,
    last_expanded: bool,
    last_mode: CompactMode,
    file_drag: bool,
    /// Mirrors NSWindow.ignoresMouseEvents so we only cross into ObjC on change.
    click_through: bool,
    /// Ignore extra wheel events from the same two-finger swipe / momentum.
    wheel_locked: bool,
    last_wheel_at: Instant,
    wheel_acc_x: f32,
    wheel_acc_y: f32,
    /// Origin for the working-agent Dot Matrix loader (seconds * speed).
    pixel_origin: Instant,
    pixel_t: f32,
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
            timers: Vec::new(),
            next_timer_id: 1,
            settings,
            first_run,
            speed_mbps: None,
            speed_progress: 0.0,
            speed_running: false,
            last_tick: Instant::now(),
            last_frame: Instant::now(),
            settings_open: false,
            settings_window: None,
            _settings_closed: None,
            screen_gen: notch::screen_generation(),
            anim_w: 0.0,
            anim_h: 0.0,
            anim_vw: 0.0,
            anim_vh: 0.0,
            content_opacity: 1.0,
            blur: 0.0,
            last_expanded: false,
            last_mode: CompactMode::Idle,
            file_drag: false,
            // NSWindow starts out grabbing events; the first poll tick corrects it.
            click_through: false,
            wheel_locked: false,
            last_wheel_at: Instant::now(),
            wheel_acc_x: 0.0,
            wheel_acc_y: 0.0,
            pixel_origin: Instant::now(),
            pixel_t: 0.0,
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
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(20))
                    .await;
                let (mx, my) = nook_core::mouse::current_mouse_logical();
                if this
                    .update(cx, |this, cx| {
                    let inside = nook_core::mouse::hit_test(mx, my);
                    let on_ui = nook_core::mouse::hit_test_exact(mx, my);
                    this.settings = nook_core::settings::get_app_settings();
                    let mut dirty = false;
                    if platform::take_open_settings() {
                        this.open_settings(cx);
                        dirty = true;
                    }
                    let dragging = nook_core::mouse::drag_active();
                    if this.file_drag != dragging {
                        this.file_drag = dragging;
                        if dragging && inside {
                            this.arm_dropzone(cx);
                        }
                        dirty = true;
                    }
                    // Own the cursor only where we actually paint. The NSWindow
                    // spans the whole screen width and is ~280px tall, so
                    // anything looser swallows clicks meant for the menu bar or
                    // the app underneath. Exception: while Finder is dragging,
                    // the window must see the cursor early (padded `inside`) or
                    // it never gets draggingEntered.
                    let ignore = !(on_ui || (this.file_drag && inside) || this.settings_open);
                    if this.click_through != ignore {
                        this.click_through = ignore;
                        platform::set_click_through_current(ignore);
                        log::debug!(
                            "click-through {} at ({mx:.0},{my:.0}) on_ui={on_ui} drag={} expanded={}",
                            if ignore { "on" } else { "off" },
                            this.file_drag,
                            this.expanded
                        );
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
                    let now = Instant::now();
                    let dt = now.duration_since(this.last_frame).as_secs_f32().min(0.05);
                    this.last_frame = now;
                    let elapsed_secs = now.duration_since(this.last_tick).as_secs() as u32;
                    if elapsed_secs >= 1 {
                        this.last_tick += Duration::from_secs(elapsed_secs as u64);
                        for t in &mut this.timers {
                            if t.running && t.remaining > 0 {
                                t.remaining = t.remaining.saturating_sub(elapsed_secs);
                                if t.remaining == 0 {
                                    t.running = false;
                                    nook_core::haptics::trigger(Some(nook_core::haptics::HapticConfig {
                                        pattern: nook_core::haptics::HapticPattern::Success,
                                        intensity: 1.0,
                                    }));
                                }
                            }
                        }
                        dirty = true;
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
                        this.pixel_t = now.duration_since(this.pixel_origin).as_secs_f32();
                        dirty = true;
                    }
                    if dirty {
                        cx.notify();
                    }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let mut idle_polls = 0u32;
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
                        || this.now_playing.elapsed_time != playing.elapsed_time;
                    this.now_playing.title = playing.title;
                    this.now_playing.artist = playing.artist;
                    this.now_playing.album = playing.album;
                    this.now_playing.artwork_base64 = playing.artwork_base64;
                    this.now_playing.duration = playing.duration;
                    this.now_playing.elapsed_time = playing.elapsed_time;
                    this.now_playing.is_playing = playing.is_playing;
                    this.now_playing.app_name = playing.app_name;
                    this.visualizer_color = media::visualizer_color_from_art(
                        this.now_playing.artwork_base64.as_deref(),
                    );
                    if !was_media && this.has_media() {
                        this.preferred = Some(CompactMode::Media);
                    }
                    if changed {
                        cx.notify();
                    }
                    this.has_media()
                });
                let Ok(has_media) = alive else {
                    break;
                };
                if has_media {
                    idle_polls = 0;
                } else {
                    idle_polls = idle_polls.saturating_add(1);
                }
                let wait = if idle_polls > 8 {
                    Duration::from_secs(2)
                } else {
                    Duration::from_millis(400)
                };
                cx.background_executor().timer(wait).await;
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
                    if let Ok(notes) = nook_core::notes::load_notes() {
                        if this.notes != notes {
                            this.notes = notes;
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

    fn has_agents(&self) -> bool {
        self.settings.show_agents && !self.agents.is_empty()
    }

    fn mode_available(&self, mode: CompactMode) -> bool {
        match mode {
            CompactMode::Media => self.has_media(),
            CompactMode::Agents => self.has_agents(),
            CompactMode::Files => !self.files.is_empty(),
            CompactMode::Timer => self.running_timer().is_some(),
            CompactMode::Onboard => self.first_run,
            CompactMode::Idle => true,
        }
    }

    fn available_modes(&self) -> Vec<CompactMode> {
        let mut modes = Vec::new();
        if self.has_media() {
            modes.push(CompactMode::Media);
        }
        if self.has_agents() {
            modes.push(CompactMode::Agents);
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

    pub(super) fn expanded_width(&self) -> f32 {
        (self.screen_width - 40.0).min(600.0)
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

        // Width grows from the centre out, so the content on either flank only
        // travels at half the box velocity; height grows downwards from the
        // pinned top edge, so there it tracks it 1:1. The fade term keeps the
        // blur up through a crossfade that does not resize at all.
        const BLUR_SPEED: f32 = 900.0;
        let content_speed = (self.anim_vw * 0.5).hypot(self.anim_vh);
        self.blur = (content_speed / BLUR_SPEED)
            .min(1.0)
            .max(1.0 - self.content_opacity);

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
            self.blur = 0.0;
        }
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
        let (vx, vy) = (self.anim_vw.abs() * 0.5, self.anim_vh.abs());
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
                self.tab = if ax > 0.0 { Tab::Files } else { Tab::Widgets };
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
        let bounds = gpui::Bounds::centered(None, size(px(420.), px(480.)), cx);
        let Ok(handle) = cx.open_window(
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
            |_, cx| cx.new(|_| SettingsView),
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
        self.preferred = Some(CompactMode::Timer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nook_core::agents::{AgentKind, AgentStatus};

    fn test_island() -> Island {
        Island {
            notch_width: 180.0,
            notch_height: 32.0,
            screen_width: 1512.0,
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
            timers: Vec::new(),
            next_timer_id: 1,
            settings: AppSettings::default(),
            first_run: false,
            speed_mbps: None,
            speed_progress: 0.0,
            speed_running: false,
            last_tick: Instant::now(),
            last_frame: Instant::now(),
            settings_open: false,
            settings_window: None,
            _settings_closed: None,
            screen_gen: 0,
            anim_w: 180.0,
            anim_h: 32.0,
            anim_vw: 0.0,
            anim_vh: 0.0,
            content_opacity: 1.0,
            blur: 0.0,
            last_expanded: false,
            last_mode: CompactMode::Idle,
            file_drag: false,
            click_through: true,
            wheel_locked: false,
            last_wheel_at: Instant::now() - Duration::from_secs(1),
            wheel_acc_x: 0.0,
            wheel_acc_y: 0.0,
            pixel_origin: Instant::now(),
            pixel_t: 0.0,
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
    fn agent_loader_is_deterministic_per_pid() {
        assert_eq!(crate::dotmatrix::pick(11), crate::dotmatrix::pick(11));
        let idle =
            crate::dotmatrix::cell_opacity(crate::dotmatrix::Kind::Circular(4), 1, 4, 0.3, false);
        let work =
            crate::dotmatrix::cell_opacity(crate::dotmatrix::Kind::Circular(4), 1, 4, 0.3, true);
        assert!((idle - work).abs() > 0.01);
    }

    #[test]
    fn format_timer_pads_seconds() {
        assert_eq!(format_timer(0), "0:00");
        assert_eq!(format_timer(65), "1:05");
        assert_eq!(format_timer(1500), "25:00");
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
        assert!((island.expanded_width() - 600.0).abs() < f32::EPSILON);
    }

    #[test]
    fn spring_settles_on_target() {
        let mut island = test_island();
        island.anim_w = 0.0;
        island.anim_h = 0.0;
        island.content_opacity = 0.0;
        let mut moving = true;
        for _ in 0..200 {
            moving = island.step_spring(0.016);
            if !moving {
                break;
            }
        }
        assert!(!moving);
        let (tw, th) = island.target_size();
        assert!((island.anim_w - tw).abs() < 0.5);
        assert!((island.anim_h - th).abs() < 0.5);
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
        island.content_opacity = 0.0;

        island.step_spring(0.016);
        assert!(island.blur > 0.5, "crossfade left the content sharp");
        let (dx, dy) = island.blur_offset().expect("crossfade needs a smear");
        assert!(dx > dy, "a still island should smear along its long axis");
    }
}
