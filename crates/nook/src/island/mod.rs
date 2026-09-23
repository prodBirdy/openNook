//! Dynamic Island entity: state, polling, springs, gestures.

mod chrome;
mod compact;
mod edit;
mod expanded;
mod files;
mod marquee;
pub(crate) mod media;
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
use nook_core::automation::ExternalAction;
use nook_core::calendar::{CalendarEvent, Reminder};
use nook_core::files::FileTrayItem;
use nook_core::high_alert::HighAlertOwner;
use nook_core::meetings::MeetingSnapshot;
use nook_core::messages::MessagesSnapshot;
use nook_core::models::{NowPlayingData, PlaybackQueue, SyncedLyrics};
use nook_core::notch;
use nook_core::notifications::NotificationEvent;
use nook_core::observe::{MetricHistory, ObserveSnapshot};
use nook_core::obsidian::{NoteEntry, VaultWatch};
use nook_core::pomodoro::{PomodoroPhase, PomodoroSpec};
use nook_core::power::PowerSnapshot;
use nook_core::settings::{AppSettings, WidgetModule};
use nook_core::system_timers::{self, SystemTimer};
use nook_core::sysvol::{self, HudEvent, HudKind, HUD_TTL};
use nook_core::vpn::VpnSnapshot;
use nook_core::weather::WeatherSnapshot;
use settings::SettingsView;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// How long an optimistic media intent overrides contradicting polls.
const MEDIA_INTENT_WINDOW: Duration = Duration::from_millis(2500);
/// Polled elapsed within this many seconds of a seek target counts as caught up.
const SEEK_CATCHUP_TOL: f64 = 1.5;

/// Keep the optimistic scrubber position while a seek is still in flight.
fn hold_seek_intent(want: f64, since: Instant, polled: Option<f64>, track_changed: bool) -> bool {
    if track_changed || since.elapsed() >= MEDIA_INTENT_WINDOW {
        return false;
    }
    match polled {
        Some(elapsed) => (elapsed - want).abs() > SEEK_CATCHUP_TOL,
        None => true,
    }
}

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
    Battery,
    Vpn,
    Recording,
    Meeting,
    Notifications,
    Onboard,
    Messages,
    Share,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Widgets,
    Files,
    Terminal,
}

