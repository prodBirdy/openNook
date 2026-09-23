//! Settings window — macOS split view: sidebar + grouped inset lists.

use super::ui::label;
use crate::icons::lucide_color;
use crate::theme;
use crate::CloseWindow;
use gpui::{
    canvas, div, linear_color_stop, linear_gradient, prelude::*, px, relative, AnyElement, Bounds,
    Context, CursorStyle, ElementId, FocusHandle, FontWeight, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Rgba, ScrollHandle, ScrollWheelEvent,
    SharedString, Window,
};
use nook_core::high_alert::HighAlertKind;
use nook_core::settings::{AppSettings, IslandSwatch, WidgetModule, ISLAND_SWATCHES};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// Default settings window. Sidebar + grouped pane.
pub(super) const SETTINGS_SIZE: (f32, f32) = (820.0, 560.0);
pub(super) const SETTINGS_MIN: (f32, f32) = (680.0, 480.0);

const SIDEBAR_W: f32 = 212.0;
const SIDEBAR_PAD: f32 = 10.0;
const SEARCH_H: f32 = 24.0;
const SEARCH_RADIUS: f32 = 6.0;
const CATEGORY_H: f32 = 30.0;
const CATEGORY_BADGE: f32 = 17.0;
const CATEGORY_BADGE_RADIUS: f32 = 4.5;
const CATEGORY_ICON: f32 = 11.0;
const CATEGORY_GAP: f32 = 9.0;
const WIDGET_ROW_H: f32 = 46.0;
const WIDGET_BADGE: f32 = 22.0;
const WIDGET_BADGE_RADIUS: f32 = 6.16;
const WIDGET_BADGE_ICON: f32 = 13.0;
const WIDGET_GRIP: f32 = 13.0;
const REMOVE_H: f32 = 22.0;
const REMOVE_RADIUS: f32 = 6.0;
const PANE_TITLE: f32 = 22.0;
const PANE_TITLE_LEADING: f32 = 26.0;
/// Room for traffic lights on a transparent titlebar.
const TITLEBAR_INSET: f32 = 52.0;
const GROUP_PAD: f32 = 12.0;
const ROW_H: f32 = 36.0;
const GRADIENT_TRACK_W: f32 = 140.0;
const GRADIENT_KNOB: f32 = 18.0;
/// Separator inset under a widget row: past grip + badge (In the Island) or badge only.
const ISLAND_LIST_INDENT: f32 = 67.0;
const MORE_LIST_INDENT: f32 = 44.0;
/// Icon-tile colours are fixed like System Settings, not the user accent.
const TILE_BLUE: u32 = 0x0A84FF;
const TILE_ORANGE: u32 = 0xFF9F0A;
/// `#FFFFFF0F` rim on grouped lists.
const LIST_BORDER: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.06,
};

/// Last surface the user had open. Survives closing the window.
static LAST_CATEGORY: AtomicU8 = AtomicU8::new(SettingsCategory::Widgets as u8);
static LAST_MODULE: AtomicU8 = AtomicU8::new(WidgetModule::Calendar as u8);

thread_local! {
    static PANE_SCROLLS: RefCell<HashMap<ElementId, ScrollHandle>> = RefCell::new(HashMap::new());
}

fn pane_scroll(id: &ElementId) -> ScrollHandle {
    PANE_SCROLLS.with_borrow_mut(|handles| handles.entry(id.clone()).or_default().clone())
}

fn token_text(token: &str, revealed: bool) -> String {
    if revealed {
        token.to_string()
    } else {
        "••••••••".into()
    }
}

fn hairline() -> Rgba {
    theme::SEPARATOR
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum SettingsCategory {
    General = 0,
    Widgets = 1,
    Appearance = 2,
    Shortcuts = 3,
    Privacy = 4,
    Updates = 5,
    About = 6,
}

impl SettingsCategory {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Widgets,
            2 => Self::Appearance,
            3 => Self::Shortcuts,
            4 => Self::Privacy,
            5 => Self::Updates,
            6 => Self::About,
            _ => Self::General,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Widgets => "Widgets",
            Self::Shortcuts => "Shortcuts",
            Self::Privacy => "Privacy",
            Self::Updates => "Updates",
            Self::About => "About",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::General => "settings",
            Self::Appearance => "sun-moon",
            Self::Widgets => "layout-grid",
            Self::Shortcuts => "keyboard",
            Self::Privacy => "lock",
            Self::Updates => "arrow-down-to-line",
            Self::About => "info",
        }
    }

    fn badge(self) -> Rgba {
        match self {
            Self::General | Self::About => theme::rgba_from_u32(0x8E8E93, 1.0),
            Self::Appearance => theme::rgba_from_u32(0x5E5CE6, 1.0),
            Self::Widgets => theme::rgba_from_u32(TILE_BLUE, 1.0),
            Self::Shortcuts => theme::rgba_from_u32(TILE_ORANGE, 1.0),
            Self::Privacy => theme::SUCCESS,
            Self::Updates => theme::rgba_from_u32(0x64D2FF, 1.0),
        }
    }

    fn matches_query(self, query: &str) -> bool {
        let q = query.trim().to_ascii_lowercase();
        q.is_empty() || self.title().to_ascii_lowercase().contains(&q)
    }
}

trait WidgetModuleExt {
    fn name(self) -> &'static str;
    fn icon(self) -> &'static str;
    /// Short static description shown under the name in the widget lists.
    fn tagline(self) -> &'static str;
    #[allow(dead_code)]
    fn subtitle(self, settings: &AppSettings) -> SharedString;
    fn enabled(self, settings: &AppSettings) -> bool;
    fn set_enabled(self, settings: &mut AppSettings);
}

#[derive(Clone, Copy)]
struct WidgetDrag(WidgetModule);

impl gpui::Render for WidgetDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(280.))
            .h(px(WIDGET_ROW_H))
            .px(px(12.))
            .rounded(px(8.))
            .bg(theme::GROUPED_BG)
            .shadow_md()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(lucide_color(
                "grip-vertical",
                WIDGET_GRIP,
                theme::TERTIARY_LABEL,
            ))
            .child(widget_badge(self.0))
            .child(
                div()
                    .text_size(px(theme::BODY.size))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(theme::LABEL)
                    .child(self.0.name()),
            )
    }
}

impl WidgetModuleExt for WidgetModule {
    fn name(self) -> &'static str {
        match self {
            Self::Calendar => "Calendar",
            Self::Music => "Music",
            Self::Files => "Files",
            Self::Notes => "Notes",
            Self::Observe => "Observe",
            Self::Timers => "Timers",
            Self::Reminders => "Reminders",
            Self::Speed => "Speed",
            Self::Agents => "Agents",
            Self::Mirror => "Mirror",
            Self::Battery => "Battery",
            Self::Messages => "Messages",
            Self::Obsidian => "Obsidian",
            Self::Weather => "Weather",
            Self::Vpn => "VPN",
            Self::HighAlert => "Alert",
            Self::SysStats => "Stats",
            Self::Recorder => "Voice",
            Self::Meeting => "Meetings",
            Self::Notifications => "Notify",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Calendar => "calendar",
            Self::Music => "music",
            Self::Files => "folder",
            Self::Notes => "notebook-pen",
            Self::Observe => "eye",
            Self::Timers => "timer",
            Self::Reminders => "bell",
            Self::Speed => "gauge",
            Self::Agents => "bot",
            Self::Mirror => "webcam",
            Self::Battery => "battery-medium",
            Self::Messages => "message-square",
            Self::Obsidian => "gem",
            Self::Weather => "cloud-sun",
            Self::Vpn => "shield",
            Self::HighAlert => "triangle-alert",
            Self::SysStats => "chart-column",
            Self::Recorder => "mic",
            Self::Meeting => "video",
            Self::Notifications => "bell-ring",
        }
    }

    fn tagline(self) -> &'static str {
        match self {
            Self::Calendar => "Next event",
            Self::Music => "Now playing",
            Self::Files => "Recent downloads",
            Self::Notes => "Quick capture",
            Self::Observe => "Screen watcher",
            Self::Timers => "Active timers",
            Self::Reminders => "Due today",
            Self::Speed => "Network speed",
            Self::Agents => "Running tasks",
            Self::Mirror => "Camera preview",
            Self::Battery => "Charge level",
            Self::Messages => "Unread threads",
            Self::Obsidian => "Daily note",
            Self::Weather => "Current conditions",
            Self::Vpn => "Tunnel status",
            Self::HighAlert => "System alerts",
            Self::SysStats => "CPU and memory",
            Self::Recorder => "Dictation",
            Self::Meeting => "Next call",
            Self::Notifications => "Notification relay",
        }
    }

    fn subtitle(self, settings: &AppSettings) -> SharedString {
        match self {
            Self::Calendar => "7 days".into(),
            Self::Music => {
                if settings.show_media_queue {
                    "Now Playing + queue".into()
                } else {
                    "Now Playing".into()
                }
            }
            Self::Files => "Tray tab".into(),
            Self::Notes => "Scratchpad".into(),
            Self::Observe => observe_subtitle(settings.observe.metrics.len()),
            Self::Timers => {
                if settings.sync_clock_timers {
                    "Island + Clock".into()
                } else {
                    "Countdown".into()
                }
            }
            Self::Reminders => "EventKit".into(),
            Self::Speed => "Cloudflare".into(),
            Self::Agents => "Sessions".into(),
            Self::Mirror => "Camera".into(),
            Self::Battery => format!(
                "Alert Below {}%",
                nook_core::power::clamp_alert_threshold(settings.battery_alert_threshold)
            )
            .into(),
            Self::Messages => "Incoming reply".into(),
            Self::Obsidian => settings
                .obsidian_vault
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| SharedString::from(name.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "No vault".into()),
            Self::Weather => weather_subtitle(settings),
            Self::Vpn => vpn_subtitle(settings.vpn_show_timer),
            Self::HighAlert => "Keep Awake".into(),
            Self::SysStats => sysstats_subtitle(settings),
            Self::Recorder => {
                if settings.recorder_transcribe {
                    "Live transcript".into()
                } else {
                    "Record only".into()
                }
            }
            Self::Meeting => "Zoom / Teams / Meet".into(),
            Self::Notifications => notify_subtitle(settings),
        }
    }

    fn enabled(self, settings: &AppSettings) -> bool {
        settings.is_enabled(self)
    }

    fn set_enabled(self, settings: &mut AppSettings) {
        let on = !settings.is_enabled(self);
        let _ = settings.set_enabled(self, on);
    }
}

#[allow(dead_code)]
fn weather_subtitle(settings: &AppSettings) -> SharedString {
    let name = settings.weather.location.name();
    if name.is_empty() {
        "Open-Meteo".into()
    } else {
        name.to_string().into()
    }
}

#[allow(dead_code)]
fn vpn_subtitle(show_timer: bool) -> SharedString {
    if show_timer {
        "Session timer".into()
    } else {
        "Status".into()
    }
}

#[allow(dead_code)]
fn sysstats_subtitle(settings: &AppSettings) -> SharedString {
    let n = [
        settings.sysstats.show_cpu,
        settings.sysstats.show_mem,
        settings.sysstats.show_net,
        settings.sysstats.show_disk,
    ]
    .into_iter()
    .filter(|on| *on)
    .count();
    match n {
        0 => "Hidden".into(),
        1 => "1 readout".into(),
        n => format!("{n} readouts").into(),
    }
}

fn notification_permission_rows(cx: &mut Context<SettingsView>) -> Vec<AnyElement> {
    use nook_core::eventtap::PermissionStatus;
    use nook_core::notifications::PermissionState;
    let ax = nook_core::eventtap::accessibility_status();
    let fda = match crate::platform::full_disk_access() {
        PermissionState::Granted => PermissionStatus::Granted,
        PermissionState::Denied => PermissionStatus::Denied,
        PermissionState::Unavailable => PermissionStatus::Unsupported,
    };
    vec![
        permission_row("Accessibility", ax).into_any_element(),
        action_row(
            "notify-ax",
            "Accessibility",
            "Open Privacy Settings",
            cx,
            |_, _, _| {
                crate::platform::open_privacy_accessibility();
            },
        )
        .into_any_element(),
        permission_row("Full Disk Access", fda).into_any_element(),
        action_row(
            "notify-fda",
            "Full Disk Access",
            "Open Privacy Settings",
            cx,
            |_, _, _| {
                crate::platform::open_privacy_full_disk_access();
            },
        )
        .into_any_element(),
    ]
}

#[allow(dead_code)]
fn notify_subtitle(settings: &AppSettings) -> SharedString {
    if !settings.show_notifications {
        return "Off".into();
    }
    if crate::platform::ax_process_trusted(false) {
        "Accessibility".into()
    } else {
        "Needs Accessibility".into()
    }
}

#[allow(dead_code)]
fn observe_subtitle(pinned: usize) -> SharedString {
    match pinned {
        0 => "Prometheus".into(),
        1 => "1 metric".into(),
        n => format!("{n} metrics").into(),
    }
}

pub(super) struct SettingsView {
    category: SettingsCategory,
    module: WidgetModule,
    url_focus: FocusHandle,
    token_focus: FocusHandle,
    query_focus: FocusHandle,
    heading_focus: FocusHandle,
    alias_focus: FocusHandle,
    pin_focus: FocusHandle,
    city_focus: FocusHandle,
    url_draft: String,
    token_draft: String,
    alias_draft: String,
    pin_draft: String,
    ignore_focus: FocusHandle,
    shell_focus: FocusHandle,
    font_draft: String,
    font_focus: FocusHandle,
    font_size_draft: String,
    font_size_focus: FocusHandle,
    ignore_draft: String,
    token_revealed: bool,
    query_draft: String,
    heading_draft: String,
    shell_draft: String,
    catalog: Vec<String>,
    catalog_error: Option<String>,
    catalog_loading: bool,
    placement_drag: bool,
    placement_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    gradient_drag: bool,
    gradient_track: Rc<RefCell<Option<(f32, f32)>>>,
    shortcut_catalog: Vec<String>,
    shortcuts_loading: bool,
    pending_destructive: Option<SharedString>,
    login_error: Option<String>,
    /// 2 s "No room for that size" caption under the Size control.
    size_budget_hint_at: Option<Instant>,
    search_draft: String,
    search_focus: FocusHandle,
}

impl SettingsView {
    fn destructive_caption(&self, id: &str, caption: &'static str) -> &'static str {
        if self.pending_destructive.as_ref().map(SharedString::as_str) == Some(id) {
            "Confirm"
        } else {
            caption
        }
    }

    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let settings = nook_core::settings::get_app_settings();
        let mut module = WidgetModule::from_u8(LAST_MODULE.load(Ordering::Relaxed));
        if !module.is_available() {
            module = WidgetModule::Calendar;
        }
        Self {
            category: SettingsCategory::from_u8(LAST_CATEGORY.load(Ordering::Relaxed)),
            module,
            url_focus: cx.focus_handle(),
            token_focus: cx.focus_handle(),
            query_focus: cx.focus_handle(),
            heading_focus: cx.focus_handle(),
            alias_focus: cx.focus_handle(),
            pin_focus: cx.focus_handle(),
            alias_draft: settings.share.device_alias.clone(),
            pin_draft: settings.share.localsend_pin.clone(),
            city_focus: cx.focus_handle(),
            ignore_focus: cx.focus_handle(),
            shell_focus: cx.focus_handle(),
            font_draft: settings.terminal_font.clone(),
            font_focus: cx.focus_handle(),
            font_size_draft: format!("{}", settings.terminal_font_size),
            font_size_focus: cx.focus_handle(),
            url_draft: settings.observe.prometheus_url,
            token_draft: settings.observe.metrics_token,
            ignore_draft: nook_core::vpn::format_ignore_list(&settings.vpn_ignore_interfaces),
            token_revealed: false,
            query_draft: String::new(),
            heading_draft: settings
                .obsidian_capture_heading
                .clone()
                .unwrap_or_default(),
            shell_draft: settings.terminal_shell.clone(),
            catalog: Vec::new(),
            catalog_error: None,
            catalog_loading: false,
            placement_drag: false,
            placement_bounds: Rc::new(RefCell::new(None)),
            gradient_drag: false,
            gradient_track: Rc::new(RefCell::new(None)),
            shortcut_catalog: Vec::new(),
            shortcuts_loading: false,
            pending_destructive: None,
            login_error: None,
            size_budget_hint_at: None,
            search_draft: String::new(),
            search_focus: cx.focus_handle(),
        }
    }

    fn apply_gradient(&mut self, x: f32, cx: &mut Context<Self>) {
        let Some((origin, width)) = *self.gradient_track.borrow() else {
            return;
        };
        let value = gradient_ratio(x, origin, width);
        let current = nook_core::settings::get_app_settings().glass_gradient();
        if (current - value).abs() < 0.001 {
            return;
        }
        nook_core::settings::tweak_app_settings(|s| s.liquid_glass_gradient = value);
        cx.notify();
    }

    fn apply_placement(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let Some(bounds) = *self.placement_bounds.borrow() else {
            return;
        };
        let origin_x: f32 = bounds.origin.x.into();
        let origin_y: f32 = bounds.origin.y.into();
        let width: f32 = bounds.size.width.into();
        let height: f32 = bounds.size.height.into();
        if width < 8.0 || height < 8.0 {
            return;
        }
        const PILL_W: f32 = 52.0;
        const PILL_H: f32 = 14.0;
        let left = (x - origin_x - PILL_W * 0.5).clamp(0.0, (width - PILL_W).max(0.0));
        let top = (y - origin_y - PILL_H * 0.5).clamp(0.0, (height - PILL_H).max(0.0));
        nook_core::settings::tweak_app_settings(|s| {
            s.set_island_origin(left, top, width, height, PILL_W);
        });
        cx.notify();
    }

    fn persist_nav(&self) {
        LAST_CATEGORY.store(self.category as u8, Ordering::Relaxed);
        LAST_MODULE.store(self.module as u8, Ordering::Relaxed);
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
        nook_core::observe::set_metrics_url(&mut s.observe, draft);
        nook_core::settings::update_app_settings(s);
    }

    fn persist_token(&self) {
        let draft = self.token_draft.trim().to_string();
        let mut s = nook_core::settings::get_app_settings();
        if s.observe.metrics_token == draft {
            return;
        }
        s.observe.metrics_token = draft;
        nook_core::settings::update_app_settings(s);
    }

    fn persist_observe(tweak: impl FnOnce(&mut AppSettings)) {
        nook_core::settings::tweak_app_settings(tweak);
    }

    fn persist_ignore(&self) {
        let names = nook_core::vpn::parse_ignore_list(&self.ignore_draft);
        nook_core::settings::tweak_app_settings(|s| {
            if s.vpn_ignore_interfaces != names {
                s.vpn_ignore_interfaces = names;
            }
        });
    }