#[derive(Clone, Debug)]
pub(crate) enum ClockTimerAction {
    Pause(String),
    Resume(String),
    Cancel(String),
    Open(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimerKind {
    Countdown,
    Pomodoro(PomodoroSpec),
}

#[derive(Clone)]
pub struct Timer {
    pub id: u64,
    pub name: String,
    pub remaining: u32,
    pub total: u32,
    pub running: bool,
    pub kind: TimerKind,
    /// Wall-clock end of a running pomodoro phase. Instant would stall across
    /// lid-close sleep; powerd-side High Alert does not need this.
    pub ends_at: Option<SystemTime>,
}

/// Compact-face view of a local island timer or a Clock.app timer.
#[derive(Clone, Debug)]
pub struct FaceTimer {
    pub remaining: u32,
    pub total: u32,
    pub running: bool,
    #[allow(dead_code)]
    pub name: String,
    pub source: FaceTimerSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaceTimerSource {
    Local(u64),
    Clock(String),
}

pub struct Island {
    pub notch_width: f32,
    pub notch_height: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    pub hovered: bool,
    pub expanded: bool,
    /// User swipe / mode-dot / tray preference.
    pub user_preferred: Option<CompactMode>,
    /// Alias for [`Self::user_preferred`] so concurrent `files.rs` still compiles.
    pub preferred: Option<CompactMode>,
    /// Background-producer takeover (media, alerts, timers, …).
    pub alert_preferred: Option<CompactMode>,
    pub tab: Tab,
    pub now_playing: NowPlayingData,
    pub visualizer_color: Option<gpui::Rgba>,
    /// Cached lyrics for the current title/artist. `None` until a fetch lands.
    pub lyrics: Option<Arc<SyncedLyrics>>,
    lyrics_key: Option<(String, String)>,
    lyrics_anchor_elapsed: f64,
    lyrics_anchor_at: Instant,
    lyrics_timer_gen: u64,
    /// 2–3 dominant artwork colors for the ambient media glow.
    pub aura_palette: Option<[gpui::Rgba; 3]>,
    /// Seconds of aura drift, only advanced while the glow is on screen.
    aura_t: f32,
    last_aura_frame: Instant,
    /// Album we last asked the catalog about (`artist`, `album`).
    motion_art_key: Option<(String, String)>,
    motion_art_bounds: Option<(f32, f32, f32, f32)>,
    motion_art_bounds_gen: u64,
    last_motion_spec: Option<platform::MotionArtSpec>,
    pub queue: PlaybackQueue,
    queue_key: Option<(Option<String>, Option<String>, Option<String>)>,
    queue_inflight: bool,
    /// Local scrubber drag 0..1. `None` when the thumb is not held.
    pub(crate) scrubber_drag: Option<f32>,
    pub(crate) scrubber_bounds: Rc<RefCell<Option<(f32, f32)>>>,
    elapsed_base: Option<f64>,
    elapsed_at: Instant,
    /// Optimistic play/pause target: (wanted is_playing, when tapped). Holds the
    /// UI at the tapped state until a poll confirms it or MEDIA_INTENT_WINDOW passes.
    play_intent: Option<(bool, Instant)>,
    /// Optimistic scrubber seek: (wanted elapsed seconds, when released). Holds
    /// elapsed against stale polls until MediaRemote catches up or the window lapses.
    seek_intent: Option<(f64, Instant)>,
    pub files: Vec<FileTrayItem>,
    pub events: Vec<CalendarEvent>,
    pub reminders: Vec<Reminder>,
    pub agents: Vec<AgentSession>,
    pub notes: String,
    /// In-card markdown editor for the Notes card, created on first edit.
    pub(crate) notes_editor: Option<Entity<crate::widgets::NotesEditor>>,
    pub(crate) notes_editing: bool,
    notes_sub: Option<Subscription>,
    pub(crate) obsidian_notes: Vec<NoteEntry>,
    pub(crate) obsidian_dirty: bool,
    obsidian_watch: Option<VaultWatch>,
    obsidian_watch_vault: Option<PathBuf>,
    pub(crate) obsidian_capture: String,
    /// Byte index of the caret in [`Self::obsidian_capture`].
    pub(crate) obsidian_capture_caret: usize,
    /// When true, the next edit replaces the whole capture string (cmd-a).
    obsidian_capture_select_all: bool,
    pub(crate) obsidian_capture_focus: Option<gpui::FocusHandle>,
    pub(crate) obsidian_typing: bool,
    pub(crate) obsidian_selected: Option<String>,
    pub(crate) obsidian_body: Option<String>,
    pub(crate) obsidian_flash: Option<String>,
    /// Quick-add field, created the first time the expanded reminders card renders.
    pub(crate) reminders_quick_add: Option<Entity<crate::widgets::QuickAdd>>,
    reminders_qa_sub: Option<Subscription>,
    pub timers: Vec<Timer>,
    pub system_timers: Vec<SystemTimer>,
    pub next_timer_id: u64,
    /// Index into the 7-day week strip (today − 3 … today + 3). 3 is today.
    pub calendar_day: u8,
    /// Manual High Alert deadline for the card readout. `None` = until off.
    /// powerd owns expiry; this is display-only and is not a wakeup source.
    pub awake_deadline: Option<Instant>,
    pub awake_active: bool,
    pub observe: ObserveSnapshot,
    observe_history: MetricHistory,
    pub messages: MessagesSnapshot,
    pub message_draft: String,
    #[allow(dead_code)]
    pub selected_conversation: Option<String>,
    pub(crate) message_focus: Option<gpui::FocusHandle>,
    pub(crate) observe_hover: Option<crate::widgets::ObserveHover>,
    pub power: PowerSnapshot,
    pub(crate) lpm_pending: bool,
    pub(crate) lpm_error: Option<String>,
    pub vpn: VpnSnapshot,
    /// Brief compact-face takeover after a connect/disconnect edge.
    vpn_reveal_until: Option<Instant>,
    pub sysstats: nook_core::sysstats::SysSnapshot,
    pub(crate) sysstats_sampling: bool,
    pub meeting: MeetingSnapshot,
    pub notifications: Vec<NotificationEvent>,
    pub notification_unread: usize,
    pub settings: AppSettings,
    /// On-island widget customize mode (dashed chrome + picker).
    pub(crate) widget_edit: bool,
    /// Settings snapshot taken when entering edit mode; restored on Cancel.
    widget_edit_snapshot: Option<AppSettings>,
    /// Brief "No room" caption under the picker after a blocked tap.
    widget_edit_budget_hint_at: Option<Instant>,
    /// Last `widget_edit` seen by [`Self::arm_content_transition`].
    last_widget_edit: bool,
    /// Next arm forces a content crossfade even if expand/tab/mode match.
    content_transition_force: bool,
    pub first_run: bool,
    pub speed_mbps: Option<f64>,
    pub speed_progress: f64,
    pub speed_running: bool,
    /// Bumped on start and on Stop so an in-flight test cannot apply after it
    /// was cancelled (Stop then Run would otherwise take the old result).
    pub speed_gen: u64,
    pub weather: Option<WeatherSnapshot>,
    pub weather_error: Option<String>,
    pub(crate) weather_inflight: bool,
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
    /// Brand glow around the island while an agent is working, 0..1 on
    /// `motion::REVEAL`. Color is latched so the fade-out still has a tint.
    agent_border: SpringValue,
    agent_border_color: Option<gpui::Rgba>,
    /// Mute HUD flash on the meeting face; overlay springs to 1 while this is live.
    meeting_flash_until: Option<Instant>,
    /// Mirrors Accessibility › Display › "Reduce motion"; refreshed by the
    /// poll loop so springs collapse to a dissolve while it is on.
    reduce_motion: bool,
    /// How hard the size spring is moving right now, 0..1. Drives the motion
    /// blur in `content_stack`; exactly 0 once the spring has settled.
    blur: f32,
    last_expanded: bool,
    /// Cheap agent/chrome paint until an expand/collapse or mode/tab context
    /// shift rests (size + content travel). Hover morphs must not flip this —
    /// that was the compact-hover flicker.
    agent_morph_lite: bool,
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
    /// Last wall time we pushed ignoresMouseEvents. Re-assert about once a
    /// second — AppKit/GPUI can drop the flag on restyle.
    click_through_at: Instant,
    /// Ignore extra wheel events from the same two-finger swipe / momentum.
    wheel_locked: bool,
    last_wheel_at: Instant,
    wheel_acc_x: f32,
    wheel_acc_y: f32,
    /// Origin for the working-agent Dot Matrix loader (seconds * speed).
    pixel_origin: Instant,
    pixel_t: f32,
    /// Last time we advanced [`pixel_t`] enough to request a paint. Capped at
    /// ~30fps so the brand shimmer does not starve the expand morph.
    last_pixel_frame: Instant,
    pub(crate) mirror_on: bool,
    mirror_gen: u64,
    pub(crate) mirror_frame: Option<std::sync::Arc<gpui::RenderImage>>,
    pub(super) hud: Option<HudState>,
    hud_fill: SpringValue,
    hud_dragging: bool,
    pub(crate) share: nook_core::share::ShareSession,
    pub(crate) terminal: Option<Entity<crate::widgets::TerminalView>>,
    terminal_sub: Option<Subscription>,
    pub shell_running: bool,
    pub shell_exit: Option<i32>,
    pub shell_focused: bool,
    /// Output-device list for the media-card picker. Rebuilt when the HAL dirty flag flips.
    pub(crate) output_devices: Vec<nook_core::audio_devices::OutputDevice>,
    pub(crate) output_picker_open: bool,
    /// Side panel with Playing Next / Up Next; toggled by the list control.
    pub(crate) queue_open: bool,
    output_hud_name: Option<String>,
    output_hud_until: Option<Instant>,
    pub recording: bool,
    pub recording_started: Option<Instant>,
    pub live_transcript: String,
    pub recordings: Vec<nook_core::recorder::RecordingItem>,
    pub recorder_level: f32,
    pub recorder_wave: VecDeque<f32>,
    pub(crate) recorder_wave_at: Instant,
    pub recorder_error: Option<String>,
    pub playing_recording: Option<i64>,
    recorder_last_notify: Instant,
    /// Keyboard focus for the island chrome (`Island::new` fills this).
    focus: Option<gpui::FocusHandle>,
    /// Outside-bounds clock for expanded hover-exit dwell.
    hover_exit_at: Option<Instant>,
    /// Option key held over the island (reposition affordance).
    alt_held: bool,
    /// Cleared tray stash for Undo (files.rs renders the Undo chip).
    pub(crate) last_cleared_files: Option<(Vec<FileTrayItem>, Instant)>,
    /// Transient flash when a tray path vanishes.
    pub(crate) tray_flash: Option<(String, Instant)>,
    /// Calendar TCC asked once from `refresh_calendar`.
    calendar_access_requested: bool,
    /// One-shot warn when expanding while the settings DB is a temp fallback.
    fallback_db_noticed: bool,
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
                theme::NOTCH_MIN_H
            },
            screen_width: info.screen_width as f32,
            screen_height: info.screen_height as f32,
            hovered: false,
            expanded: false,
            user_preferred: None,
            preferred: None,
            alert_preferred: None,
            tab: Tab::Widgets,
            now_playing: NowPlayingData::default(),
            visualizer_color: None,
            lyrics: None,
            lyrics_key: None,
            lyrics_anchor_elapsed: 0.0,
            lyrics_anchor_at: Instant::now(),
            lyrics_timer_gen: 0,
            aura_palette: None,
            aura_t: 0.0,
            last_aura_frame: Instant::now(),
            motion_art_key: None,
            motion_art_bounds: None,
            motion_art_bounds_gen: 0,
            last_motion_spec: None,
            queue: PlaybackQueue::default(),
            queue_key: None,
            queue_inflight: false,
            scrubber_drag: None,
            scrubber_bounds: Rc::new(RefCell::new(None)),
            elapsed_base: None,
            elapsed_at: Instant::now(),
            play_intent: None,
            seek_intent: None,
            files,
            events: Vec::new(),
            reminders: Vec::new(),
            agents: Vec::new(),
            notes,
            notes_editor: None,
            notes_editing: false,
            notes_sub: None,
            obsidian_notes: Vec::new(),
            obsidian_dirty: false,
            obsidian_watch: None,
            obsidian_watch_vault: None,
            obsidian_capture: String::new(),
            obsidian_capture_caret: 0,
            obsidian_capture_select_all: false,
            obsidian_capture_focus: None,
            obsidian_typing: false,
            obsidian_selected: None,
            obsidian_body: None,
            obsidian_flash: None,
            reminders_quick_add: None,
            reminders_qa_sub: None,
            timers: Vec::new(),
            system_timers: Vec::new(),
            next_timer_id: 1,
            calendar_day: 3,
            awake_deadline: None,
            awake_active: false,
            observe: ObserveSnapshot::default(),
            observe_history: nook_core::observe::load_history(),
            messages: MessagesSnapshot::default(),
            message_draft: String::new(),
            selected_conversation: None,
            message_focus: None,
            observe_hover: None,
            power: nook_core::power::current(),
            lpm_pending: false,
            lpm_error: None,
            vpn: nook_core::vpn::current(),
            vpn_reveal_until: None,
            sysstats: nook_core::sysstats::SysSnapshot::default(),
            sysstats_sampling: false,
            meeting: MeetingSnapshot::default(),
            notifications: nook_core::notifications::snapshot(),
            notification_unread: nook_core::notifications::unread_count(),
            settings,
            widget_edit: false,
            widget_edit_snapshot: None,
            widget_edit_budget_hint_at: None,
            last_widget_edit: false,
            content_transition_force: false,
            first_run,
            speed_mbps: None,
            speed_progress: 0.0,
            speed_running: false,
            speed_gen: 0,
            weather: nook_core::weather::cached_snapshot(),
            weather_error: None,
            weather_inflight: false,
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
            agent_border: SpringValue::at(0.0),
            agent_border_color: None,
            meeting_flash_until: None,
            reduce_motion: platform::reduce_motion(),
            blur: 0.0,
            last_expanded: false,
            agent_morph_lite: false,
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
            click_through_at: Instant::now() - Duration::from_secs(2),
            wheel_locked: false,
            last_wheel_at: Instant::now(),
            wheel_acc_x: 0.0,
            wheel_acc_y: 0.0,
            pixel_origin: Instant::now(),
            pixel_t: 0.0,
            last_pixel_frame: Instant::now(),
            mirror_on: false,
            mirror_gen: 0,
            mirror_frame: None,
            hud: None,
            hud_fill: SpringValue::at(0.0),
            hud_dragging: false,
            share: nook_core::share::ShareSession::default(),
            terminal: None,
            terminal_sub: None,
            shell_running: false,
            shell_exit: None,
            shell_focused: false,
            output_devices: nook_core::audio_devices::snapshot(),
            output_picker_open: false,
            queue_open: false,
            output_hud_name: None,
            output_hud_until: None,
            recording: false,
            recording_started: None,
            live_transcript: String::new(),
            recordings: nook_core::recorder::list(),
            recorder_level: 0.0,
            recorder_wave: VecDeque::new(),
            recorder_wave_at: Instant::now(),
            recorder_error: None,
            playing_recording: None,
            recorder_last_notify: Instant::now(),
            focus: Some(cx.focus_handle()),
            hover_exit_at: None,
            alt_held: false,
            last_cleared_files: None,
            tray_flash: None,
            calendar_access_requested: false,
            fallback_db_noticed: false,
        };
        // Start at the compact idle size so the first paint isn't a jump.
        let (w, h) = this.target_size();
        this.anim_w.set(w);
        this.anim_h.set(h);

        platform::install_media_observers();
        platform::install_mouse_monitors();
        platform::install_osd_wake_observer();
        platform::install_weather_observers();
        platform::install_meeting_observers();
        nook_core::notifications::sync_backends(&this.settings);
        // Do not touch GPUI's Window handle here — HasWindowHandle RefCell-panics
        // during construction. Chrome is applied via NSApp window enumeration.
        let _ = window;
        this.spawn_loops(cx);
        Self::spawn_pin(cx);
        this.sync_obsidian_watch(cx);
        Self::spawn_external_actions(cx);
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
            platform::request_pin();
            // Park until a screen/space notification (or a 30s backstop).
            // The previous 250 ms poll was an idle wakeup; handlers already
            // pin on the main thread, this only covers a missed first pin.
            loop {
                let needed = cx
                    .background_executor()
                    .spawn(async { platform::wait_pin_needed(Duration::from_secs(30)) })
                    .await;
                if this.update(cx, |_, _| ()).is_err() {
                    break;
                }
                // Always clear PIN_NEEDED; `||` would skip take and spin forever.
                let taken = platform::take_pin_needed();
                if needed || taken {
                    platform::pin_island_windows();
                }
            }
        })
        .detach();
    }

    fn spawn_loops(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            // Event-driven: park on ui_tick wake (mouse near/drag, settings)
            // with a 1 s backstop when idle. Keep 20 ms only while something
            // is live or the cursor is near enough to reach the island.
            let mut wait_ms = 20u64;
            loop {
                let timeout = Duration::from_millis(wait_ms);
                cx.background_executor()
                    .spawn(async move {
                        nook_core::runtime()
                            .block_on(nook_core::ui_tick::wait_or_timeout(timeout))
                    })
                    .await;
                match this
                    .update(cx, |this, cx| {
                    // Finder's drag-tracking loop often swallows leftMouseDragged
                    // before our NSEvent monitors see it, and the mouse thread
                    // must not touch NSPasteboard. Sample here on the main
                    // thread so an inbound file drag still arms the tray.
                    nook_core::mouse::sample_now();
                    let (mx, my) = nook_core::mouse::current_mouse_logical();
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
                        this.sync_lyrics(cx);
                        nook_core::osd::apply_from_settings();
                        if !this.settings.show_volume_brightness_hud {
                            this.hud = None;
                            this.hud_dragging = false;
                        }
                        this.sync_obsidian_watch(cx);
                        nook_core::high_alert::set_low_battery_release_pct(
                            this.settings.low_battery_release_pct,
                        );
                        if !this.settings.terminal_enabled {
                            this.close_terminal(cx);
                        }
                        this.correct_hidden_tab();
                        // Per-settings-change syncs (VPN, motion art).
                        nook_core::vpn::refresh();
                        this.sync_motion_art_from_settings(cx);
                        nook_core::notifications::sync_backends(&this.settings);
                        dirty = true;
                    }
                    if this.repositioning {
                        this.apply_reposition(mx as f32, my as f32);
                        dirty = true;
                    }
                    if !platform::island_glass_setting_on() {
                        platform::sync_island_glass(None);
                    }
                    if this.sync_output_devices() {
                        dirty = true;
                    }
                    let want_suppress = this.settings.hide_when_maximized
                        && !this.repositioning
                        && !this.settings_open
                        && !this.widget_edit
                        && nook_core::occupancy::frontmost_fills_display();
                    if this.suppressed != want_suppress {
                        this.suppressed = want_suppress;
                        if want_suppress {
                            this.hovered = false;
                            if this.expanded {
                                this.expanded = false;
                                this.close_notes_editor(cx);
                                this.stop_mirror(cx);
                                this.park_terminal();
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
                    let now = Instant::now();
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
                    // Re-assert about once a second — AppKit/GPUI can drop the flag.
                    if changed || now.duration_since(this.click_through_at) >= Duration::from_secs(1)
                    {
                        this.click_through = ignore;
                        this.click_through_at = now;
                        platform::set_click_through_current(ignore);
                        if changed {
                            log::debug!(
                                "click-through {} at ({mx:.0},{my:.0}) on_ui={on_ui} drag={} expanded={}",
                                if ignore { "on" } else { "off" },
                                this.file_drag,
                                this.expanded
                            );
                        }
                    }
                    if !this.suppressed && !this.repositioning {
                        if inside {
                            this.hover_exit_at = None;
                            if !this.hovered {
                                this.hovered = true;
                                nook_core::haptics::trigger(None);
                                if this.file_drag {
                                    this.arm_dropzone(cx);
                                }
                                dirty = true;
                            }
                        } else if this.hovered {
                            let can_collapse = this.expanded
                                && !this.settings_open
                                && !this.widget_edit
                                && !this.notes_editing
                                && !this.obsidian_typing
                                && !this.shell_focused
                                && !this.mirror_on
                                && !this.file_drag
                                && !nook_core::files::outbound_drag_active();
                            if can_collapse {
                                let exit_at = *this.hover_exit_at.get_or_insert(now);
                                if now.duration_since(exit_at) >= motion::HOVER_EXIT_DWELL {
                                    this.hover_exit_at = None;
                                    this.hovered = false;
                                    this.expanded = false;
                                    this.close_notes_editor(cx);
                                    this.park_terminal();
                                    this.obsidian_typing = false;
                                    dirty = true;
                                }
                            } else {
                                this.hover_exit_at = None;
                                this.hovered = false;
                                dirty = true;
                            }
                        }
                    }
                    this.sync_alert_preferred();
                    // `SpringValue::step` substeps at 120Hz internally, so a
                    // large dt is numerically fine — but it would fast-forward
                    // a freshly retargeted spring, visibly skipping the start
                    // of an animation after an idle tick. Cap perceived
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
                        let paint_live = this.paints_live_widgets();
                        for t in &mut this.timers {
                            if t.running && t.remaining > 0 {
                                t.remaining = t.remaining.saturating_sub(elapsed_secs);
                                if paint_live {
                                    dirty = true;
                                }
                                if t.remaining == 0 {
                                    t.running = false;
                                    crate::notify::cancel_island_timer(t.id);
                                    nook_core::haptics::trigger(Some(nook_core::haptics::HapticConfig {
                                        pattern: nook_core::haptics::HapticPattern::Success,
                                    }));
                                    dirty = true;
                                }
                            }
                    }
                        if this.apply_timer_tick(SystemTime::now(), elapsed_secs) && paint_live {
                            dirty = true;
                        }
                        if this.sync_high_alert_ui(Instant::now()) {
                            dirty = true;
                        } else if this.paints_live_widgets() && this.expanded && this.awake_active {
                            // Card readout only; idle face is an on/off glyph.
                            dirty = true;
                        }
                        if this.paints_live_widgets() && this.clock_timer_visible() {
                            dirty = true;
                        }
                        if this.paints_live_widgets() && this.vpn_elapsed_should_tick() {
                            dirty = true;
                        }
                        if this.paints_live_widgets() && this.recording {
                            dirty = true;
                        }
                    }
                    if this.settings.show_recorder && nook_core::recorder::is_live() {
                        nook_core::recorder::pump();
                        let snap = nook_core::recorder::snapshot();
                        this.recording = true;
                        this.recording_started = snap.started;
                        this.recorder_level = snap.level;
                        if this.paints_live_widgets()
                            && this.sample_recorder_wave(snap.level, Instant::now())
                        {
                            dirty = true;
                        }
                        if this.user_preferred.is_none() && this.alert_preferred.is_none() {
                            this.alert_preferred = Some(CompactMode::Recording);
                        }
                        if this.paints_live_widgets()
                            && this.expanded
                            && snap.transcript != this.live_transcript
                            && this.recorder_last_notify.elapsed() >= Duration::from_millis(250)
                        {
                            this.live_transcript = snap.transcript;
                            this.recorder_last_notify = Instant::now();
                            dirty = true;
                        }
                    } else if this.recording {
                        this.recording = false;
                        this.recording_started = None;
                        this.clear_recorder_wave();
                        this.recordings = nook_core::recorder::list();
                        this.live_transcript = nook_core::recorder::snapshot().transcript;
                        if this.alert_preferred == Some(CompactMode::Recording) {
                            this.alert_preferred = None;
                        }
                        dirty = true;
                    }
                    if this.clear_expired_vpn_reveal() {
                        dirty = true;
                    }
                    if this.share.maybe_reset_failure() {
                        dirty = true;
                    }
                    if this.expanded && !this.fallback_db_noticed {
                        this.fallback_db_noticed = true;
                        if nook_core::database::FALLBACK_DB_IN_USE.load(Ordering::Relaxed) {
                            // No in-island toast path renders arbitrary notices;
                            // log once so the temporary DB is visible in Console.
                            log::warn!(
                                "Settings are stored in a temporary database and won't survive a restart."
                            );
                        }
                    }
                    // Customize keeps real widget previews but freezes these
                    // frame pumps so dashed chrome is not rebuilt ~30–50×/s.
                    if this.paints_live_widgets() {
                        if this.interpolate_elapsed() {
                            dirty = true;
                        }
                        if this.expanded && nook_core::queue::take_artwork_ready() {
                            dirty = true;
                        }
                    }
                    if this.step_spring(dt) {
                        dirty = true;
                    }
                    let any_working = this.agents.iter().any(|a| a.status.is_working());
                    if this.paints_live_widgets()
                        && !this.agent_morph_lite
                        && (any_working
                            || this.speed_running
                            || (!this.expanded && this.shell_running))
                    {
                        // ~30fps is enough for the brand shimmer; painting the
                        // glyph matrix every 20ms tick starved the expand morph
                        // and Files↔Agents context shifts. Size/content springs
                        // still dirty via `step_spring` at full rate; skip
                        // pixel notifies entirely while lite is held.
                        if now.duration_since(this.last_pixel_frame)
                            >= Duration::from_millis(33)
                        {
                            this.last_pixel_frame = now;
                            this.pixel_t =
                                now.duration_since(this.pixel_origin).as_secs_f32();
                            dirty = true;
                        }
                    }
                    if this.paints_live_widgets()
                        && this.aura_should_animate()
                        && now.duration_since(this.last_aura_frame)
                            >= Duration::from_millis(33)
                    {
                        this.last_aura_frame = now;
                        this.aura_t = now.duration_since(this.pixel_origin).as_secs_f32();
                        dirty = true;
                    }
                    if let Some(rect) = media::take_art_bounds(&mut this.motion_art_bounds_gen)
                    {
                        this.motion_art_bounds = Some(rect);
                    }
                    this.apply_motion_art_layer();
                    if this.paints_live_widgets() && this.mirror_on {
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
                        || this.speed_running
                        || this.hud_active()
                        || this.shell_running;
                    // Media visualizer paints via request_animation_frame on
                    // the Media face — it does not keep this loop hot.
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
                        1000
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
                    let track_changed = this.now_playing.title != playing.title
                        || this.now_playing.bundle_id != playing.bundle_id;
                    let changed = this.now_playing.title != playing.title
                        || this.now_playing.artist != playing.artist
                        || this.now_playing.is_playing != playing.is_playing
                        || this.now_playing.elapsed_time != playing.elapsed_time
                        || this.now_playing.app_name != playing.app_name
                        || this.now_playing.bundle_id != playing.bundle_id;
                    let album_changed = this.now_playing.artist != playing.artist
                        || this.now_playing.album != playing.album;
                    let art_changed = this.now_playing.artwork_base64 != playing.artwork_base64;
                    this.now_playing.title = playing.title;
                    this.now_playing.artist = playing.artist;
                    this.now_playing.album = playing.album;
                    this.now_playing.artwork_base64 = playing.artwork_base64;
                    this.now_playing.duration = playing.duration;
                    if this.scrubber_drag.is_none() {
                        // Optimistic hold: while a fresh seek disagrees with the backend
                        // (command still in flight), keep the scrubbed elapsed so the thumb
                        // does not snap back. Accept once MediaRemote is near the target,
                        // once the window lapses, or when the track itself changes.
                        match this.seek_intent {
                            Some((want, since))
                                if hold_seek_intent(
                                    want,
                                    since,
                                    playing.elapsed_time,
                                    track_changed,
                                ) =>
                            {
                                // hold: do not overwrite elapsed this poll
                            }
                            _ => {
                                this.seek_intent = None;
                                this.now_playing.elapsed_time = playing.elapsed_time;
                                this.elapsed_base = playing.elapsed_time;
                                this.elapsed_at = Instant::now();
                            }
                        }
                    }
                    // Optimistic hold: while a fresh play/pause intent disagrees with the
                    // backend (command still in flight), keep the tapped state so the button
                    // does not flip back and forth. Accept the poll once it catches up, once
                    // the window lapses, or when the track itself changes.
                    match this.play_intent {
                        Some((want, since))
                            if !track_changed
                                && since.elapsed() < MEDIA_INTENT_WINDOW
                                && playing.is_playing != want =>
                        {
                            // hold: do not overwrite is_playing this poll
                        }
                        _ => {
                            this.play_intent = None;
                            this.now_playing.is_playing = playing.is_playing;
                        }
                    }
                    this.now_playing.app_name = playing.app_name;
                    this.now_playing.bundle_id = playing.bundle_id;
                    this.lyrics_anchor_elapsed = this.now_playing.elapsed_time.unwrap_or(0.0);
                    this.lyrics_anchor_at = Instant::now();
                    this.visualizer_color = media::visualizer_color_from_art(
                        this.now_playing.artwork_base64.as_deref(),
                    );
                    if art_changed || this.aura_palette.is_none() {
                        this.aura_palette =
                            media::art_palette(this.now_playing.artwork_base64.as_deref());
                    }
                    if album_changed {
                        this.motion_art_key = None;
                        this.now_playing.motion_artwork_url = None;
                        this.request_motion_art(cx);
                    }
                    this.apply_motion_art_layer();
                    if !was_media && this.has_media() {
                        this.alert_preferred = Some(CompactMode::Media);
                    }
                    this.sync_lyrics(cx);
                    let fetch = this.maybe_start_queue_fetch();
                    if changed {
                        this.arm_lyrics_line_timer(cx);
                    }
                    if changed || album_changed {
                        cx.notify();
                    }
                    (
                        this.has_media() && this.now_playing.is_playing,
                        fetch,
                        this.now_playing.clone(),
                    )
                });
                let Ok((is_playing, fetch_now, np)) = alive else {
                    break;
                };
                if fetch_now {
                    let queue = cx
                        .background_executor()
                        .spawn(async move {
                            nook_core::runtime()
                                .block_on(nook_core::queue::fetch_playback_queue(&np))
                        })
                        .await;
                    if this
                        .update(cx, |this, cx| {
                            this.apply_queue_fetch_result(queue);
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                // Stream-backed adapter reads are a cheap lock; cadence is
                // for interpolated elapsed (1s while playing) and the
                // AppleScript fallback (5s idle). Distributed-notification
                // observers and the MediaRemote stream poke `note_media_event`
                // so this parks until a real change instead of slicing 250 ms.
                let cadence = if is_playing {
                    Duration::from_secs(1)
                } else {
                    Duration::from_secs(5)
                };
                cx.background_executor()
                    .spawn(async move {
                        nook_core::runtime()
                            .block_on(nook_core::audio::wait_media_or_timeout(cadence))
                    })
                    .await;
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
                    // Fetch still runs every 30s; notify only when the
                    // published events, reminders, or notes actually change.
                    let mut changed = this.events != events || this.reminders != reminders;
                    this.events = events;
                    this.reminders = reminders;
                    if !this.notes_editing {
                        if let Ok(notes) = nook_core::notes::load_notes() {
                            if this.notes != notes {
                                this.notes = notes;
                                changed = true;
                            }
                        }
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
                        && this.user_preferred != Some(CompactMode::Media)
                        && this.alert_preferred != Some(CompactMode::Media)
                    {
                        this.alert_preferred = Some(CompactMode::Agents);
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
                        this.alert_preferred = Some(CompactMode::Observe);
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

        // Power is push-based: the watch fires on IOKit / LPM notifications.
        cx.spawn(async move |this, cx| {
            let mut rx = nook_core::power::subscribe();
            loop {
                let snap = *rx.borrow();
                if this
                    .update(cx, |this, cx| {
                        this.apply_power_snapshot(snap, cx);
                    })
                    .is_err()
                {
                    break;
                }
                let (alive, next_rx) = cx
                    .background_executor()
                    .spawn(async move {
                        let ok = nook_core::runtime().block_on(rx.changed()).is_ok();
                        (ok, rx)
                    })
                    .await;
                rx = next_rx;
                if !alive {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let mut rx = nook_core::messages::subscribe();
            loop {
                let enabled = nook_core::settings::get_app_settings().show_messages;
                let snapshot = if enabled {
                    cx.background_executor()
                        .spawn(async { nook_core::messages::snapshot() })
                        .await
                } else {
                    MessagesSnapshot::default()
                };
                if this
                    .update(cx, |this, cx| {
                        let was_quiet = this.messages.incoming.is_none();
                        this.messages = snapshot;
                        if was_quiet && this.messages.incoming.is_some() {
                            this.alert_preferred = Some(CompactMode::Messages);
                            this.message_draft.clear();
                            nook_core::haptics::trigger(None);
                        }
                        if this.messages.incoming.is_none()
                            && this.alert_preferred == Some(CompactMode::Messages)
                        {
                            this.alert_preferred = None;
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let mut rx = system_timers::subscribe();
            loop {
                let timers = rx.borrow().clone();
                if this
                    .update(cx, |this, cx| {
                        if this.system_timers == timers {
                            return;
                        }
                        let was_running = this.system_timers.iter().any(|t| t.state.is_running());
                        let now_running = timers.iter().any(|t| t.state.is_running());
                        let now_fired = timers
                            .iter()
                            .any(|t| t.state == system_timers::MTTimerState::Fired);
                        let was_fired = this
                            .system_timers
                            .iter()
                            .any(|t| t.state == system_timers::MTTimerState::Fired);
                        this.system_timers = timers;
                        if this.settings.show_timers && this.settings.sync_clock_timers {
                            if !was_running && now_running {
                                this.alert_preferred = Some(CompactMode::Timer);
                            }
                            if !was_fired && now_fired {
                                nook_core::haptics::trigger(Some(
                                    nook_core::haptics::HapticConfig {
                                        pattern: nook_core::haptics::HapticPattern::Success,
                                    },
                                ));
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
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
                if this
                    .update(cx, |this, cx| this.apply_hud_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        // VPN is push-based: SCDynamicStore / getifaddrs publish on the watch.
        cx.spawn(async move |this, cx| {
            let mut rx = nook_core::vpn::subscribe();
            loop {
                let snap = rx.borrow().clone();
                if this
                    .update(cx, |this, cx| {
                        this.apply_vpn_snapshot(snap, cx);
                    })
                    .is_err()
                {
                    break;
                }
                let (alive, next_rx) = cx
                    .background_executor()
                    .spawn(async move {
                        let ok = nook_core::runtime().block_on(rx.changed()).is_ok();
                        (ok, rx)
                    })
                    .await;
                rx = next_rx;
                if !alive {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let mut rx = nook_core::notifications::subscribe();
            loop {
                let items = nook_core::notifications::snapshot();
                let unread = nook_core::notifications::unread_count();
                if this
                    .update(cx, |this, cx| {
                        let was = this.notification_unread;
                        this.notifications = items;
                        this.notification_unread = unread;
                        if this.settings.show_notifications && unread > was {
                            this.alert_preferred = Some(CompactMode::Notifications);
                            nook_core::haptics::trigger(None);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .detach();

        // Weather: 30 min TTL, fetch only when stale and the card/adornment
        // is visible, or after a wake.
        cx.spawn(async move |this, cx| loop {
            let wake = nook_core::weather::take_wake();
            let plan = this
                .update(cx, |this, _| {
                    let enabled = this.settings.weather.enabled;
                    let has_coords = this.settings.weather.location.coords().is_some();
                    let fresh = nook_core::weather::is_fresh_for(&this.settings.weather);
                    let visible = this.weather_visible();
                    (
                        enabled
                            && has_coords
                            && (!fresh || this.weather.is_none())
                            && (visible || wake),
                        fresh,
                    )
                })
                .unwrap_or((false, true));
            if plan.0 {
                let _ = this.update(cx, |this, cx| this.refresh_weather(cx));
            }
            let wait = if plan.1 {
                Duration::from_secs(30 * 60)
            } else {
                Duration::from_secs(30)
            };
            cx.background_executor().timer(wait).await;
        })
        .detach();

        self.spawn_meetings_loop(cx);
    }

    fn apply_power_snapshot(&mut self, snap: PowerSnapshot, cx: &mut Context<Self>) {
        let was_alert = self.has_battery_alert();
        self.power = snap;
        let now_alert = self.has_battery_alert();
        if !was_alert && now_alert {
            self.alert_preferred = Some(CompactMode::Battery);
            nook_core::haptics::trigger(None);
        } else if was_alert && !now_alert && self.alert_preferred == Some(CompactMode::Battery) {
            self.alert_preferred = None;
        }
        nook_core::power::set_detail_watch(self.expanded && self.settings.show_battery);
        cx.notify();
    }

    pub(crate) fn has_battery_alert(&self) -> bool {
        self.settings.show_battery
            && self
                .power
                .is_alerting(nook_core::power::clamp_alert_threshold(
                    self.settings.battery_alert_threshold,
                ))
    }

    pub(crate) fn toggle_low_power_mode(&mut self, cx: &mut Context<Self>) {
        if self.lpm_pending {
            return;
        }
        self.lpm_pending = true;
        self.lpm_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async {
                    nook_core::runtime().block_on(nook_core::power::toggle_low_power_mode())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.lpm_pending = false;
                match result {
                    Ok(_) => this.lpm_error = None,
                    Err(err) => this.lpm_error = Some(err),
                }
                this.power = nook_core::power::current();
                nook_core::power::set_detail_watch(this.expanded && this.settings.show_battery);
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_vpn_snapshot(&mut self, snap: VpnSnapshot, cx: &mut Context<Self>) {
        let was = self.vpn.connected;
        let changed = self.vpn != snap;
        self.vpn = snap;
        if was != self.vpn.connected {
            self.alert_preferred = Some(CompactMode::Vpn);
            self.vpn_reveal_until = Some(Instant::now() + motion::VPN_REVEAL);
            nook_core::haptics::trigger(None);
        }
        if changed {
            cx.notify();
        }
    }

    fn has_vpn_face(&self) -> bool {
        self.settings.show_vpn && (self.vpn.connected || self.vpn_revealing())
    }

    fn vpn_revealing(&self) -> bool {
        self.vpn_reveal_until
            .is_some_and(|until| Instant::now() < until)
    }

    fn clear_expired_vpn_reveal(&mut self) -> bool {
        let Some(until) = self.vpn_reveal_until else {
            return false;
        };
        if Instant::now() < until {
            return false;
        }
        self.vpn_reveal_until = None;
        if !self.vpn.connected && self.alert_preferred == Some(CompactMode::Vpn) {
            self.alert_preferred = None;
        }
        true
    }

    fn vpn_elapsed_should_tick(&self) -> bool {
        if !self.settings.show_vpn || self.vpn.since.is_none() {
            return false;
        }
        let compact =
            !self.expanded && self.mode() == CompactMode::Vpn && self.settings.vpn_show_timer;
        let card = self.expanded && self.tab == Tab::Widgets;
        compact || card
    }

    fn spawn_meetings_loop(&self, cx: &mut Context<Self>) {
        // Meetings: wait on hardware events. The only timed work is a 1–2s
        // Zoom menu-title readback, and only while the meeting face is shown.
        cx.spawn(async move |this, cx| loop {
            let face = this
                .update(cx, |this, _| this.meeting_face_shown())
                .unwrap_or(false);
            let cadence = if face {
                Duration::from_millis(1500)
            } else {
                Duration::from_secs(30)
            };
            let slice = Duration::from_millis(250);
            let mut waited = Duration::ZERO;
            loop {
                cx.background_executor().timer(slice).await;
                waited += slice;
                if nook_core::meetings::take_meeting_event() || waited >= cadence {
                    break;
                }
            }
            let zoom_readback = this
                .update(cx, |this, _| {
                    this.meeting_face_shown()
                        && this.meeting.app() == Some(nook_core::meetings::MeetingApp::Zoom)
                })
                .unwrap_or(false);
            let snap = cx
                .background_executor()
                .spawn(async move {
                    if zoom_readback {
                        if let Some(muted) = nook_core::meetings::read_zoom_mute() {
                            nook_core::meetings::apply_zoom_mute(Some(muted));
                        }
                    }
                    nook_core::meetings::refresh()
                })
                .await;
            if this
                .update(cx, |this, cx| {
                    let was = this.meeting.in_meeting();
                    let changed = this.meeting != snap;
                    this.meeting = snap;
                    if !was && this.meeting.in_meeting() {
                        this.alert_preferred = Some(CompactMode::Meeting);
                        nook_core::haptics::trigger(None);
                    }
                    if changed {
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
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
        let repaint = nook_core::observe::should_repaint(&self.observe, &snapshot);
        self.observe = snapshot;
        if repaint {
            cx.notify();
        }
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

    /// Consume the HAL dirty flag (and HUD expiry) without adding a timer.
    fn sync_output_devices(&mut self) -> bool {
        let mut dirty = false;
        if !self.settings.audio_output_picker && self.output_picker_open {
            self.output_picker_open = false;
            dirty = true;
        }
        if !self.expanded && self.output_picker_open {
            self.output_picker_open = false;
            dirty = true;
        }
        if (!self.settings.show_media_queue || !self.expanded) && self.queue_open {
            self.queue_open = false;
            dirty = true;
        }
        if nook_core::audio_devices::take_dirty() {
            let devices = nook_core::audio_devices::snapshot();
            let old_id = self
                .output_devices
                .iter()
                .find(|d| d.is_default)
                .map(|d| d.id);
            let had_list = !self.output_devices.is_empty();
            let new_default = devices.iter().find(|d| d.is_default).cloned();
            self.output_devices = devices;
            if had_list {
                if let Some(dev) = new_default {
                    if Some(dev.id) != old_id {
                        self.output_hud_name = Some(dev.name);
                        self.output_hud_until = Some(Instant::now() + motion::OUTPUT_HUD_TTL);
                    }
                }
            }
            dirty = true;
        }
        if let Some(until) = self.output_hud_until {
            if Instant::now() >= until {
                self.output_hud_until = None;
                self.output_hud_name = None;
                dirty = true;
            }
        }
        dirty
    }

    pub(crate) fn has_media(&self) -> bool {
        self.settings.show_media
            && (self.now_playing.is_playing
                || self.now_playing.title.is_some()
                || self.now_playing.artist.is_some())
    }

    pub(crate) fn output_picker_enabled(&self) -> bool {
        self.settings.audio_output_picker && nook_core::audio_devices::available()
    }

    pub(crate) fn output_hud_label(&self) -> Option<&str> {
        let until = self.output_hud_until?;
        if Instant::now() < until {
            self.output_hud_name.as_deref()
        } else {
            None
        }
    }

    pub(crate) fn toggle_output_picker(&mut self, cx: &mut Context<Self>) {
        if !self.output_picker_enabled() {
            self.output_picker_open = false;
            cx.notify();
            return;
        }
        if self.output_picker_open {
            self.output_picker_open = false;
        } else {
            nook_core::audio_devices::refresh();
            self.output_devices = nook_core::audio_devices::snapshot();
            let _ = nook_core::audio_devices::take_dirty();
            self.output_picker_open = true;
            self.queue_open = false;
        }
        cx.notify();
    }

    pub(crate) fn toggle_queue_panel(&mut self, cx: &mut Context<Self>) {
        if !self.settings.show_media_queue
            || !self.has_media()
            || !nook_core::queue::supports_local_queue(
                self.now_playing.app_name.as_deref(),
                self.now_playing.bundle_id.as_deref(),
            )
        {
            self.queue_open = false;
            cx.notify();
            return;
        }
        self.queue_open = !self.queue_open;
        if self.queue_open {
            self.output_picker_open = false;
            // Always re-pull on open so a prior empty/error does not stick,
            // and so a stuck inflight flag cannot block the panel forever.
            self.queue_key = None;
            self.queue_inflight = false;
            if self.maybe_start_queue_fetch() {
                let np = self.now_playing.clone();
                cx.spawn(async move |this, cx| {
                    let queue = cx
                        .background_executor()
                        .spawn(async move {
                            nook_core::runtime()
                                .block_on(nook_core::queue::fetch_playback_queue(&np))
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.apply_queue_fetch_result(queue);
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }

    /// Width the Music cell claims: base cells plus the open queue panel.
    pub(crate) fn music_pane_width(&self, cells: u8) -> f32 {
        cells as f32 * theme::NOOK_CELL + self.queue_extra_width()
    }

    pub(crate) fn select_output_device(&mut self, id: u32, cx: &mut Context<Self>) {
        match nook_core::audio_devices::set_default_output(id) {
            Ok(()) => {
                for device in &mut self.output_devices {
                    device.is_default = device.id == id;
                }
                self.output_picker_open = false;
            }
            Err(_) => {
                self.output_hud_name = Some("Couldn't switch output".into());
                self.output_hud_until = Some(Instant::now() + motion::OUTPUT_HUD_TTL);
            }
        }
        cx.notify();
    }

    /// Interpolated playback position from the last now-playing snapshot.
    pub(crate) fn lyrics_position(&self) -> f64 {
        let extra = if self.now_playing.is_playing {
            self.lyrics_anchor_at.elapsed().as_secs_f64()
        } else {
            0.0
        };
        let pos = (self.lyrics_anchor_elapsed + extra).max(0.0);
        match self.now_playing.duration {
            Some(duration) if duration > 0.0 => pos.min(duration),
            _ => pos,
        }
    }
    pub(crate) fn queue_visible(&self) -> bool {
        self.settings.show_media
            && self.settings.show_media_queue
            && self.expanded
            && self.tab == Tab::Widgets
            && self.has_media()
            && nook_core::queue::supports_local_queue(
                self.now_playing.app_name.as_deref(),
                self.now_playing.bundle_id.as_deref(),
            )
    }

    /// Extra island width while the Playing Next side panel is open.
    pub(crate) fn queue_extra_width(&self) -> f32 {
        if self.queue_panel_visible() {
            QUEUE_PANEL_W + QUEUE_SIDE_CHROME
        } else {
            0.0
        }
    }

    /// Width reserved for Now Playing (art / scrubber / transport) while the
    /// queue panel is open. Matches the Music cell's closed size so controls
    /// are not flex-shrunk away.
    pub(crate) fn music_player_width(&self) -> f32 {
        self.settings
            .cells_for(nook_core::settings::WidgetModule::Music) as f32
            * theme::NOOK_CELL
    }

    pub(crate) fn queue_panel_visible(&self) -> bool {
        self.queue_open
            && self.settings.show_media
            && self.settings.show_media_queue
            && self.expanded
            && self.tab == Tab::Widgets
            && self.has_media()
            && nook_core::queue::supports_local_queue(
                self.now_playing.app_name.as_deref(),
                self.now_playing.bundle_id.as_deref(),
            )
    }

    fn maybe_start_queue_fetch(&mut self) -> bool {
        if !self.queue_visible() {
            return false;
        }
        let key = nook_core::queue::queue_identity(&self.now_playing);
        if self.queue_inflight {
            return false;
        }
        if self.queue_key.as_ref() == Some(&key) {
            return false;
        }
        // Key is applied when the fetch finishes so a transient empty/error
        // does not block retries for the same track.
        self.queue_inflight = true;
        true
    }

    fn apply_queue_fetch_result(&mut self, queue: nook_core::models::PlaybackQueue) {
        let key = nook_core::queue::queue_identity(&self.now_playing);
        // Cache anything that isn't a bare default (transient script/osascript
        // failure). Empty-but-labeled Music snapshots and hide reasons stick.
        let cacheable = queue != nook_core::models::PlaybackQueue::default();
        self.queue = queue;
        self.queue_inflight = false;
        if cacheable {
            self.queue_key = Some(key);
        } else {
            self.queue_key = None;
        }
    }

    fn interpolate_elapsed(&mut self) -> bool {
        if self.scrubber_drag.is_some() || !self.now_playing.is_playing {
            return false;
        }
        let Some(base) = self.elapsed_base else {
            return false;
        };
        let Some(duration) = self.now_playing.duration.filter(|d| *d > 0.0) else {
            return false;
        };
        let elapsed = (base + self.elapsed_at.elapsed().as_secs_f64()).min(duration);
        let prev = self.now_playing.elapsed_time.unwrap_or(-1.0);
        if (elapsed - prev).abs() < 0.04 {
            return false;
        }
        self.now_playing.elapsed_time = Some(elapsed);
        true
    }

    pub(crate) fn update_scrubber_from_x(&mut self, x: f32) {
        let Some((origin, width)) = *self.scrubber_bounds.borrow() else {
            return;
        };
        let ratio = media::scrubber_ratio(x, origin, width);
        self.scrubber_drag = Some(ratio);
        if let Some(duration) = self.now_playing.duration.filter(|d| *d > 0.0) {
            let position = duration * ratio as f64;
            self.now_playing.elapsed_time = Some(position);
            self.elapsed_base = Some(position);
            self.elapsed_at = Instant::now();
        }
    }

    pub(crate) fn finish_scrubber(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(ratio) = self.scrubber_drag.take() else {
            return false;
        };
        let Some(duration) = self.now_playing.duration.filter(|d| *d > 0.0) else {
            return true;
        };
        let position = duration * ratio as f64;
        // Re-anchors the lyrics clock, sets elapsed, and arms seek_intent.
        self.note_media_seek(position, cx);
        nook_core::runtime().spawn(async move {
            let _ = nook_core::audio::media_seek(position).await;
        });
        true
    }

    pub(crate) fn running_timer(&self) -> Option<&Timer> {
        self.timers.iter().find(|t| t.running)
    }

    pub(crate) fn lyrics_position_ms(&self) -> u64 {
        (self.lyrics_position() * 1000.0).max(0.0) as u64
    }

    pub(crate) fn note_media_seek(&mut self, position: f64, cx: &mut Context<Self>) {
        let position = position.max(0.0);
        self.now_playing.elapsed_time = Some(position);
        self.elapsed_base = Some(position);
        self.elapsed_at = Instant::now();
        self.seek_intent = Some((position, Instant::now()));
        self.lyrics_anchor_elapsed = position;
        self.lyrics_anchor_at = Instant::now();
        self.arm_lyrics_line_timer(cx);
        cx.notify();
    }

    pub(crate) fn note_media_play_pause(&mut self, cx: &mut Context<Self>) {
        let pos = self.lyrics_position();
        self.now_playing.is_playing = !self.now_playing.is_playing;
        self.play_intent = Some((self.now_playing.is_playing, Instant::now()));
        self.lyrics_anchor_elapsed = pos;
        self.lyrics_anchor_at = Instant::now();
        self.arm_lyrics_line_timer(cx);
        cx.notify();
    }

    pub(crate) fn note_media_skip(&mut self, cx: &mut Context<Self>) {
        // Optimistically reset the position; the next poll brings the new track's
        // metadata, but the scrubber should not linger at the old elapsed.
        self.now_playing.elapsed_time = Some(0.0);
        self.elapsed_base = Some(0.0);
        self.elapsed_at = Instant::now();
        self.lyrics_anchor_elapsed = 0.0;
        self.lyrics_anchor_at = Instant::now();
        self.arm_lyrics_line_timer(cx);
        cx.notify();
    }

    fn lyrics_timer_should_run(&self) -> bool {
        self.settings.show_lyrics
            && self.settings.show_media
            && self.expanded
            && self.now_playing.is_playing
            && self
                .lyrics
                .as_ref()
                .is_some_and(|lyrics| lyrics.has_synced())
    }

    fn disarm_lyrics_timer(&mut self) {
        self.lyrics_timer_gen = self.lyrics_timer_gen.wrapping_add(1);
    }

    pub(crate) fn toggle_mirror(&mut self, cx: &mut Context<Self>) {
        if self.mirror_on {
            self.stop_mirror(cx);
        } else if platform::start_mirror() {
            self.mirror_on = true;
            self.expanded = true;
            nook_core::audio::note_media_event();
        }
        cx.notify();
    }

    /// One-shot timer for the next lyric line. Bumping `lyrics_timer_gen`
    /// cancels a previously armed wait. Not a poll loop.
    fn arm_lyrics_line_timer(&mut self, cx: &mut Context<Self>) {
        self.disarm_lyrics_timer();
        if !self.lyrics_timer_should_run() {
            return;
        }
        let Some(lyrics) = self.lyrics.clone() else {
            return;
        };
        let Some(wait) = lyrics.delay_until_next(self.lyrics_position_ms()) else {
            return;
        };
        let gen = self.lyrics_timer_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |this, cx| {
                if this.lyrics_timer_gen != gen {
                    return;
                }
                cx.notify();
                this.arm_lyrics_line_timer(cx);
            });
        })
        .detach();
    }

    fn sync_lyrics(&mut self, cx: &mut Context<Self>) {
        if !self.settings.show_lyrics || !self.settings.show_media {
            if self.lyrics.is_some() || self.lyrics_key.is_some() {
                self.lyrics = None;
                self.lyrics_key = None;
                self.disarm_lyrics_timer();
            }
            return;
        }
        let title = self.now_playing.title.clone().unwrap_or_default();
        let artist = self.now_playing.artist.clone().unwrap_or_default();
        if title.is_empty() && artist.is_empty() {
            self.lyrics = None;
            self.lyrics_key = None;
            self.disarm_lyrics_timer();
            return;
        }
        let key = (title, artist);
        if self.lyrics_key.as_ref() == Some(&key) {
            return;
        }
        self.lyrics_key = Some(key.clone());
        self.lyrics = None;
        self.disarm_lyrics_timer();
        let album = self.now_playing.album.clone();
        let duration = self.now_playing.duration;
        let artist = key.1.clone();
        let title = key.0.clone();
        cx.spawn(async move |this, cx| {
            let fetched = cx
                .background_executor()
                .spawn(async move {
                    nook_core::runtime().block_on(nook_core::lyrics::fetch_for_track(
                        &artist,
                        &title,
                        album.as_deref(),
                        duration,
                    ))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.lyrics_key.as_ref() != Some(&key) {
                    return;
                }
                this.lyrics = fetched.map(Arc::new);
                this.arm_lyrics_line_timer(cx);
                cx.notify();
            });
        })
        .detach();
    }
    fn motion_art_album_key(&self) -> (String, String) {
        (
            self.now_playing.artist.clone().unwrap_or_default(),
            self.now_playing.album.clone().unwrap_or_default(),
        )
    }

    fn aura_should_animate(&self) -> bool {
        self.expanded
            && self.tab == Tab::Widgets
            && self.settings.show_media
            && self.settings.ambient_art_glow
            && self.now_playing.is_playing
            && !self.reduce_motion
            && self.aura_palette.is_some()
            && !self.suppressed
    }

    fn sync_motion_art_from_settings(&mut self, cx: &mut Context<Self>) {
        if !self.settings.animated_album_art || !self.settings.show_media {
            self.motion_art_key = None;
            self.now_playing.motion_artwork_url = None;
            self.apply_motion_art_layer();
            return;
        }
        if self.motion_art_key.is_none() {
            self.request_motion_art(cx);
        }
        self.apply_motion_art_layer();
    }

    fn request_motion_art(&mut self, cx: &mut Context<Self>) {
        if !self.settings.animated_album_art || !self.settings.show_media {
            return;
        }
        let key = self.motion_art_album_key();
        if key.0.trim().is_empty() || key.1.trim().is_empty() {
            self.now_playing.motion_artwork_url = None;
            return;
        }
        if self.motion_art_key.as_ref() == Some(&key) {
            return;
        }
        self.motion_art_key = Some(key.clone());
        self.now_playing.motion_artwork_url = None;
        let lookup_key = key.clone();
        cx.spawn(async move |this, cx| {
            let fetched = cx
                .background_executor()
                .spawn({
                    let lookup_key = lookup_key.clone();
                    async move {
                        nook_core::runtime().block_on(nook_core::motion_artwork::lookup(
                            &lookup_key.0,
                            &lookup_key.1,
                            None,
                        ))
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.motion_art_key.as_ref() != Some(&lookup_key) {
                    return;
                }
                this.now_playing.motion_artwork_url = fetched.map(|art| art.m3u8_url);
                this.apply_motion_art_layer();
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_motion_art_layer(&mut self) {
        let spec = self.motion_art_spec();
        if spec == self.last_motion_spec {
            return;
        }
        self.last_motion_spec = spec.clone();
        match spec.as_ref() {
            Some(spec) => platform::sync_motion_art(Some(spec)),
            None => platform::hide_motion_art(),
        }
    }

    fn motion_art_spec(&self) -> Option<platform::MotionArtSpec> {
        if self.reduce_motion
            || !self.expanded
            || self.tab != Tab::Widgets
            || self.suppressed
            || !self.settings.animated_album_art
            || !self.settings.show_media
        {
            return None;
        }
        let url = self.now_playing.motion_artwork_url.clone()?;
        let (x, y, w, h) = self.motion_art_bounds?;
        if w < 8.0 || h < 8.0 {
            return None;
        }
        Some(platform::MotionArtSpec {
            url,
            x: x as f64,
            y: y as f64,
            w: w as f64,
            h: h as f64,
            radius: media::NOOK_ART_RADIUS as f64,
            playing: self.now_playing.is_playing,
        })
    }
    fn clock_timers(&self) -> impl Iterator<Item = &SystemTimer> {
        self.system_timers.iter().filter(|t| t.state.is_active())
    }

    fn clock_timer_visible(&self) -> bool {
        self.settings.show_timers
            && self.settings.sync_clock_timers
            && self.clock_timers().any(|t| t.state.is_counting())
    }

    fn has_live_timer(&self) -> bool {
        self.running_timer().is_some()
            || (self.settings.sync_clock_timers
                && self
                    .clock_timers()
                    .any(|t| t.state.is_running() || t.state.is_counting()))
    }

    fn clock_face(timer: &SystemTimer, now: f64) -> FaceTimer {
        FaceTimer {
            remaining: timer.remaining_secs(now),
            total: timer.total_secs().max(1),
            running: timer.state.is_running(),
            name: if timer.title.is_empty() {
                "Clock".into()
            } else {
                timer.title.clone()
            },
            source: FaceTimerSource::Clock(timer.id.clone()),
        }
    }

    fn local_face(timer: &Timer) -> FaceTimer {
        FaceTimer {
            remaining: timer.remaining,
            total: timer.total.max(1),
            running: timer.running,
            name: timer.name.clone(),
            source: FaceTimerSource::Local(timer.id),
        }
    }

    /// Compact face: a finished timer first so the ring turns red, else the
    /// soonest running countdown (island or Clock), else the first local.
    pub(crate) fn face_timer(&self) -> Option<FaceTimer> {
        if !self.settings.show_timers {
            return None;
        }
        let now = system_timers::unix_now();
        let mut faces: Vec<FaceTimer> = self.timers.iter().map(Self::local_face).collect();
        if self.settings.sync_clock_timers {
            faces.extend(self.clock_timers().map(|t| Self::clock_face(t, now)));
        }
        faces
            .iter()
            .find(|t| t.remaining == 0 && t.total > 0)
            .cloned()
            .or_else(|| {
                faces
                    .iter()
                    .filter(|t| t.running)
                    .min_by_key(|t| t.remaining)
                    .cloned()
            })
            .or_else(|| faces.into_iter().next())
    }

    pub(crate) fn toggle_face_timer(&mut self) {
        match self.face_timer().map(|t| t.source) {
            Some(FaceTimerSource::Local(id)) => self.toggle_local_timer(id),
            Some(FaceTimerSource::Clock(id)) => {
                if self.clock_timers().any(|t| t.state.is_running()) {
                    nook_core::shortcuts::pause_timer(&id);
                } else {
                    nook_core::shortcuts::resume_timer(&id);
                }
            }
            None => {}
        }
    }

    pub(crate) fn toggle_local_timer(&mut self, id: u64) {
        if let Some(t) = self.timers.iter_mut().find(|t| t.id == id) {
            t.running = !t.running;
            if t.running && t.remaining > 0 {
                crate::notify::schedule_island_timer(t.id, t.remaining, &t.name);
            } else {
                crate::notify::cancel_island_timer(t.id);
            }
        }
    }

    pub(crate) fn reset_timer(&mut self, id: u64) {
        if let Some(t) = self.timers.iter_mut().find(|t| t.id == id) {
            t.remaining = t.total;
            t.running = false;
            crate::notify::cancel_island_timer(t.id);
            t.ends_at = None;
            if let TimerKind::Pomodoro(spec) = t.kind {
                t.name = spec.label().to_string();
            }
        }
        self.sync_pomodoro_awake();
    }

    pub(crate) fn remove_timer(&mut self, id: u64) {
        crate::notify::cancel_island_timer(id);
        self.timers.retain(|t| t.id != id);
        self.sync_pomodoro_awake();
    }

    pub(crate) fn toggle_timer(&mut self, id: u64) {
        let now = SystemTime::now();
        if let Some(t) = self.timers.iter_mut().find(|t| t.id == id) {
            t.running = !t.running;
            if t.running {
                if matches!(t.kind, TimerKind::Pomodoro(_)) {
                    t.ends_at = Some(now + Duration::from_secs(t.remaining.max(1) as u64));
                }
            } else {
                t.ends_at = None;
            }
        }
        self.sync_pomodoro_awake();
    }

    /// Advance running timers. Pomodoro remaining is computed from a wall-clock
    /// deadline so a lid-close does not stall a break. Countdown still uses the
    /// existing Instant-derived `elapsed_secs`. Returns whether anything moved.
    pub(crate) fn apply_timer_tick(&mut self, now: SystemTime, elapsed_secs: u32) -> bool {
        let mut dirty = false;
        let mut edges: Vec<(bool, PomodoroPhase)> = Vec::new();
        for t in &mut self.timers {
            if !t.running {
                continue;
            }
            match t.kind {
                TimerKind::Countdown => {
                    if t.remaining == 0 {
                        continue;
                    }
                    t.remaining = t.remaining.saturating_sub(elapsed_secs);
                    dirty = true;
                    if t.remaining == 0 {
                        t.running = false;
                        nook_core::haptics::trigger(Some(nook_core::haptics::HapticConfig {
                            pattern: nook_core::haptics::HapticPattern::Success,
                        }));
                    }
                }
                TimerKind::Pomodoro(spec) => {
                    let remaining = nook_core::pomodoro::remaining_until(t.ends_at, now);
                    if remaining != t.remaining {
                        t.remaining = remaining;
                        dirty = true;
                    }
                    if remaining > 0 {
                        continue;
                    }
                    nook_core::haptics::trigger(Some(nook_core::haptics::HapticConfig {
                        pattern: nook_core::haptics::HapticPattern::Success,
                    }));
                    if spec.auto_advance {
                        let next = spec.advance();
                        t.kind = TimerKind::Pomodoro(next);
                        t.total = next.duration_secs();
                        t.remaining = next.duration_secs();
                        t.name = next.label().to_string();
                        t.running = true;
                        t.ends_at = Some(now + Duration::from_secs(next.duration_secs() as u64));
                        edges.push((next.phase.is_work(), next.phase));
                    } else {
                        t.running = false;
                        t.ends_at = None;
                    }
                    dirty = true;
                }
            }
        }
        for (is_work, _) in edges {
            self.on_pomodoro_edge(is_work);
        }
        if dirty {
            self.sync_pomodoro_awake();
        }
        dirty
    }

    fn on_pomodoro_edge(&self, work: bool) {
        let name = if work {
            self.settings.focus_shortcut_work.as_deref()
        } else {
            self.settings.focus_shortcut_break.as_deref()
        };
        nook_core::focus::run_shortcut_detached(name);
    }

    fn running_pomodoro_work(&self) -> bool {
        self.timers.iter().any(|t| {
            t.running
                && matches!(
                    t.kind,
                    TimerKind::Pomodoro(spec) if spec.phase == PomodoroPhase::Work
                )
        })
    }

    fn sync_pomodoro_awake(&mut self) {
        if self.settings.pomodoro_keep_awake && self.running_pomodoro_work() {
            nook_core::high_alert::set_low_battery_release_pct(
                self.settings.low_battery_release_pct,
            );
            let _ = nook_core::high_alert::acquire(
                HighAlertOwner::Pomodoro,
                self.settings.high_alert_kind,
                None,
            );
        } else {
            nook_core::high_alert::release(HighAlertOwner::Pomodoro);
        }
        self.awake_active = nook_core::high_alert::is_active();
    }

    pub(crate) fn high_alert_active(&self) -> bool {
        self.awake_active || nook_core::high_alert::is_active()
    }

    pub(crate) fn high_alert_remaining_secs(&self) -> Option<u32> {
        let deadline = self.awake_deadline?;
        Some(
            deadline
                .saturating_duration_since(Instant::now())
                .as_secs()
                .min(u32::MAX as u64) as u32,
        )
    }

    /// Sync UI with powerd / low-battery release. Cheap atomics; no extra loops.
    pub(crate) fn sync_high_alert_ui(&mut self, now: Instant) -> bool {
        let stale = nook_core::high_alert::take_ui_stale();
        let expired = self.awake_deadline.is_some_and(|deadline| now >= deadline);
        if expired && nook_core::high_alert::is_held_by(HighAlertOwner::Manual) {
            nook_core::high_alert::release(HighAlertOwner::Manual);
        }
        let active = nook_core::high_alert::is_active();
        let changed = stale || expired || self.awake_active != active;
        if expired {
            self.awake_deadline = None;
        }
        self.awake_active = active;
        if !nook_core::high_alert::is_held_by(HighAlertOwner::Manual) {
            self.awake_deadline = None;
        }
        nook_core::high_alert::reap_idle();
        changed
    }

    pub(crate) fn set_high_alert(&mut self, on: bool, duration_secs: Option<u32>) {
        nook_core::high_alert::set_low_battery_release_pct(self.settings.low_battery_release_pct);
        if on {
            let secs = duration_secs.unwrap_or(self.settings.high_alert_default_duration_secs);
            let timeout = if secs == 0 {
                None
            } else {
                Some(Duration::from_secs(secs as u64))
            };
            if nook_core::high_alert::acquire(
                HighAlertOwner::Manual,
                self.settings.high_alert_kind,
                timeout,
            )
            .is_ok()
            {
                self.awake_deadline = timeout.map(|d| Instant::now() + d);
                self.awake_active = true;
            }
        } else {
            nook_core::high_alert::release(HighAlertOwner::Manual);
            self.awake_deadline = None;
            self.awake_active = nook_core::high_alert::is_active();
        }
    }

    pub(crate) fn control_clock_timer(&self, action: ClockTimerAction) {
        match action {
            ClockTimerAction::Pause(id) => nook_core::shortcuts::pause_timer(&id),
            ClockTimerAction::Resume(id) => nook_core::shortcuts::resume_timer(&id),
            ClockTimerAction::Cancel(id) => nook_core::shortcuts::cancel_timer(&id),
            ClockTimerAction::Open(id) => nook_core::shortcuts::open_timer(&id),
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

    pub(crate) fn obsidian_capture_focus(&mut self, cx: &mut Context<Self>) -> gpui::FocusHandle {
        self.obsidian_capture_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
    }

    pub(crate) fn focus_obsidian_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.obsidian_capture_focus(cx);
        window.focus(&focus);
        window.activate_window();
        platform::activate_app();
        self.obsidian_typing = true;
        self.obsidian_capture_caret = self.obsidian_capture.len();
        self.obsidian_capture_select_all = false;
        cx.notify();
    }

    pub(crate) fn on_obsidian_capture_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &event.keystroke;
        if ks.key == "enter" {
            self.submit_obsidian_capture(cx);
            return;
        }
        if ks.key == "escape" {
            self.obsidian_typing = false;
            self.obsidian_capture_select_all = false;
            cx.notify();
            return;
        }
        if ks.modifiers.secondary() && ks.key == "a" {
            self.obsidian_capture_select_all = !self.obsidian_capture.is_empty();
            cx.notify();
            return;
        }
        if ks.modifiers.secondary() && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.obsidian_insert_text(&text);
                cx.notify();
            }
            return;
        }
        if ks.modifiers.platform || ks.modifiers.control {
            return;
        }
        match ks.key.as_str() {
            "left" => {
                self.obsidian_capture_select_all = false;
                self.obsidian_capture_caret =
                    prev_char_boundary(&self.obsidian_capture, self.obsidian_capture_caret);
                cx.notify();
            }
            "right" => {
                self.obsidian_capture_select_all = false;
                self.obsidian_capture_caret =
                    next_char_boundary(&self.obsidian_capture, self.obsidian_capture_caret);
                cx.notify();
            }
            "backspace" => {
                self.obsidian_delete_backward();
                cx.notify();
            }
            "delete" => {
                self.obsidian_delete_forward();
                cx.notify();
            }
            _ => {
                if let Some(ch) = &ks.key_char {
                    if !ch.chars().any(|c| c.is_control()) {
                        self.obsidian_insert_text(ch);
                        cx.notify();
                    }
                }
            }
        }
    }

    fn obsidian_insert_text(&mut self, text: &str) {
        if self.obsidian_capture_select_all {
            self.obsidian_capture.clear();
            self.obsidian_capture_caret = 0;
            self.obsidian_capture_select_all = false;
        }
        let caret = self.obsidian_capture_caret.min(self.obsidian_capture.len());
        self.obsidian_capture.insert_str(caret, text);
        self.obsidian_capture_caret = caret + text.len();
    }

    fn obsidian_delete_backward(&mut self) {
        if self.obsidian_capture_select_all {
            self.obsidian_capture.clear();
            self.obsidian_capture_caret = 0;
            self.obsidian_capture_select_all = false;
            return;
        }
        let caret = self.obsidian_capture_caret.min(self.obsidian_capture.len());
        if caret == 0 {
            return;
        }
        let start = prev_char_boundary(&self.obsidian_capture, caret);
        self.obsidian_capture.replace_range(start..caret, "");
        self.obsidian_capture_caret = start;
    }

    fn obsidian_delete_forward(&mut self) {
        if self.obsidian_capture_select_all {
            self.obsidian_capture.clear();
            self.obsidian_capture_caret = 0;
            self.obsidian_capture_select_all = false;
            return;
        }
        let caret = self.obsidian_capture_caret.min(self.obsidian_capture.len());
        if caret >= self.obsidian_capture.len() {
            return;
        }
        let end = next_char_boundary(&self.obsidian_capture, caret);
        self.obsidian_capture.replace_range(caret..end, "");
    }

    pub(crate) fn submit_obsidian_capture(&mut self, cx: &mut Context<Self>) {
        let text = self.obsidian_capture.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(vault) = self.settings.obsidian_vault.clone() else {
            self.obsidian_flash = Some("Choose a vault in Settings".into());
            cx.notify();
            return;
        };
        self.obsidian_capture.clear();
        self.obsidian_capture_caret = 0;
        self.obsidian_capture_select_all = false;
        self.obsidian_typing = false;
        let heading = self.settings.obsidian_capture_heading.clone();
        let use_uri = self.settings.obsidian_uri_capture;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    nook_core::obsidian::capture_to_daily(
                        &vault,
                        heading.as_deref(),
                        &text,
                        use_uri,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(_) => {
                        this.obsidian_flash = Some("Captured".into());
                        this.obsidian_dirty = true;
                        this.flush_obsidian_dirty(cx);
                    }
                    Err(err) => this.obsidian_flash = Some(err),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn select_obsidian_note(&mut self, rel: String, cx: &mut Context<Self>) {
        let Some(vault) = self.settings.obsidian_vault.clone() else {
            return;
        };
        self.obsidian_selected = Some(rel.clone());
        self.obsidian_body = None;
        cx.spawn(async move |this, cx| {
            let body = cx
                .background_executor()
                .spawn(async move { nook_core::obsidian::read_note(&vault, &rel).ok() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.obsidian_body = body;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn open_obsidian_note(&mut self, rel: &str, _cx: &mut Context<Self>) {
        let Some(vault) = self.settings.obsidian_vault.as_ref() else {
            return;
        };
        let url = nook_core::obsidian::open_file_url(&nook_core::obsidian::vault_name(vault), rel);
        if let Err(err) = nook_core::obsidian::open_url(&url) {
            log::warn!("obsidian open: {err}");
        }
    }

    pub(crate) fn open_obsidian_daily(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(vault) = self.settings.obsidian_vault.clone() else {
            self.obsidian_flash = Some("Choose a vault in Settings".into());
            cx.notify();
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let config = nook_core::obsidian::read_daily_notes_config(&vault);
                    let rel = nook_core::obsidian::ensure_daily_note(
                        &vault,
                        &config,
                        nook_core::obsidian::CivilDate::today(),
                    )?;
                    let url = nook_core::obsidian::open_file_url(
                        &nook_core::obsidian::vault_name(&vault),
                        &rel,
                    );
                    nook_core::obsidian::open_url(&url)?;
                    Ok::<_, String>(rel)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(_) => {
                        this.obsidian_flash = Some("Today".into());
                        this.obsidian_dirty = true;
                        this.flush_obsidian_dirty(cx);
                    }
                    Err(err) => this.obsidian_flash = Some(err),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn flush_obsidian_dirty(&mut self, cx: &mut Context<Self>) {
        if !self.obsidian_dirty {
            return;
        }
        self.refresh_obsidian_index(cx);
    }

    fn refresh_obsidian_index(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.settings.obsidian_vault.clone() else {
            self.obsidian_notes.clear();
            self.obsidian_dirty = false;
            return;
        };
        self.obsidian_dirty = false;
        cx.spawn(async move |this, cx| {
            let notes = cx
                .background_executor()
                .spawn(async move { nook_core::obsidian::index_vault(&vault) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.obsidian_notes = notes;
                cx.notify();
            });
        })
        .detach();
    }

    fn sync_obsidian_watch(&mut self, cx: &mut Context<Self>) {
        let want = if self
            .settings
            .is_enabled(nook_core::settings::WidgetModule::Obsidian)
        {
            self.settings.obsidian_vault.clone()
        } else {
            None
        };
        if want == self.obsidian_watch_vault {
            if want.is_none() {
                self.stop_obsidian_watch();
            }
            return;
        }
        self.stop_obsidian_watch();
        if let Some(vault) = want {
            self.start_obsidian_watch(vault, cx);
        }
    }

    fn stop_obsidian_watch(&mut self) {
        self.obsidian_watch = None;
        self.obsidian_watch_vault = None;
    }

    fn start_obsidian_watch(&mut self, vault: PathBuf, cx: &mut Context<Self>) {
        self.obsidian_watch_vault = Some(vault.clone());
        self.obsidian_dirty = true;
        self.refresh_obsidian_index(cx);
        match nook_core::obsidian::watch_vault(vault.clone()) {
            Ok((watch, mut rx)) => {
                self.obsidian_watch = Some(watch);
                cx.spawn(async move |this, cx| {
                    while let Some(paths) = rx.recv().await {
                        let vault = vault.clone();
                        let _ = this.update(cx, |this, cx| {
                            if this.expanded {
                                nook_core::obsidian::patch_index(
                                    &mut this.obsidian_notes,
                                    &vault,
                                    &paths,
                                );
                                cx.notify();
                            } else {
                                this.obsidian_dirty = true;
                            }
                        });
                    }
                })
                .detach();
            }
            Err(err) => log::warn!("obsidian watch: {err}"),
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

    pub(crate) fn ensure_reminders_quick_add(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Entity<crate::widgets::QuickAdd> {
        if let Some(entity) = &self.reminders_quick_add {
            return entity.clone();
        }
        let entity = cx.new(|cx| {
            crate::widgets::QuickAdd::new(
                nook_core::nl_parse::EntryKind::Reminder,
                "Remind me to call mom at 5pm…",
                cx,
            )
        });
        self.reminders_qa_sub = Some(cx.subscribe(
            &entity,
            |this, _, _: &crate::widgets::QuickAddEvent, cx| {
                this.refresh_calendar(cx);
            },
        ));
        self.reminders_quick_add = Some(entity.clone());
        entity
    }

    pub(crate) fn refresh_calendar(&mut self, cx: &mut Context<Self>) {
        if !self.settings.show_calendar && !self.settings.show_reminders {
            return;
        }
        if !self.calendar_access_requested {
            self.calendar_access_requested = true;
            nook_core::runtime().spawn(async {
                let _ = nook_core::calendar::request_calendar_access().await;
            });
        }
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

    pub(super) fn mode(&self) -> CompactMode {
        let modes = self.available_modes();
        if let Some(preferred) = self.alert_preferred.filter(|mode| modes.contains(mode)) {
            return preferred;
        }
        let user = self.user_preferred.or(self.preferred);
        if let Some(preferred) = user.filter(|mode| modes.contains(mode)) {
            return preferred;
        }
        modes.into_iter().next().unwrap_or(CompactMode::Idle)
    }

    fn sync_alert_preferred(&mut self) {
        if let Some(mode) = self.alert_preferred {
            if !self.available_modes().contains(&mode) {
                self.alert_preferred = None;
            }
        }
        // files.rs may still assign `preferred` directly.
        if self.preferred != self.user_preferred {
            if self.user_preferred.is_none() && self.preferred.is_some() {
                self.user_preferred = self.preferred;
            } else {
                self.preferred = self.user_preferred;
            }
        }
    }

    fn has_agents(&self) -> bool {
        self.settings.show_agents && !self.agents.is_empty()
    }

    fn has_observe_outage(&self) -> bool {
        self.settings.show_observe && self.observe.has_outage()
    }

    pub(super) fn has_incoming_message(&self) -> bool {
        self.settings.show_messages && self.messages.incoming.is_some()
    }
    fn has_meeting(&self) -> bool {
        self.settings.show_meetings && self.meeting.in_meeting()
    }

    fn meeting_face_shown(&self) -> bool {
        self.has_meeting()
            && (self.mode() == CompactMode::Meeting
                || (self.expanded && self.settings.show_meetings))
    }

    pub(crate) fn flash_meeting_mute(&mut self) {
        self.overlay_fade.set(1.0);
        self.meeting_flash_until = Some(Instant::now() + motion::MUTE_FLASH);
    }

    pub(crate) fn toggle_meeting_mute(&mut self, cx: &mut Context<Self>) {
        nook_core::meetings::toggle_mute();
        self.flash_meeting_mute();
        nook_core::haptics::trigger(None);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let snap = cx
                .background_executor()
                .spawn(async { nook_core::meetings::refresh() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.meeting = snap;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn leave_meeting(&mut self, cx: &mut Context<Self>) {
        nook_core::meetings::leave_meeting();
        nook_core::haptics::trigger(None);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            let snap = cx
                .background_executor()
                .spawn(async { nook_core::meetings::refresh() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.meeting = snap;
                cx.notify();
            });
        })
        .detach();
    }
    fn has_notifications(&self) -> bool {
        self.settings.show_notifications && self.notification_unread > 0
    }

    pub(crate) fn refresh_notifications(&mut self) {
        self.notifications = nook_core::notifications::snapshot();
        self.notification_unread = nook_core::notifications::unread_count();
    }

    fn available_modes(&self) -> Vec<CompactMode> {
        let mut modes = Vec::new();
        if self.has_battery_alert() {
            modes.push(CompactMode::Battery);
        }
        if self.share.is_live() {
            modes.push(CompactMode::Share);
        }
        if self.settings.experimental_widgets {
            if self.settings.show_recorder && self.recording {
                modes.push(CompactMode::Recording);
            }
            if self.has_meeting() {
                modes.push(CompactMode::Meeting);
            }
            if self.has_observe_outage() {
                modes.push(CompactMode::Observe);
            }
            if self.has_incoming_message() {
                modes.push(CompactMode::Messages);
            }
            if self.has_vpn_face() {
                modes.push(CompactMode::Vpn);
            }
            if self.has_notifications() {
                modes.push(CompactMode::Notifications);
            }
        }
        if self.has_media() {
            modes.push(CompactMode::Media);
        }
        if self.has_agents() {
            modes.push(CompactMode::Agents);
        }
        if self.settings.show_timers && self.has_live_timer() {
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

    /// Extra compact-flank inset in Liquid Glass so content clears the camera.
    /// Resting idle is a 1px wrap around the housing and has no flanks.
    pub(crate) fn glass_notch_gap(&self) -> f32 {
        if !crate::platform::island_glass_setting_on() || self.suppressed {
            return 0.0;
        }
        if self.mode() == CompactMode::Idle && !self.hovered && !self.hud_active() {
            return 0.0;
        }
        theme::GLASS_NOTCH_GAP
    }

    fn lockup_size(&self, body: f32) -> (f32, f32) {
        let w = (self.screen_width - theme::SCREEN_MARGIN)
            .min(theme::LOCKUP_MAX_WIDTH)
            .max(self.notch_width.max(180.0) + theme::COMPACT_HUD_EXTRA);
        (w, self.notch_height.max(theme::NOTCH_MIN_H) + body)
    }

    fn target_size(&self) -> (f32, f32) {
        let base_w = self.notch_width.max(180.0);
        let base_h = self.notch_height.max(theme::NOTCH_MIN_H);
        if self.expanded {
            if self.tab == Tab::Widgets
                && !self.widget_edit
                && self.has_incoming_message()
                && self.mode() == CompactMode::Messages
            {
                return self.lockup_size(theme::NOOK_INSET + theme::NOOK_BODY);
            }
            if self.tab == Tab::Widgets
                && !self.widget_edit
                && self.settings.show_recorder
                && self.mode() == CompactMode::Recording
            {
                return self.lockup_size(theme::NOOK_INSET + theme::RECORDER_BODY);
            }
            let w = self.expanded_width();
            let body = if self.tab == Tab::Terminal {
                // Outer pad lives on the pane so the PTY grid is not sized
                // through a parent that then clips it. Default 18×14 cells.
                // Terminal states are out of scope for the 0923 chrome pass;
                // keep the PTY-sized height.
                theme::EXPANDED_PAD + crate::widgets::terminal_pane_min_height()
            } else if self.tab == Tab::Files {
                let extra = if self.share.shows_picker() { 88.0 } else { 0.0 };
                theme::NOOK_BODY + extra
            } else {
                let (rows, _) = self.nook_row_count_for_render();
                let rows = rows.max(1) as f32;
                let mut body = rows * theme::NOOK_BODY + (rows - 1.0) * theme::NOOK_ROW_GAP;
                if self.widget_edit {
                    body += theme::WIDGET_EDIT_PICKER_H + theme::NOOK_INSET;
                }
                body
            };
            let tab_h = if self.tab == Tab::Terminal {
                self.notch_height.max(theme::NOTCH_MIN_H)
            } else {
                theme::EXPANDED_TAB_H
            };
            let h = (tab_h + body).min(self.screen_height - theme::SCREEN_MARGIN);
            return (w, h);
        }
        let glass_gap = 2.0 * self.glass_notch_gap();
        let live_w = theme::COMPACT_LIVE_W.max(base_w);
        if self.mode() == CompactMode::Recording {
            let extra = if self.hovered {
                crate::widgets::RECORDER_COMPACT_HOVER_EXTRA
            } else {
                crate::widgets::RECORDER_COMPACT_EXTRA
            };
            let h = if self.hovered {
                base_h + theme::COMPACT_HOVER_CHIN
            } else {
                base_h + theme::COMPACT_HEIGHT_OVERFLOW
            };
            return (base_w + extra + glass_gap, h);
        }
        if self.hovered {
            return (
                live_w + (theme::COMPACT_HOVER_EXTRA - theme::COMPACT_LIVE_EXTRA) + glass_gap,
                base_h + theme::COMPACT_HOVER_CHIN,
            );
        }
        if self.hud_active() {
            return (
                live_w.max(base_w + theme::COMPACT_HUD_EXTRA) + glass_gap,
                base_h + theme::COMPACT_HEIGHT_OVERFLOW,
            );
        }
        if self.mode() == CompactMode::Idle {
            let h = if self.settings.non_notch_mode {
                1.0
            } else {
                self.notch_height + theme::IDLE_NOTCH_OVERFLOW + theme::COMPACT_HEIGHT_OVERFLOW
            };
            // Glass gap is for live-activity flanks, not the idle housing wrap.
            return (self.notch_width + theme::IDLE_NOTCH_OVERFLOW, h);
        }
        (live_w + glass_gap, base_h + theme::COMPACT_HEIGHT_OVERFLOW)
    }

    pub(super) fn expanded_width(&self) -> f32 {
        // Every expanded tab uses the full default width (the Terminal width) so
        // the island stays one consistent size across Nook / Tray / Term.
        let screen_cap = (self.screen_width - theme::SCREEN_MARGIN)
            .min(theme::EXPANDED_MAX_WIDTH + self.queue_extra_width());
        let base = (self.screen_width - theme::SCREEN_MARGIN).min(theme::EXPANDED_MAX_WIDTH);
        // Music "Up Next" widens beyond the base when open.
        let width = base + self.queue_extra_width();
        width.min(screen_cap)
    }

    /// Modules `render_nook` would actually paint, in order, with cell widths.
    pub(super) fn visible_nook_items(&self) -> Vec<(WidgetModule, u8)> {
        use nook_core::messages::FdaStatus;
        let editing = self.widget_edit;
        let mut out = Vec::new();
        for module in self.settings.ordered_widgets() {
            if !self.settings.widget_visible(module) {
                continue;
            }
            let show = match module {
                WidgetModule::Music => self.settings.show_media,
                WidgetModule::Calendar => self.settings.show_calendar,
                WidgetModule::Mirror => self.settings.show_mirror,
                WidgetModule::Agents => self.settings.show_agents,
                WidgetModule::Meeting => self.settings.is_enabled(module),
                WidgetModule::Observe => self.settings.show_observe,
                WidgetModule::Reminders => self.settings.show_reminders,
                WidgetModule::Timers => self.settings.show_timers,
                WidgetModule::Notes => self.settings.show_notes,
                WidgetModule::Obsidian => self.settings.is_enabled(module),
                WidgetModule::Speed => self.settings.show_speed,
                WidgetModule::Battery => self.settings.show_battery,
                WidgetModule::Messages => {
                    self.settings.show_messages
                        && (editing
                            || self.messages.incoming.is_some()
                            || self.messages.fda != FdaStatus::Granted)
                }
                WidgetModule::Weather => self.settings.weather.enabled,
                WidgetModule::Vpn => self.settings.show_vpn,
                WidgetModule::HighAlert => self.settings.show_high_alert,
                WidgetModule::SysStats => self.settings.show_sysstats,
                WidgetModule::Recorder => self.settings.show_recorder,
                WidgetModule::Notifications => self.settings.show_notifications,
                WidgetModule::Files => false,
            };
            if show {
                out.push((module, self.settings.cells_for(module)));
            }
        }
        out
    }

    /// Packed Nook rows for sizing / layout (same greedy pack as Settings).
    pub(super) fn nook_rows_for_render(&self) -> Vec<Vec<(WidgetModule, u8)>> {
        nook_core::settings::pack_rows(&self.visible_nook_items(), AppSettings::TOTAL_CELLS)
    }

    /// Row count (and last-row leftover cells) matching what `render_nook` paints,
    /// including the customize append slot when the last row is full.
    pub(super) fn nook_row_count_for_render(&self) -> (usize, u8) {
        let mut packed = self.nook_rows_for_render();
        packed.truncate(AppSettings::MAX_ROWS);
        let last_remaining = match packed.last() {
            Some(row) => {
                let used = row
                    .iter()
                    .map(|(_, cells)| *cells)
                    .fold(0u8, |sum, cells| sum.saturating_add(cells));
                AppSettings::TOTAL_CELLS.saturating_sub(used)
            }
            None => AppSettings::TOTAL_CELLS,
        };
        let last_row_full = !packed.is_empty() && last_remaining == 0;
        let mut rows = packed.len();
        if self.widget_edit && last_row_full && rows < AppSettings::MAX_ROWS {
            rows += 1;
        }
        (rows.clamp(1, AppSettings::MAX_ROWS), last_remaining)
    }

    /// Enter on-island widget customize mode.
    pub(crate) fn begin_widget_edit(&mut self, cx: &mut Context<Self>) {
        if self.widget_edit {
            return;
        }
        self.widget_edit_snapshot = Some(self.settings.clone());
        self.widget_edit = true;
        self.widget_edit_budget_hint_at = None;
        self.tab = Tab::Widgets;
        self.expanded = true;
        self.close_notes_editor(cx);
        self.obsidian_typing = false;
        self.shell_focused = false;
        self.repositioning = false;
        // Size morph runs via MORPH; force a content dissolve even when already
        // expanded on Widgets (arm would otherwise no-op).
        self.arm_content_transition();
        cx.notify();
    }

    /// While customizing, keep real widget previews on screen but freeze the
    /// per-frame pumps (visualizer, LED shimmer, aura, mirror frames) so edit
    /// chrome does not repaint at ~30–50 Hz.
    pub(crate) fn paints_live_widgets(&self) -> bool {
        !self.widget_edit
    }

    pub(crate) fn finish_widget_edit(&mut self, cx: &mut Context<Self>) {
        self.widget_edit = false;
        self.widget_edit_snapshot = None;
        self.widget_edit_budget_hint_at = None;
        // Live tweaks already persisted; refresh local cache from store.
        self.settings = nook_core::settings::get_app_settings();
        self.arm_content_transition();
        cx.notify();
    }

    pub(crate) fn cancel_widget_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.widget_edit_snapshot.take() {
            nook_core::settings::update_app_settings(snapshot.clone());
            self.settings = snapshot;
        }
        self.widget_edit = false;
        self.widget_edit_budget_hint_at = None;
        self.arm_content_transition();
        cx.notify();
    }

    /// Lowest screen edge the island can reach over every expanded tab.
    /// Files height uses the Files-tab width (screen-capped
    /// [`theme::EXPANDED_MAX_WIDTH`]), not the content-sized Widgets width.
    /// Used to reserve the overlay strip so a tab switch or expand never
    /// resizes the NSWindow mid-animation (CoreAnimation would show one
    /// stretched stale frame).
    pub(super) fn expanded_bottom(&self) -> f32 {
        let files_w = (self.screen_width - theme::SCREEN_MARGIN).min(theme::EXPANDED_MAX_WIDTH);
        let mut widgets_body = theme::NOOK_BODY;
        if self.widget_edit {
            widgets_body += theme::WIDGET_EDIT_PICKER_H + theme::NOOK_INSET;
        }
        let mut body = widgets_body;
        if self.settings.show_files {
            let extra = if self.share.shows_picker() { 88.0 } else { 0.0 };
            body = body.max(theme::NOOK_BODY + extra);
        }
        let tab_h = theme::EXPANDED_TAB_H;
        let mut h = tab_h + body;
        if self.settings.terminal_enabled {
            let term = self.notch_height.max(theme::NOTCH_MIN_H)
                + theme::EXPANDED_PAD
                + crate::widgets::terminal_pane_min_height();
            h = h.max(term);
        }
        let w = self.expanded_width().max(files_w);
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
            let tabs = self.shown_tabs();
            let from = tabs.iter().position(|tab| *tab == self.last_tab);
            let to = tabs.iter().position(|tab| *tab == self.tab);
            let delta = match (from, to) {
                (Some(from), Some(to)) => to as isize - from as isize,
                _ => {
                    if self.tab == Tab::Files {
                        1
                    } else {
                        -1
                    }
                }
            };
            return (HORIZONTAL * delta.signum() as f32, 0.0);
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

    /// Arm the content crossfade at the moment expand/tab/mode/edit changes.
    /// No-op when nothing changed — safe to call before every notify.
    /// Use [`Self::force_content_transition`] for in-place edit add/remove/reorder.
    pub(super) fn arm_content_transition(&mut self) {
        let mode = self.mode();
        let force = self.content_transition_force;
        if !force
            && self.expanded == self.last_expanded
            && mode == self.last_mode
            && self.tab == self.last_tab
            && self.widget_edit == self.last_widget_edit
        {
            return;
        }
        self.content_transition_force = false;
        let (x, y) = if force {
            (0.0, 0.0)
        } else {
            self.content_transition_offset(mode)
        };
        self.content_fade.set(0.0);
        self.content_x.set(x);
        self.content_y.set(y);
        if !self.reduce_motion {
            // Hold lite for the whole morph/shift, including the last
            // pixels — a mid-spring threshold flipped full paint back on
            // too early. Mode/tab swaps need this too: size may already
            // be parked while content_x still travels.
            self.agent_morph_lite = true;
        }
        self.last_expanded = self.expanded;
        self.last_mode = mode;
        self.last_tab = self.tab;
        self.last_widget_edit = self.widget_edit;
    }

    /// Crossfade after an in-place widget edit layout change (tap/drop/−).
    pub(super) fn force_content_transition(&mut self) {
        self.content_transition_force = true;
        self.arm_content_transition();
    }

    /// Advance every animated value one frame on its `motion` spring.
    /// Returns whether we still need frames.
    fn step_spring(&mut self, dt: f32) -> bool {
        let (tw, th) = self.target_size();
        self.arm_content_transition();

        let mut moving = false;
        if let Some(at) = self.widget_edit_budget_hint_at {
            if at.elapsed() >= Duration::from_secs(2) {
                self.widget_edit_budget_hint_at = None;
                moving = true;
            }
        }
        if self.reduce_motion {
            // HIG › Motion: motion must be optional. Size and spatial travel
            // park instantly and the blur stays off; the crossfade below still
            // runs as a simple dissolve.
            self.anim_w.set(tw);
            self.anim_h.set(th);
            self.content_x.set(0.0);
            self.content_y.set(0.0);
            self.agent_morph_lite = false;
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
        if !self.reduce_motion
            && self.agent_morph_lite
            && (self.anim_w.value - tw).abs() <= motion::REST_PX
            && (self.anim_h.value - th).abs() <= motion::REST_PX
            && self.content_x.value.abs() <= motion::REST_PX
            && self.content_y.value.abs() <= motion::REST_PX
            && (1.0 - self.content_fade.value).abs() <= motion::REST_ALPHA
        {
            self.agent_morph_lite = false;
        }
        let overlay = if self.mode() == CompactMode::Meeting {
            if self
                .meeting_flash_until
                .is_some_and(|until| Instant::now() < until)
            {
                1.0
            } else {
                self.meeting_flash_until = None;
                0.0
            }
        } else {
            media::album_overlay_target(self.hovered)
        };
        moving |= self
            .overlay_fade
            .step(motion::REVEAL, overlay, dt, motion::REST_ALPHA);
        self.latch_agent_border_color();
        let border_target = if self.agent_is_working() { 1.0 } else { 0.0 };
        if self.reduce_motion {
            self.agent_border.set(border_target);
        } else {
            moving |= self
                .agent_border
                .step(motion::REVEAL, border_target, dt, motion::REST_ALPHA);
        }
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

    fn agent_is_working(&self) -> bool {
        self.agents.iter().any(|agent| agent.status.is_working())
    }

    /// True from expand/collapse or a mode/tab context shift until size,
    /// content travel, and the crossfade fully rest. Brand faces and chrome
    /// drop glow detail for that window only — not on compact hover, which
    /// was flickering when keyed off size delta.
    pub(crate) fn size_morphing(&self) -> bool {
        self.agent_morph_lite
    }

    fn latch_agent_border_color(&mut self) {
        if let Some(agent) = crate::widgets::face_agent(&self.agents) {
            if agent.status.is_working() {
                self.agent_border_color = Some(crate::dotmatrix::led_color_on(
                    agent.kind,
                    theme::island_fill(self.settings.island_color),
                ));
            }
        }
        if self.agent_border.value <= motion::REST_ALPHA && !self.agent_is_working() {
            self.agent_border_color = None;
        }
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

    /// Close the topmost open layer, then collapse.
    pub(crate) fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        if self.widget_edit {
            self.cancel_widget_edit(cx);
            return;
        }
        if self.output_picker_open {
            self.output_picker_open = false;
            cx.notify();
            return;
        }
        if self.queue_open {
            self.queue_open = false;
            cx.notify();
            return;
        }
        if self.mirror_on {
            self.stop_mirror(cx);
            return;
        }
        if self.notes_editing {
            self.close_notes_editor(cx);
            return;
        }
        if self.expanded {
            self.expanded = false;
            self.close_notes_editor(cx);
            self.stop_mirror(cx);
            self.park_terminal();
            nook_core::power::set_detail_watch(false);
            nook_core::haptics::trigger(None);
            self.arm_lyrics_line_timer(cx);
            self.arm_content_transition();
            cx.notify();
        }
    }

    fn on_island_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &event.keystroke;
        if ks.modifiers.platform && ks.key == "," {
            self.open_settings(cx);
            return;
        }
        let own = self.focus.as_ref().is_some_and(|f| f.is_focused(window));
        if !own {
            return;
        }
        if ks.key == "escape" {
            self.dismiss(window, cx);
            return;
        }
        if ks.modifiers.platform && ks.key == "w" {
            self.dismiss(window, cx);
            return;
        }
        if ks.key == "up" {
            if self.expanded {
                self.dismiss(window, cx);
            }
            return;
        }
        if ks.key == "down" {
            if !self.expanded {
                self.toggle_expanded(cx);
            }
            return;
        }
        if ks.key == "space" || ks.key == "enter" {
            self.toggle_expanded(cx);
            return;
        }
        if ks.key == "left" || ks.key == "right" {
            let next = ks.key == "right";
            let acted = if self.expanded {
                self.cycle_tab(next);
                true
            } else {
                self.cycle_mode(next)
            };
            if acted {
                if !self.expanded {
                    self.close_notes_editor(cx);
                    self.stop_mirror(cx);
                    self.park_terminal();
                }
                self.arm_lyrics_line_timer(cx);
                self.arm_content_transition();
                cx.notify();
            }
        }
    }

    fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        if self.widget_edit {
            return;
        }
        self.expanded = !self.expanded;
        if self.expanded {
            if self.tab != Tab::Terminal || !self.settings.terminal_enabled {
                self.tab = match self.mode() {
                    CompactMode::Files => Tab::Files,
                    _ => Tab::Widgets,
                };
            }
            self.correct_hidden_tab();
            nook_core::audio::note_media_event();
            if self.first_run {
                self.first_run = false;
                nook_core::settings::mark_onboarded();
            }
        } else {
            self.close_notes_editor(cx);
            self.stop_mirror(cx);
            self.park_terminal();
        }
        nook_core::power::set_detail_watch(self.expanded && self.settings.show_battery);
        nook_core::haptics::trigger(None);
        self.arm_lyrics_line_timer(cx);
        self.arm_content_transition();
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
        self.user_preferred = Some(modes[new_idx]);
        self.preferred = self.user_preferred;
        self.alert_preferred = None;
        nook_core::haptics::trigger(None);
        true
    }

    fn on_wheel(&mut self, event: &gpui::ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(16.0));
        if self.apply_wheel(delta.x.into(), delta.y.into(), event.touch_phase) {
            if !self.expanded {
                self.close_notes_editor(cx);
                self.stop_mirror(cx);
                self.park_terminal();
            }
            self.arm_lyrics_line_timer(cx);
            cx.notify();
        }
    }

    /// One physical two-finger swipe → one compact-mode / expand / tab change.
    fn apply_wheel(&mut self, dx: f32, dy: f32, phase: TouchPhase) -> bool {
        if self.widget_edit {
            return false;
        }
        let threshold = motion::SWIPE_THRESHOLD;
        let idle = motion::SWIPE_IDLE;

        let now = Instant::now();
        if matches!(phase, TouchPhase::Started)
            || now.saturating_duration_since(self.last_wheel_at) >= idle
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
            if ax.abs() <= threshold {
                false
            } else if self.expanded {
                self.cycle_tab(ax > 0.0);
                true
            } else {
                self.cycle_mode(ax > 0.0)
            }
        } else if ay.abs() <= threshold {
            false
        } else if !self.expanded && ay > 0.0 {
            // AppKit scrollingDeltaY: two-finger swipe *down* is positive.
            self.expanded = true;
            nook_core::power::set_detail_watch(self.settings.show_battery);
            nook_core::audio::note_media_event();
            nook_core::haptics::trigger(None);
            true
        } else if self.expanded && ay < 0.0 {
            self.expanded = false;
            nook_core::power::set_detail_watch(false);
            nook_core::haptics::trigger(None);
            true
        } else {
            false
        };

        if acted {
            self.wheel_locked = true;
            self.wheel_acc_x = 0.0;
            self.wheel_acc_y = 0.0;
            self.arm_content_transition();
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
        if self.widget_edit {
            return;
        }
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
        if self.widget_edit {
            self.cancel_widget_edit(cx);
        }
        if self.expanded {
            self.expanded = false;
            self.close_notes_editor(cx);
            self.stop_mirror(cx);
            self.park_terminal();
            nook_core::power::set_detail_watch(false);
            self.arm_lyrics_line_timer(cx);
            self.arm_content_transition();
        }
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
        let background = if platform::reduce_transparency() {
            WindowBackgroundAppearance::Opaque
        } else {
            WindowBackgroundAppearance::Blurred
        };
        let Ok(handle) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("openNook Settings".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                kind: WindowKind::Normal,
                is_resizable: true,
                focus: true,
                show: true,
                window_background: background,
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
        let files: Vec<std::path::PathBuf> = paths.paths().to_vec();
        if files.is_empty() {
            return;
        }
        nook_core::haptics::trigger(None);
        platform::share_via_airdrop(&files);
        cx.notify();
    }

    fn ingest_paths(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        self.ingest_external_paths(paths.paths().to_vec(), cx);
    }

    /// Shared by drag-drop, `opennook://tray/add`, and Finder Services.
    pub(crate) fn ingest_external_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let mut added = false;
        let mut dupes = 0u32;
        let mut invalid = 0u32;
        for path in paths {
            let raw = path.to_string_lossy().into_owned();
            let Ok(resolved) = nook_core::automation::validate_tray_path(&raw) else {
                invalid += 1;
                continue;
            };
            if self.files.iter().any(|f| f.path == resolved) {
                dupes += 1;
                continue;
            }
            match nook_core::files::add_dropped_path(&resolved) {
                Ok(item) => {
                    self.files.push(item);
                    added = true;
                }
                Err(_) => invalid += 1,
            }
        }
        if dupes > 0 {
            self.output_hud_name = Some(format!("{dupes} already in the tray"));
            self.output_hud_until = Some(Instant::now() + motion::OUTPUT_HUD_TTL);
        } else if invalid > 0 {
            self.output_hud_name = Some(format!("{invalid} couldn't be added"));
            self.output_hud_until = Some(Instant::now() + motion::OUTPUT_HUD_TTL);
        }
        if added {
            let _ = nook_core::files::save_file_tray(self.files.clone());
            self.user_preferred = Some(CompactMode::Files);
            self.preferred = self.user_preferred;
            self.alert_preferred = None;
            self.tab = Tab::Files;
            self.expanded = true;
            nook_core::audio::note_media_event();
            nook_core::haptics::trigger(None);
            self.arm_content_transition();
        }
        if added || dupes > 0 || invalid > 0 {
            cx.notify();
        }
    }

    pub(crate) fn apply_external_action(&mut self, action: ExternalAction, cx: &mut Context<Self>) {
        if self.widget_edit && !matches!(action, ExternalAction::EditWidgets) {
            return;
        }
        match action {
            ExternalAction::TrayAdd(paths) => self.ingest_external_paths(paths, cx),
            ExternalAction::TrayClear => {
                if !self.files.is_empty() {
                    // files.rs renders the Undo chip
                    self.last_cleared_files =
                        Some((std::mem::take(&mut self.files), Instant::now()));
                    let _ = nook_core::files::save_file_tray(self.files.clone());
                }
                cx.notify();
            }
            ExternalAction::TimerStart { seconds } => {
                self.add_timer(seconds);
                if self.hovered {
                    self.expanded = true;
                    self.tab = Tab::Widgets;
                    self.arm_content_transition();
                } else {
                    self.alert_preferred = Some(CompactMode::Timer);
                }
                cx.notify();
            }
            ExternalAction::Expand => {
                self.expanded = true;
                self.arm_content_transition();
                cx.notify();
            }
            ExternalAction::OpenSettings => {
                self.open_settings(cx);
            }
            ExternalAction::EditWidgets => {
                self.begin_widget_edit(cx);
            }
        }
    }

    fn spawn_external_actions(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            let action = cx
                .background_executor()
                .spawn(async { nook_core::automation::recv_action().await })
                .await;
            if this
                .update(cx, |this, cx| this.apply_external_action(action, cx))
                .is_err()
            {
                break;
            }
        })
        .detach();
    }

    pub(crate) fn shown_tabs(&self) -> Vec<Tab> {
        let mut tabs = vec![Tab::Widgets];
        if self.settings.show_files {
            tabs.push(Tab::Files);
        }
        if self.settings.terminal_enabled {
            tabs.push(Tab::Terminal);
        }
        tabs
    }

    /// Keep `self.tab` on a visible tab after settings hide Files/Terminal.
    fn correct_hidden_tab(&mut self) {
        let tabs = self.shown_tabs();
        if !tabs.contains(&self.tab) {
            self.tab = tabs.first().copied().unwrap_or(Tab::Widgets);
        }
    }
}

fn prev_char_boundary(text: &str, index: usize) -> usize {
    if index == 0 {
        return 0;
    }
    let mut i = index.min(text.len());
    while i > 0 {
        i -= 1;
        if text.is_char_boundary(i) {
            return i;
        }
    }
    0
}

fn next_char_boundary(text: &str, index: usize) -> usize {
    let len = text.len();
    let mut i = index.min(len);
    if i >= len {
        return len;
    }
    i += 1;
    while i < len && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

impl Island {
    fn cycle_tab(&mut self, next: bool) {
        if self.widget_edit {
            return;
        }
        let tabs = self.shown_tabs();
        if tabs.is_empty() {
            self.tab = Tab::Widgets;
            self.arm_content_transition();
            return;
        }
        let idx = tabs.iter().position(|tab| *tab == self.tab).unwrap_or(0);
        let new_idx = if next {
            (idx + 1) % tabs.len()
        } else {
            (idx + tabs.len() - 1) % tabs.len()
        };
        self.tab = tabs[new_idx];
        self.arm_content_transition();
    }

    pub(crate) fn ensure_terminal(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Entity<crate::widgets::TerminalView> {
        if let Some(existing) = &self.terminal {
            return existing.clone();
        }
        let shell = nook_core::shell::resolved_shell(&self.settings.terminal_shell);
        let view = cx.new(|cx| crate::widgets::TerminalView::new(shell, cx));
        self.terminal_sub = Some(cx.subscribe(&view, |this, _, event, cx| match event {
            crate::widgets::TerminalEvent::State { running, exit } => {
                this.shell_running = *running;
                this.shell_exit = *exit;
                cx.notify();
            }
            crate::widgets::TerminalEvent::Focus(focused) => {
                this.shell_focused = *focused;
                cx.notify();
            }
        }));
        self.shell_running = true;
        self.shell_exit = None;
        self.terminal = Some(view.clone());
        view
    }

    /// Collapse without killing the shell: the session keeps running in the
    /// background and is shown again on the next expand.
    pub(crate) fn park_terminal(&mut self) {
        self.shell_focused = false;
    }

    pub(crate) fn close_terminal(&mut self, cx: &mut Context<Self>) {
        self.terminal_sub.take();
        if let Some(view) = self.terminal.take() {
            view.update(cx, |view, _| view.shutdown());
        }
        self.shell_running = false;
        self.shell_focused = false;
    }

    pub(crate) fn restart_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shell = nook_core::shell::resolved_shell(&self.settings.terminal_shell);
        if let Some(view) = self.terminal.clone() {
            view.update(cx, |view, cx| view.restart(shell, cx));
            window.focus(&view.read(cx).focus_handle(cx));
            self.shell_running = true;
            self.shell_exit = None;
            self.shell_focused = true;
        } else {
            let _ = self.ensure_terminal(cx);
        }
        cx.notify();
    }

    pub(crate) fn add_timer(&mut self, seconds: u32) {
        let id = self.next_timer_id;
        self.timers.push(Timer {
            id,
            name: String::new(),
            remaining: seconds,
            total: seconds,
            running: true,
            kind: TimerKind::Countdown,
            ends_at: None,
        });
        self.next_timer_id += 1;
        self.alert_preferred = Some(CompactMode::Timer);
        crate::notify::schedule_island_timer(id, seconds, "");
    }

    pub(crate) fn add_pomodoro(&mut self) {
        let spec = PomodoroSpec::new(
            self.settings.pomodoro_work_secs,
            self.settings.pomodoro_break_secs,
            self.settings.pomodoro_long_break_secs,
            self.settings.pomodoro_cycles_per_long,
            self.settings.pomodoro_auto_advance,
        );
        let secs = spec.duration_secs();
        self.timers.push(Timer {
            id: self.next_timer_id,
            name: spec.label().to_string(),
            remaining: secs,
            total: secs,
            running: true,
            kind: TimerKind::Pomodoro(spec),
            ends_at: Some(SystemTime::now() + Duration::from_secs(secs as u64)),
        });
        self.next_timer_id += 1;
        self.alert_preferred = Some(CompactMode::Timer);
        self.on_pomodoro_edge(true);
        self.sync_pomodoro_awake();
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
        if dx * dx + dy * dy < motion::DRAG_SLOP as f64 {
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
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string();
        let n = self.files.len();
        self.files.retain(|f| f.path != path);
        if self.files.len() == n {
            return true;
        }
        self.tray_flash = Some((format!("{name} is missing"), Instant::now()));
        let _ = nook_core::files::save_file_tray(self.files.clone());
        true
    }

    /// Restore the last cleared tray. files.rs renders the Undo chip.
    pub(crate) fn undo_clear_files(&mut self, cx: &mut Context<Self>) {
        if let Some((files, _)) = self.last_cleared_files.take() {
            self.files = files;
            let _ = nook_core::files::save_file_tray(self.files.clone());
            cx.notify();
        }
    }
}

pub(crate) const QUEUE_ROW_H: f32 = 40.0;
pub(crate) const QUEUE_PANEL_W: f32 = 240.0;
/// Flex gaps (12 + 12) plus the 1px rule between player and queue.
pub(crate) const QUEUE_SIDE_CHROME: f32 = 25.0;

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
    use crate::island::files::{file_tile_height, files_pane_min_height};
    use crate::island::ui::format_timer;
    use nook_core::agents::{AgentKind, AgentStatus};
    use nook_core::notifications::NotificationEvent;
    use std::collections::{HashMap, VecDeque};
    use std::rc::Rc;
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
            user_preferred: None,
            preferred: None,
            alert_preferred: None,
            tab: Tab::Widgets,
            now_playing: NowPlayingData::default(),
            visualizer_color: None,
            lyrics: None,
            lyrics_key: None,
            lyrics_anchor_elapsed: 0.0,
            lyrics_anchor_at: Instant::now(),
            lyrics_timer_gen: 0,
            aura_palette: None,
            aura_t: 0.0,
            last_aura_frame: Instant::now(),
            motion_art_key: None,
            motion_art_bounds: None,
            motion_art_bounds_gen: 0,
            last_motion_spec: None,
            queue: PlaybackQueue::default(),
            queue_key: None,
            queue_inflight: false,
            scrubber_drag: None,
            scrubber_bounds: Rc::new(RefCell::new(None)),
            elapsed_base: None,
            elapsed_at: Instant::now(),
            play_intent: None,
            seek_intent: None,
            files: Vec::new(),
            events: Vec::new(),
            reminders: Vec::new(),
            agents: Vec::new(),
            notes: String::new(),
            notes_editor: None,
            notes_editing: false,
            notes_sub: None,
            obsidian_notes: Vec::new(),
            obsidian_dirty: false,
            obsidian_watch: None,
            obsidian_watch_vault: None,
            obsidian_capture: String::new(),
            obsidian_capture_caret: 0,
            obsidian_capture_select_all: false,
            obsidian_capture_focus: None,
            obsidian_typing: false,
            obsidian_selected: None,
            obsidian_body: None,
            obsidian_flash: None,
            reminders_quick_add: None,
            reminders_qa_sub: None,
            timers: Vec::new(),
            system_timers: Vec::new(),
            next_timer_id: 1,
            calendar_day: 3,
            awake_deadline: None,
            awake_active: false,
            observe: ObserveSnapshot::default(),
            observe_history: HashMap::new(),
            messages: MessagesSnapshot::default(),
            message_draft: String::new(),
            selected_conversation: None,
            message_focus: None,
            observe_hover: None,
            power: PowerSnapshot::default(),
            lpm_pending: false,
            lpm_error: None,
            vpn: VpnSnapshot::default(),
            vpn_reveal_until: None,
            sysstats: nook_core::sysstats::SysSnapshot::default(),
            sysstats_sampling: false,
            meeting: MeetingSnapshot::default(),
            notifications: Vec::new(),
            notification_unread: 0,
            settings: AppSettings::default(),
            widget_edit: false,
            widget_edit_snapshot: None,
            widget_edit_budget_hint_at: None,
            last_widget_edit: false,
            content_transition_force: false,
            first_run: false,
            speed_mbps: None,
            speed_progress: 0.0,
            speed_running: false,
            speed_gen: 0,
            weather: None,
            weather_error: None,
            weather_inflight: false,
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
            agent_border: SpringValue::at(0.0),
            agent_border_color: None,
            meeting_flash_until: None,
            reduce_motion: false,
            blur: 0.0,
            last_expanded: false,
            agent_morph_lite: false,
            last_mode: CompactMode::Idle,
            last_tab: Tab::Widgets,
            file_drag: false,
            pending_file_drag: None,
            suppressed: false,
            repositioning: false,
            reposition_grab_x: 0.0,
            reposition_grab_y: 0.0,
            click_through: true,
            click_through_at: Instant::now(),
            wheel_locked: false,
            last_wheel_at: Instant::now() - Duration::from_secs(1),
            wheel_acc_x: 0.0,
            wheel_acc_y: 0.0,
            pixel_origin: Instant::now(),
            pixel_t: 0.0,
            last_pixel_frame: Instant::now(),
            mirror_on: false,
            mirror_gen: 0,
            mirror_frame: None,
            hud: None,
            hud_fill: SpringValue::at(0.0),
            hud_dragging: false,
            share: nook_core::share::ShareSession::default(),
            terminal: None,
            terminal_sub: None,
            shell_running: false,
            shell_exit: None,
            shell_focused: false,
            output_devices: Vec::new(),
            output_picker_open: false,
            queue_open: false,
            output_hud_name: None,
            output_hud_until: None,
            recording: false,
            recording_started: None,
            live_transcript: String::new(),
            recordings: Vec::new(),
            recorder_level: 0.0,
            recorder_wave: VecDeque::new(),
            recorder_wave_at: Instant::now(),
            recorder_error: None,
            playing_recording: None,
            recorder_last_notify: Instant::now(),
            focus: None,
            hover_exit_at: None,
            alt_held: false,
            last_cleared_files: None,
            tray_flash: None,
            calendar_access_requested: false,
            fallback_db_noticed: false,
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
        let island_h = 160.0_f64;
        let radius = 36.0_f64;
        let window_h = 1169.0_f64;
        let (_, y, _, h) =
            crate::platform::cocoa_rect_from_gpui(200.0, 0.0, 400.0, island_h, window_h);
        // Attached glass is island height plus corner radius so the top
        // rounding sits past the window edge and is clipped.
        let under = h + radius.max(0.0);
        assert_eq!(y, window_h - island_h, "bottom of the island stays put");
        assert!(
            y + under > window_h,
            "top rounding sits past the window edge and is clipped"
        );
        assert_eq!(h, island_h, "detached glass matches the island height");
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
                tint: None,
                border: None,
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
        assert_eq!(format_timer(60), "1:00");
        assert_eq!(format_timer(65), "1:05");
        assert_eq!(format_timer(1500), "25:00");
        assert_eq!(format_timer(3600), "1:00:00");
        assert_eq!(super::ui::format_timer_compact(0), "0s");
        assert_eq!(super::ui::format_timer_compact(45), "45s");
        assert_eq!(super::ui::format_timer_compact(65), "1m05");
        assert_eq!(super::ui::format_timer_compact(3600), "1h");
        assert_eq!(super::ui::format_timer_compact(3900), "1h05");
    }

    #[test]
    fn speed_run_starts_at_zero_and_clears_on_cancel() {
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
        assert_eq!(island.speed_mbps, None);
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
    fn queue_panel_adds_width_when_toggled_open() {
        let mut island = test_island();
        island.now_playing.title = Some("Track".into());
        island.now_playing.app_name = Some("Music".into());
        island.settings.show_media = true;
        island.settings.show_media_queue = true;
        island.expanded = true;
        island.tab = Tab::Widgets;
        let closed = island.expanded_width();
        let closed_music = island.music_pane_width(5);
        assert!(!island.queue_panel_visible());
        assert_eq!(island.queue_extra_width(), 0.0);
        assert_eq!(closed_music, 5.0 * theme::NOOK_CELL);

        island.queue_open = true;
        assert!(island.queue_panel_visible());
        assert!(island.queue_extra_width() > 0.0);
        assert!(island.expanded_width() > closed);
        // Music cell must claim the extra width — otherwise the panel
        // crushes the player inside a fixed cell while the island grows empty.
        assert!(
            (island.music_pane_width(5) - closed_music - island.queue_extra_width()).abs() < 0.5
        );
        assert_eq!(
            island.music_player_width(),
            5.0 * theme::NOOK_CELL,
            "queue open must keep the player column at the closed Music width"
        );
    }

    #[test]
    fn queue_panel_raises_expanded_max_so_full_row_still_fits() {
        let mut island = test_island();
        island.now_playing.title = Some("Track".into());
        island.now_playing.app_name = Some("Music".into());
        island.settings.show_media = true;
        island.settings.show_media_queue = true;
        island.settings.show_calendar = true;
        island.settings.show_timers = true;
        island
            .settings
            .set_cells(nook_core::settings::WidgetModule::Music, 5);
        island
            .settings
            .set_cells(nook_core::settings::WidgetModule::Calendar, 4);
        island
            .settings
            .set_cells(nook_core::settings::WidgetModule::Timers, 2);
        island.expanded = true;
        island.tab = Tab::Widgets;
        island.screen_width = 1800.0;
        island.queue_open = true;
        let w = island.expanded_width();
        let need = island.settings.nook_content_width(
            theme::NOOK_CELL,
            theme::NOOK_DIVIDER,
            theme::NOOK_INSET,
        ) + island.queue_extra_width();
        assert!(
            w + 0.5 >= need.min(island.screen_width - 40.0),
            "expanded_width={w} need={need} max={}",
            theme::EXPANDED_MAX_WIDTH
        );
        assert!(w > theme::EXPANDED_MAX_WIDTH);
    }

    #[test]
    fn nook_stays_one_row_within_budget() {
        let mut island = test_island();
        island.expanded = true;
        island.tab = Tab::Widgets;
        island.settings.show_media = true;
        island.settings.show_calendar = true;
        island.settings.show_timers = true;
        island.settings.show_battery = true;
        island.settings.weather.enabled = true;
        island
            .settings
            .set_cells(nook_core::settings::WidgetModule::Music, 5);
        island
            .settings
            .set_cells(nook_core::settings::WidgetModule::Calendar, 4);
        island
            .settings
            .set_cells(nook_core::settings::WidgetModule::Timers, 2);
        island
            .settings
            .set_cells(nook_core::settings::WidgetModule::Weather, 3);
        island
            .settings
            .set_cells(nook_core::settings::WidgetModule::Battery, 3);
        // Music 5 + Calendar 4 + Timers 2 + Weather 3 + Battery 3 = 17.
        assert_eq!(island.nook_rows_for_render().len(), 1);
        let h1 = island.target_size().1;
        let h2 = island.target_size().1;
        assert_eq!(h1, h2);
    }

    #[test]
    fn visible_nook_items_matches_settings_rows_when_nothing_special() {
        let mut island = test_island();
        island.settings.show_media = true;
        island.settings.show_calendar = true;
        island.settings.show_timers = true;
        island.settings.show_messages = false;
        island.messages.incoming = None;
        let visible = island.visible_nook_items();
        assert_eq!(visible, island.settings.nook_items());
    }

    #[test]
    fn queue_fetch_skips_spotify_without_web_api() {
        let mut island = test_island();
        island.now_playing.title = Some("Track".into());
        island.now_playing.app_name = Some("Spotify".into());
        island.now_playing.bundle_id = Some("com.spotify.client".into());
        island.settings.show_media = true;
        island.settings.show_media_queue = true;
        island.expanded = true;
        island.tab = Tab::Widgets;
        assert!(!island.queue_visible());
        assert!(!island.maybe_start_queue_fetch());
        island.queue_open = true;
        assert!(!island.queue_panel_visible());
    }

    #[test]
    fn queue_fetch_only_when_expanded_media_card_is_visible() {
        let mut island = test_island();
        island.now_playing.title = Some("Track".into());
        island.now_playing.app_name = Some("Music".into());
        island.settings.show_media = true;
        island.settings.show_media_queue = true;
        assert!(!island.queue_visible());
        assert!(!island.maybe_start_queue_fetch());

        island.expanded = true;
        island.tab = Tab::Widgets;
        assert!(island.queue_visible());
        assert!(island.maybe_start_queue_fetch());
        assert!(island.queue_inflight);
        assert!(!island.maybe_start_queue_fetch());

        island.queue_inflight = false;
        island.expanded = false;
        assert!(!island.maybe_start_queue_fetch());
    }

    #[test]
    fn queue_fetch_result_caches_rows_but_retries_bare_defaults() {
        let mut island = test_island();
        island.now_playing.title = Some("Track".into());
        island.now_playing.app_name = Some("Music".into());
        island.settings.show_media = true;
        island.settings.show_media_queue = true;
        island.expanded = true;
        island.tab = Tab::Widgets;

        assert!(island.maybe_start_queue_fetch());
        island.apply_queue_fetch_result(PlaybackQueue::default());
        assert!(island.queue_key.is_none());
        assert!(!island.queue_inflight);
        // Transient failure must not block the next pull for the same track.
        assert!(island.maybe_start_queue_fetch());

        let filled = PlaybackQueue {
            source: Some(nook_core::models::QueueSource::MusicPlaylist),
            label: "Up Next in playlist".into(),
            items: vec![nook_core::models::QueueItem {
                id: "music-2".into(),
                title: "Next".into(),
                artist: "A".into(),
                artwork_url: None,
                artwork_base64: None,
                source: nook_core::models::QueueSource::MusicPlaylist,
                jump: nook_core::models::QueueJump::MusicTrack { index: 2 },
            }],
            hidden: None,
            context_uri: None,
        };
        island.apply_queue_fetch_result(filled);
        assert!(island.queue_key.is_some());
        assert_eq!(island.queue.items.len(), 1);
        assert!(!island.maybe_start_queue_fetch());
    }

    #[test]
    fn expanded_bottom_covers_files_and_widgets_tabs() {
        let mut island = test_island();
        island.expanded = true;
        island.settings.show_files = true;
        island.notch_height = 38.0;
        island.screen_width = 1800.0;

        island.tab = Tab::Widgets;
        let (ww, wh) = island.target_size();
        let (_, wtop) = island.settings.island_origin(
            island.screen_width,
            island.screen_height,
            ww.max(1.0),
            wh.max(1.0),
        );
        island.tab = Tab::Files;
        let (fw, fh) = island.target_size();
        let (_, ftop) = island.settings.island_origin(
            island.screen_width,
            island.screen_height,
            fw.max(1.0),
            fh.max(1.0),
        );

        let reserved = island.expanded_bottom();
        assert!(
            reserved + 0.05 >= wtop + wh,
            "expanded_bottom={reserved} widgets bottom={}",
            wtop + wh
        );
        assert!(
            reserved + 0.05 >= ftop + fh,
            "expanded_bottom={reserved} files bottom={}",
            ftop + fh
        );
    }

    #[test]
    fn expanded_island_shows_a_full_file_tile() {
        let mut island = test_island();
        island.expanded = true;
        island.tab = Tab::Files;
        island.notch_height = 38.0;
        island.screen_width = 1800.0;
        let (w, h) = island.target_size();
        let leftover = h - theme::EXPANDED_TAB_H;
        assert!(
            leftover + 0.05 >= files_pane_min_height(w),
            "h={h} leftover={leftover} need={}",
            files_pane_min_height(w)
        );
        assert!(
            leftover + 0.05 >= file_tile_height(0.0),
            "leftover={leftover} tile_h={}",
            file_tile_height(0.0)
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
            kind: TimerKind::Countdown,
            ends_at: None,
        });
        assert!(island.available_modes().contains(&CompactMode::Files));
        assert!(island.available_modes().contains(&CompactMode::Timer));
        island.settings.show_files = false;
        island.settings.show_timers = false;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
    }

    #[test]
    fn face_timer_lets_a_running_clock_timer_take_the_compact_face() {
        let mut island = test_island();
        island.settings.sync_clock_timers = true;
        island
            .system_timers
            .push(nook_core::system_timers::SystemTimer {
                id: "clock-1".into(),
                title: "Pasta".into(),
                duration: 600.0,
                state: nook_core::system_timers::MTTimerState::Running,
                fire_date: Some(nook_core::system_timers::unix_now() + 120.0),
                remaining: None,
                deep_link: "x-apple-clock:timer?id=clock-1".into(),
            });
        assert!(island.available_modes().contains(&CompactMode::Timer));
        let face = island.face_timer().expect("clock timer on the face");
        assert!(matches!(face.source, FaceTimerSource::Clock(_)));
        assert!(face.running);
        assert!(face.remaining <= 120);
        assert_eq!(face.name, "Pasta");
        island.settings.sync_clock_timers = false;
        assert!(island.face_timer().is_none());
        assert!(!island.available_modes().contains(&CompactMode::Timer));
    }

    #[test]
    fn face_timer_prefers_a_finished_local_timer_over_a_running_clock() {
        let mut island = test_island();
        island.timers.push(Timer {
            id: 1,
            name: "Local".into(),
            remaining: 0,
            total: 60,
            running: false,
            kind: TimerKind::Countdown,
            ends_at: None,
        });
        island
            .system_timers
            .push(nook_core::system_timers::SystemTimer {
                id: "clock-1".into(),
                title: "Pasta".into(),
                duration: 600.0,
                state: nook_core::system_timers::MTTimerState::Running,
                fire_date: Some(nook_core::system_timers::unix_now() + 30.0),
                remaining: None,
                deep_link: "x-apple-clock:timer?id=clock-1".into(),
            });
        let face = island.face_timer().expect("finished local wins");
        assert!(matches!(face.source, FaceTimerSource::Local(1)));
        assert_eq!(face.remaining, 0);
    }

    #[test]
    fn output_hud_label_tracks_ttl() {
        let mut island = test_island();
        assert!(island.output_hud_label().is_none());
        island.output_hud_name = Some("AirPods Pro".into());
        island.output_hud_until = Some(Instant::now() + Duration::from_secs(2));
        assert_eq!(island.output_hud_label(), Some("AirPods Pro"));
        island.output_hud_until = Some(Instant::now() - Duration::from_millis(1));
        assert!(island.output_hud_label().is_none());
        island.settings.audio_output_picker = false;
        island.output_picker_open = true;
        assert!(island.sync_output_devices());
        assert!(!island.output_picker_open);
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
        assert!(
            live.0 > idle.0,
            "HUD should widen the idle sliver, {live:?} vs {idle:?}"
        );
        assert_eq!(live.1, 32.0 + theme::COMPACT_HEIGHT_OVERFLOW);
        assert!((island.hud.unwrap().display_value() - 0.6).abs() < f32::EPSILON);

        island.settings.non_notch_mode = true;
        island.hud = None;
        let collapsed = island.target_size();
        island.hud = Some(HudState {
            kind: event.kind,
            value: event.value,
            shown_at: Instant::now(),
            gen: 1,
        });
        let raised = island.target_size();
        assert!(
            raised.1 > collapsed.1,
            "HUD should lift the 1px non-notch sliver"
        );

        island.settings.show_volume_brightness_hud = false;
        assert!(!island.hud_active());
        assert_eq!(island.target_size(), collapsed);
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
    fn compact_hover_reveals_modes_without_returning_to_the_old_size() {
        let mut island = test_island();
        island.notch_width = 185.0;
        island.notch_height = 38.0;
        island.now_playing.title = Some("Track".into());
        island.now_playing.is_playing = true;
        island.settings.show_media = true;
        assert_eq!(island.mode(), CompactMode::Media);

        let compact = island.target_size();
        assert_eq!(
            compact,
            (theme::COMPACT_LIVE_W, 38.0 + theme::COMPACT_HEIGHT_OVERFLOW)
        );

        island.hovered = true;
        let hovered = island.target_size();
        assert_eq!(
            hovered,
            (
                theme::COMPACT_LIVE_W + (theme::COMPACT_HOVER_EXTRA - theme::COMPACT_LIVE_EXTRA),
                38.0 + 11.0
            )
        );
        assert!(hovered.0 > compact.0 && hovered.1 > compact.1);
    }

    #[test]
    fn available_modes_includes_agents() {
        let mut island = test_island();
        island.settings.show_agents = true;
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
    fn available_modes_includes_incoming_messages() {
        let mut island = test_island();
        island.settings.experimental_widgets = true;
        island.settings.show_messages = true;
        island.messages.incoming = Some(nook_core::messages::IncomingPeek {
            conversation_id: "iMessage;-;+1".into(),
            sender: "Ada".into(),
            snippet: "hi".into(),
            service: nook_core::messages::MessageService::IMessage,
            last_date: 1_700_000_000.0,
            last_rowid: 1,
        });
        assert_eq!(
            island.available_modes(),
            vec![CompactMode::Messages, CompactMode::Idle]
        );
        assert_eq!(island.mode(), CompactMode::Messages);
        island.settings.show_messages = false;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
    }

    #[test]
    fn expanded_incoming_message_uses_lockup_width() {
        let mut island = test_island();
        island.expanded = true;
        island.settings.experimental_widgets = true;
        island.settings.show_messages = true;
        island.messages.incoming = Some(nook_core::messages::IncomingPeek {
            conversation_id: "iMessage;-;+1".into(),
            sender: "Ada".into(),
            snippet: "hi".into(),
            service: nook_core::messages::MessageService::IMessage,
            last_date: 1_700_000_000.0,
            last_rowid: 1,
        });
        let (w, h) = island.target_size();
        assert_eq!(w, 420.0);
        assert_eq!(h, 32.0 + theme::NOOK_INSET + theme::NOOK_BODY);
        island.tab = Tab::Files;
        let (full_w, _) = island.target_size();
        assert!(full_w > 420.0);
    }

    #[test]
    fn expanded_recording_uses_memo_lockup_size() {
        let mut island = test_island();
        island.expanded = true;
        island.settings.experimental_widgets = true;
        island.settings.show_recorder = true;
        island.recording = true;
        let (w, h) = island.target_size();
        assert_eq!(w, 420.0);
        assert_eq!(h, 32.0 + theme::NOOK_INSET + 260.0);
    }

    #[test]
    fn compact_recording_grows_for_waveform_and_stop() {
        let mut island = test_island();
        island.settings.experimental_widgets = true;
        island.settings.show_recorder = true;
        island.recording = true;
        let (w, h) = island.target_size();
        assert_eq!(w, 180.0 + crate::widgets::RECORDER_COMPACT_EXTRA);
        assert_eq!(h, 32.0 + theme::COMPACT_HEIGHT_OVERFLOW);
        island.hovered = true;
        let (hw, hh) = island.target_size();
        assert_eq!(hw, 180.0 + crate::widgets::RECORDER_COMPACT_HOVER_EXTRA);
        assert_eq!(hh, 32.0 + 11.0);
        assert!(hw > w && hh > h);
    }

    #[test]
    fn compact_widths_unchanged_when_glass_is_off() {
        let mut island = test_island();
        assert!(
            !crate::platform::island_glass_setting_on(),
            "tests start with Liquid Glass island off"
        );
        assert_eq!(island.glass_notch_gap(), 0.0);
        assert_eq!(island.target_size().0, 180.0 + theme::IDLE_NOTCH_OVERFLOW);

        island.now_playing.title = Some("Track".into());
        island.now_playing.is_playing = true;
        island.settings.show_media = true;
        assert_eq!(island.target_size().0, theme::COMPACT_LIVE_W);
        island.hovered = true;
        assert_eq!(
            island.target_size().0,
            theme::COMPACT_LIVE_W + (theme::COMPACT_HOVER_EXTRA - theme::COMPACT_LIVE_EXTRA)
        );
        island.hovered = false;

        island.hud = Some(HudState {
            kind: HudKind::Volume,
            value: 0.5,
            shown_at: Instant::now(),
            gen: 1,
        });
        assert_eq!(
            island.target_size().0,
            theme::COMPACT_LIVE_W.max(180.0 + theme::COMPACT_HUD_EXTRA)
        );
        island.hud = None;

        island.settings.experimental_widgets = true;
        island.settings.show_recorder = true;
        island.recording = true;
        assert_eq!(
            island.target_size().0,
            180.0 + crate::widgets::RECORDER_COMPACT_EXTRA
        );
    }

    #[test]
    fn recorder_wave_samples_at_interval_and_keeps_newest_first() {
        let mut island = test_island();
        let t0 = island.recorder_wave_at + Duration::from_millis(50);
        assert!(island.sample_recorder_wave(0.4, t0));
        assert!(!island.sample_recorder_wave(0.1, t0 + Duration::from_millis(10)));
        assert!(island.sample_recorder_wave(0.9, t0 + Duration::from_millis(50)));
        assert_eq!(island.recorder_wave.len(), 2);
        assert!((island.recorder_wave[0] - 0.9).abs() < f32::EPSILON);
        assert!((island.recorder_wave[1] - 0.4).abs() < f32::EPSILON);
        island.clear_recorder_wave();
        assert!(island.recorder_wave.is_empty());
        assert_eq!(island.recorder_level, 0.0);
    }
    #[test]
    fn available_modes_share_while_transfer_is_live() {
        let mut island = test_island();
        assert!(!island.available_modes().contains(&CompactMode::Share));
        island.share.phase = nook_core::share::SharePhase::Transferring;
        island.share.status = "Sending".into();
        assert_eq!(island.available_modes()[0], CompactMode::Share);
        assert_eq!(island.mode(), CompactMode::Share);
        island.share.phase = nook_core::share::SharePhase::Idle;
        island.share.hud = Some("Sent".into());
        assert_eq!(island.mode(), CompactMode::Share);
        island.share.hud = None;
    }
    #[test]
    fn available_modes_includes_recording() {
        let mut island = test_island();
        island.settings.experimental_widgets = true;
        island.settings.show_recorder = true;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
        island.recording = true;
        assert_eq!(
            island.available_modes(),
            vec![CompactMode::Recording, CompactMode::Idle]
        );
        assert_eq!(island.mode(), CompactMode::Recording);
        island.settings.show_recorder = false;
    }
    #[test]
    fn available_modes_includes_meeting() {
        use nook_core::meetings::{MeetingApp, MeetingState};
        let mut island = test_island();
        island.settings.experimental_widgets = true;
        island.settings.show_meetings = true;
        island.meeting.state = MeetingState::InMeeting {
            app: MeetingApp::Zoom,
            pid: 7,
            muted: Some(false),
            started: Instant::now(),
        };
        assert_eq!(
            island.available_modes(),
            vec![CompactMode::Meeting, CompactMode::Idle]
        );
        assert_eq!(island.mode(), CompactMode::Meeting);
        island.settings.show_meetings = false;
    }
    #[test]
    fn available_modes_includes_notifications() {
        let mut island = test_island();
        island.settings.experimental_widgets = true;
        island.settings.show_notifications = true;
        island.notification_unread = 2;
        island.notifications.push(NotificationEvent::new(
            "com.hnc.Discord",
            "Discord",
            "Hello",
            "",
            "there",
            1,
        ));
        assert_eq!(
            island.available_modes(),
            vec![CompactMode::Notifications, CompactMode::Idle]
        );
        assert_eq!(island.mode(), CompactMode::Notifications);
        island.settings.show_notifications = false;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
        island.settings.show_notifications = true;
        island.notification_unread = 0;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
    }

    #[test]
    fn available_modes_observe_only_when_user_alert_fires() {
        let mut island = test_island();
        island.settings.experimental_widgets = true;
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
    fn available_modes_battery_only_while_alerting() {
        let mut island = test_island();
        island.settings.show_battery = true;
        island.settings.battery_alert_threshold = 20;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
        island.power = PowerSnapshot {
            percent: Some(12),
            is_charging: false,
            on_ac: false,
            time_to_empty_min: Some(40),
            warning_level: nook_core::power::BatteryWarning::None,
            low_power_mode: false,
            has_battery: true,
        };
        assert_eq!(
            island.available_modes(),
            vec![CompactMode::Battery, CompactMode::Idle]
        );
        assert_eq!(island.mode(), CompactMode::Battery);
        island.power.is_charging = true;
        island.power.on_ac = true;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
        island.power.is_charging = false;
        island.power.on_ac = false;
        island.power.has_battery = false;
        assert_eq!(
            island.available_modes(),
            vec![CompactMode::Idle],
            "desktop Macs hide the compact battery face"
        );
        island.power.has_battery = true;
        island.settings.show_battery = false;
    }
    #[test]
    fn available_modes_includes_vpn_while_connected() {
        let mut island = test_island();
        island.settings.experimental_widgets = true;
        island.settings.show_vpn = true;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
        island.vpn.connected = true;
        island.vpn.service_name = "Tailscale".into();
        island.vpn.interface = "utun4".into();
        assert_eq!(
            island.available_modes(),
            vec![CompactMode::Vpn, CompactMode::Idle]
        );
        assert_eq!(island.mode(), CompactMode::Vpn);
        island.settings.show_vpn = false;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
    }

    #[test]
    fn vpn_reveal_keeps_the_face_after_disconnect() {
        let mut island = test_island();
        island.settings.experimental_widgets = true;
        island.settings.show_vpn = true;
        island.vpn.connected = false;
        island.vpn.service_name = "Tailscale".into();
        island.vpn_reveal_until = Some(Instant::now() + Duration::from_secs(4));
        island.alert_preferred = Some(CompactMode::Vpn);
        assert!(island.has_vpn_face());
        assert_eq!(island.mode(), CompactMode::Vpn);
        island.vpn_reveal_until = Some(Instant::now() - Duration::from_secs(1));
        assert!(island.clear_expired_vpn_reveal());
        assert!(!island.has_vpn_face());
        assert_eq!(island.alert_preferred, None);
        assert_eq!(island.mode(), CompactMode::Idle);
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
        island.user_preferred = Some(CompactMode::Idle);
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
        island.user_preferred = Some(CompactMode::Idle);

        assert!(!island.apply_wheel(8.0, 0.0, TouchPhase::Moved));
        assert!(!island.apply_wheel(8.0, 0.0, TouchPhase::Moved));
        assert_eq!(island.mode(), CompactMode::Idle);
        assert!(island.apply_wheel(8.0, 0.0, TouchPhase::Moved));
        assert_eq!(island.mode(), CompactMode::Files);
        assert!(!island.apply_wheel(8.0, 0.0, TouchPhase::Moved));
        assert_eq!(island.mode(), CompactMode::Files);
    }

    #[test]
    fn expanded_horizontal_swipe_cycles_tab() {
        let mut island = test_island();
        island.expanded = true;
        island.tab = Tab::Widgets;
        assert_eq!(island.shown_tabs(), vec![Tab::Widgets, Tab::Files]);

        assert!(island.apply_wheel(40.0, 0.0, TouchPhase::Moved));
        assert_eq!(island.tab, Tab::Files);
        assert!(!island.apply_wheel(40.0, 0.0, TouchPhase::Moved));
        assert_eq!(island.tab, Tab::Files);

        island.last_wheel_at = Instant::now() - Duration::from_millis(400);
        assert!(island.apply_wheel(-40.0, 0.0, TouchPhase::Moved));
        assert_eq!(island.tab, Tab::Widgets);
    }

    #[test]
    fn compact_swipe_rearms_after_idle() {
        let mut island = test_island();
        with_file(&mut island);
        island.user_preferred = Some(CompactMode::Idle);

        assert!(island.apply_wheel(-40.0, 0.0, TouchPhase::Moved));
        let after_first = island.mode();
        assert_ne!(after_first, CompactMode::Idle);

        island.last_wheel_at = Instant::now() - Duration::from_millis(400);
        assert!(island.apply_wheel(-40.0, 0.0, TouchPhase::Moved));
        assert_ne!(island.mode(), after_first);
    }

    #[test]
    fn widget_edit_cancel_restores_snapshot() {
        let mut island = test_island();
        island.settings.show_timers = true;
        let before = island.settings.clone();
        island.widget_edit_snapshot = Some(before.clone());
        island.widget_edit = true;
        island.settings.show_timers = false;
        // Simulate cancel without GPUI context: restore snapshot fields.
        if let Some(snapshot) = island.widget_edit_snapshot.take() {
            island.settings = snapshot;
        }
        island.widget_edit = false;
        assert!(island.settings.show_timers);
        assert_eq!(island.settings.show_media, before.show_media);
    }

    #[test]
    fn widget_edit_freezes_live_widget_paints() {
        let mut island = test_island();
        assert!(island.paints_live_widgets());
        island.widget_edit = true;
        // Tick-loop frame pumps pause while editing; real widget previews still
        // render, they just stop updating every visualizer tick.
        assert!(!island.paints_live_widgets());
    }

    #[test]
    fn widgets_tab_uses_default_width() {
        let mut island = test_island();
        island.tab = Tab::Widgets;
        island.settings.show_media = true;
        island.settings.show_calendar = false;
        island.settings.show_timers = false;
        island
            .settings
            .set_cells(nook_core::settings::WidgetModule::Music, 5);
        let w = island.expanded_width();
        let expected = (island.screen_width - 40.0).min(theme::EXPANDED_MAX_WIDTH);
        assert!((w - expected).abs() < f32::EPSILON);
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
    fn arm_content_transition_zeros_fade_and_shifts_on_tab_change() {
        let mut island = test_island();
        island.expanded = true;
        island.last_expanded = true;
        island.last_tab = Tab::Widgets;
        island.tab = Tab::Files;
        island.arm_content_transition();
        assert_eq!(island.content_fade.value, 0.0);
        assert_ne!(island.content_x.value, 0.0);
        assert_eq!(island.last_tab, Tab::Files);
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
        island.user_preferred = Some(CompactMode::Idle);
        while island.step_spring(0.016) {}

        island.user_preferred = Some(CompactMode::Files);
        island.step_spring(0.016);

        assert!(island.content_x.value.abs() > 1.0);
        assert_eq!(island.content_y.value, 0.0);
    }

    /// Mode swaps often leave size already parked; lite must still hold while
    /// content_x travels so agent LED glow does not starve the context shift.
    #[test]
    fn mode_swap_holds_lite_until_content_travel_rests() {
        let mut island = test_island();
        with_file(&mut island);
        island.settings.show_agents = true;
        island.agents = vec![AgentSession {
            kind: AgentKind::Grok,
            pid: 7,
            project: "~".into(),
            cwd: "/tmp".into(),
            status: AgentStatus::Working,
            session_id: None,
            name: None,
            model: None,
        }];
        island.user_preferred = Some(CompactMode::Files);
        while island.step_spring(0.016) {}
        assert!(!island.size_morphing());
        assert_eq!(island.content_x.value, 0.0);

        island.user_preferred = Some(CompactMode::Agents);
        island.step_spring(0.016);

        assert!(island.size_morphing(), "lite should arm on mode swap");
        assert!(
            island.content_x.value.abs() > motion::REST_PX,
            "content should still be traveling: {}",
            island.content_x.value
        );
        let (tw, th) = island.target_size();
        assert!((island.anim_w.value - tw).abs() <= motion::REST_PX);
        assert!((island.anim_h.value - th).abs() <= motion::REST_PX);

        let mut moving = true;
        for _ in 0..200 {
            moving = island.step_spring(0.016);
            if !island.size_morphing() {
                break;
            }
        }
        assert!(
            !island.size_morphing(),
            "lite must clear once content rests"
        );
        assert!(island.content_x.value.abs() <= motion::REST_PX);
        assert!(!moving || island.content_fade.value >= 1.0 - motion::REST_ALPHA);
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
    fn lyrics_position_holds_when_paused_and_advances_when_playing() {
        let mut island = test_island();
        island.lyrics_anchor_elapsed = 12.0;
        island.lyrics_anchor_at = Instant::now() - Duration::from_millis(80);
        island.now_playing.is_playing = false;
        assert!((island.lyrics_position() - 12.0).abs() < 0.01);
        island.now_playing.is_playing = true;
        island.now_playing.duration = Some(100.0);
        let pos = island.lyrics_position();
        assert!(pos >= 12.05, "pos={pos}");
        assert!(pos < 13.0, "pos={pos}");
        assert!(!island.lyrics_timer_should_run());
        island.settings.show_lyrics = true;
        island.expanded = true;
        island.lyrics = Some(Arc::new(SyncedLyrics {
            lines: nook_core::lyrics::parse_lrc("[00:00.00] A\n[00:20.00] B\n"),
            ..SyncedLyrics::default()
        }));
        assert!(island.lyrics_timer_should_run());
        island.expanded = false;
        assert!(!island.lyrics_timer_should_run());
    }

    #[test]
    fn seek_intent_holds_stale_polls_until_caught_up() {
        let since = Instant::now();
        assert!(hold_seek_intent(90.0, since, Some(12.0), false));
        assert!(hold_seek_intent(90.0, since, None, false));
        assert!(!hold_seek_intent(90.0, since, Some(90.4), false));
        assert!(!hold_seek_intent(90.0, since, Some(12.0), true));
        let expired = Instant::now() - MEDIA_INTENT_WINDOW - Duration::from_millis(1);
        assert!(!hold_seek_intent(90.0, expired, Some(12.0), false));
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

    fn pomo_timer(remaining: u32, spec: PomodoroSpec, ends_at: SystemTime) -> Timer {
        Timer {
            id: 1,
            name: spec.label().to_string(),
            remaining,
            total: spec.duration_secs(),
            running: true,
            kind: TimerKind::Pomodoro(spec),
            ends_at: Some(ends_at),
        }
    }

    #[test]
    fn pomodoro_tick_advances_work_to_short_break() {
        let mut island = test_island();
        island.settings.pomodoro_keep_awake = false;
        let spec = PomodoroSpec::new(25, 5, 15, 4, true);
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        island.timers.push(pomo_timer(1, spec, now));
        assert!(island.apply_timer_tick(now, 1));
        let t = &island.timers[0];
        assert_eq!(
            t.kind,
            TimerKind::Pomodoro(PomodoroSpec {
                phase: PomodoroPhase::ShortBreak,
                cycle: 1,
                ..spec
            })
        );
        assert_eq!(t.remaining, 5);
        assert_eq!(t.total, 5);
        assert!(t.running);
        assert_eq!(t.name, "Break");
    }

    #[test]
    fn pomodoro_tick_fourth_work_goes_to_long_break() {
        let mut island = test_island();
        island.settings.pomodoro_keep_awake = false;
        let spec = PomodoroSpec {
            phase: PomodoroPhase::Work,
            cycle: 4,
            work_secs: 25,
            break_secs: 5,
            long_break_secs: 15,
            cycles_per_long: 4,
            auto_advance: true,
        };
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000);
        island.timers.push(pomo_timer(0, spec, now));
        island.apply_timer_tick(now, 1);
        match island.timers[0].kind {
            TimerKind::Pomodoro(next) => {
                assert_eq!(next.phase, PomodoroPhase::LongBreak);
                assert_eq!(next.cycle, 4);
                assert_eq!(island.timers[0].remaining, 15);
            }
            TimerKind::Countdown => panic!("expected pomodoro"),
        }
    }

    #[test]
    fn pomodoro_without_auto_advance_stops() {
        let mut island = test_island();
        island.settings.pomodoro_keep_awake = false;
        let spec = PomodoroSpec::new(25, 5, 15, 4, false);
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(3_000);
        island.timers.push(pomo_timer(0, spec, now));
        island.apply_timer_tick(now, 1);
        assert!(!island.timers[0].running);
        assert_eq!(island.timers[0].remaining, 0);
        assert!(matches!(
            island.timers[0].kind,
            TimerKind::Pomodoro(s) if s.phase == PomodoroPhase::Work
        ));
    }

    #[test]
    fn countdown_tick_still_decrements() {
        let mut island = test_island();
        island.timers.push(Timer {
            id: 1,
            name: String::new(),
            remaining: 10,
            total: 10,
            running: true,
            kind: TimerKind::Countdown,
            ends_at: None,
        });
        assert!(island.apply_timer_tick(SystemTime::UNIX_EPOCH, 3));
        assert_eq!(island.timers[0].remaining, 7);
        assert!(island.timers[0].running);
        island.apply_timer_tick(SystemTime::UNIX_EPOCH, 7);
        assert_eq!(island.timers[0].remaining, 0);
        assert!(!island.timers[0].running);
    }
    #[test]
    fn terminal_tab_is_opt_in() {
        let mut island = test_island();
        assert_eq!(island.shown_tabs(), vec![Tab::Widgets, Tab::Files]);
        island.settings.terminal_enabled = true;
        assert_eq!(
            island.shown_tabs(),
            vec![Tab::Widgets, Tab::Files, Tab::Terminal]
        );
        island.settings.show_files = false;
        assert_eq!(island.shown_tabs(), vec![Tab::Widgets, Tab::Terminal]);
    }

    #[test]
    fn expanded_terminal_fits_the_pty_grid() {
        let mut island = test_island();
        island.expanded = true;
        island.settings.terminal_enabled = true;
        island.tab = Tab::Terminal;
        island.notch_height = 38.0;
        let (_, h) = island.target_size();
        let leftover = h - island.notch_height.max(32.0) - theme::EXPANDED_PAD;
        assert!(
            leftover + 0.05 >= crate::widgets::terminal_pane_min_height(),
            "h={h} leftover={leftover} need={}",
            crate::widgets::terminal_pane_min_height()
        );
        island.tab = Tab::Widgets;
        let (_, widget_h) = island.target_size();
        assert!(
            h > widget_h,
            "term tab must be taller than the widget row (term={h} widgets={widget_h})"
        );
    }

    #[test]
    fn shell_never_appears_in_compact_modes() {
        let mut island = test_island();
        island.settings.terminal_enabled = true;
        island.shell_running = true;
        assert_eq!(island.available_modes(), vec![CompactMode::Idle]);
    }
    #[test]
    fn motion_art_layer_stays_off_when_collapsed_or_paused() {
        let mut island = test_island();
        island.settings.animated_album_art = true;
        island.now_playing.artist = Some("Taylor Swift".into());
        island.now_playing.album = Some("Folklore".into());
        island.now_playing.is_playing = true;
        island.now_playing.motion_artwork_url = Some("https://example.com/a.m3u8".into());
        island.motion_art_bounds = Some((40.0, 20.0, 84.0, 84.0));
        assert!(
            island.motion_art_spec().is_none(),
            "collapsed hides the layer"
        );

        island.expanded = true;
        let spec = island.motion_art_spec().expect("expanded playing");
        assert_eq!(spec.url, "https://example.com/a.m3u8");
        assert!(spec.playing);
        assert_eq!(spec.radius, media::NOOK_ART_RADIUS as f64);

        island.now_playing.is_playing = false;
        assert!(
            !island
                .motion_art_spec()
                .expect("paused still has a spec")
                .playing,
            "pause hides via playing=false"
        );

        island.now_playing.is_playing = true;
        island.reduce_motion = true;
        assert!(island.motion_art_spec().is_none());
        island.reduce_motion = false;
        island.suppressed = true;
        assert!(island.motion_art_spec().is_none());
    }

    #[test]
    fn aura_only_animates_while_expanded_and_playing() {
        let mut island = test_island();
        island.settings.ambient_art_glow = true;
        island.aura_palette = Some([
            gpui::Rgba {
                r: 0.2,
                g: 0.3,
                b: 0.8,
                a: 1.0,
            },
            gpui::Rgba {
                r: 0.8,
                g: 0.2,
                b: 0.2,
                a: 1.0,
            },
            gpui::Rgba {
                r: 0.2,
                g: 0.7,
                b: 0.3,
                a: 1.0,
            },
        ]);
        island.now_playing.is_playing = true;
        assert!(!island.aura_should_animate());
        island.expanded = true;
        assert!(island.aura_should_animate());
        island.now_playing.is_playing = false;
        assert!(!island.aura_should_animate());
        island.now_playing.is_playing = true;
        island.reduce_motion = true;
        assert!(!island.aura_should_animate());
        island.reduce_motion = false;
        island.settings.ambient_art_glow = false;
        assert!(!island.aura_should_animate());
    }

    fn working_agent() -> AgentSession {
        AgentSession {
            kind: AgentKind::Grok,
            pid: 42,
            project: "~".into(),
            cwd: "/tmp".into(),
            status: AgentStatus::Working,
            session_id: None,
            name: Some("border".into()),
            model: None,
        }
    }

    #[test]
    fn agent_border_springs_in_when_work_starts() {
        let mut island = test_island();
        island.agents = vec![working_agent()];
        assert_eq!(island.agent_border.value, 0.0);
        assert!(island.agent_border_color.is_none());
        assert!(island.step_spring(1.0 / 60.0));
        assert!(
            island.agent_border.value > 0.0 && island.agent_border.value < 1.0,
            "border popped instead of revealing, value={}",
            island.agent_border.value
        );
        assert!(island.agent_border_color.is_some());
        island.reduce_motion = true;
        island.step_spring(1.0 / 60.0);
        assert_eq!(island.agent_border.value, 1.0);
    }

    #[test]
    fn agent_border_springs_out_when_work_stops() {
        let mut island = test_island();
        island.agents = vec![working_agent()];
        island.reduce_motion = true;
        island.step_spring(1.0 / 60.0);
        assert_eq!(island.agent_border.value, 1.0);
        island.reduce_motion = false;
        island.agents[0].status = AgentStatus::Waiting;
        assert!(island.step_spring(1.0 / 60.0));
        assert!(
            island.agent_border.value > 0.0 && island.agent_border.value < 1.0,
            "border dropped instead of fading, value={}",
            island.agent_border.value
        );
        assert!(
            island.agent_border_color.is_some(),
            "fade-out must keep the brand tint"
        );
    }
}