    fn apply_key(draft: &mut String, event: &KeyDownEvent, cx: &Context<Self>) -> bool {
        let ks = &event.keystroke;
        if ks.modifiers.secondary() && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                draft.push_str(text.trim());
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

    fn fetch_shortcuts(&mut self, cx: &mut Context<Self>) {
        if self.shortcuts_loading {
            return;
        }
        self.shortcuts_loading = true;
        cx.spawn(async move |this, cx| {
            let names = cx
                .background_executor()
                .spawn(async { nook_core::runtime().block_on(nook_core::focus::list_shortcuts()) })
                .await;
            this.update(cx, |this, cx| {
                this.shortcuts_loading = false;
                this.shortcut_catalog = names;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl gpui::Render for SettingsView {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = nook_core::settings::get_app_settings();
        let url_focused = self.url_focus.is_focused(window);
        let token_focused = self.token_focus.is_focused(window);
        let query_focused = self.query_focus.is_focused(window);
        let heading_focused = self.heading_focus.is_focused(window);
        let alias_focused = self.alias_focus.is_focused(window);
        let pin_focused = self.pin_focus.is_focused(window);
        let city_focused = self.city_focus.is_focused(window);
        let ignore_focused = self.ignore_focus.is_focused(window);

        div()
            .id("settings-root")
            .tab_group()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab" {
                    this.pending_destructive = None;
                    if event.keystroke.modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .size_full()
            .flex()
            .bg(theme::WINDOW_BG)
            .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.pending_destructive = None;
                    cx.notify();
                }),
            )
            .text_color(theme::LABEL)
            .when(self.placement_drag || self.gradient_drag, |d| {
                d.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    let x = event.position.x.into();
                    let y = event.position.y.into();
                    if this.gradient_drag {
                        this.apply_gradient(x, cx);
                    } else {
                        this.apply_placement(x, y, cx);
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, _, cx| {
                        this.placement_drag = false;
                        this.gradient_drag = false;
                        cx.notify();
                    }),
                )
            })
            .child(self.sidebar(window, cx))
            .child(div().w(px(1.)).h_full().bg(theme::HAIRLINE))
            .child(match self.category {
                SettingsCategory::Widgets => self
                    .render_widgets(
                        &settings,
                        url_focused,
                        token_focused,
                        query_focused,
                        heading_focused,
                        city_focused,
                        ignore_focused,
                        cx,
                    )
                    .into_any_element(),
                SettingsCategory::Appearance => {
                    self.render_appearance(&settings, cx).into_any_element()
                }
                SettingsCategory::Shortcuts => {
                    self.render_shortcuts(&settings, cx).into_any_element()
                }
                SettingsCategory::Privacy => self.render_privacy(cx).into_any_element(),
                SettingsCategory::Updates => self.render_updates(cx).into_any_element(),
                SettingsCategory::About => self.render_about(cx).into_any_element(),
                SettingsCategory::General => self
                    .render_general(&settings, window, alias_focused, pin_focused, cx)
                    .into_any_element(),
            })
    }
}

impl SettingsView {
    fn sidebar(&self, window: &gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let search_focused = self.search_focus.is_focused(window);
        let query = self.search_draft.clone();
        let show = |cat: SettingsCategory| cat.matches_query(&query);
        div()
            .id("settings-sidebar")
            .w(px(SIDEBAR_W))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(theme::SETTINGS_WELL)
            .pt(px(TITLEBAR_INSET))
            .px(px(SIDEBAR_PAD))
            .pb(px(16.))
            .gap(px(1.))
            .child(self.sidebar_search(search_focused, cx))
            .child(div().h(px(10.)).w_full())
            .when(show(SettingsCategory::General), |d| {
                d.child(self.sidebar_item(SettingsCategory::General, cx))
            })
            .when(show(SettingsCategory::Appearance), |d| {
                d.child(self.sidebar_item(SettingsCategory::Appearance, cx))
            })
            .when(show(SettingsCategory::Widgets), |d| {
                d.child(self.sidebar_item(SettingsCategory::Widgets, cx))
            })
            .when(show(SettingsCategory::Shortcuts), |d| {
                d.child(self.sidebar_item(SettingsCategory::Shortcuts, cx))
            })
            .child(div().h(px(16.)).w_full())
            .child(
                div().px(px(7.)).pb(px(5.)).child(
                    div()
                        .text_size(px(theme::SUBHEADLINE.size))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::secondary_label())
                        .child("Advanced"),
                ),
            )
            .when(show(SettingsCategory::Privacy), |d| {
                d.child(self.sidebar_item(SettingsCategory::Privacy, cx))
            })
            .when(show(SettingsCategory::Updates), |d| {
                d.child(self.sidebar_item(SettingsCategory::Updates, cx))
            })
            .when(show(SettingsCategory::About), |d| {
                d.child(self.sidebar_item(SettingsCategory::About, cx))
            })
            .child(div().flex_1())
            .child(
                div().px(px(7.)).pb(px(4.)).child(
                    div()
                        .text_size(px(theme::SUBHEADLINE.size))
                        .text_color(theme::secondary_label())
                        .child(format!("openNook {}", env!("CARGO_PKG_VERSION"))),
                ),
            )
    }

    fn sidebar_search(&self, focused: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.search_focus.clone();
        let placeholder = self.search_draft.is_empty();
        let value = if placeholder {
            "Search"
        } else {
            self.search_draft.as_str()
        };
        div()
            .id("settings-search")
            .track_focus(&focus)
            .w_full()
            .h(px(SEARCH_H))
            .px(px(7.))
            .rounded(px(SEARCH_RADIUS))
            .bg(theme::FILL_TERTIARY)
            .border_1()
            .border_color(if focused {
                theme::accent()
            } else {
                theme::FILL_TERTIARY
            })
            .flex()
            .items_center()
            .gap(px(5.))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.pending_destructive = None;
                    cx.stop_propagation();
                    window.focus(&this.search_focus);
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if Self::apply_key(&mut this.search_draft, event, cx) {
                    cx.notify();
                }
            }))
            .child(lucide_color("search", 12.0, theme::tertiary_label()))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(theme::CALLOUT.size))
                    .text_color(if placeholder {
                        theme::secondary_label()
                    } else {
                        theme::LABEL
                    })
                    .child(SharedString::from(value.to_string())),
            )
            .when(focused, |d| {
                d.child(
                    div()
                        .w(px(1.))
                        .h(px(12.))
                        .flex_shrink_0()
                        .bg(theme::accent()),
                )
            })
    }

    fn sidebar_item(&self, category: SettingsCategory, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.category == category;
        div()
            .id(SharedString::from(format!("sidebar-{}", category.title())))
            .h(px(CATEGORY_H))
            .px(px(7.))
            .rounded(px(SEARCH_RADIUS))
            .flex()
            .items_center()
            .gap(px(CATEGORY_GAP))
            .when(selected, |d| d.bg(theme::FILL))
            .hover(|s| {
                if selected {
                    s
                } else {
                    s.bg(theme::FILL_TERTIARY)
                }
            })
            .tab_index(0)
            .focus(|s| s.border_1().border_color(theme::accent()))
            .active(|s| s.opacity(0.85))
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.pending_destructive = None;
                this.category = category;
                this.persist_nav();
                if category == SettingsCategory::Shortcuts {
                    this.fetch_shortcuts(cx);
                }
                cx.notify();
            }))
            .child(
                div()
                    .size(px(CATEGORY_BADGE))
                    .rounded(px(CATEGORY_BADGE_RADIUS))
                    .bg(category.badge())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(lucide_color(category.icon(), CATEGORY_ICON, theme::LABEL)),
            )
            .child(
                div()
                    .text_size(px(theme::BODY.size))
                    .line_height(px(theme::BODY.leading))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(if selected {
                        theme::LABEL
                    } else {
                        theme::secondary_label()
                    })
                    .child(category.title()),
            )
    }

    fn pane(
        title: &'static str,
        subtitle: Option<&'static str>,
        body: impl IntoElement,
    ) -> impl IntoElement {
        let body_id: ElementId = SharedString::from(format!("pane-body-{title}")).into();
        let scroll = pane_scroll(&body_id);
        let mut scroller = div()
            .id(body_id)
            .track_scroll(&scroll)
            .flex_1()
            .min_h(px(0.))
            .overflow_x_hidden()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(20.))
            .on_scroll_wheel({
                let scroll = scroll.clone();
                move |event: &ScrollWheelEvent, window: &mut Window, cx: &mut gpui::App| {
                    let delta = event.delta.pixel_delta(window.line_height());
                    if delta.y.abs() > delta.x.abs() && scroll.max_offset().height > px(0.5) {
                        cx.stop_propagation();
                    }
                }
            })
            .child(body);
        scroller.style().restrict_scroll_to_axis = Some(true);

        div()
            .id(SharedString::from(format!("pane-{title}")))
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .pt(px(TITLEBAR_INSET.max(22.0)))
            .px(px(24.))
            .pb(px(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .flex_shrink_0()
                    .pb(px(20.))
                    .child(
                        div()
                            .text_size(px(PANE_TITLE))
                            .line_height(px(PANE_TITLE_LEADING))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::LABEL)
                            .child(title),
                    )
                    .when_some(subtitle, |d, text| {
                        d.child(
                            div()
                                .text_size(px(theme::CALLOUT.size))
                                .line_height(px(17.))
                                .text_color(theme::secondary_label())
                                .child(text),
                        )
                    }),
            )
            .child(scroller)
    }

    fn persist_share_alias(&self) {
        let draft = self.alias_draft.trim().to_string();
        let alias = if draft.is_empty() {
            nook_core::share::default_device_alias()
        } else {
            draft
        };
        nook_core::settings::tweak_app_settings(|s| s.share.device_alias = alias);
    }

    fn persist_share_pin(&self) {
        let draft = self.pin_draft.trim().to_string();
        nook_core::settings::tweak_app_settings(|s| s.share.localsend_pin = draft);
    }

    fn render_general(
        &self,
        settings: &AppSettings,
        window: &gpui::Window,
        alias_focused: bool,
        pin_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        Self::pane(
            "General",
            Some("Launch, position, and how the island behaves."),
            div()
                .id("general-pane")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "Position",
                    settings_group(vec![
                        self.placement_canvas(settings, cx).into_any_element(),
                        self.alignment_row(settings, cx).into_any_element(),
                        self.reset_row(cx).into_any_element(),
                    ]),
                    Some("Drag the island on the preview. Option-drag it on the display to place it."),
                ))
                .child(section(
                    "Behavior",
                    settings_group(vec![
                        settings_row("launch-login")
                            .opacity(if nook_core::login_item::is_supported() { 1.0 } else { 0.5 })
                            .child(label("Launch at login", theme::BODY, true))
                            .child(toggle_knob(nook_core::login_item::is_enabled()))
                            .when(!nook_core::login_item::is_supported(), |d| d.child(label("Available when openNook runs from the app bundle (macOS 13 or later).", theme::CALLOUT, false)))
                            .when_some(self.login_error.clone(), |d, msg| d.child(label(msg, theme::CALLOUT, false).text_color(theme::DESTRUCTIVE)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if nook_core::login_item::is_supported() {
                                    let on = !nook_core::login_item::is_enabled();
                                    this.login_error = nook_core::login_item::set_enabled(on).err();
                                    cx.notify();
                                }
                            }))
                            .into_any_element(),
                        toggle_row(
                            "Show island without a notch",
                            settings.non_notch_mode,
                            cx,
                            |s| {
                                s.non_notch_mode = !s.non_notch_mode;
                            },
                        )
                        .into_any_element(),
                        toggle_row(
                            "Hide when an app fills the display",
                            settings.hide_when_maximized,
                            cx,
                            |s| {
                                s.hide_when_maximized = !s.hide_when_maximized;
                            },
                        )
                        .into_any_element(),
                        action_row("onboard-again", "First-run tips", "Show Again", cx, |_, _, cx| {
                            if let Err(err) = nook_core::settings::reset_onboarded() {
                                log::warn!("reset first-run tips: {err}");
                            }
                            cx.notify();
                        }).into_any_element(),
                    ]),
                    Some("Hover the island to expand. Click the gear on the expanded island to open Settings. Press ⌘Q to quit. First-Run Tips appear again on the next launch."),
                ))
                .child(section(
                    "HUD",
                    settings_group(vec![
                        toggle_row(
                            "Show volume & brightness HUD",
                            settings.show_volume_brightness_hud,
                            cx,
                            |s| {
                                s.show_volume_brightness_hud = !s.show_volume_brightness_hud;
                            },
                        )
                        .into_any_element(),
                        toggle_row(
                            "Replace the system volume/brightness HUD",
                            settings.replace_system_hud,
                            cx,
                            |s| {
                                s.replace_system_hud = !s.replace_system_hud;
                                nook_core::osd::apply(s.replace_system_hud);
                            },
                        )
                        .into_any_element(),
                    ]),
                    Some(hud_caption(settings)),
                ))
                .child(section(
                    "Terminal",
                    settings_group(vec![
                        toggle_row(
                            "Show Terminal in the island",
                            settings.terminal_enabled,
                            cx,
                            |s| s.terminal_enabled = !s.terminal_enabled,
                        )
                        .into_any_element(),
                        field_row(
                            "term-shell",
                            "Shell",
                            if self.shell_draft.is_empty() {
                                "$SHELL"
                            } else {
                                self.shell_draft.as_str()
                            },
                            self.shell_draft.is_empty(),
                            self.shell_focus.is_focused(window),
                            &self.shell_focus,
                            cx,
                            |this, event, cx| {
                                if SettingsView::apply_key(&mut this.shell_draft, event, cx) {
                                    nook_core::settings::tweak_app_settings(|s| {
                                        s.terminal_shell = this.shell_draft.trim().to_string();
                                    });
                                    cx.notify();
                                }
                            },
                        )
                        .into_any_element(),
                        field_row(
                            "term-font",
                            "Font",
                            if self.font_draft.is_empty() {
                                "SF Mono / Menlo"
                            } else {
                                self.font_draft.as_str()
                            },
                            self.font_draft.is_empty(),
                            self.font_focus.is_focused(window),
                            &self.font_focus,
                            cx,
                            |this, event, cx| {
                                if SettingsView::apply_key(&mut this.font_draft, event, cx) {
                                    nook_core::settings::tweak_app_settings(|s| {
                                        s.terminal_font = this.font_draft.trim().to_string();
                                    });
                                    cx.notify();
                                }
                            },
                        )
                        .into_any_element(),
                        field_row(
                            "term-font-size",
                            "Size",
                            if self.font_size_draft.is_empty() {
                                "11"
                            } else {
                                self.font_size_draft.as_str()
                            },
                            self.font_size_draft.is_empty(),
                            self.font_size_focus.is_focused(window),
                            &self.font_size_focus,
                            cx,
                            |this, event, cx| {
                                if SettingsView::apply_key(&mut this.font_size_draft, event, cx) {
                                    this.font_size_draft.retain(|c| c.is_ascii_digit() || c == '.');
                                    let parsed = this.font_size_draft.trim().parse::<f32>().ok();
                                    nook_core::settings::tweak_app_settings(|s| {
                                        s.terminal_font_size =
                                            parsed.filter(|v| (6.0..=32.0).contains(v)).unwrap_or(11.0);
                                    });
                                    cx.notify();
                                }
                            },
                        )
                        .into_any_element(),
                        toggle_row("Command history", settings.terminal_history, cx, |s| {
                            s.terminal_history = !s.terminal_history
                        }).into_any_element(),
                        action_row("term-clear-history", "Command history",
                            self.destructive_caption("term-clear-history-btn", "Clear"), cx, |_, _, cx| {
                                if let Err(err) = nook_core::shell::clear_history() {
                                    log::warn!("clear terminal history: {err}");
                                }
                                cx.notify();
                            }).into_any_element(),
                    ]),
                    Some("Interactive login shell in the island with ANSI colors. Font accepts any installed monospace family (e.g. JetBrains Mono, Fira Code). Typed here only — opennook://, the CLI, and Finder Services never run commands. Default off. Optional command history (off by default): Keeps your last 50 commands on this Mac."),
                ))
                .child(self.render_sharing(
                    settings,
                    alias_focused,
                    pin_focused,
                    cx,
                )),
        )
    }

    fn render_appearance(
        &self,
        settings: &AppSettings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        Self::pane(
            "Appearance",
            Some("Island color, Liquid Glass, and the expand gradient."),
            div()
                .id("appearance-pane")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "Island",
                    settings_group(vec![
                        toggle_row("Liquid Glass island", settings.liquid_glass_mode, cx, |s| {
                            s.liquid_glass_mode = !s.liquid_glass_mode
                        })
                        .into_any_element(),
                        self.gradient_row(settings, cx).into_any_element(),
                        self.color_row(settings, cx).into_any_element(),
                    ]),
                    Some("A custom color replaces the default black island. Compact stays black; the glass gradient fades in when the island expands."),
                )),
        )
    }

    fn render_shortcuts(
        &mut self,
        settings: &AppSettings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut rows = vec![
            action_row(
                "clock-shortcuts",
                "Clock Shortcuts",
                "Install Shortcuts",
                cx,
                |_, _, _| {
                    if let Err(err) = nook_core::shortcuts::import_bundled_shortcuts() {
                        log::info!("clock shortcuts: {err}");
                    }
                },
            )
            .into_any_element(),
            action_row(
                "lpm-shortcut",
                "Low Power Mode",
                "Install Shortcut",
                cx,
                |_, _, _| {
                    if let Err(err) = nook_core::power::install_lpm_shortcut() {
                        log::warn!("install LPM shortcut: {err}");
                    }
                },
            )
            .into_any_element(),
            toggle_row("Apple Clock Timers", settings.sync_clock_timers, cx, |s| {
                s.sync_clock_timers = !s.sync_clock_timers
            })
            .into_any_element(),
        ];
        rows.extend(pomodoro_rows(settings, &self.shortcut_catalog, cx));
        Self::pane(
            "Shortcuts",
            Some("Clock, Low Power Mode, and Focus shortcuts the island can run."),
            div()
                .id("shortcuts-pane")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "Installed",
                    settings_group(rows),
                    Some("Import the bundled Nook Clock shortcuts once to pause, resume, or cancel timers from the island."),
                )),
        )
    }

    fn render_privacy(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Self::pane(
            "Privacy",
            Some("Permissions the island needs on this Mac."),
            div()
                .id("privacy-pane")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "Permissions",
                    settings_group(notification_permission_rows(cx)),
                    Some("Captures other apps' banners via Accessibility. Optional usernoted backfill needs a manual Full Disk Access grant."),
                )),
        )
    }

    fn render_updates(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Self::pane(
            "Updates",
            Some("Restore every setting to its default."),
            div()
                .id("updates-pane")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "Reset",
                    settings_group(vec![action_row(
                        "reset-all",
                        "All Settings",
                        self.destructive_caption("reset-all-btn", "Reset to Defaults…"),
                        cx,
                        |this, _, cx| {
                            nook_core::settings::update_app_settings(AppSettings::default());
                            nook_core::osd::apply(false);
                            nook_core::weather::invalidate();
                            *this = SettingsView::new(cx);
                            this.category = SettingsCategory::Updates;
                            cx.notify();
                        },
                    )
                    .into_any_element()]),
                    Some("Returns every setting on every page to its default."),
                )),
        )
    }

    fn render_about(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Self::pane(
            "About",
            Some("Version and links."),
            div()
                .id("about-pane")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "openNook",
                    settings_group(vec![
                        action_row(
                            "quit",
                            "Quit",
                            self.destructive_caption("quit-btn", "Quit"),
                            cx,
                            |_, _, cx| {
                                nook_core::high_alert::release_all();
                                cx.quit();
                            },
                        )
                        .into_any_element(),
                        settings_row("version")
                            .child(label("Version", theme::BODY, true))
                            .child(label(env!("CARGO_PKG_VERSION"), theme::BODY, false))
                            .into_any_element(),
                        action_row("github", "View on GitHub", "Open", cx, |_, _, _| {
                            let _ = std::process::Command::new("/usr/bin/open")
                                .arg("https://github.com/prodBirdy/openNook")
                                .spawn();
                        })
                        .into_any_element(),
                    ]),
                    None::<&str>,
                )),
        )
    }

    fn render_sharing(
        &self,
        _settings: &AppSettings,
        alias_focused: bool,
        pin_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let rows = vec![
            field_row(
                "share-alias",
                "Device Name",
                if self.alias_draft.is_empty() {
                    "openNook"
                } else {
                    &self.alias_draft
                },
                self.alias_draft.is_empty(),
                alias_focused,
                &self.alias_focus,
                cx,
                |this, event, cx| {
                    if Self::apply_key(&mut this.alias_draft, event, cx) {
                        this.persist_share_alias();
                        cx.notify();
                    }
                },
            )
            .into_any_element(),
            field_row(
                "share-pin",
                "LocalSend PIN",
                if self.pin_draft.is_empty() {
                    "optional"
                } else {
                    &self.pin_draft
                },
                self.pin_draft.is_empty(),
                pin_focused,
                &self.pin_focus,
                cx,
                |this, event, cx| {
                    if Self::apply_key(&mut this.pin_draft, event, cx) {
                        this.persist_share_pin();
                        cx.notify();
                    }
                },
            )
            .into_any_element(),
        ];
        section(
            "Sharing",
            settings_group(rows),
            Some(
                "Drag files onto the Tray tab, then onto AirDrop — or LocalSend when it is installed on this Mac. LocalSend is send-only.",
            ),
        )
    }

    fn color_row(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let mut swatches = div().flex().items_center().gap(px(6.));
        for swatch in ISLAND_SWATCHES {
            swatches = swatches.child(color_swatch(swatch, settings.island_color, cx));
        }
        settings_row("island-color")
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(label("Island color", theme::BODY, true))
                    .child(label(
                        settings.island_swatch_name(),
                        theme::SUBHEADLINE,
                        false,
                    )),
            )
            .child(swatches)
    }

    fn gradient_row(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = settings.liquid_glass_mode;
        let value = settings.glass_gradient();
        let pct = format!("{}%", (value * 100.0).round() as i32);
        settings_row("glass-gradient")
            .opacity(if enabled {
                1.0
            } else {
                theme::DISABLED_OPACITY
            })
            .when(enabled, |row| {
                row.tab_index(0)
                    .focus(|style| style.border_1().border_color(theme::accent()))
                    .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                        let step = match event.keystroke.key.as_str() {
                            "left" => -0.05,
                            "right" => 0.05,
                            _ => return,
                        };
                        cx.stop_propagation();
                        nook_core::settings::tweak_app_settings(|s| {
                            s.liquid_glass_gradient = (s.glass_gradient() + step).clamp(0.0, 1.0);
                        });
                        cx.notify();
                    }))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(label("Glass gradient", theme::BODY, true))
                    .child(label("Fades in on expand", theme::SUBHEADLINE, false)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.gradient_slider(value, enabled, cx))
                    .child(
                        div()
                            .w(px(40.))
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .child(label(pct, theme::BODY, true)),
                    ),
            )
    }

    fn gradient_slider(
        &self,
        value: f32,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let value = value.clamp(0.0, 1.0);
        let knob_left = value * (GRADIENT_TRACK_W - GRADIENT_KNOB);
        let fill = (knob_left + GRADIENT_KNOB * 0.5) / GRADIENT_TRACK_W;
        let track = self.gradient_track.clone();
        let dragging = self.gradient_drag;
        div()
            .id("glass-gradient-slider")
            .relative()
            .flex_shrink_0()
            .w(px(GRADIENT_TRACK_W))
            .h(px(theme::HIT_MIN))
            .cursor(if !enabled {
                CursorStyle::Arrow
            } else if dragging {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::PointingHand
            })
            .when(enabled, |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.pending_destructive = None;
                        this.gradient_drag = true;
                        this.apply_gradient(event.position.x.into(), cx);
                        cx.notify();
                    }),
                )
            })
            .child(
                div()
                    .absolute()
                    .top(px((theme::HIT_MIN - theme::TRACK_H) * 0.5))
                    .left_0()
                    .right_0()
                    .h(px(theme::TRACK_H))
                    .rounded(px(theme::TRACK_RADIUS))
                    .bg(theme::FILL_SECONDARY)
                    .overflow_hidden()
                    .child(div().h_full().w(relative(fill)).bg(if enabled {
                        theme::accent()
                    } else {
                        theme::FILL
                    })),
            )
            .child(
                div()
                    .absolute()
                    .top(px((theme::HIT_MIN - GRADIENT_KNOB) * 0.5))
                    .left(px(knob_left))
                    .size(px(GRADIENT_KNOB))
                    .rounded_full()
                    .bg(theme::LABEL)
                    .border_1()
                    .border_color(theme::with_alpha(theme::ISLAND, 0.22)),
            )
            .child(
                canvas(
                    move |bounds, _, _| {
                        let origin: f32 = bounds.origin.x.into();
                        let width: f32 = bounds.size.width.into();
                        *track.borrow_mut() = Some((origin, width));
                        bounds
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
    }

    fn placement_canvas(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        const PILL_W: f32 = 52.0;
        const PILL_H: f32 = 14.0;
        const FALLBACK_W: f32 = 640.0;
        const FALLBACK_H: f32 = 128.0;
        let (cw, ch) = self
            .placement_bounds
            .borrow()
            .map(|b| {
                let w: f32 = b.size.width.into();
                let h: f32 = b.size.height.into();
                (w.max(1.0), h.max(1.0))
            })
            .unwrap_or((FALLBACK_W, FALLBACK_H));
        let (pill_x, pill_y) = settings.island_origin(cw, ch, PILL_W, PILL_H);
        let fill = theme::island_fill(settings.island_color);
        let bounds_cell = self.placement_bounds.clone();

        div()
            .id("placement-canvas")
            .w_full()
            .h(px(FALLBACK_H))
            .overflow_hidden()
            .relative()
            .cursor(if self.placement_drag {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .child(div().absolute().inset_0().bg(theme::WINDOW_BG))
            .child(
                canvas(
                    move |bounds, _, _| {
                        *bounds_cell.borrow_mut() = Some(bounds);
                        bounds
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(
                div()
                    .absolute()
                    .left(px(pill_x))
                    .top(px(pill_y))
                    .w(px(PILL_W))
                    .h(px(PILL_H))
                    .rounded_full()
                    .bg(fill),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.pending_destructive = None;
                    this.placement_drag = true;
                    this.apply_placement(event.position.x.into(), event.position.y.into(), cx);
                }),
            )
    }

    fn alignment_row(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let x = settings.island_x;
        let left = (x - 0.0).abs() < 0.02;
        let center = (x - 0.5).abs() < 0.02;
        let right = (x - 1.0).abs() < 0.02;
        settings_row("island-align")
            .child(label("Alignment", theme::BODY, true))
            .child(
                segmented_group()
                    .child(segment("Left", left, cx, |_, _, cx| {
                        nook_core::settings::tweak_app_settings(|s| s.island_x = 0.0);
                        cx.notify();
                    }))
                    .child(segment("Center", center, cx, |_, _, cx| {
                        nook_core::settings::tweak_app_settings(|s| s.island_x = 0.5);
                        cx.notify();
                    }))
                    .child(segment("Right", right, cx, |_, _, cx| {
                        nook_core::settings::tweak_app_settings(|s| s.island_x = 1.0);
                        cx.notify();
                    })),
            )
    }

    fn reset_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        settings_row("island-reset")
            .child(label("Restore default position", theme::BODY, true))
            .child(push_button(
                "island-reset-btn",
                self.destructive_caption("island-reset-btn", "Reset"),
                cx,
                |_, _, cx| {
                    nook_core::settings::tweak_app_settings(|s| s.reset_island_position());
                    cx.notify();
                },
            ))
    }

    fn render_widgets(
        &mut self,
        settings: &AppSettings,
        url_focused: bool,
        token_focused: bool,
        query_focused: bool,
        heading_focused: bool,
        city_focused: bool,
        ignore_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut on_nook = Vec::new();
        let mut available = Vec::new();
        let mut blocked_available = false;
        let query = self.search_draft.trim().to_ascii_lowercase();
        for module in settings.ordered_widgets() {
            if !module.is_available() || !settings.widget_visible(module) {
                continue;
            }
            if !query.is_empty() && !module.name().to_ascii_lowercase().contains(&query) {
                continue;
            }
            if module.enabled(settings) {
                on_nook.push(
                    self.widget_row(module, settings, true, cx)
                        .into_any_element(),
                );
            } else {
                if module.occupies_nook_cells() && !settings.can_enable(module) {
                    blocked_available = true;
                }
                available.push(
                    self.widget_row(module, settings, false, cx)
                        .into_any_element(),
                );
            }
        }
        let used = settings.used_cells();
        let total = AppSettings::TOTAL_CELLS;
        let available_n = available.len();
        let available_footer = blocked_available
            .then(|| "No room left. Turn off or shrink a widget to make room.".to_string());

        let show_module = settings.widget_visible(self.module);
        let preview = self.island_preview(settings, cx);

        Self::pane(
            "Widgets",
            Some("Pick the widgets that live in the island and drag them into the order you want."),
            div()
                .id("custom-widgets")
                .flex()
                .flex_col()
                .child(preview)
                .child(div().h(px(24.)).flex_shrink_0())
                .child(widget_list_block(
                    "In the Island",
                    format!("{used} of {total}"),
                    ISLAND_LIST_INDENT,
                    if on_nook.is_empty() {
                        vec![empty_hint("No widgets on the island yet.").into_any_element()]
                    } else {
                        on_nook
                    },
                ))
                .child(div().h(px(22.)).flex_shrink_0())
                .child(widget_list_block(
                    "More Widgets",
                    format!("{available_n} available"),
                    MORE_LIST_INDENT,
                    if available.is_empty() {
                        vec![empty_hint("Every widget is already on the island.").into_any_element()]
                    } else {
                        available
                    },
                ))
                .when_some(available_footer, |d, text| {
                    d.child(div().px(px(4.)).pt(px(6.)).child(caption_text(text)))
                })
                .child(
                    div()
                        .pt(px(22.))
                        .flex()
                        .flex_col()
                        .gap(px(22.))
                        .when(show_module, |d| {
                            d.child(self.module_section(
                                settings,
                                url_focused,
                                token_focused,
                                query_focused,
                                heading_focused,
                                city_focused,
                                ignore_focused,
                                cx,
                            ))
                        })
                        .child(section(
                            "Layout",
                            settings_group(vec![action_row(
                                "customize-on-nook",
                                "Customize Layout",
                                "Edit on Island",
                                cx,
                                |_, _, _| {
                                    nook_core::automation::push_action(
                                        nook_core::automation::ExternalAction::EditWidgets,
                                    );
                                },
                            )
                            .into_any_element()]),
                            Some("Drag widgets on the island, or reorder them in the lists above."),
                        ))
                        .child(section(
                            "Experimental",
                            settings_group(vec![toggle_row(
                                "Show Experimental Widgets",
                                settings.experimental_widgets,
                                cx,
                                |s| s.experimental_widgets = !s.experimental_widgets,
                            )
                            .into_any_element()]),
                            Some(
                                "Reveals in-progress widgets (Observe, Obsidian, VPN, Meetings, Messages, and more). They are unfinished and off by default.",
                            ),
                        )),
                )
                .child(div().h(px(20.)).flex_shrink_0())
                .child(self.widgets_reset_footer(cx)),
        )
    }

    fn widget_row(
        &self,
        module: WidgetModule,
        settings: &AppSettings,
        on_island: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.module == module;
        let on = module.enabled(settings);
        let caption = widget_caption(module, settings);
        let action_label = if on_island { "Remove" } else { "Add" };
        let action_icon = if on_island { "minus" } else { "plus" };
        let action_color = if on_island {
            theme::secondary_label()
        } else {
            theme::accent()
        };
        div()
            .id(SharedString::from(format!("mod-{}", module.name())))
            .px(px(GROUP_PAD))
            .h(px(WIDGET_ROW_H))
            .flex()
            .items_center()
            .gap(px(10.))
            .when(selected, |d| d.bg(theme::FILL))
            .hover(|s| {
                if selected {
                    s
                } else {
                    s.bg(theme::FILL_TERTIARY)
                }
            })
            .drag_over::<WidgetDrag>(move |style, drag, _, _| {
                if drag.0 == module {
                    style
                } else {
                    style.bg(theme::FILL_SECONDARY)
                }
            })
            .can_drop(move |value, _, _| {
                value
                    .downcast_ref::<WidgetDrag>()
                    .is_some_and(|drag| drag.0 != module)
            })
            .on_drop(cx.listener(move |_, drag: &WidgetDrag, _, cx| {
                nook_core::settings::tweak_app_settings(|settings| {
                    let _ = settings.try_move_widget_to(drag.0, module);
                });
                cx.notify();
            }))
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.module = module;
                if module == WidgetModule::Vpn {
                    this.ignore_draft = nook_core::vpn::format_ignore_list(
                        &nook_core::settings::get_app_settings().vpn_ignore_interfaces,
                    );
                }
                this.persist_nav();
                if module == WidgetModule::Timers {
                    this.fetch_shortcuts(cx);
                }
                cx.notify();
            }))
            .on_drag(WidgetDrag(module), |drag, _, _, cx| cx.new(|_| *drag))
            .when(on_island, |d| {
                d.child(lucide_color(
                    "grip-vertical",
                    WIDGET_GRIP,
                    theme::TERTIARY_LABEL,
                ))
            })
            .child(widget_badge(module))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(label(module.name(), theme::BODY, false).text_color(theme::LABEL))
                    .child(
                        label(caption, theme::FOOTNOTE, false).text_color(theme::secondary_label()),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("mod-act-{}", module.name())))
                    .h(px(REMOVE_H))
                    .px(px(9.))
                    .rounded(px(REMOVE_RADIUS))
                    .bg(theme::FILL)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.bg(theme::FILL_SECONDARY))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        let s = nook_core::settings::get_app_settings();
                        let can = on || s.can_enable(module);
                        if !can && !on {
                            return;
                        }
                        let mut s = s;
                        let was_enabled = s.show_notifications;
                        module.set_enabled(&mut s);
                        let request_notifications = module == WidgetModule::Notifications
                            && !was_enabled
                            && s.show_notifications;
                        nook_core::settings::update_app_settings(s);
                        if request_notifications {
                            nook_core::notifications::ax_trusted(true);
                        }
                        cx.notify();
                    }))
                    .child(lucide_color(action_icon, 11.0, action_color))
                    .child(
                        div()
                            .text_size(px(theme::SUBHEADLINE.size))
                            .text_color(action_color)
                            .child(action_label),
                    ),
            )
    }

    fn island_preview(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let used = settings.used_cells();
        let total = AppSettings::TOTAL_CELLS;
        let mut cells = div()
            .id("island-preview")
            .flex()
            .flex_1()
            .gap(px(4.))
            .justify_center()
            .rounded(px(theme::ROW_RADIUS))
            .bg(theme::ISLAND)
            .shadow_lg()
            .p(px(14.));
        let enabled: Vec<WidgetModule> = settings
            .ordered_widgets()
            .into_iter()
            .filter(|m| {
                m.enabled(settings) && settings.widget_visible(*m) && m.occupies_nook_cells()
            })
            .collect();
        if enabled.is_empty() {
            cells = cells.child(div().flex_1().flex().items_center().justify_center().child(
                label("No widgets on the island yet.", theme::FOOTNOTE, false),
            ));
        } else {
            for module in enabled {
                let selected = self.module == module;
                cells = cells.child(
                    div()
                        .id(SharedString::from(format!("preview-{}", module.name())))
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(7.))
                        .px(px(4.))
                        .py(px(6.))
                        .rounded(px(theme::CONTROL_RADIUS))
                        .when(selected, |d| d.bg(theme::FILL))
                        .cursor(CursorStyle::PointingHand)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.module = module;
                            this.persist_nav();
                            cx.notify();
                        }))
                        .child(lucide_color(module.icon(), 17.0, theme::LABEL))
                        .child(
                            div()
                                .text_size(px(theme::FOOTNOTE.size))
                                .line_height(px(theme::FOOTNOTE.leading))
                                .text_color(theme::secondary_label())
                                .child(module.name()),
                        ),
                );
            }
        }
        div()
            .flex()
            .flex_col()
            .w_full()
            .child(group_header(
                "Island Preview",
                format!("{used} of {total} slots used"),
            ))
            .child(
                div()
                    .w_full()
                    .rounded(px(theme::INNER_RADIUS))
                    .border_1()
                    .border_color(theme::FILL_TERTIARY)
                    .p(px(18.))
                    .bg(linear_gradient(
                        90.0,
                        linear_color_stop(theme::rgba_from_u32(0x2A2A30, 1.0), 0.0),
                        linear_color_stop(theme::rgba_from_u32(0x141417, 1.0), 1.0),
                    ))
                    .flex()
                    .justify_center()
                    .child(cells),
            )
            .child(div().pt(px(8.)).child(label(
                "The island updates live as you make changes.",
                theme::SUBHEADLINE,
                false,
            )))
    }

    fn widgets_reset_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().px(px(2.)).flex().child(
            div()
                .id("widgets-reset")
                .h(px(26.))
                .px(px(11.))
                .rounded(px(6.))
                .bg(theme::FILL)
                .border_1()
                .border_color(theme::FILL_TERTIARY)
                .flex()
                .items_center()
                .gap(px(4.))
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(theme::FILL_SECONDARY))
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    if !confirm_destructive(&mut this.pending_destructive, "reset-all-btn".into()) {
                        cx.notify();
                        return;
                    }
                    nook_core::settings::update_app_settings(AppSettings::default());
                    nook_core::osd::apply(false);
                    nook_core::weather::invalidate();
                    *this = SettingsView::new(cx);
                    this.category = SettingsCategory::Widgets;
                    cx.notify();
                }))
                .child(lucide_color("rotate-ccw", 12.0, theme::secondary_label()))
                .child(
                    div()
                        .text_size(px(theme::CALLOUT.size))
                        .text_color(theme::LABEL)
                        .child(self.destructive_caption("reset-all-btn", "Reset to Defaults")),
                ),
        )
    }

    fn size_picker(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let module = self.module;
        let enabled = module.occupies_nook_cells();
        let current = settings.size_for(module);
        let show_hint = self
            .size_budget_hint_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(2));
        let mut segments = segmented_group();
        for size in nook_core::settings::WidgetSize::ALL {
            let selected = current == size;
            let size_label = size.label();
            segments = segments.child(
                div()
                    .id(SharedString::from(format!("size-{size_label}")))
                    .h(px(24.))
                    .px(px(10.))
                    .rounded(px(theme::CONTROL_RADIUS))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |d| d.bg(theme::FILL_SECONDARY))
                    .opacity(if enabled { 1.0 } else { 0.45 })
                    .cursor(if enabled {
                        CursorStyle::PointingHand
                    } else {
                        CursorStyle::Arrow
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !enabled {
                            return;
                        }
                        cx.stop_propagation();
                        this.module = module;
                        let mut ok = false;
                        nook_core::settings::tweak_app_settings(|s| {
                            ok = s.set_size(module, size);
                        });
                        this.size_budget_hint_at = if ok { None } else { Some(Instant::now()) };
                        cx.notify();
                    }))
                    .child(label(size_label, theme::SUBHEADLINE, selected)),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .w_full()
            .child(
                settings_row("size-row")
                    .opacity(if enabled { 1.0 } else { 0.45 })
                    .child(label("Size", theme::BODY, true))
                    .child(div().flex_1())
                    .child(segments),
            )
            .when(show_hint, |d| {
                d.child(
                    label(
                        "No room for that size — turn off a widget first.",
                        theme::FOOTNOTE,
                        false,
                    )
                    .text_color(theme::DESTRUCTIVE)
                    .px(px(4.)),
                )
            })
    }

    fn module_section(
        &mut self,
        settings: &AppSettings,
        url_focused: bool,
        token_focused: bool,
        query_focused: bool,
        heading_focused: bool,
        city_focused: bool,
        ignore_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = self.module.name();
        let enabled = self.module.enabled(settings);
        let mut rows = Vec::new();
        rows.push(
            settings_row("mod-enabled")
                .child(label("On the Island", theme::BODY, true))
                .child(module_toggle(enabled, self.module, settings, cx))
                .into_any_element(),
        );
        match self.module {
            WidgetModule::Music => {
                rows.push(
                    toggle_row("Show Lyrics", settings.show_lyrics, cx, |s| {
                        s.show_lyrics = !s.show_lyrics;
                    })
                    .into_any_element(),
                );
                rows.push(
                    toggle_row(
                        "Animated Album Art (Apple Music)",
                        settings.animated_album_art,
                        cx,
                        |s| s.animated_album_art = !s.animated_album_art,
                    )
                    .into_any_element(),
                );
                rows.push(
                    toggle_row("Ambient Art Glow", settings.ambient_art_glow, cx, |s| {
                        s.ambient_art_glow = !s.ambient_art_glow
                    })
                    .into_any_element(),
                );
                rows.push(
                    toggle_row(
                        "Output device picker",
                        settings.audio_output_picker,
                        cx,
                        |s| s.audio_output_picker = !s.audio_output_picker,
                    )
                    .into_any_element(),
                );
            }
            WidgetModule::Calendar => {
                rows.push(
                    action_row(
                        "calendar-app",
                        "Calendar app",
                        "Open Calendar",
                        cx,
                        |_, _, _| {
                            crate::platform::open_calendar();
                        },
                    )
                    .into_any_element(),
                );
            }
            WidgetModule::Reminders => {
                rows.push(
                    toggle_row("Quick Add", settings.quick_add, cx, |s| {
                        s.quick_add = !s.quick_add;
                    })
                    .into_any_element(),
                );
            }
            WidgetModule::Notes => {
                rows.push(
                    action_row("notes-edit", "Notes", "Edit Notes", cx, |_, _, _| {
                        if let Err(err) = nook_core::notes::open_notes_editor() {
                            log::warn!("open notes: {err}");
                        }
                    })
                    .into_any_element(),
                );
            }
            WidgetModule::Observe => {
                rows.push(
                    action_row(
                        "observe-browse",
                        "Metric Names",
                        "Browse Metrics",
                        cx,
                        |this, _, cx| {
                            this.browse_metrics(cx);
                        },
                    )
                    .into_any_element(),
                );
            }
            WidgetModule::Battery => {
                rows.push(threshold_row(settings.battery_alert_threshold, cx).into_any_element());
                rows.push(
                    action_row(
                        "lpm-shortcut",
                        "Low Power Mode",
                        "Install Shortcut",
                        cx,
                        |_, _, _| {
                            if let Err(err) = nook_core::power::install_lpm_shortcut() {
                                log::warn!("install LPM shortcut: {err}");
                            }
                        },
                    )
                    .into_any_element(),
                );
            }
            WidgetModule::Messages => {
                let fda = nook_core::messages::fda_status();
                let status = match fda {
                    nook_core::messages::FdaStatus::Granted => "Granted",
                    nook_core::messages::FdaStatus::Denied => "Not Granted",
                    nook_core::messages::FdaStatus::Unavailable => "Not Available",
                };
                rows.push(
                    settings_row("msg-fda-status")
                        .child(label("Full Disk Access", theme::BODY, true))
                        .child(label(status, theme::BODY, false))
                        .into_any_element(),
                );
                rows.push(
                    action_row(
                        "msg-fda-open",
                        "Privacy Settings",
                        "Open Full Disk Access",
                        cx,
                        |_, _, _| {
                            if let Err(err) = nook_core::messages::open_fda_settings() {
                                log::warn!("open FDA settings: {err}");
                            }
                        },
                    )
                    .into_any_element(),
                );
                rows.push(
                    toggle_row(
                        "Experimental WhatsApp Auto-Send",
                        settings.experimental_whatsapp_autosend,
                        cx,
                        |s| {
                            s.experimental_whatsapp_autosend = !s.experimental_whatsapp_autosend;
                        },
                    )
                    .into_any_element(),
                );
            }
            WidgetModule::Obsidian => {
                let vault_label = settings
                    .obsidian_vault
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "None".into());
                rows.push(
                    settings_row("obsidian-path")
                        .child(label("Vault", theme::BODY, true))
                        .child(label(vault_label, theme::SUBHEADLINE, false))
                        .into_any_element(),
                );
                rows.push(
                    action_row(
                        "obsidian-folder",
                        "Vault Folder",
                        "Choose Folder…",
                        cx,
                        |_, _, cx| {
                            if let Some(path) = crate::platform::choose_directory() {
                                nook_core::settings::tweak_app_settings(|s| {
                                    s.obsidian_vault = Some(path);
                                });
                                cx.notify();
                            }
                        },
                    )
                    .into_any_element(),
                );
                if settings.obsidian_vault.is_some() {
                    rows.push(
                        action_row(
                            "obsidian-clear",
                            "Remove Vault",
                            self.destructive_caption("obsidian-clear-btn", "Clear"),
                            cx,
                            |_, _, cx| {
                                nook_core::settings::tweak_app_settings(|s| {
                                    s.obsidian_vault = None;
                                });
                                cx.notify();
                            },
                        )
                        .into_any_element(),
                    );
                }
                rows.push(
                    toggle_row(
                        "Capture via Obsidian URI",
                        settings.obsidian_uri_capture,
                        cx,
                        |s| s.obsidian_uri_capture = !s.obsidian_uri_capture,
                    )
                    .into_any_element(),
                );
            }
            WidgetModule::Timers => {
                rows.push(
                    toggle_row("Apple Clock Timers", settings.sync_clock_timers, cx, |s| {
                        s.sync_clock_timers = !s.sync_clock_timers
                    })
                    .into_any_element(),
                );
                rows.push(
                    action_row(
                        "clock-shortcuts",
                        "Clock Shortcuts",
                        "Install Shortcuts",
                        cx,
                        |_, _, _| {
                            if let Err(err) = nook_core::shortcuts::import_bundled_shortcuts() {
                                log::info!("clock shortcuts: {err}");
                            }
                        },
                    )
                    .into_any_element(),
                );
                rows.extend(pomodoro_rows(settings, &self.shortcut_catalog, cx));
            }
            WidgetModule::Vpn => {
                rows.push(
                    toggle_row(
                        "Timer on Compact Island",
                        settings.vpn_show_timer,
                        cx,
                        |s| {
                            s.vpn_show_timer = !s.vpn_show_timer;
                        },
                    )
                    .into_any_element(),
                );
                let ignore_placeholder = self.ignore_draft.is_empty();
                let ignore_text = if ignore_placeholder {
                    "utun3, ipsec0"
                } else {
                    self.ignore_draft.as_str()
                };
                rows.push(
                    field_row(
                        "vpn-ignore",
                        "Ignore",
                        ignore_text,
                        ignore_placeholder,
                        ignore_focused,
                        &self.ignore_focus,
                        cx,
                        |this, event, cx| {
                            let persist =
                                SettingsView::apply_key(&mut this.ignore_draft, event, cx)
                                    || event.keystroke.key == "enter";
                            if persist {
                                this.persist_ignore();
                                cx.notify();
                            }
                        },
                    )
                    .into_any_element(),
                );
            }
            WidgetModule::Meeting => {
                rows.push(
                    toggle_row("Zoom", settings.meetings.zoom, cx, |s| {
                        s.meetings.zoom = !s.meetings.zoom;
                    })
                    .into_any_element(),
                );
                rows.push(
                    toggle_row("Microsoft Teams", settings.meetings.teams, cx, |s| {
                        s.meetings.teams = !s.meetings.teams;
                    })
                    .into_any_element(),
                );
                rows.push(
                    toggle_row("Google Meet", settings.meetings.meet, cx, |s| {
                        s.meetings.meet = !s.meetings.meet;
                    })
                    .into_any_element(),
                );
                let mut modes = segmented_group();
                for (caption, mode) in [
                    ("Focus Tab", nook_core::settings::MeetControlMode::FocusTab),
                    (
                        "Apple Events JS",
                        nook_core::settings::MeetControlMode::AppleEventsJs,
                    ),
                ] {
                    modes = modes.child(segment(
                        caption,
                        settings.meetings.meet_mode == mode,
                        cx,
                        move |_, _, cx| {
                            nook_core::settings::tweak_app_settings(|s| {
                                s.meetings.meet_mode = mode
                            });
                            cx.notify();
                        },
                    ));
                }
                rows.push(
                    settings_row("meet-mode")
                        .child(label("Meet Control", theme::BODY, true))
                        .child(modes)
                        .into_any_element(),
                );
                rows.push(
                    permission_row("Accessibility", nook_core::eventtap::accessibility_status())
                        .into_any_element(),
                );
                rows.push(
                    action_row(
                        "ax-status",
                        "Accessibility",
                        "Open Privacy Settings",
                        cx,
                        |_, _, _| {
                            crate::platform::open_accessibility_settings();
                        },
                    )
                    .into_any_element(),
                );
            }
            WidgetModule::HighAlert => {
                rows.extend(high_alert_rows(settings, cx));
            }
            WidgetModule::Recorder => {
                rows.push(
                    toggle_row(
                        "Live Transcription",
                        settings.recorder_transcribe,
                        cx,
                        |s| s.recorder_transcribe = !s.recorder_transcribe,
                    )
                    .into_any_element(),
                );
            }
            WidgetModule::Notifications => {
                rows.extend(notification_permission_rows(cx));
            }
            _ => {}
        }

        div()
            .id("module-controls")
            .flex()
            .flex_col()
            .gap(px(16.))
            .opacity(if enabled { 1.0 } else { 0.55 })
            .child(section(
                "Size",
                settings_group(vec![self.size_picker(settings, cx).into_any_element()]),
                Some("How much of the island row the widget takes."),
            ))
            .child(section(
                name,
                settings_group(rows),
                Some(module_blurb(self.module)),
            ))
            .when(self.module == WidgetModule::Observe, |d| {
                d.child(self.render_observe_settings(
                    settings,
                    url_focused,
                    token_focused,
                    query_focused,
                    cx,
                ))
            })
            .when(self.module == WidgetModule::Obsidian, |d| {
                d.child(self.render_obsidian_settings(settings, heading_focused, cx))
            })
            .when(self.module == WidgetModule::Weather, |d| {
                d.child(self.render_weather_settings(settings, city_focused, cx))
            })
            .when(self.module == WidgetModule::SysStats, |d| {
                d.child(self.render_sysstats_settings(settings, cx))
            })
            .when(self.module == WidgetModule::Music, |d| {
                d.child(settings_group(vec![div()
                    .id("music-browser-art")
                    .child(toggle_row(
                        "Use browser tab artwork",
                        settings.browser_artwork,
                        cx,
                        |s| {
                            s.browser_artwork = !s.browser_artwork;
                        },
                    ))
                    .into_any_element()]))
                    .child(self.render_music_settings(settings, cx))
            })
            .when(self.module == WidgetModule::Notifications, |d| {
                d.child(self.render_notification_settings(settings, cx))
            })
    }

    fn render_weather_settings(
        &self,
        settings: &AppSettings,
        _city_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use nook_core::weather::WeatherUnits;

        let units = settings.weather.units;
        let mut unit_row = segmented_group();
        for (caption, value) in [
            ("°C", WeatherUnits::Celsius),
            ("°F", WeatherUnits::Fahrenheit),
        ] {
            unit_row = unit_row.child(segment(caption, units == value, cx, move |_, _, cx| {
                nook_core::settings::tweak_app_settings(|s| s.weather.units = value);
                nook_core::weather::invalidate();
                cx.notify();
            }));
        }

        // Weather always follows the Mac's location (nook-core refreshes it at
        // most every 15 min); this section only reports where and whether.
        let location = &settings.weather.location;
        let error = nook_core::weather::location_error();
        let located = location.coords().is_some();
        let place = if located {
            format!("{} (current)", location.display_name())
        } else {
            "Locating…".to_string()
        };
        let permission = if error.is_some() {
            "Unavailable"
        } else if nook_core::weather::has_location_fix() {
            "Allowed"
        } else {
            "Waiting for a fix"
        };
        let mut location_rows = vec![
            settings_row("weather-location")
                .child(label("Location", theme::BODY, true))
                .child(label(place, theme::BODY, false))
                .into_any_element(),
            settings_row("weather-permission")
                .child(label("Location Services", theme::BODY, true))
                .child(label(permission, theme::BODY, false))
                .into_any_element(),
        ];
        if let Some(err) = error {
            location_rows.push(
                settings_row("weather-location-hint")
                    .child(
                        label(err, theme::FOOTNOTE, false)
                            .flex_1()
                            .min_w(px(0.))
                            .text_color(theme::secondary_label()),
                    )
                    .child(push_button(
                        "weather-location-settings",
                        "Open Location Settings",
                        cx,
                        |_, _, _| super::ui::open_privacy_pane("Privacy_LocationServices"),
                    ))
                    .into_any_element(),
            );
        }
        let location_note = "Uses your Mac's approximate location (city-level), checked at most every 15 minutes. The Location Services grant is keyed to this build's signature and resets after an ad-hoc re-sign.";

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(section(
                "Units",
                settings_group(vec![settings_row("weather-units")
                    .child(label("Temperature", theme::BODY, true))
                    .child(unit_row)
                    .into_any_element()]),
                None::<SharedString>,
            ))
            .child(section(
                "Location",
                settings_group(location_rows),
                Some(location_note),
            ))
            .child(section(
                "Compact Island",
                settings_group(vec![toggle_row(
                    "Show on the Compact Island",
                    settings.weather.show_on_compact_face,
                    cx,
                    |s| {
                        s.weather.show_on_compact_face = !s.weather.show_on_compact_face;
                    },
                )
                .into_any_element()]),
                Some("Temp and condition next to the notch while the island is idle."),
            ))
            .child(section(
                "Attribution",
                settings_group(vec![settings_row("weather-attr")
                    .child(label(nook_core::weather::ATTRIBUTION, theme::BODY, false))
                    .into_any_element()]),
                Some("Required by Open-Meteo's CC-BY 4.0 license."),
            ))
    }

    fn render_music_settings(
        &self,
        settings: &AppSettings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut rows = vec![
            toggle_row("Show Up Next", settings.show_media_queue, cx, |s| {
                s.show_media_queue = !s.show_media_queue;
            })
            .into_any_element(),
        ];
        if nook_core::queue::music_automation_denied() {
            rows.push(
                settings_row("music-tcc")
                    .child(caption_text("Music Automation was denied. Grant it in System Settings → Privacy & Security → Automation to show Up Next in playlist."))
                    .into_any_element(),
            );
        }

        section(
            "Up Next",
            settings_group(rows),
            Some(
                "Shows upcoming tracks from the current Apple Music playlist via local Automation — not Music’s real Playing Next queue. Hidden while shuffle or radio is on. Spotify’s desktop app does not expose a queue to macOS, so the list control stays Music-only.",
            ),
        )
    }

    fn render_sysstats_settings(
        &self,
        settings: &AppSettings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        section(
            "Readouts",
            settings_group(vec![
                toggle_row("CPU", settings.sysstats.show_cpu, cx, |s| {
                    s.sysstats.show_cpu = !s.sysstats.show_cpu;
                })
                .into_any_element(),
                toggle_row("Memory", settings.sysstats.show_mem, cx, |s| {
                    s.sysstats.show_mem = !s.sysstats.show_mem;
                })
                .into_any_element(),
                toggle_row("Network", settings.sysstats.show_net, cx, |s| {
                    s.sysstats.show_net = !s.sysstats.show_net;
                })
                .into_any_element(),
                toggle_row("Disk", settings.sysstats.show_disk, cx, |s| {
                    s.sysstats.show_disk = !s.sysstats.show_disk;
                })
                .into_any_element(),
                toggle_row(
                    "Physical Interfaces Only",
                    settings.sysstats.physical_nics,
                    cx,
                    |s| s.sysstats.physical_nics = !s.sysstats.physical_nics,
                )
                .into_any_element(),
            ]),
            Some("Samples only while the expanded card is visible. CPU and network need two ticks; a collapse longer than a few minutes resets the rates."),
        )
    }

    fn render_notification_settings(
        &self,
        settings: &AppSettings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut filter_rows = Vec::new();
        let apps = nook_core::notifications::known_apps();
        if apps.is_empty() {
            filter_rows.push(
                settings_row("notify-apps-empty")
                    .child(caption_text(
                        "Apps appear here after a notification is captured.",
                    ))
                    .into_any_element(),
            );
        } else {
            for (id, name) in apps {
                let blocked = settings.notification_app_blocked(&id);
                let toggle_id = id.clone();
                filter_rows.push(
                    settings_row(SharedString::from(format!("notify-app-{id}")))
                        .cursor(CursorStyle::PointingHand)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            nook_core::settings::tweak_app_settings(|s| {
                                s.toggle_notification_app(&toggle_id);
                            });
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(1.))
                                .child(label(name, theme::BODY, true))
                                .child(label(id, theme::SUBHEADLINE, false)),
                        )
                        .child(label(
                            if blocked { "Hidden" } else { "Shown" },
                            theme::SUBHEADLINE,
                            false,
                        ))
                        .child(toggle_knob(!blocked))
                        .into_any_element(),
                );
            }
        }

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(section(
                "Full Disk Access",
                settings_group(vec![toggle_row(
                    "Read Notification History",
                    settings.notification_fda_opt_in,
                    cx,
                    |s| s.notification_fda_opt_in = !s.notification_fda_opt_in,
                )
                .into_any_element()]),
                Some("Backfills banners the Accessibility scrape missed. You must add openNook in System Settings › Privacy & Security › Full Disk Access — there is no prompt."),
            ))
            .child(section(
                "Per-App Filter",
                settings_group(filter_rows),
                Some("Hidden apps never enter the shelf."),
            ))
    }

    fn render_observe_settings(
        &self,
        settings: &AppSettings,
        url_focused: bool,
        token_focused: bool,
        query_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut pinned_rows = Vec::new();
        if settings.observe.metrics.is_empty() {
            pinned_rows.push(
                settings_row("pin-empty")
                    .child(label("No pinned metrics", theme::BODY, false))
                    .into_any_element(),
            );
        } else {
            for metric in &settings.observe.metrics {
                let query = metric.query.clone();
                let mut charts = segmented_group();
                for chart in [
                    nook_core::observe::ObserveChartKind::Off,
                    nook_core::observe::ObserveChartKind::Sparkline,
                    nook_core::observe::ObserveChartKind::Bars,
                ] {
                    let query = metric.query.clone();
                    charts = charts.child(segment(
                        if chart == nook_core::observe::ObserveChartKind::Off {
                            "No Chart"
                        } else {
                            chart.caption()
                        },
                        metric.chart == chart,
                        cx,
                        move |_, _, cx| {
                            SettingsView::persist_observe(|s| {
                                if let Some(metric) =
                                    s.observe.metrics.iter_mut().find(|m| m.query == query)
                                {
                                    metric.chart = chart;
                                }
                            });
                            cx.notify();
                        },
                    ));
                }
                let mut alerts = segmented_group();
                for (caption, threshold) in [
                    ("Off", None),
                    ("> 0", Some(0)),
                    ("> 10", Some(10)),
                    ("> 100", Some(100)),
                ] {
                    let query = metric.query.clone();
                    alerts = alerts.child(segment(
                        caption,
                        metric.alert_above == threshold,
                        cx,
                        move |_, _, cx| {
                            SettingsView::persist_observe(|s| {
                                if let Some(metric) =
                                    s.observe.metrics.iter_mut().find(|m| m.query == query)
                                {
                                    metric.alert_above = threshold;
                                }
                            });
                            cx.notify();
                        },
                    ));
                }
                pinned_rows.push(
                    div()
                        .id(SharedString::from(format!("pin-{}", metric.query)))
                        .px(px(GROUP_PAD))
                        .py(px(6.))
                        .min_h(px(ROW_H))
                        .flex()
                        .flex_col()
                        .items_start()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(1.))
                                .child(label(metric.label.clone(), theme::BODY, true))
                                .child(label(metric.query.clone(), theme::SUBHEADLINE, false)),
                        )
                        .child(
                            settings_row(SharedString::from(format!("chart-{}", metric.query)))
                                .child(label("Chart", theme::BODY, true))
                                .child(charts),
                        )
                        .child(
                            settings_row(SharedString::from(format!("alert-{}", metric.query)))
                                .child(label("Alert Above", theme::BODY, true))
                                .child(alerts),
                        )
                        .child(push_button(
                            SharedString::from(format!("unpin-{}", metric.query)),
                            self.destructive_caption(&format!("unpin-{}", metric.query), "Remove"),
                            cx,
                            move |_, _, cx| {
                                cx.stop_propagation();
                                SettingsView::persist_observe(|s| {
                                    nook_core::observe::unpin_metric(&mut s.observe, &query);
                                });
                                cx.notify();
                            },
                        ))
                        .into_any_element(),
                );
            }
        }

        let mut catalog_rows = Vec::new();
        if self.catalog_loading {
            catalog_rows.push(
                settings_row("cat-loading")
                    .child(label("Loading metric names…", theme::BODY, false))
                    .into_any_element(),
            );
        } else if let Some(err) = &self.catalog_error {
            catalog_rows.push(
                settings_row("cat-error")
                    .child(caption_text(err.clone()))
                    .into_any_element(),
            );
        }
        for name in self.catalog.iter().take(12) {
            let query = name.clone();
            let label_text = name.clone();
            catalog_rows.push(
                div()
                    .id(SharedString::from(format!("cat-{name}")))
                    .px(px(GROUP_PAD))
                    .min_h(px(ROW_H))
                    .flex()
                    .items_center()
                    .hover(|s| s.bg(theme::FILL_TERTIARY))
                    .cursor(CursorStyle::PointingHand)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        let query = query.clone();
                        let label_text = label_text.clone();
                        SettingsView::persist_observe(|s| {
                            let _ =
                                nook_core::observe::pin_metric(&mut s.observe, &label_text, &query);
                        });
                        cx.notify();
                    }))
                    .child(label(name.clone(), theme::BODY, true))
                    .into_any_element(),
            );
        }

        let url_placeholder = self.url_draft.is_empty();
        let url_text = if url_placeholder {
            nook_core::observe::DEFAULT_OBSERVE_URL
        } else {
            self.url_draft.as_str()
        };
        let token_placeholder = self.token_draft.is_empty();
        let token_text = if token_placeholder {
            "Bearer token for /admin/metrics".to_string()
        } else {
            token_text(&self.token_draft, self.token_revealed)
        };
        let query_placeholder = self.query_draft.is_empty();
        let query_text = if query_placeholder {
            "total_requests, 5xx, or PromQL"
        } else {
            self.query_draft.as_str()
        };

        let mut range = segmented_group();
        for option in nook_core::observe::ObserveRange::all() {
            let selected = settings.observe.range == option;
            range = range.child(segment(option.label(), selected, cx, move |_, _, cx| {
                SettingsView::persist_observe(|s| {
                    nook_core::observe::set_range(&mut s.observe, option);
                });
                cx.notify();
            }));
        }

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(section(
                "Source",
                settings_group(vec![
                    field_row(
                        "prom-url",
                        "URL",
                        url_text,
                        url_placeholder,
                        url_focused,
                        &self.url_focus,
                        cx,
                        |this, event, cx| {
                            let persist = SettingsView::apply_key(&mut this.url_draft, event, cx)
                                || event.keystroke.key == "enter";
                            if persist {
                                this.persist_url();
                                cx.notify();
                            }
                        },
                    )
                    .into_any_element(),
                    field_row(
                        "prom-token",
                        "Token",
                        &token_text,
                        token_placeholder,
                        token_focused,
                        &self.token_focus,
                        cx,
                        |this, event, cx| {
                            let persist = SettingsView::apply_key(&mut this.token_draft, event, cx)
                                || event.keystroke.key == "enter";
                            if persist {
                                this.persist_token();
                                cx.notify();
                            }
                        },
                    )
                    .into_any_element(),
                    settings_row("observe-actions")
                        .child(div().flex_1())
                        .child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .child(push_button("paste-url", "Paste URL", cx, |this, _, cx| {
                                    if let Some(text) =
                                        cx.read_from_clipboard().and_then(|item| item.text())
                                    {
                                        this.url_draft = text.trim().to_string();
                                        this.persist_url();
                                        cx.notify();
                                    }
                                }))
                                .child(push_button(
                                    "toggle-token",
                                    if self.token_revealed {
                                        "Hide Token"
                                    } else {
                                        "Show Token"
                                    },
                                    cx,
                                    |this, _, cx| {
                                        this.token_revealed = !this.token_revealed;
                                        cx.notify();
                                    },
                                )),
                        )
                        .into_any_element(),
                ]),
                Some("Defaults to the warmUP API. Prometheus still works if you point at a Prom host."),
            ))
            .child(section(
                "Lookback",
                settings_group(vec![settings_row("observe-range")
                    .child(label("Range", theme::BODY, true))
                    .child(range)
                    .into_any_element()]),
                Some("Pins default to a line chart. Prometheus uses query_range; warmUP keeps 24 h of samples."),
            ))
            .child(section(
                "Pinned Metrics",
                settings_group(pinned_rows),
                Some("The compact island shows Observe only while a threshold you set is firing."),
            ))
            .child(section(
                "Pin Query",
                settings_group(vec![
                    field_row(
                        "prom-query",
                        "Query",
                        query_text,
                        query_placeholder,
                        query_focused,
                        &self.query_focus,
                        cx,
                        |this, event, cx| {
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
                        },
                    )
                    .into_any_element(),
                    settings_row("pin-query-action")
                        .child(div().flex_1())
                        .child(push_button("pin-query", "Pin Query", cx, |this, _, cx| {
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
                        .into_any_element(),
                ]),
                None::<SharedString>,
            ))
            .when(!catalog_rows.is_empty(), |d| {
                d.child(section(
                    "Catalog",
                    settings_group(catalog_rows),
                    Some("Choose a name to pin it."),
                ))
            })
    }

    fn persist_heading(&self) {
        let draft = self.heading_draft.trim().to_string();
        let heading = if draft.is_empty() { None } else { Some(draft) };
        nook_core::settings::tweak_app_settings(|s| {
            s.obsidian_capture_heading = heading;
        });
    }

    fn render_obsidian_settings(
        &self,
        settings: &AppSettings,
        heading_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let known = nook_core::obsidian::discover_vaults();
        let mut vault_rows = Vec::new();
        if known.is_empty() {
            vault_rows.push(
                settings_row("obs-known-empty")
                    .child(label("No vaults in Obsidian yet", theme::BODY, false))
                    .into_any_element(),
            );
        } else {
            for vault in &known {
                let path = vault.path.clone();
                let selected = settings.obsidian_vault.as_ref() == Some(&vault.path);
                let name = vault.name.clone();
                let open = vault.open;
                vault_rows.push(
                    div()
                        .id(SharedString::from(format!("obs-vault-{}", vault.id)))
                        .px(px(GROUP_PAD))
                        .min_h(px(ROW_H))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .when(selected, |d| d.bg(theme::FILL_TERTIARY))
                        .hover(|s| s.bg(theme::FILL_TERTIARY))
                        .cursor(CursorStyle::PointingHand)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            let path = path.clone();
                            nook_core::settings::tweak_app_settings(|s| {
                                s.obsidian_vault = Some(path);
                            });
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(label(name, theme::BODY, true))
                                .child(label(
                                    if open {
                                        "Open in Obsidian"
                                    } else {
                                        "Registered"
                                    },
                                    theme::SUBHEADLINE,
                                    false,
                                )),
                        )
                        .into_any_element(),
                );
            }
        }

        let heading_placeholder = self.heading_draft.is_empty();
        let heading_text = if heading_placeholder {
            "Inbox (optional)"
        } else {
            self.heading_draft.as_str()
        };

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(section(
                "Known Vaults",
                settings_group(vault_rows),
                Some("Read from Obsidian’s vault registry. Choose Folder if yours is not listed."),
            ))
            .child(section(
                "Capture",
                settings_group(vec![field_row(
                    "obs-heading",
                    "Heading",
                    heading_text,
                    heading_placeholder,
                    heading_focused,
                    &self.heading_focus,
                    cx,
                    |this, event, cx| {
                        let persist = SettingsView::apply_key(&mut this.heading_draft, event, cx)
                            || event.keystroke.key == "enter";
                        if persist {
                            this.persist_heading();
                            cx.notify();
                        }
                    },
                )
                .into_any_element()]),
                Some("Daily-note capture appends under this heading, or at the end of the file if empty."),
            ))
    }
}

fn hud_caption(settings: &AppSettings) -> SharedString {
    if settings.replace_system_hud {
        "Hides the system volume, brightness, caps-lock, and keyboard-backlight bezels while openNook is running. If OSDUIHelper is missing, the island HUD still appears beside the system bezel.".into()
    } else {
        "Volume and brightness keys still show the system bezel. Turn on replacement to hide it — that also hides caps-lock and keyboard-backlight bezels.".into()
    }
}

fn module_blurb(module: WidgetModule) -> SharedString {
    match module {
        WidgetModule::Calendar => "Shows events from your Calendar accounts. macOS asks for Calendar access the first time.".into(),
        WidgetModule::Notes => "Scratchpad on the island. Edit here or in the expanded card.".into(),
        WidgetModule::Observe => {
            "Pinned metrics on the compact island and the expanded card.".into()
        }
        WidgetModule::Music => {
            "Now Playing from MediaRemote. Optional extras are all opt-in: time-synced lyrics from LRCLIB (fetched at runtime, never bundled), browser artwork and Google favicons, Apple Music motion art (fails silent to static covers; the glow uses local artwork colors), and Up Next from the current Music playlist via local Automation. Spotify’s queue is not available on-device. The output picker lists CoreAudio devices; it cannot start AirPlay to a HomePod or Apple TV.".into()
        }
        WidgetModule::Files => {
            "Drop zone and tray live on the Tray tab. Drag onto AirDrop, or LocalSend when it is installed."
                .into()
        }
        WidgetModule::Timers => {
            "Island countdowns plus Apple Clock timers (read from mobiletimerd) — import the bundled Nook Clock shortcuts once to pause, resume, or cancel from the island. Includes a Pomodoro work/break cycle and an optional Focus shortcut.".into()
        }
        WidgetModule::Reminders => {
            "Shows your incomplete reminders. macOS asks for Reminders access the first time.".into()
        }
        WidgetModule::Speed => "Cloudflare (then OVH) download probe. Runs from the island card. Each test downloads about 25 MB. The OVH fallback can download up to 100 MB.".into(),
        WidgetModule::Agents => {
            "Working coding-agent sessions on the compact face and expanded card.".into()
        }
        WidgetModule::Mirror => "Shows your camera in the island. macOS asks for Camera access the first time you open the Mirror card; the camera runs only while that card is open.".into(),
        WidgetModule::Battery => {
            "Low-battery takeover on the compact face. Low Power Mode uses a one-time Shortcuts import, then falls back to an admin prompt.".into()
        }
        WidgetModule::Messages => {
            "Shows only when a message arrives. Reply to iMessage from the island; WhatsApp opens a prefilled chat. Full Disk Access is required to read messages.".into()
        }
        WidgetModule::Obsidian => {
            "Vault notes on the shelf. FSEvents keeps the list current; capture appends to today's daily note.".into()
        }
        WidgetModule::Weather => {
            "Current conditions and a short hourly strip from Open-Meteo. Manual city by default.".into()
        }
        WidgetModule::Vpn => {
            "Live utun/ipsec/ppp status. The compact face flashes on connect and disconnect; the card shows the session clock. Ignore listed interfaces to hide helpers that look like a VPN.".into()
        }
        WidgetModule::HighAlert => {
            "IOPM keep-awake. Timed chips expire in powerd — lid-close sleep is not prevented.".into()
        }
        WidgetModule::SysStats => {
            "Live CPU, memory, network, and disk capacity. Idle cost is zero — sampling starts on expand and stops on collapse.".into()
        }
        WidgetModule::Recorder => {
            "Record from the island. Transcription uses Apple's on-device Speech model when available; turn it off for long recordings.".into()
        }
        WidgetModule::Meeting => {
            "Mute and leave from the island. Zoom reads mute from the Meeting menu. Teams is a blind shortcut (the localhost API is gone). Meet focuses the tab unless you enable Apple Events JS.".into()
        }
        WidgetModule::Notifications => {
            "Captures other apps' banners via Accessibility. Optional usernoted backfill needs a manual Full Disk Access grant. Misses Focus / Do Not Disturb / 'None' styles.".into()
        }
    }
}

fn section(
    header: impl Into<SharedString>,
    group: impl IntoElement,
    footer: Option<impl Into<SharedString>>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(section_header(header))
        .child(group)
        .when_some(footer, |d, text| {
            d.child(div().px(px(2.)).pt(px(8.)).child(caption_text(text)))
        })
}

fn section_header(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px(px(2.))
        .pb(px(8.))
        .text_size(px(theme::SUBHEADLINE.size))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::secondary_label())
        .child(text.into())
}

fn caption_text(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(theme::SUBHEADLINE.size))
        .line_height(px(theme::SUBHEADLINE.leading))
        .text_color(theme::SECONDARY_LABEL)
        .child(text.into())
}

fn high_alert_rows(settings: &AppSettings, cx: &mut Context<SettingsView>) -> Vec<AnyElement> {
    let duration = settings.high_alert_default_duration_secs;
    let kind = settings.high_alert_kind;
    let battery = settings.low_battery_release_pct;
    vec![
        chip_row(
            "Default Duration",
            &[
                ("15m", duration == 15 * 60),
                ("30m", duration == 30 * 60),
                ("1h", duration == 60 * 60),
                ("Until Off", duration == 0),
            ],
            cx,
            |caption, s| {
                s.high_alert_default_duration_secs = match caption {
                    "15m" => 15 * 60,
                    "1h" => 60 * 60,
                    "Until Off" => 0,
                    _ => 30 * 60,
                };
            },
        )
        .into_any_element(),
        chip_row(
            "Keep Awake",
            &[
                ("Display", kind == HighAlertKind::Display),
                ("System", kind == HighAlertKind::System),
            ],
            cx,
            |caption, s| {
                s.high_alert_kind = if caption == "System" {
                    HighAlertKind::System
                } else {
                    HighAlertKind::Display
                };
            },
        )
        .into_any_element(),
        chip_row(
            "Release Below",
            &[
                ("Never", battery == 0),
                ("10%", battery == 10),
                ("20%", battery == 20),
            ],
            cx,
            |caption, s| {
                s.low_battery_release_pct = match caption {
                    "Never" => 0,
                    "20%" => 20,
                    _ => 10,
                };
            },
        )
        .into_any_element(),
    ]
}

fn pomodoro_rows(
    settings: &AppSettings,
    catalog: &[String],
    cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    let work = settings.pomodoro_work_secs;
    let brk = settings.pomodoro_break_secs;
    let long = settings.pomodoro_long_break_secs;
    let cycles = settings.pomodoro_cycles_per_long;
    let mut work_options = segmented_group().h_auto().flex_wrap();
    let mut break_options = segmented_group().h_auto().flex_wrap();
    let mut options = vec![None];
    options.extend(catalog.iter().cloned().map(Some));
    for saved in [
        &settings.focus_shortcut_work,
        &settings.focus_shortcut_break,
    ] {
        if saved.is_some() && !options.contains(saved) {
            options.push(saved.clone());
        }
    }
    for option in options {
        let caption = option.clone().unwrap_or_else(|| "None".into());
        let work_option = option.clone();
        work_options = work_options.child(segment(
            caption.clone(),
            settings.focus_shortcut_work == option,
            cx,
            move |_, _, cx| {
                nook_core::settings::tweak_app_settings(|s| {
                    s.focus_shortcut_work = work_option.clone()
                });
                cx.notify();
            },
        ));
        break_options = break_options.child(segment(
            caption,
            settings.focus_shortcut_break == option,
            cx,
            move |_, _, cx| {
                nook_core::settings::tweak_app_settings(|s| {
                    s.focus_shortcut_break = option.clone()
                });
                cx.notify();
            },
        ));
    }
    vec![
        chip_row(
            "Work",
            &[("25m", work == 25 * 60), ("50m", work == 50 * 60)],
            cx,
            |caption, s| {
                s.pomodoro_work_secs = if caption == "50m" { 50 * 60 } else { 25 * 60 };
            },
        )
        .into_any_element(),
        chip_row(
            "Break",
            &[("5m", brk == 5 * 60), ("10m", brk == 10 * 60)],
            cx,
            |caption, s| {
                s.pomodoro_break_secs = if caption == "10m" { 10 * 60 } else { 5 * 60 };
            },
        )
        .into_any_element(),
        chip_row(
            "Long Break",
            &[("15m", long == 15 * 60), ("20m", long == 20 * 60)],
            cx,
            |caption, s| {
                s.pomodoro_long_break_secs = if caption == "20m" { 20 * 60 } else { 15 * 60 };
            },
        )
        .into_any_element(),
        chip_row(
            "Cycles",
            &[("3", cycles == 3), ("4", cycles == 4)],
            cx,
            |caption, s| {
                s.pomodoro_cycles_per_long = if caption == "3" { 3 } else { 4 };
            },
        )
        .into_any_element(),
        toggle_row(
            "Auto-Advance Phases",
            settings.pomodoro_auto_advance,
            cx,
            |s| s.pomodoro_auto_advance = !s.pomodoro_auto_advance,
        )
        .into_any_element(),
        toggle_row(
            "Keep Awake on Work",
            settings.pomodoro_keep_awake,
            cx,
            |s| s.pomodoro_keep_awake = !s.pomodoro_keep_awake,
        )
        .into_any_element(),
        settings_row("focus-work")
            .flex_col()
            .items_start()
            .py(px(6.))
            .child(label("Work Shortcut", theme::BODY, true))
            .child(work_options)
            .into_any_element(),
        settings_row("focus-break")
            .flex_col()
            .items_start()
            .py(px(6.))
            .child(label("Break Shortcut", theme::BODY, true))
            .child(break_options)
            .into_any_element(),
    ]
}

fn chip_row(
    title: &'static str,
    chips: &[(&'static str, bool)],
    cx: &mut Context<SettingsView>,
    tweak: impl Fn(&'static str, &mut AppSettings) + Copy + 'static,
) -> impl IntoElement {
    let mut group = segmented_group();
    for (caption, selected) in chips.iter().copied() {
        group = group.child(segment(caption, selected, cx, move |_, _, cx| {
            nook_core::settings::tweak_app_settings(|s| tweak(caption, s));
            cx.notify();
        }));
    }
    settings_row(title)
        .child(label(title, theme::BODY, true))
        .child(group)
}

fn widget_badge_color(module: WidgetModule) -> Rgba {
    let rgb = match module {
        WidgetModule::Music | WidgetModule::SysStats => 0xFF375F,
        WidgetModule::Calendar | WidgetModule::Reminders | WidgetModule::Recorder => 0xFF453A,
        WidgetModule::Speed | WidgetModule::Battery | WidgetModule::Messages => 0x30D158,
        WidgetModule::Mirror | WidgetModule::Observe => 0x64D2FF,
        WidgetModule::Agents | WidgetModule::Meeting => 0xBF5AF2,
        WidgetModule::Timers | WidgetModule::HighAlert | WidgetModule::Notifications => TILE_ORANGE,
        WidgetModule::Notes => 0xFFD60A,
        WidgetModule::Weather | WidgetModule::Vpn => TILE_BLUE,
        WidgetModule::Files => 0x5E5CE6,
        WidgetModule::Obsidian => 0x7C5CFF,
    };
    theme::rgba_from_u32(rgb, 1.0)
}

fn widget_badge(module: WidgetModule) -> impl IntoElement {
    div()
        .size(px(WIDGET_BADGE))
        .rounded(px(WIDGET_BADGE_RADIUS))
        .bg(widget_badge_color(module))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(lucide_color(module.icon(), WIDGET_BADGE_ICON, theme::LABEL))
}

fn widget_caption(module: WidgetModule, settings: &AppSettings) -> SharedString {
    let trailing = if module.is_experimental() {
        "Experimental".to_string()
    } else if !module.occupies_nook_cells() {
        "Tray tab".to_string()
    } else {
        match settings.cells_for(module) {
            1 => "1 slot".to_string(),
            cells => format!("{cells} slots"),
        }
    };
    format!("{} · {trailing}", module.tagline()).into()
}

fn widget_list_block(
    title: &'static str,
    trailing: String,
    indent: f32,
    rows: Vec<AnyElement>,
) -> impl IntoElement {
    let mut group = div()
        .flex()
        .flex_col()
        .rounded(px(theme::INNER_RADIUS))
        .border_1()
        .border_color(LIST_BORDER)
        .bg(theme::GROUPED_BG)
        .overflow_hidden();
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            group = group.child(div().h(px(1.)).ml(px(indent)).bg(hairline()));
        }
        group = group.child(row);
    }
    div()
        .flex()
        .flex_col()
        .w_full()
        .child(group_header(title, trailing))
        .child(group)
}

/// Small semibold group title with a trailing count, as above each widget list.
fn group_header(title: impl Into<SharedString>, trailing: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .px(px(2.))
        .pb(px(8.))
        .child(
            div()
                .text_size(px(theme::SUBHEADLINE.size))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::secondary_label())
                .child(title.into()),
        )
        .child(div().flex_1())
        .child(
            div()
                .text_size(px(theme::SUBHEADLINE.size))
                .text_color(theme::secondary_label())
                .child(trailing.into()),
        )
}

fn settings_group(rows: Vec<AnyElement>) -> impl IntoElement {
    let mut group = div()
        .flex()
        .flex_col()
        .rounded(px(theme::INNER_RADIUS))
        .border_1()
        .border_color(LIST_BORDER)
        .bg(theme::GROUPED_BG)
        .overflow_hidden();
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            group = group.child(div().h(px(1.)).ml(px(GROUP_PAD)).bg(hairline()));
        }
        group = group.child(row);
    }
    group
}

fn settings_row(id: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px(px(GROUP_PAD))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.))
        .min_h(px(ROW_H))
}

fn empty_hint(text: &'static str) -> impl IntoElement {
    div()
        .px(px(GROUP_PAD))
        .min_h(px(ROW_H))
        .flex()
        .items_center()
        .child(label(text, theme::SUBHEADLINE, false))
}

fn action_row(
    id: &'static str,
    title: &'static str,
    caption: impl Into<SharedString>,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    settings_row(id)
        .child(label(title, theme::BODY, true))
        .child(push_button(
            SharedString::from(format!("{id}-btn")),
            caption,
            cx,
            on_click,
        ))
}

fn threshold_row(value: u8, cx: &mut Context<SettingsView>) -> impl IntoElement {
    let value = nook_core::power::clamp_alert_threshold(value);
    settings_row("battery-threshold")
        .child(label(format!("Alert Below {value}%"), theme::BODY, true))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(stepper_btn("thr-dec", "−", cx, move |_, _, cx| {
                    nook_core::settings::tweak_app_settings(|s| {
                        s.battery_alert_threshold = nook_core::power::clamp_alert_threshold(
                            s.battery_alert_threshold.saturating_sub(5),
                        );
                    });
                    cx.notify();
                }))
                .child(div().w(px(44.)).flex().justify_center().child(label(
                    format!("{value}%"),
                    theme::BODY,
                    true,
                )))
                .child(stepper_btn("thr-inc", "+", cx, move |_, _, cx| {
                    nook_core::settings::tweak_app_settings(|s| {
                        s.battery_alert_threshold = nook_core::power::clamp_alert_threshold(
                            s.battery_alert_threshold.saturating_add(5),
                        );
                    });
                    cx.notify();
                })),
        )
}

fn stepper_btn(
    id: &'static str,
    caption: &'static str,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .size(px(theme::HIT_MIN))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::CONTROL_RADIUS))
        .bg(theme::FILL)
        .hover(|s| s.bg(theme::FILL_SECONDARY))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(label(caption, theme::BODY, true))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.pending_destructive = None;
            cx.stop_propagation();
            on_click(this, window, cx);
        }))
}
fn permission_row(
    title: &'static str,
    status: nook_core::eventtap::PermissionStatus,
) -> impl IntoElement {
    let (text, color) = match status {
        nook_core::eventtap::PermissionStatus::Granted => ("Granted", theme::SUCCESS),
        nook_core::eventtap::PermissionStatus::Denied => ("Not Granted", theme::DESTRUCTIVE),
        nook_core::eventtap::PermissionStatus::Unsupported => {
            ("Not Available", theme::TERTIARY_LABEL)
        }
    };
    settings_row(title)
        .child(label(title, theme::BODY, true))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().size(px(7.)).rounded_full().bg(color))
                .child(
                    div()
                        .text_size(px(theme::SUBHEADLINE.size))
                        .text_color(color)
                        .child(text),
                ),
        )
}

fn toggle_row(
    label_text: &'static str,
    on: bool,
    cx: &mut Context<SettingsView>,
    tweak: impl Fn(&mut AppSettings) + 'static,
) -> impl IntoElement {
    settings_row(label_text)
        .tab_index(0)
        .focus(|s| s.border_1().border_color(theme::accent()))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.pending_destructive = None;
            let mut s = nook_core::settings::get_app_settings();
            tweak(&mut s);
            nook_core::settings::update_app_settings(s);
            cx.notify();
        }))
        .child(label(label_text, theme::BODY, true))
        .child(toggle_knob(on))
}

fn module_toggle(
    on: bool,
    module: WidgetModule,
    settings: &AppSettings,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let can = on || settings.can_enable(module);
    div()
        .id(SharedString::from(format!("tog-{}", module.name())))
        .min_h(px(theme::HIT_MIN))
        .flex()
        .items_center()
        .opacity(if can { 1.0 } else { 0.4 })
        .cursor(if can {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Arrow
        })
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.stop_propagation();
            if !can && !on {
                return;
            }
            let mut s = nook_core::settings::get_app_settings();
            let was_enabled = s.show_notifications;
            module.set_enabled(&mut s);
            let request_notifications =
                module == WidgetModule::Notifications && !was_enabled && s.show_notifications;
            nook_core::settings::update_app_settings(s);
            if request_notifications {
                nook_core::notifications::ax_trusted(true);
            }
            cx.notify();
        }))
        .child(toggle_knob(on))
}

fn color_swatch(
    swatch: IslandSwatch,
    selected: Option<u32>,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let on = swatch.rgb == selected;
    let fill = theme::island_fill(swatch.rgb);
    let name = swatch.name;
    div()
        .id(SharedString::from(format!("swatch-{name}")))
        .size(px(theme::HIT_MIN))
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.stop_propagation();
            nook_core::settings::tweak_app_settings(|s| s.island_color = swatch.rgb);
            cx.notify();
        }))
        .child(
            div()
                .size(px(16.))
                .rounded_full()
                .bg(fill)
                .when(on, |d| d.border_2().border_color(theme::LABEL))
                .when(!on, |d| d.border_1().border_color(theme::SEPARATOR)),
        )
}

fn toggle_knob(on: bool) -> impl IntoElement {
    div()
        .w(px(38.))
        .h(px(22.))
        .rounded_full()
        .bg(if on {
            theme::accent()
        } else {
            theme::FILL_SECONDARY
        })
        .flex()
        .items_center()
        .when(on, |d| d.justify_end())
        .px(px(2.))
        .child(div().size(px(18.)).rounded_full().bg(theme::LABEL))
}

fn segmented_group() -> gpui::Div {
    div()
        .h(px(theme::HIT_MIN))
        .p(px(2.))
        .rounded(px(theme::CONTROL_RADIUS))
        .bg(theme::FILL)
        .flex()
        .items_center()
}

fn segment(
    caption: impl Into<SharedString>,
    selected: bool,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    let caption = caption.into();
    div()
        .id(SharedString::from(format!("seg-{caption}")))
        .h(px(24.))
        .px(px(10.))
        .rounded(px(theme::CONTROL_RADIUS))
        .flex()
        .items_center()
        .justify_center()
        .when(selected, |d| d.bg(theme::FILL_SECONDARY))
        .hover(|s| {
            if selected {
                s
            } else {
                s.bg(theme::FILL_TERTIARY)
            }
        })
        .active(|s| s.opacity(0.85))
        .tab_index(0)
        .focus(|s| s.border_1().border_color(theme::accent()))
        .cursor(CursorStyle::PointingHand)
        .child(
            div()
                .text_size(px(theme::CALLOUT.size))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if selected {
                    theme::LABEL
                } else {
                    theme::SECONDARY_LABEL
                })
                .child(caption),
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            this.pending_destructive = None;
            cx.stop_propagation();
            on_click(this, window, cx);
        }))
}

fn confirm_destructive(pending: &mut Option<SharedString>, id: SharedString) -> bool {
    if pending.as_ref() == Some(&id) {
        *pending = None;
        true
    } else {
        *pending = Some(id);
        false
    }
}

fn push_button(
    id: impl Into<SharedString>,
    caption: impl Into<SharedString>,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    let id = id.into();
    let destructive = id.as_ref() == "island-reset-btn"
        || id.as_ref() == "obsidian-clear-btn"
        || id.as_ref() == "reset-all-btn"
        || id.as_ref() == "term-clear-history-btn"
        || id.as_ref() == "quit-btn"
        || id.starts_with("unpin-");
    let caption = caption.into();
    let pressed_id = id.clone();
    div()
        .id(id.clone())
        .h(px(theme::HIT_MIN))
        .px(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::CONTROL_RADIUS))
        .bg(theme::FILL)
        .hover(|s| s.bg(theme::FILL_SECONDARY))
        .active(|s| s.opacity(0.85))
        .tab_index(0)
        .focus(|s| s.border_1().border_color(theme::accent()))
        .cursor(CursorStyle::PointingHand)
        .child(label(caption, theme::CALLOUT, true))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                if this.pending_destructive.as_ref() != Some(&pressed_id) {
                    this.pending_destructive = None;
                    cx.notify();
                }
                cx.stop_propagation();
            }),
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            if destructive && !confirm_destructive(&mut this.pending_destructive, id.clone()) {
                cx.notify();
                return;
            }
            this.pending_destructive = None;
            on_click(this, window, cx);
        }))
}

fn field_row(
    id: &'static str,
    title: &'static str,
    value: &str,
    placeholder: bool,
    focused: bool,
    focus: &FocusHandle,
    cx: &mut Context<SettingsView>,
    on_key: impl Fn(&mut SettingsView, &KeyDownEvent, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    let focus = focus.clone();
    settings_row(id)
        .child(label(title, theme::BODY, true).w(px(92.)))
        .child(
            div()
                .id(SharedString::from(format!("{id}-field")))
                .track_focus(&focus)
                .flex_1()
                .min_w(px(0.))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(theme::CONTROL_RADIUS))
                .bg(theme::FILL)
                .when(focused, |d| d.border_1().border_color(theme::accent()))
                .flex()
                .items_center()
                .cursor(CursorStyle::IBeam)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.pending_destructive = None;
                        cx.stop_propagation();
                        window.focus(&focus);
                        cx.notify();
                    }),
                )
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                    on_key(this, event, cx);
                }))
                .child(
                    div()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_color(if placeholder {
                            theme::TERTIARY_LABEL
                        } else {
                            theme::LABEL
                        })
                        .text_size(px(theme::BODY.size))
                        .child(SharedString::from(value.to_string())),
                )
                .when(focused, |d| {
                    d.child(
                        div()
                            .w(px(1.))
                            .h(px(14.))
                            .flex_shrink_0()
                            .bg(theme::accent()),
                    )
                }),
        )
}

/// Map a click on the gradient track to `0..=1`, using the knob's travel
/// so the centre of the knob is the value.
fn gradient_ratio(x: f32, origin: f32, width: f32) -> f32 {
    let travel = (width - GRADIENT_KNOB).max(1.0);
    ((x - origin - GRADIENT_KNOB * 0.5) / travel).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_track_ratio_follows_the_knob() {
        let width = GRADIENT_TRACK_W;
        let half = GRADIENT_KNOB * 0.5;
        assert_eq!(gradient_ratio(half, 0.0, width), 0.0);
        let mid = half + (width - GRADIENT_KNOB) * 0.5;
        assert!((gradient_ratio(mid, 0.0, width) - 0.5).abs() < 1e-4);
        assert_eq!(gradient_ratio(-20.0, 0.0, width), 0.0);
        assert_eq!(gradient_ratio(width + 40.0, 0.0, width), 1.0);
        assert!((gradient_ratio(half + 10.0, 10.0, width)).abs() < 1e-4);
    }

    #[test]
    fn destructive_confirmation_requires_the_same_row_twice() {
        let mut pending = None;
        assert!(!confirm_destructive(&mut pending, "reset".into()));
        assert!(!confirm_destructive(&mut pending, "remove".into()));
        assert!(confirm_destructive(&mut pending, "remove".into()));
        assert!(pending.is_none());
        assert!(!confirm_destructive(&mut pending, "reset".into()));
        pending = None; // Another press cancels the pending action.
        assert!(!confirm_destructive(&mut pending, "reset".into()));
    }

    #[test]
    fn metrics_token_is_masked_by_default() {
        assert_eq!(token_text("secret-value", false), "••••••••");
        assert_eq!(token_text("secret-value", true), "secret-value");
    }

    #[test]
    fn settings_window_is_landscape() {
        let (w, h) = SETTINGS_SIZE;
        assert_eq!((w, h), (820.0, 560.0));
        assert!(w > h, "default size stays landscape");
        let (min_w, min_h) = SETTINGS_MIN;
        assert!(min_w > min_h, "min size stays landscape");
        assert!(min_w >= 680.0 && min_h >= 480.0);
        assert!(w > SIDEBAR_W + 400.0, "pane has room beside the sidebar");
        assert_eq!(SIDEBAR_W, 212.0);
        assert_eq!(SEARCH_H, 24.0);
        assert!((SIDEBAR_W - SIDEBAR_PAD * 2.0 - 192.0).abs() < 0.05);
    }

    #[test]
    fn weather_subtitle_uses_the_saved_city() {
        let mut settings = AppSettings::default();
        assert_eq!(
            WidgetModule::Weather.subtitle(&settings).as_ref(),
            "Open-Meteo"
        );
        settings.weather.location = nook_core::weather::WeatherLocationMode::System {
            name: "Oslo".into(),
            lat: 59.91,
            lon: 10.75,
        };
        assert_eq!(WidgetModule::Weather.subtitle(&settings).as_ref(), "Oslo");
    }

    #[test]
    fn calendar_subtitle_uses_the_week_strip_count() {
        let settings = AppSettings::default();
        assert_eq!(
            WidgetModule::Calendar.subtitle(&settings).as_ref(),
            "7 days"
        );
    }

    #[test]
    fn battery_subtitle_shows_the_alert_threshold() {
        let mut settings = AppSettings::default();
        assert_eq!(
            WidgetModule::Battery.subtitle(&settings).as_ref(),
            "Alert Below 20%"
        );
        settings.battery_alert_threshold = 5;
        assert_eq!(
            WidgetModule::Battery.subtitle(&settings).as_ref(),
            "Alert Below 5%"
        );
    }

    #[test]
    fn timers_subtitle_mentions_clock_when_sync_is_on() {
        let mut settings = AppSettings::default();
        assert_eq!(
            WidgetModule::Timers.subtitle(&settings).as_ref(),
            "Island + Clock"
        );
        settings.sync_clock_timers = false;
        assert_eq!(
            WidgetModule::Timers.subtitle(&settings).as_ref(),
            "Countdown"
        );
    }

    #[test]
    fn sysstats_subtitle_counts_enabled_readouts() {
        let mut settings = AppSettings::default();
        assert_eq!(
            WidgetModule::SysStats.subtitle(&settings).as_ref(),
            "4 readouts"
        );
        settings.sysstats.show_disk = false;
        settings.sysstats.show_net = false;
        settings.sysstats.show_mem = false;
        assert_eq!(
            WidgetModule::SysStats.subtitle(&settings).as_ref(),
            "1 readout"
        );
        settings.sysstats.show_cpu = false;
        assert_eq!(
            WidgetModule::SysStats.subtitle(&settings).as_ref(),
            "Hidden"
        );
    }

    #[test]
    fn notifications_subtitle_is_honest_when_off() {
        let settings = AppSettings::default();
        assert!(!settings.show_notifications);
        assert_eq!(
            WidgetModule::Notifications.subtitle(&settings).as_ref(),
            "Off"
        );
    }

    #[test]
    fn observe_subtitle_counts_pinned_metrics() {
        assert_eq!(observe_subtitle(0).as_ref(), "Prometheus");
        assert_eq!(observe_subtitle(1).as_ref(), "1 metric");
        assert_eq!(observe_subtitle(5).as_ref(), "5 metrics");
    }

    #[test]
    fn vpn_subtitle_follows_the_timer_toggle() {
        assert_eq!(vpn_subtitle(true).as_ref(), "Session timer");
        assert_eq!(vpn_subtitle(false).as_ref(), "Status");
    }

    #[test]
    fn module_list_is_our_widgets_only() {
        let names: Vec<_> = WidgetModule::ALL.iter().map(|m| m.name()).collect();
        assert_eq!(
            names,
            [
                "Calendar",
                "Music",
                "Files",
                "Notes",
                "Observe",
                "Timers",
                "Reminders",
                "Speed",
                "Agents",
                "Mirror",
                "Battery",
                "Messages",
                "Obsidian",
                "Weather",
                "VPN",
                "Alert",
                "Stats",
                "Voice",
                "Meetings",
                "Notify",
            ]
        );
        assert!(!names
            .iter()
            .any(|n| n.contains("Shortcuts") || n.contains("Tencent") || n.contains("License")));
    }

    #[test]
    fn widgets_chrome_matches_mockup() {
        assert_eq!(WIDGET_ROW_H, 46.0);
        assert_eq!(WIDGET_BADGE, 22.0);
        assert_eq!(WIDGET_GRIP, 13.0);
        assert_eq!(CATEGORY_H, 30.0);
        assert_eq!(PANE_TITLE, 22.0);
        assert_eq!(SettingsCategory::Widgets.title(), "Widgets");
        assert_eq!(SettingsCategory::Appearance.icon(), "sun-moon");
        let settings = AppSettings::default();
        let caption = widget_caption(WidgetModule::Calendar, &settings);
        assert!(caption.starts_with("Next event · "), "{caption}");
        assert!(caption.contains("slot"), "{caption}");
        let experimental = widget_caption(WidgetModule::Vpn, &settings);
        assert!(experimental.ends_with("· Experimental"), "{experimental}");
    }

    #[test]
    fn nav_enums_round_trip() {
        assert_eq!(SettingsCategory::from_u8(0), SettingsCategory::General);
        assert_eq!(SettingsCategory::from_u8(1), SettingsCategory::Widgets);
        assert_eq!(SettingsCategory::from_u8(2), SettingsCategory::Appearance);
        assert_eq!(SettingsCategory::from_u8(3), SettingsCategory::Shortcuts);
        assert_eq!(SettingsCategory::from_u8(4), SettingsCategory::Privacy);
        assert_eq!(SettingsCategory::from_u8(5), SettingsCategory::Updates);
        assert_eq!(SettingsCategory::from_u8(6), SettingsCategory::About);
        assert_eq!(SettingsCategory::from_u8(255), SettingsCategory::General);
        assert_eq!(SettingsCategory::from_u8(99), SettingsCategory::General);
        assert_eq!(WidgetModule::from_u8(0), WidgetModule::Calendar);
        assert_eq!(WidgetModule::from_u8(99), WidgetModule::Calendar);
    }

    #[test]
    fn hud_caption_explains_bezel_suppression() {
        let off = AppSettings::default();
        assert!(!off.replace_system_hud);
        assert!(hud_caption(&off).as_ref().contains("system bezel"));
        let on = AppSettings {
            replace_system_hud: true,
            ..Default::default()
        };
        assert!(hud_caption(&on).as_ref().contains("caps-lock"));
    }

    #[test]
    fn remaining_cells_is_row_remainder() {
        let mut settings = AppSettings::default();
        assert_eq!(settings.nook_row_count(), 1);
        assert_eq!(settings.remaining_cells(), 6);
        settings.show_battery = true;
        settings.weather.enabled = true;
        assert_eq!(settings.nook_row_count(), 1);
        assert_eq!(settings.used_cells(), 17);
        assert_eq!(settings.remaining_cells(), 0);
    }
}
