//! Settings window — macOS split view: sidebar + grouped inset lists.

use super::ui::label;
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    canvas, div, img, prelude::*, px, rgb, rgba, AnyElement, Bounds, Context, CursorStyle, Entity,
    FocusHandle, FontWeight, Image, KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent,
    ObjectFit, Pixels, Rgba, SharedString, Subscription, Window,
};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use nook_core::high_alert::HighAlertKind;
use nook_core::settings::{AppSettings, IslandSwatch, WidgetModule, ISLAND_SWATCHES};
use nook_core::share::LinkBackendKind;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Default settings window. Sidebar + grouped pane, landscape so the widget
/// list and island preview sit side by side with the navigation.
pub(super) const SETTINGS_SIZE: (f32, f32) = (780.0, 560.0);
pub(super) const SETTINGS_MIN: (f32, f32) = (680.0, 480.0);

const SIDEBAR_W: f32 = 180.0;
/// Room for traffic lights on a transparent titlebar.
const TITLEBAR_INSET: f32 = 52.0;
const GROUP_PAD: f32 = 12.0;
const ROW_H: f32 = 36.0;

/// Last surface the user had open. Survives closing the window.
static LAST_CATEGORY: AtomicU8 = AtomicU8::new(SettingsCategory::Widgets as u8);
static LAST_MODULE: AtomicU8 = AtomicU8::new(WidgetModule::Calendar as u8);

fn token_text(token: &str, revealed: bool) -> String {
    if revealed {
        token.to_string()
    } else {
        "••••••••".into()
    }
}

fn hairline() -> Rgba {
    rgba(0xffffff14)
}

fn desktop_wallpaper_image() -> Option<std::sync::Arc<Image>> {
    static WALLPAPER: std::sync::OnceLock<Option<std::sync::Arc<Image>>> =
        std::sync::OnceLock::new();
    WALLPAPER
        .get_or_init(|| {
            crate::platform::desktop_wallpaper_png()
                .map(|png| std::sync::Arc::new(Image::from_bytes(gpui::ImageFormat::Png, png)))
        })
        .clone()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum SettingsCategory {
    General = 0,
    Widgets = 1,
    Keyboard = 2,
    Scrolling = 3,
}

impl SettingsCategory {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Widgets,
            2 => Self::Keyboard,
            3 => Self::Scrolling,
            _ => Self::General,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Widgets => "Widgets",
            Self::Keyboard => "Keyboard",
            Self::Scrolling => "Scrolling",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::General => "settings",
            Self::Widgets => "layout-grid",
            Self::Keyboard => "keyboard",
            Self::Scrolling => "mouse",
        }
    }
}

trait WidgetModuleExt {
    fn name(self) -> &'static str;
    fn icon(self) -> &'static str;
    fn subtitle(self, settings: &AppSettings) -> SharedString;
    fn enabled(self, settings: &AppSettings) -> bool;
    fn set_enabled(self, settings: &mut AppSettings);
    fn preview_label(self) -> &'static str;
}

#[derive(Clone, Copy)]
struct WidgetDrag(WidgetModule);

impl gpui::Render for WidgetDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(220.))
            .h(px(ROW_H))
            .px(px(10.))
            .rounded(px(8.))
            .bg(theme::GROUPED_BG)
            .shadow_md()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(lucide_color("grip-vertical", 14.0, theme::TERTIARY_LABEL))
            .child(lucide_color(self.0.icon(), 14.0, theme::LABEL))
            .child(
                div()
                    .text_size(px(theme::BODY.size))
                    .font_weight(FontWeight::MEDIUM)
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
            Self::Speed => "Speed Test",
            Self::Agents => "Agents",
            Self::Mirror => "Mirror",
            Self::Battery => "Battery",
            Self::Messages => "Messages",
            Self::Obsidian => "Obsidian",
            Self::Mixer => "Mixer",
            Self::Weather => "Weather",
            Self::Vpn => "VPN",
            Self::HighAlert => "High Alert",
            Self::SysStats => "Stats",
            Self::Recorder => "Voice",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Calendar => "calendar",
            Self::Music => "music",
            Self::Files => "files",
            Self::Notes => "notebook",
            Self::Observe => "activity",
            Self::Timers => "clock",
            Self::Reminders => "list-checks",
            Self::Speed => "gauge",
            Self::Agents => "bot",
            Self::Mirror => "webcam",
            Self::Battery => "battery",
            Self::Messages => "message-circle",
            Self::Obsidian => "book",
            Self::Mixer => "volume-2",
            Self::Weather => "cloud-sun",
            Self::Vpn => "shield",
            Self::HighAlert => "sun",
            Self::SysStats => "activity",
            Self::Recorder => "mic",
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
                "Alert at {}%",
                nook_core::power::clamp_alert_threshold(settings.battery_alert_threshold)
            )
            .into(),
            Self::Messages => "iMessage".into(),
            Self::Obsidian => settings
                .obsidian_vault
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| SharedString::from(name.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "No vault".into()),
            Self::Mixer => "Per-app volume".into(),
            Self::Weather => weather_subtitle(settings),
            Self::Vpn => vpn_subtitle(settings.vpn_show_timer),
            Self::HighAlert => "Keep awake".into(),
            Self::SysStats => sysstats_subtitle(settings),
            Self::Recorder => {
                if settings.recorder_transcribe {
                    "Live transcript".into()
                } else {
                    "Record only".into()
                }
            }
        }
    }

    fn enabled(self, settings: &AppSettings) -> bool {
        settings.is_enabled(self)
    }

    fn set_enabled(self, settings: &mut AppSettings) {
        settings.toggle_enabled(self);
    }

    fn preview_label(self) -> &'static str {
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
            Self::Mixer => "Mixer",
            Self::Weather => "Weather",
            Self::Vpn => "VPN",
            Self::HighAlert => "Alert",
            Self::SysStats => "Stats",
            Self::Recorder => "Voice",
        }
    }
}

fn share_field_a(settings: &AppSettings) -> String {
    match settings.share.link_backend {
        LinkBackendKind::WebDav => settings.share.webdav_url.clone(),
        LinkBackendKind::S3 => settings.share.s3_bucket.clone(),
        LinkBackendKind::ZeroXZero => String::new(),
    }
}

fn share_field_b(settings: &AppSettings) -> String {
    match settings.share.link_backend {
        LinkBackendKind::WebDav => settings.share.webdav_username.clone(),
        LinkBackendKind::S3 => settings.share.s3_access_key.clone(),
        LinkBackendKind::ZeroXZero => String::new(),
    }
}

fn share_field_c(settings: &AppSettings) -> String {
    match settings.share.link_backend {
        LinkBackendKind::WebDav => settings.share.webdav_password.clone(),
        LinkBackendKind::S3 => settings.share.s3_secret_key.clone(),
        LinkBackendKind::ZeroXZero => String::new(),
    }
}

fn share_blurb(backend: LinkBackendKind, receive: bool) -> SharedString {
    let host = match backend {
        LinkBackendKind::ZeroXZero => {
            "0x0.st is a public community host (512 MiB, 30–365 day retention). Files are public-by-URL."
        }
        LinkBackendKind::WebDav => {
            "WebDAV PUT needs a base URL that is already publicly readable. Nextcloud share links need its OCS API — not this mode."
        }
        LinkBackendKind::S3 => {
            "S3 PUT uses SigV4. Permanent links need a public bucket or CloudFront; otherwise treat the object URL as short-lived."
        }
    };
    if receive {
        format!("{host} Receive is saved but this release does not open a listener.")
            .into()
    } else {
        format!("{host} LocalSend is send-only until you turn receive on (listener ships later).")
            .into()
fn weather_subtitle(settings: &AppSettings) -> SharedString {
    let name = settings.weather.location.name();
    if name.is_empty() {
        "Open-Meteo".into()
    } else {
        name.to_string().into()
fn vpn_subtitle(show_timer: bool) -> SharedString {
    if show_timer {
        "Session timer".into()
    } else {
        "Status".into()
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

fn observe_subtitle(pinned: usize) -> SharedString {
    match pinned {
        0 => "Prometheus".into(),
        1 => "1 metric".into(),
        n => format!("{n} metrics").into(),
    }
}

fn create_width_slider(
    module: WidgetModule,
    settings: &AppSettings,
    cx: &mut Context<SettingsView>,
) -> (Entity<SliderState>, Subscription) {
    let min = module.min_cells();
    let max = settings.max_cells_for(module).max(min);
    let value = settings.cells_for(module).clamp(min, max);
    let slider = cx.new(|_| {
        SliderState::new()
            .min(min as f32)
            .max(max as f32)
            .step(1.0)
            .default_value(value as f32)
    });
    let subscription = cx.subscribe(&slider, move |_, _, event: &SliderEvent, cx| {
        let SliderEvent::Change(value) = event;
        nook_core::settings::tweak_app_settings(|settings| {
            settings.set_cells(module, value.start().round() as u8)
        });
        cx.notify();
    });
    (slider, subscription)
}

fn create_float_slider(
    min: f32,
    max: f32,
    step: f32,
    value: f32,
    cx: &mut Context<SettingsView>,
    write: impl Fn(f32) + 'static,
) -> (Entity<SliderState>, Subscription) {
    let slider = cx.new(|_| {
        SliderState::new()
            .min(min)
            .max(max)
            .step(step)
            .default_value(value)
    });
    let subscription = cx.subscribe(&slider, move |_, _, event: &SliderEvent, cx| {
        let SliderEvent::Change(value) = event;
        write(value.start());
        cx.notify();
    });
    (slider, subscription)
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
    share_a_focus: FocusHandle,
    share_b_focus: FocusHandle,
    share_c_focus: FocusHandle,
    city_focus: FocusHandle,
    shell_focus: FocusHandle,
    timeout_focus: FocusHandle,
    url_draft: String,
    token_draft: String,
    alias_draft: String,
    pin_draft: String,
    share_a_draft: String,
    share_b_draft: String,
    share_c_draft: String,
    ignore_focus: FocusHandle,
    url_draft: String,
    token_draft: String,
    ignore_draft: String,
    client_id_focus: FocusHandle,
    url_draft: String,
    token_draft: String,
    client_id_draft: String,
    token_revealed: bool,
    query_draft: String,
    heading_draft: String,
    city_draft: String,
    geo_results: Vec<nook_core::weather::GeoPlace>,
    geo_error: Option<String>,
    geo_loading: bool,
    location_status: Option<String>,
    location_busy: bool,
    shell_draft: String,
    timeout_draft: String,
    catalog: Vec<String>,
    catalog_error: Option<String>,
    catalog_loading: bool,
    width_slider: Entity<SliderState>,
    width_slider_config: (WidgetModule, u8, u8),
    _width_slider_subscription: Subscription,
    volume_slider: Entity<SliderState>,
    _volume_slider_subscription: Subscription,
    scroll_speed_slider: Entity<SliderState>,
    _scroll_speed_slider_subscription: Subscription,
    scroll_duration_slider: Entity<SliderState>,
    _scroll_duration_slider_subscription: Subscription,
    exclude_focus: FocusHandle,
    exclude_draft: String,
    placement_drag: bool,
    placement_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
}

impl SettingsView {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let settings = nook_core::settings::get_app_settings();
        let mut module = WidgetModule::from_u8(LAST_MODULE.load(Ordering::Relaxed));
        if module == WidgetModule::Mixer && !nook_core::mixer::is_available() {
            module = WidgetModule::Calendar;
        }
        let min = module.min_cells();
        let max = settings.max_cells_for(module).max(min);
        let (width_slider, width_slider_subscription) = create_width_slider(module, &settings, cx);
        let (volume_slider, volume_slider_subscription) = create_float_slider(
            0.0,
            1.0,
            0.05,
            settings.keysound_volume.clamp(0.0, 1.0),
            cx,
            |value| {
                nook_core::settings::tweak_app_settings(|s| {
                    s.keysound_volume = value.clamp(0.0, 1.0);
                });
            },
        );
        let (scroll_speed_slider, scroll_speed_subscription) = create_float_slider(
            0.25,
            3.0,
            0.05,
            settings.scroll_speed.clamp(0.25, 3.0),
            cx,
            |value| {
                nook_core::settings::tweak_app_settings(|s| {
                    s.scroll_speed = value.clamp(0.25, 3.0);
                });
            },
        );
        let (scroll_duration_slider, scroll_duration_subscription) = create_float_slider(
            0.1,
            1.0,
            0.05,
            settings.scroll_duration.clamp(0.1, 1.0),
            cx,
            |value| {
                nook_core::settings::tweak_app_settings(|s| {
                    s.scroll_duration = value.clamp(0.1, 1.0);
                });
            },
        );
        Self {
            category: SettingsCategory::from_u8(LAST_CATEGORY.load(Ordering::Relaxed)),
            module,
            url_focus: cx.focus_handle(),
            token_focus: cx.focus_handle(),
            query_focus: cx.focus_handle(),
            heading_focus: cx.focus_handle(),
            alias_focus: cx.focus_handle(),
            pin_focus: cx.focus_handle(),
            share_a_focus: cx.focus_handle(),
            share_b_focus: cx.focus_handle(),
            share_c_focus: cx.focus_handle(),
            alias_draft: settings.share.device_alias.clone(),
            pin_draft: settings.share.localsend_pin.clone(),
            share_a_draft: share_field_a(&settings),
            share_b_draft: share_field_b(&settings),
            share_c_draft: share_field_c(&settings),
            city_focus: cx.focus_handle(),
            ignore_focus: cx.focus_handle(),
            shell_focus: cx.focus_handle(),
            timeout_focus: cx.focus_handle(),
            exclude_focus: cx.focus_handle(),
            url_draft: settings.observe.prometheus_url,
            token_draft: settings.observe.metrics_token,
            ignore_draft: nook_core::vpn::format_ignore_list(&settings.vpn_ignore_interfaces),
            client_id_focus: cx.focus_handle(),
            url_draft: settings.observe.prometheus_url,
            token_draft: settings.observe.metrics_token,
            client_id_draft: settings.spotify_client_id,
            token_revealed: false,
            query_draft: String::new(),
            heading_draft: settings.obsidian_capture_heading.clone().unwrap_or_default(),
            city_draft: settings.weather.location.name().to_string(),
            geo_results: Vec::new(),
            geo_error: None,
            geo_loading: false,
            location_status: None,
            location_busy: false,
            shell_draft: settings.terminal_shell.clone(),
            timeout_draft: settings.terminal_timeout_secs.to_string(),
            exclude_draft: String::new(),
            catalog: Vec::new(),
            catalog_error: None,
            catalog_loading: false,
            width_slider,
            width_slider_config: (module, min, max),
            _width_slider_subscription: width_slider_subscription,
            volume_slider,
            _volume_slider_subscription: volume_slider_subscription,
            scroll_speed_slider,
            _scroll_speed_slider_subscription: scroll_speed_subscription,
            scroll_duration_slider,
            _scroll_duration_slider_subscription: scroll_duration_subscription,
            placement_drag: false,
            placement_bounds: Rc::new(RefCell::new(None)),
        }
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

    fn search_city(&mut self, cx: &mut Context<Self>) {
        let query = self.city_draft.trim().to_string();
        if query.is_empty() || self.geo_loading {
            return;
        }
        self.geo_loading = true;
        self.geo_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    nook_core::runtime().block_on(nook_core::weather::search_places(&query, 5))
                })
                .await;
            this.update(cx, |this, cx| {
                this.geo_loading = false;
                match result {
                    Ok(places) => {
                        this.geo_results = places;
                        this.geo_error = if this.geo_results.is_empty() {
                            Some("No matching cities.".into())
                        } else {
                            None
                        };
                    }
                    Err(err) => this.geo_error = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn pick_place(&mut self, place: nook_core::weather::GeoPlace, cx: &mut Context<Self>) {
        let name = place.display_name();
        nook_core::settings::tweak_app_settings(|s| {
            s.weather.location = nook_core::weather::WeatherLocationMode::Manual {
                name: name.clone(),
                lat: place.latitude,
                lon: place.longitude,
            };
        });
        nook_core::weather::invalidate();
        self.city_draft = name;
        self.geo_results.clear();
        self.location_status = None;
        cx.notify();
    }

    fn use_system_location(&mut self, cx: &mut Context<Self>) {
        if self.location_busy {
            return;
        }
        self.location_busy = true;
        self.location_status = Some("Locating…".into());
        cx.notify();
        let rx = nook_core::location::begin_request();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { rx.await.unwrap_or_else(|_| Err("Location request ended.".into())) })
                .await;
            this.update(cx, |this, cx| {
                this.location_busy = false;
                match result {
                    Ok((lat, lon)) => {
                        nook_core::settings::tweak_app_settings(|s| {
                            s.weather.location = nook_core::weather::WeatherLocationMode::System {
                                name: "Current location".into(),
                                lat,
                                lon,
                            };
                        });
                        nook_core::weather::invalidate();
                        this.city_draft = "Current location".into();
                        this.location_status = None;
                    }
                    Err(err) => this.location_status = Some(err),
                }
    fn fetch_shortcuts(&mut self, cx: &mut Context<Self>) {
        if self.catalog_loading {
            return;
        }
        self.catalog_loading = true;
        cx.spawn(async move |this, cx| {
            let names = cx
                .background_executor()
                .spawn(async { nook_core::runtime().block_on(nook_core::focus::list_shortcuts()) })
                .await;
            this.update(cx, |this, cx| {
                this.catalog_loading = false;
                this.catalog = names;
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
        let share_a_focused = self.share_a_focus.is_focused(window);
        let share_b_focused = self.share_b_focus.is_focused(window);
        let share_c_focused = self.share_c_focus.is_focused(window);
        let city_focused = self.city_focus.is_focused(window);
        let ignore_focused = self.ignore_focus.is_focused(window);
        let exclude_focused = self.exclude_focus.is_focused(window);
        let client_id_focused = self.client_id_focus.is_focused(window);

        div()
            .id("settings-root")
            .size_full()
            .flex()
            .bg(theme::SETTINGS_GLASS)
            .text_color(theme::LABEL)
            .when(self.placement_drag, |d| {
                d.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    this.apply_placement(event.position.x.into(), event.position.y.into(), cx);
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, _, cx| {
                        this.placement_drag = false;
                        cx.notify();
                    }),
                )
            })
            .child(self.sidebar(cx))
            .child(div().w(px(1.)).h_full().bg(hairline()))
            .child(if self.category == SettingsCategory::Widgets {
                self.render_widgets(
                    &settings,
                    url_focused,
                    token_focused,
                    query_focused,
                    heading_focused,
                    city_focused,
                    ignore_focused,
                    client_id_focused,
                    cx,
                )
                    .into_any_element()
            } else {
                self.render_general(
                    &settings,
                    alias_focused,
                    pin_focused,
                    share_a_focused,
                    share_b_focused,
                    share_c_focused,
                    cx,
                )
                .into_any_element()
                self.render_general(&settings, window, cx).into_any_element()
            .child(match self.category {
                SettingsCategory::Widgets => self
                    .render_widgets(&settings, url_focused, token_focused, query_focused, cx)
                    .into_any_element(),
                SettingsCategory::Keyboard => self.render_keyboard(&settings, cx).into_any_element(),
                SettingsCategory::Scrolling => self
                    .render_scrolling(&settings, exclude_focused, cx)
                    .into_any_element(),
                SettingsCategory::General => self.render_general(&settings, cx).into_any_element(),
            })
    }
}

impl SettingsView {
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings-sidebar")
            .w(px(SIDEBAR_W))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(theme::SETTINGS_WELL)
            .pt(px(TITLEBAR_INSET))
            .px(px(10.))
            .pb(px(16.))
            .gap(px(2.))
            .child(self.sidebar_item(SettingsCategory::General, cx))
            .child(self.sidebar_item(SettingsCategory::Widgets, cx))
            .child(self.sidebar_item(SettingsCategory::Keyboard, cx))
            .child(self.sidebar_item(SettingsCategory::Scrolling, cx))
    }

    fn sidebar_item(&self, category: SettingsCategory, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.category == category;
        let icon_color = if selected {
            theme::accent()
        } else {
            theme::SECONDARY_LABEL
        };
        div()
            .id(SharedString::from(format!("sidebar-{}", category.title())))
            .h(px(28.))
            .px(px(8.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .when(selected, |d| d.bg(theme::FILL_SECONDARY))
            .hover(|s| if selected { s } else { s.bg(theme::FILL) })
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.category = category;
                    this.persist_nav();
                    cx.notify();
                }),
            )
            .child(lucide_color(category.icon(), 15.0, icon_color))
            .child(
                div()
                    .text_size(px(theme::BODY.size))
                    .line_height(px(theme::BODY.leading))
                    .font_weight(if selected {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if selected {
                        theme::LABEL
                    } else {
                        theme::SECONDARY_LABEL
                    })
                    .child(category.title()),
            )
    }

    fn pane(title: &'static str, body: impl IntoElement) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("pane-{title}")))
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .pt(px(TITLEBAR_INSET))
            .child(
                div()
                    .px(px(20.))
                    .pb(px(12.))
                    .flex_shrink_0()
                    .text_size(px(theme::TITLE_2.size))
                    .line_height(px(theme::TITLE_2.leading))
                    .font_weight(theme::TITLE_2.emphasized)
                    .text_color(theme::LABEL)
                    .child(title),
            )
            .child(
                div()
                    .id(SharedString::from(format!("pane-body-{title}")))
                    .flex_1()
                    .min_h(px(0.))
                    .px(px(20.))
                    .pb(px(24.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(body),
            )
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

    fn persist_share_creds(&self) {
        let a = self.share_a_draft.trim().to_string();
        let b = self.share_b_draft.trim().to_string();
        let c = self.share_c_draft.trim().to_string();
        nook_core::settings::tweak_app_settings(|s| match s.share.link_backend {
            LinkBackendKind::WebDav => {
                s.share.webdav_url = a;
                s.share.webdav_username = b;
                s.share.webdav_password = c;
            }
            LinkBackendKind::S3 => {
                s.share.s3_bucket = a;
                s.share.s3_access_key = b;
                s.share.s3_secret_key = c;
            }
            LinkBackendKind::ZeroXZero => {}
        });
    }

    fn render_general(
        &self,
        settings: &AppSettings,
        alias_focused: bool,
        pin_focused: bool,
        share_a_focused: bool,
        share_b_focused: bool,
        share_c_focused: bool,
    fn render_general(
        &self,
        settings: &AppSettings,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        Self::pane(
            "General",
            div()
                .id("general-pane")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "Appearance",
                    settings_group(vec![
                        toggle_row("Liquid Glass island", settings.liquid_glass_mode, cx, |s| {
                            s.liquid_glass_mode = !s.liquid_glass_mode
                        })
                        .into_any_element(),
                        self.color_row(settings, cx).into_any_element(),
                    ]),
                    Some("A custom color replaces the default black island."),
                ))
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
                    ]),
                    Some("Hover the island to expand. Settings and Quit are in the menu bar extra."),
                ))
                .child(section(
                    "Window Snap",
                    settings_group({
                        let mut rows = vec![
                            toggle_row(
                                "Snap hotkeys",
                                settings.window_snap_enabled,
                                cx,
                                |s| {
                                    s.window_snap_enabled = !s.window_snap_enabled;
                                    if s.window_snap_enabled
                                        && !crate::platform::accessibility_trusted()
                                    {
                                        crate::platform::prompt_accessibility();
                                    }
                                },
                            )
                            .into_any_element(),
                            status_row(
                                "Accessibility",
                                nook_core::window_snap::accessibility_status().label(),
                            )
                            .into_any_element(),
                            action_row(
                                "ax-prompt",
                                "Request Accessibility",
                                "Prompt",
                                cx,
                                |_, _, cx| {
                                    crate::platform::prompt_accessibility();
                                    cx.notify();
                                },
                            )
                            .into_any_element(),
                            action_row(
                                "ax-open",
                                "Privacy settings",
                                "Open",
                                cx,
                                |_, _, _| {
                                    nook_core::window_snap::open_accessibility_settings();
                                },
                            )
                            .into_any_element(),
                        ];
                        for (kind, hotkey) in nook_core::hotkeys::default_bindings() {
                            rows.push(
                                shortcut_row(kind.label(), hotkey.display()).into_any_element(),
                            );
                        }
                        rows
                    }),
                    Some("⌃⌥ plus arrows, U/I/J/K, or Return. macOS 15+ tiling can fight drag-to-edge; hotkeys stay independent."),
                ))
                .child(section(
                    "Menu bar (Thaw)",
                    settings_group(vec![
                        toggle_row("Hide extras with a separator", settings.thaw_enabled, cx, |s| {
                            s.thaw_enabled = !s.thaw_enabled;
                            if !s.thaw_enabled {
                                s.thaw_hidden = false;
                            }
                        })
                        .into_any_element(),
                        toggle_row("Extras hidden", settings.thaw_hidden, cx, |s| {
                            if s.thaw_enabled {
                                s.thaw_hidden = !s.thaw_hidden;
                            }
                        })
                        .into_any_element(),
                    ]),
                    Some("⌘-drag extras so hidden items sit to the left of the Nook chevron. Click the chevron to hide or show. No Screen Recording."),
                    "HUD",
                    settings_group(vec![
                        toggle_row(
                            "Show volume & brightness HUD",
                            settings.show_volume_brightness_hud,
                            cx,
                            |s| {
                                s.show_volume_brightness_hud = !s.show_volume_brightness_hud;
                    "Termi-Notch",
                    settings_group(vec![
                        toggle_row(
                            "Enable one-shot shell",
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
                            "term-timeout",
                            "Timeout",
                            &self.timeout_draft,
                            false,
                            self.timeout_focus.is_focused(window),
                            &self.timeout_focus,
                            cx,
                            |this, event, cx| {
                                if SettingsView::apply_key(&mut this.timeout_draft, event, cx) {
                                    this.timeout_draft
                                        .retain(|ch| ch.is_ascii_digit());
                                    if let Ok(secs) = this.timeout_draft.parse::<u32>() {
                                        nook_core::settings::tweak_app_settings(|s| {
                                            s.terminal_timeout_secs = secs.clamp(1, 600);
                                        });
                                    }
                                    cx.notify();
                                }
                            },
                        )
                        .into_any_element(),
                        toggle_row(
                            "Replace system volume/brightness HUD",
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
                .child(self.render_sharing(
                    settings,
                    alias_focused,
                    pin_focused,
                    share_a_focused,
                    share_b_focused,
                    share_c_focused,
                    cx,
                            "Remember command history",
                            settings.terminal_history,
                            cx,
                            |s| s.terminal_history = !s.terminal_history,
                        )
                        .into_any_element(),
                    ]),
                    Some("Typed in the island only. opennook://, the CLI, and Finder Services never run commands. Default off."),
                    "Sound",
                    settings_group(vec![toggle_row(
                        "Output picker on the media card",
                        settings.audio_output_picker,
                        cx,
                        |s| s.audio_output_picker = !s.audio_output_picker,
                    )
                    .into_any_element()]),
                    Some(nook_core::audio_devices::AIRPLAY_INITIATE_NOTE),
                )),
        )
    }

    fn render_sharing(
        &self,
        settings: &AppSettings,
        alias_focused: bool,
        pin_focused: bool,
        share_a_focused: bool,
        share_b_focused: bool,
        share_c_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let backend = settings.share.link_backend;
        let mut rows = vec![
            field_row(
                "share-alias",
                "Alias",
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
    fn render_keyboard(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let listen = nook_core::eventtap::input_monitoring_status();
        let packs = nook_core::keysounds::list_packs();
        let mut pack_rows = Vec::new();
        for pack in packs {
            let selected = settings.keysound_pack == pack.id;
            let id = pack.id.clone();
            pack_rows.push(
                settings_row(SharedString::from(format!("pack-{}", pack.id)))
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _, _, cx| {
                            let id = id.clone();
                            nook_core::settings::tweak_app_settings(|s| s.keysound_pack = id);
                            cx.notify();
                        }),
                    )
                    .child(label(pack.name, theme::BODY, true))
                    .child(label(
                        if selected { "Selected" } else { " " },
                        theme::SUBHEADLINE,
                        false,
                    ))
                    .into_any_element(),
            );
        }
        Self::pane(
            "Keyboard Sounds",
            div()
                .id("keyboard-pane")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "Mechey",
                    settings_group(vec![
                        toggle_row(
                            "Play sounds while typing",
                            settings.keysounds_enabled,
                            cx,
                            |s| {
                                s.keysounds_enabled = !s.keysounds_enabled;
                                if s.keysounds_enabled
                                    && !nook_core::eventtap::input_monitoring_status().granted()
                                {
                                    nook_core::eventtap::request_input_monitoring();
                                }
                            },
                        )
                        .into_any_element(),
                        permission_row("Input Monitoring", listen).into_any_element(),
                        action_row(
                            "listen-prompt",
                            "Request Input Monitoring",
                            "Prompt",
                            cx,
                            |_, _, cx| {
                                nook_core::eventtap::request_input_monitoring();
                                cx.notify();
                            },
                        )
                        .into_any_element(),
                        action_row(
                            "listen-open",
                            "Privacy settings",
                            "Open",
                            cx,
                            |_, _, _| {
                                nook_core::eventtap::open_input_monitoring_settings();
                            },
                        )
                        .into_any_element(),
                    ]),
                    Some("Opt-in. The key tap is created only while this is on. Password fields stay silent (secure input). Ad-hoc signing can drop the grant after each rebuild."),
                ))
                .child(section(
                    "Pack",
                    settings_group({
                        let mut rows = pack_rows;
                        rows.push(
                            self.float_slider_row(
                                "keysound-volume",
                                "Volume",
                                settings.keysound_volume,
                                &self.volume_slider,
                                format!("{:.0}%", settings.keysound_volume * 100.0),
                            )
                            .into_any_element(),
                        );
                        rows.push(
                            action_row("keysound-test", "Preview this pack", "Test", cx, |_, _, cx| {
                                nook_core::keysounds::play_test();
                                cx.notify();
                            })
                            .into_any_element(),
                        );
                        rows
                    }),
                    Some("Drop Mechvibes packs (config.json + OGG) into Application Support/openNook-gpui/soundpacks. Builtin clicks are original CC0 tones, not switch recordings."),
                )),
        )
    }

    fn render_scrolling(
        &self,
        settings: &AppSettings,
        exclude_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let ax = nook_core::eventtap::accessibility_status();
        let conflicts = nook_core::eventtap::running_conflict_ids();
        let mut rows = vec![
            toggle_row(
                "Smooth scrolling for mice",
                settings.smooth_scroll_enabled,
                cx,
                |s| {
                    s.smooth_scroll_enabled = !s.smooth_scroll_enabled;
                    if s.smooth_scroll_enabled
                        && !nook_core::eventtap::accessibility_status().granted()
                    {
                        nook_core::eventtap::request_accessibility();
                    }
                },
            )
            .into_any_element(),
            toggle_row(
                "Receive LocalSend",
                settings.share.localsend_receive,
                cx,
                |s| s.share.localsend_receive = !s.share.localsend_receive,
            )
            .into_any_element(),
            field_row(
                "share-pin",
                "PIN",
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
                "Reverse mouse wheel",
                settings.reverse_mouse_scroll,
                cx,
                |s| {
                    s.reverse_mouse_scroll = !s.reverse_mouse_scroll;
                    if s.reverse_mouse_scroll
                        && !nook_core::eventtap::accessibility_status().granted()
                    {
                        nook_core::eventtap::request_accessibility();
                    }
                },
            )
            .into_any_element(),
            settings_row("share-backend")
                .child(label("Link host", theme::BODY, true))
                .child(
                    segmented_group()
                        .child(segment(
                            "0x0.st",
                            backend == LinkBackendKind::ZeroXZero,
                            cx,
                            |this, _, cx| {
                                nook_core::settings::tweak_app_settings(|s| {
                                    s.share.link_backend = LinkBackendKind::ZeroXZero
                                });
                                let settings = nook_core::settings::get_app_settings();
                                this.share_a_draft = share_field_a(&settings);
                                this.share_b_draft = share_field_b(&settings);
                                this.share_c_draft = share_field_c(&settings);
                                cx.notify();
                            },
                        ))
                        .child(segment(
                            "WebDAV",
                            backend == LinkBackendKind::WebDav,
                            cx,
                            |this, _, cx| {
                                nook_core::settings::tweak_app_settings(|s| {
                                    s.share.link_backend = LinkBackendKind::WebDav
                                });
                                let settings = nook_core::settings::get_app_settings();
                                this.share_a_draft = share_field_a(&settings);
                                this.share_b_draft = share_field_b(&settings);
                                this.share_c_draft = share_field_c(&settings);
                                cx.notify();
                            },
                        ))
                        .child(segment(
                            "S3",
                            backend == LinkBackendKind::S3,
                            cx,
                            |this, _, cx| {
                                nook_core::settings::tweak_app_settings(|s| {
                                    s.share.link_backend = LinkBackendKind::S3
                                });
                                let settings = nook_core::settings::get_app_settings();
                                this.share_a_draft = share_field_a(&settings);
                                this.share_b_draft = share_field_b(&settings);
                                this.share_c_draft = share_field_c(&settings);
                                cx.notify();
                            },
                        )),
                )
                .into_any_element(),
        ];
        match backend {
            LinkBackendKind::ZeroXZero => {}
            LinkBackendKind::WebDav => {
                rows.push(self.share_cred_row(
                    "share-url",
                    "URL",
                    &self.share_a_draft,
                    "https://dav.example/public",
                    share_a_focused,
                    &self.share_a_focus,
                    cx,
                    |this, event, cx| {
                        if Self::apply_key(&mut this.share_a_draft, event, cx) {
                            this.persist_share_creds();
                            cx.notify();
                        }
                    },
                ));
                rows.push(self.share_cred_row(
                    "share-user",
                    "User",
                    &self.share_b_draft,
                    "optional",
                    share_b_focused,
                    &self.share_b_focus,
                    cx,
                    |this, event, cx| {
                        if Self::apply_key(&mut this.share_b_draft, event, cx) {
                            this.persist_share_creds();
                            cx.notify();
                        }
                    },
                ));
                rows.push(self.share_cred_row(
                    "share-pass",
                    "Pass",
                    if share_c_focused {
                        &self.share_c_draft
                    } else if self.share_c_draft.is_empty() {
                        "Keychain"
                    } else {
                        "••••••••"
                    },
                    "Keychain",
                    share_c_focused,
                    &self.share_c_focus,
                    cx,
                    |this, event, cx| {
                        if Self::apply_key(&mut this.share_c_draft, event, cx) {
                            this.persist_share_creds();
                            cx.notify();
                        }
                    },
                ));
            }
            LinkBackendKind::S3 => {
                rows.push(self.share_cred_row(
                    "share-bucket",
                    "Bucket",
                    &self.share_a_draft,
                    "bucket",
                    share_a_focused,
                    &self.share_a_focus,
                    cx,
                    |this, event, cx| {
                        if Self::apply_key(&mut this.share_a_draft, event, cx) {
                            this.persist_share_creds();
                            cx.notify();
                        }
                    },
                ));
                rows.push(self.share_cred_row(
                    "share-access",
                    "Key",
                    &self.share_b_draft,
                    "access key",
                    share_b_focused,
                    &self.share_b_focus,
                    cx,
                    |this, event, cx| {
                        if Self::apply_key(&mut this.share_b_draft, event, cx) {
                            this.persist_share_creds();
                            cx.notify();
                        }
                    },
                ));
                rows.push(self.share_cred_row(
                    "share-secret",
                    "Secret",
                    if share_c_focused {
                        &self.share_c_draft
                    } else if self.share_c_draft.is_empty() {
                        "Keychain"
                    } else {
                        "••••••••"
                    },
                    "Keychain",
                    share_c_focused,
                    &self.share_c_focus,
                    cx,
                    |this, event, cx| {
                        if Self::apply_key(&mut this.share_c_draft, event, cx) {
                            this.persist_share_creds();
                            cx.notify();
                        }
                    },
                ));
            }
        }
        section(
            "Sharing",
            settings_group(rows),
            Some(share_blurb(backend, settings.share.localsend_receive)),
        )
    }

    fn share_cred_row(
        &self,
        id: &'static str,
        title: &'static str,
        value: &str,
        placeholder: &str,
        focused: bool,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
        on_key: impl Fn(&mut SettingsView, &KeyDownEvent, &mut Context<SettingsView>) + 'static,
    ) -> AnyElement {
        let empty = value.is_empty();
        field_row(
            id,
            title,
            if empty { placeholder } else { value },
            empty,
            focused,
            focus,
            cx,
            on_key,
        )
        .into_any_element()
            permission_row("Accessibility", ax).into_any_element(),
            action_row(
                "scroll-ax-prompt",
                "Request Accessibility",
                "Prompt",
                cx,
                |_, _, cx| {
                    nook_core::eventtap::request_accessibility();
                    cx.notify();
                },
            )
            .into_any_element(),
            action_row(
                "scroll-ax-open",
                "Privacy settings",
                "Open",
                cx,
                |_, _, _| {
                    nook_core::eventtap::open_accessibility_settings();
                },
            )
            .into_any_element(),
            self.float_slider_row(
                "scroll-speed",
                "Speed",
                settings.scroll_speed,
                &self.scroll_speed_slider,
                format!("{:.2}×", settings.scroll_speed),
            )
            .into_any_element(),
            self.float_slider_row(
                "scroll-duration",
                "Coast",
                settings.scroll_duration,
                &self.scroll_duration_slider,
                format!("{:.0} ms", settings.scroll_duration * 1000.0),
            )
            .into_any_element(),
        ];
        if !conflicts.is_empty() {
            rows.push(
                status_row(
                    "Also running",
                    "Mos / LinearMouse / similar — expect conflicts",
                )
                .into_any_element(),
            );
        }
        let mut exclude_rows = vec![field_row(
            "scroll-exclude",
            "Add",
            if self.exclude_draft.is_empty() {
                "com.example.app"
            } else {
                &self.exclude_draft
            },
            self.exclude_draft.is_empty(),
            exclude_focused,
            &self.exclude_focus,
            cx,
            |this, event, cx| {
                if Self::apply_key(&mut this.exclude_draft, event, cx) {
                    cx.notify();
                }
                if event.keystroke.key == "enter" {
                    let id = this.exclude_draft.trim().to_string();
                    if !id.is_empty() {
                        nook_core::settings::tweak_app_settings(|s| {
                            if !s.scroll_excluded_apps.iter().any(|item| item == &id) {
                                s.scroll_excluded_apps.push(id);
                            }
                        });
                        this.exclude_draft.clear();
                    }
                    cx.notify();
                }
            },
        )
        .into_any_element()];
        for bundle in &settings.scroll_excluded_apps {
            let id = bundle.clone();
            exclude_rows.push(
                settings_row(SharedString::from(format!("ex-{id}")))
                    .child(label(id.clone(), theme::BODY, true))
                    .child(push_button(
                        SharedString::from(format!("rm-{id}")),
                        "Remove",
                        cx,
                        move |_, _, cx| {
                            let id = id.clone();
                            nook_core::settings::tweak_app_settings(|s| {
                                s.scroll_excluded_apps.retain(|item| item != &id);
                            });
                            cx.notify();
                        },
                    ))
                    .into_any_element(),
            );
        }

        Self::pane(
            "Scrolling",
            div()
                .id("scrolling-pane")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "LiquidMouse",
                    settings_group(rows),
                    Some("Trackpads pass through (IsContinuous). Wheel mice get pixel momentum. The tap exists only while a toggle is on. Per-device overrides are not shipped — they need private sender IDs."),
                ))
                .child(section(
                    "Excluded apps",
                    settings_group(exclude_rows),
                    Some("Games, VMs, and remotes should stay on the raw wheel. Built-in defaults already cover UTM, VMware, Parallels, VirtualBox, Steam, and Screen Sharing."),
                )),
        )
    }

    fn float_slider_row(
        &self,
        id: &'static str,
        title: &'static str,
        value: f32,
        slider: &Entity<SliderState>,
        caption: String,
    ) -> impl IntoElement {
        let _ = value;
        settings_row(id)
            .child(label(title, theme::BODY, true))
            .child(
                div()
                    .id(SharedString::from(format!("{id}-slider")))
                    .flex_1()
                    .h(px(theme::HIT_MIN))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .child(
                        Slider::new(slider)
                            .bg(theme::accent())
                            .text_color(rgb(0xffffff)),
                    ),
            )
            .child(
                div().w(px(52.)).flex().justify_end().child(
                    div()
                        .text_size(px(theme::BODY.size))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::SECONDARY_LABEL)
                        .child(caption),
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
            .child(div().absolute().inset_0().bg(rgb(0x1b1b1f)))
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
            .child(push_button("island-reset-btn", "Reset", cx, |_, _, cx| {
                nook_core::settings::tweak_app_settings(|s| s.reset_island_position());
                cx.notify();
            }))
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
        client_id_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let remaining = settings.remaining_cells();
        let total = AppSettings::TOTAL_CELLS;
        let preview_footer = if remaining == 0 {
            format!("All {total} cells are in use.")
        } else if remaining == 1 {
            "1 cell remaining.".to_string()
        } else {
            format!("{remaining} of {total} cells remaining.")
        };

        let mut list = Vec::new();
        for module in settings.ordered_widgets() {
            if module == WidgetModule::Mixer && !nook_core::mixer::is_available() {
                continue;
            }
            list.push(self.widget_row(module, settings, cx).into_any_element());
        }

        Self::pane(
            "Widgets",
            div()
                .id("custom-widgets")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "Preview",
                    settings_group(vec![self.island_preview(settings).into_any_element()]),
                    Some(preview_footer),
                ))
                .child(section(
                    "Widgets",
                    settings_group(list),
                    Some("Drag to change the order on the island."),
                ))
                .child(self.module_section(
                    settings,
                    url_focused,
                    token_focused,
                    query_focused,
                    heading_focused,
                    city_focused,
                    ignore_focused,
                    client_id_focused,
                    cx,
                )),
        )
    }

    fn widget_row(
        &self,
        module: WidgetModule,
        settings: &AppSettings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.module == module;
        let on = module.enabled(settings);
        let cells = settings.cells_for(module);
        let caption = if module.occupies_nook_cells() {
            if cells == 1 {
                "1 cell".to_string()
            } else {
                format!("{cells} cells")
            }
        } else {
            "Tray tab".to_string()
        };
        div()
            .id(SharedString::from(format!("mod-{}", module.name())))
            .px(px(GROUP_PAD))
            .min_h(px(ROW_H + 4.0))
            .flex()
            .items_center()
            .gap(px(8.))
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
                    settings.move_widget_to(drag.0, module)
                });
                cx.notify();
            }))
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
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
                }),
            )
            .on_drag(WidgetDrag(module), |drag, _, _, cx| cx.new(|_| *drag))
            .child(lucide_color("grip-vertical", 14.0, theme::TERTIARY_LABEL))
            .child(lucide_color(module.icon(), 15.0, theme::LABEL))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(label(module.name(), theme::BODY, true))
                    .child(label(caption, theme::SUBHEADLINE, false)),
            )
            .child(module_toggle(on, module, cx))
    }

    fn island_preview(&self, settings: &AppSettings) -> impl IntoElement {
        let enabled: Vec<_> = settings
            .ordered_widgets()
            .into_iter()
            .filter(|module| module.occupies_nook_cells() && module.enabled(settings))
            .collect();
        let used = enabled
            .iter()
            .map(|module| settings.cells_for(*module) as f32)
            .sum::<f32>()
            .max(1.0);
        let inner = 480.0_f32;
        let wallpaper = desktop_wallpaper_image();
        let mut chips = div().flex().items_center().gap(px(6.)).px(px(8.));
        if enabled.is_empty() {
            chips = chips.child(
                div()
                    .text_size(px(theme::SUBHEADLINE.size))
                    .text_color(theme::SECONDARY_LABEL)
                    .child("No widgets enabled"),
            );
        } else {
            for module in enabled {
                let cells = settings.cells_for(module) as f32;
                let width = ((cells / used) * inner).max(56.0);
                chips = chips.child(self.preview_chip(module, width, module == self.module));
            }
        }

        div()
            .id("island-preview")
            .w_full()
            .h(px(132.))
            .overflow_hidden()
            .relative()
            .bg(theme::SETTINGS_GLASS)
            .when_some(wallpaper, |preview, wallpaper| {
                preview.child(
                    img(wallpaper)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .h(px(64.))
                            .px(px(8.))
                            .rounded(px(20.))
                            .bg(rgb(0x000000))
                            .flex()
                            .items_center()
                            .child(chips),
                    ),
            )
    }

    fn preview_chip(&self, module: WidgetModule, width: f32, selected: bool) -> impl IntoElement {
        div()
            .h(px(48.))
            .w(px(width))
            .rounded(px(12.))
            .bg(if selected {
                rgb(0x2a2a2a)
            } else {
                rgb(0x1a1a1a)
            })
            .when(selected, |d| d.border_1().border_color(rgba(0xffffff33)))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(3.))
            .child(lucide_color(module.icon(), 14.0, theme::LABEL))
            .child(
                div()
                    .text_size(px(theme::FOOTNOTE.size))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::LABEL)
                    .child(module.preview_label()),
            )
    }

    fn width_slider(&mut self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let module = self.module;
        let value = settings.cells_for(module);
        let min = module.min_cells();
        let max = settings.max_cells_for(module).max(min);
        let config = (module, min, max);
        if self.width_slider_config != config {
            let (slider, subscription) = create_width_slider(module, settings, cx);
            self.width_slider = slider;
            self._width_slider_subscription = subscription;
            self.width_slider_config = config;
        }
        let enabled = module.occupies_nook_cells();

        settings_row("width-row")
            .opacity(if enabled { 1.0 } else { 0.45 })
            .child(label("Width", theme::BODY, true))
            .child(
                div()
                    .id("width-slider")
                    .flex_1()
                    .h(px(theme::HIT_MIN))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .child(
                        Slider::new(&self.width_slider)
                            .disabled(!enabled)
                            .bg(theme::accent())
                            .text_color(rgb(0xffffff)),
                    ),
            )
            .child(
                div().w(px(28.)).flex().justify_end().child(
                    div()
                        .text_size(px(theme::BODY.size))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::SECONDARY_LABEL)
                        .child(value.to_string()),
                ),
            )
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
        client_id_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = self.module.name();
        let enabled = self.module.enabled(settings);
        let mut rows = vec![self.width_slider(settings, cx).into_any_element()];
        match self.module {
            WidgetModule::Music => {
                rows.push(
                    toggle_row("Show lyrics", settings.show_lyrics, cx, |s| {
                        s.show_lyrics = !s.show_lyrics;
                    toggle_row(
                        "Animated album art (Apple Music)",
                        settings.animated_album_art,
                        cx,
                        |s| s.animated_album_art = !s.animated_album_art,
                    )
                    .into_any_element(),
                );
                rows.push(
                    toggle_row("Ambient art glow", settings.ambient_art_glow, cx, |s| {
                        s.ambient_art_glow = !s.ambient_art_glow
                    })
                    .into_any_element(),
                );
            }
            WidgetModule::Calendar => {
                rows.push(
                    toggle_row("Quick add", settings.quick_add, cx, |s| {
                        s.quick_add = !s.quick_add;
                    })
                    .into_any_element(),
                );
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
                    toggle_row("Quick add", settings.quick_add, cx, |s| {
                        s.quick_add = !s.quick_add;
                    })
                    .into_any_element(),
                );
            }
            WidgetModule::Notes => {
                rows.push(
                    action_row("notes-edit", "Notes", "Edit Notes…", cx, |_, _, _| {
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
                        "Metric names",
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
                        "Install shortcut",
                        cx,
                        |_, _, _| {
                            if let Err(err) = nook_core::power::install_lpm_shortcut() {
                                log::warn!("install LPM shortcut: {err}");
            WidgetModule::Messages => {
                let fda = nook_core::messages::fda_status();
                let status = match fda {
                    nook_core::messages::FdaStatus::Granted => "On",
                    nook_core::messages::FdaStatus::Denied => "Off",
                    nook_core::messages::FdaStatus::Unavailable => "Unavailable",
                };
                rows.push(
                    settings_row("msg-fda-status")
                        .child(label("Full Disk Access", theme::BODY, true))
                        .child(label(status, theme::BODY, false))
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
            WidgetModule::Mixer => {
                rows.push(
                    settings_row("mixer-permission")
                        .child(label("Permission", theme::BODY, true))
                        .child(label(
                            nook_core::mixer::capture_status_label(
                                nook_core::mixer::capture_status(),
                            ),
                            theme::SUBHEADLINE,
                            false,
                        ))
                        .into_any_element(),
                );
                rows.push(
                    action_row(
                        "msg-fda-open",
                        "Privacy settings",
                        "Open Full Disk Access",
                        cx,
                        |_, _, _| {
                            if let Err(err) = nook_core::messages::open_fda_settings() {
                                log::warn!("open FDA settings: {err}");
            WidgetModule::Timers => {
                rows.push(
                    toggle_row(
                        "Apple Clock timers",
                        settings.sync_clock_timers,
                        cx,
                        |s| s.sync_clock_timers = !s.sync_clock_timers,
                    )
                    .into_any_element(),
                );
                rows.push(
                    action_row(
                        "clock-shortcuts",
                        "Clock shortcuts",
                        "Install…",
                        cx,
                        |_, _, _| {
                            if let Err(err) = nook_core::shortcuts::import_bundled_shortcuts() {
                                log::info!("clock shortcuts: {err}");
                        "obsidian-folder",
                        "Folder",
                        "Choose Folder…",
                        cx,
                        |_, _, cx| {
                            if let Some(path) = crate::platform::choose_directory() {
                                nook_core::settings::tweak_app_settings(|s| {
                                    s.obsidian_vault = Some(path);
                                });
            WidgetModule::Vpn => {
                rows.push(
                    toggle_row(
                        "Timer on compact face",
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
                            let persist = SettingsView::apply_key(&mut this.ignore_draft, event, cx)
                                || event.keystroke.key == "enter";
                            if persist {
                                this.persist_ignore();
                                cx.notify();
                            }
                        },
                    )
                    .into_any_element(),
                );
                rows.push(
                    toggle_row(
                        "Experimental WhatsApp auto-send",
                        settings.experimental_whatsapp_autosend,
                        cx,
                        |s| {
                            s.experimental_whatsapp_autosend = !s.experimental_whatsapp_autosend;
                        },
                if settings.obsidian_vault.is_some() {
                    rows.push(
                        action_row("obsidian-clear", "Vault", "Clear", cx, |_, _, cx| {
                            nook_core::settings::tweak_app_settings(|s| {
                                s.obsidian_vault = None;
                            });
                            cx.notify();
                        })
                        .into_any_element(),
                    );
                }
                rows.push(
                    toggle_row(
                        "Capture via Obsidian URI",
                        settings.obsidian_uri_capture,
                        cx,
                        |s| s.obsidian_uri_capture = !s.obsidian_uri_capture,
                        "mixer-reset",
                        "Volumes",
                        "Reset All",
                        cx,
                        |_, _, cx| {
                            nook_core::mixer::reset_all();
                            nook_core::mixer::pump();
                            cx.notify();
                        },
                    )
                    .into_any_element(),
                );
            WidgetModule::HighAlert => {
                rows.extend(high_alert_rows(settings, cx));
            }
            WidgetModule::Timers => {
                rows.extend(pomodoro_rows(settings, &self.catalog, cx));
            WidgetModule::Recorder => {
                rows.push(
                    toggle_row(
                        "Live transcription",
                        settings.recorder_transcribe,
                        cx,
                        |s| s.recorder_transcribe = !s.recorder_transcribe,
                    )
                    .into_any_element(),
                );
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
    }

    fn render_weather_settings(
        &self,
        settings: &AppSettings,
        city_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use nook_core::weather::{WeatherLocationMode, WeatherUnits};

        let units = settings.weather.units;
        let mut unit_row = segmented_group();
        for (caption, value) in [("°C", WeatherUnits::Celsius), ("°F", WeatherUnits::Fahrenheit)] {
            unit_row = unit_row.child(segment(caption, units == value, cx, move |_, _, cx| {
                nook_core::settings::tweak_app_settings(|s| s.weather.units = value);
                nook_core::weather::invalidate();
                cx.notify();
            }));
        }

        let city_text = if self.city_draft.is_empty() {
            "City name"
        } else {
            self.city_draft.as_str()
        };
        let mut location_rows = vec![
            field_row(
                "weather-city",
                "City",
                city_text,
                self.city_draft.is_empty(),
                city_focused,
                &self.city_focus,
                cx,
                |this, event, cx| {
                    if event.keystroke.key == "enter" {
                        this.search_city(cx);
                    } else if SettingsView::apply_key(&mut this.city_draft, event, cx) {
            .when(self.module == WidgetModule::Music, |d| {
                d.child(self.render_music_settings(settings, client_id_focused, cx))
            })
    }

    fn persist_client_id(&self) {
        let draft = self.client_id_draft.trim().to_string();
        if nook_core::settings::get_app_settings().spotify_client_id == draft {
            return;
        }
        nook_core::settings::tweak_app_settings(|s| s.spotify_client_id = draft);
    }

    fn render_music_settings(
        &self,
        settings: &AppSettings,
        client_id_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if !client_id_focused {
            self.persist_client_id();
        }
        use nook_core::spotify::SpotifyStatus;
        let status = nook_core::spotify::status();
        let status_text = match &status {
            SpotifyStatus::Disconnected => "Not connected".to_string(),
            SpotifyStatus::Connecting => "Waiting for Spotify login…".to_string(),
            SpotifyStatus::Connected => "Connected".to_string(),
            SpotifyStatus::NeedsClientId => "Add a client ID first".to_string(),
            SpotifyStatus::PremiumRequired => {
                "Spotify Premium is required for queue control".to_string()
            }
            SpotifyStatus::Error(err) => err.clone(),
        };
        let connected = matches!(
            status,
            SpotifyStatus::Connected | SpotifyStatus::PremiumRequired
        );
        let client_placeholder = self.client_id_draft.is_empty();
        let client_value = if client_placeholder {
            "Paste your Spotify client ID"
        } else {
            self.client_id_draft.as_str()
        };
        let mut rows = vec![
            toggle_row("Show Up Next", settings.show_media_queue, cx, |s| {
                s.show_media_queue = !s.show_media_queue;
            })
            .into_any_element(),
            field_row(
                "spotify-client-id",
                "Client ID",
                client_value,
                client_placeholder,
                client_id_focused,
                &self.client_id_focus,
                cx,
                |this, event, cx| {
                    if event.keystroke.key == "enter" {
                        this.persist_client_id();
                        cx.notify();
                    } else if SettingsView::apply_key(&mut this.client_id_draft, event, cx) {
                        cx.notify();
                    }
                },
            )
            .into_any_element(),
            settings_row("weather-city-actions")
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(push_button(
                            "weather-search",
                            if self.geo_loading {
                                "Searching…"
                            } else {
                                "Search"
                            },
                            cx,
                            |this, _, cx| this.search_city(cx),
                        ))
                        .child(push_button(
                            "weather-system",
                            if self.location_busy {
                                "Locating…"
                            } else {
                                "Use system location"
                            },
                            cx,
                            |this, _, cx| this.use_system_location(cx),
                        )),
                )
                .into_any_element(),
        ];
        if let Some(status) = &self.location_status {
            location_rows.push(
                settings_row("weather-loc-status")
                    .child(label(status.clone(), theme::BODY, false))
                    .into_any_element(),
            );
        }
        if let Some(err) = &self.geo_error {
            location_rows.push(
                settings_row("weather-geo-err")
                    .child(label(err.clone(), theme::BODY, false))
                    .into_any_element(),
            );
        }
        for (i, place) in self.geo_results.iter().enumerate() {
            let caption = place.display_name();
            let picked = place.clone();
            location_rows.push(
                settings_row(SharedString::from(format!("weather-hit-{i}")))
                    .child(label(caption, theme::BODY, true))
                    .child(push_button(
                        SharedString::from(format!("weather-pick-{i}")),
                        "Use",
                        cx,
                        move |this, _, cx| this.pick_place(picked.clone(), cx),
            settings_row("spotify-status")
                .child(label("Spotify", theme::BODY, true))
                .child(label(status_text, theme::SUBHEADLINE, false))
                .into_any_element(),
        ];
        if connected {
            rows.push(
                action_row(
                    "spotify-disconnect",
                    "Account",
                    "Disconnect",
                    cx,
                    |_, _, cx| {
                        nook_core::spotify::disconnect();
                        cx.notify();
                    },
                )
                .into_any_element(),
            );
        } else {
            rows.push(
                action_row(
                    "spotify-connect",
                    "Account",
                    "Connect Spotify",
                    cx,
                    |this, _, cx| {
                        this.persist_client_id();
                        cx.spawn(async move |this, cx| {
                            let result = cx
                                .background_executor()
                                .spawn(async {
                                    nook_core::runtime().block_on(nook_core::spotify::connect())
                                })
                                .await;
                            this.update(cx, |_, cx| {
                                if let Err(err) = result {
                                    log::warn!("spotify connect: {err}");
                                }
                                cx.notify();
                            })
                            .ok();
                        })
                        .detach();
                        cx.notify();
                    },
                )
                .into_any_element(),
            );
        }
        if nook_core::queue::music_automation_denied() {
            rows.push(
                settings_row("music-tcc")
                    .child(label(
                        "Music Automation was denied. Grant it in System Settings → Privacy & Security → Automation to show Up Next in playlist.",
                        theme::SUBHEADLINE,
                        false,
                    ))
                    .into_any_element(),
            );
        }

        let location_note = match &settings.weather.location {
            WeatherLocationMode::System { .. } => {
                "Using a one-shot system fix (city-level). The Location Services grant is keyed to this build's signature and resets after an ad-hoc re-sign."
            }
            WeatherLocationMode::Manual { name, .. } if !name.is_empty() => {
                "Manual city. System location is opt-in and optional."
            }
            _ => "Enter a city (no permission prompt). System location is opt-in and resets on re-sign.",
        };

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
                "Compact face",
                settings_group(vec![toggle_row(
                    "Show on idle face",
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
            .when(self.module == WidgetModule::SysStats, |d| {
                d.child(self.render_sysstats_settings(settings, cx))
            })
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
                    "Physical interfaces only",
                    settings.sysstats.physical_nics,
                    cx,
                    |s| s.sysstats.physical_nics = !s.sysstats.physical_nics,
                )
                .into_any_element(),
            ]),
            Some("Samples only while the expanded card is visible. CPU and network need two ticks; a collapse longer than a few minutes resets the rates."),
        section(
            "Playing Next",
            settings_group(rows),
            Some(format!(
                "Register redirect URI {} on your Spotify developer app. No client secret is used. Apple Music shows upcoming tracks from the current playlist — not the real Playing Next queue — and hides the list when shuffle or radio is on.",
                nook_core::spotify::REDIRECT_URI
            )),
        )
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
                let chart_query = metric.query.clone();
                let alert_query = metric.query.clone();
                let chart_caption = metric.chart.caption();
                let alert_caption = metric.alert_caption();
                pinned_rows.push(
                    div()
                        .id(SharedString::from(format!("pin-{}", metric.query)))
                        .px(px(GROUP_PAD))
                        .py(px(6.))
                        .min_h(px(ROW_H))
                        .flex()
                        .items_center()
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
                        .child(push_button(
                            SharedString::from(format!("chart-{}", metric.query)),
                            chart_caption,
                            cx,
                            move |_, _, cx| {
                                cx.stop_propagation();
                                SettingsView::persist_observe(|s| {
                                    nook_core::observe::cycle_metric_chart(
                                        &mut s.observe,
                                        &chart_query,
                                    );
                                });
                                cx.notify();
                            },
                        ))
                        .child(push_button(
                            SharedString::from(format!("alert-{}", metric.query)),
                            alert_caption,
                            cx,
                            move |_, _, cx| {
                                cx.stop_propagation();
                                SettingsView::persist_observe(|s| {
                                    nook_core::observe::cycle_metric_alert(
                                        &mut s.observe,
                                        &alert_query,
                                    );
                                });
                                cx.notify();
                            },
                        ))
                        .child(push_button(
                            SharedString::from(format!("unpin-{}", metric.query)),
                            "Remove",
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
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                let path = path.clone();
                                nook_core::settings::tweak_app_settings(|s| {
                                    s.obsidian_vault = Some(path);
                                });
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(label(name, theme::BODY, true))
                                .child(label(
                                    if open { "Open in Obsidian" } else { "Registered" },
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
        WidgetModule::Calendar => {
            "Week strip is today ± 3 days. Type a line like “lunch tomorrow 12:30” to add an event.".into()
        }
        WidgetModule::Notes => "Scratchpad on the island. Edit here or in the expanded card.".into(),
        WidgetModule::Observe => {
            "Pinned metrics on the compact island and the expanded card.".into()
        }
            WidgetModule::Music => {
            "Now Playing from MediaRemote. Optional time-synced lyrics from LRCLIB — opt-in, fetched at runtime, never bundled.".into()
        WidgetModule::Music => {
            "Now Playing from MediaRemote on macOS. The output picker lists CoreAudio devices; it cannot start AirPlay to a HomePod or Apple TV.".into()
            "Now Playing from MediaRemote. Optional Apple Music motion art is opt-in and fails silent to static covers; the glow uses local artwork colors.".into()
            "Now Playing from MediaRemote on macOS, with an AppleScript fallback. Up Next lists the current Music playlist (unshuffled) or the Spotify Web API queue.".into()
        }
        WidgetModule::Files => "Drop zone and tray live on the Tray tab of the expanded island.".into(),
        WidgetModule::Timers => {
            "Island countdowns plus Apple Clock timers (read from mobiletimerd). Import the bundled Nook Clock shortcuts once to pause, resume, or cancel from the island.".into()
        }
        WidgetModule::Files => {
            "Drop zone and tray live on the Tray tab. Drag onto LocalSend or Get a link.".into()
        }
        WidgetModule::Timers => "Countdown presets and a compact ring while a timer is running.".into(),
        WidgetModule::Reminders => {
            "Incomplete reminders from EventKit. Type “remind me to …” to add one.".into()
        }
            "Countdown presets, a Pomodoro work/break cycle, and an optional Focus shortcut.".into()
        }
        WidgetModule::Reminders => "Incomplete reminders from EventKit, same store as Calendar.".into(),
        WidgetModule::Speed => "Cloudflare (then OVH) download probe. Runs from the island card.".into(),
        WidgetModule::Agents => {
            "Working coding-agent sessions on the compact face and expanded card.".into()
        }
        WidgetModule::Mirror => "A live camera preview that opens when you click the Mirror card.".into(),
        WidgetModule::Battery => {
            "Low-battery takeover on the compact face. Low Power Mode uses a one-time Shortcuts import, then falls back to an admin prompt.".into()
        WidgetModule::Messages => {
            "iMessage read + send from chat.db. WhatsApp is notify + prefill only — the Mac app cannot auto-send. Full Disk Access is required to read messages.".into()
        WidgetModule::Obsidian => {
            "Vault notes on the shelf. FSEvents keeps the list current; capture appends to today's daily note.".into()
        }
        WidgetModule::Mixer => nook_core::mixer::TCC_PREPROMPT.into(),
        WidgetModule::Weather => {
            "Current conditions and a short hourly strip from Open-Meteo. Manual city by default.".into()
        WidgetModule::Vpn => {
            "Live utun/ipsec/ppp status. The compact face flashes on connect and disconnect; the card shows the session clock. Ignore listed interfaces to hide helpers that look like a VPN.".into()
        WidgetModule::HighAlert => {
            "IOPM keep-awake. Timed chips expire in powerd — lid-close sleep is not prevented.".into()
        WidgetModule::SysStats => {
            "Live CPU, memory, network, and disk capacity. Idle cost is zero — sampling starts on expand and stops on collapse.".into()
        WidgetModule::Recorder => {
            "Record from the island. Transcription uses Apple's on-device Speech model when available; turn it off for long recordings.".into()
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
        .gap(px(6.))
        .child(section_header(header))
        .child(group)
        .when_some(footer, |d, text| {
            d.child(div().px(px(4.)).child(caption_text(text)))
        })
}

fn section_header(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px(px(4.))
        .text_size(px(theme::BODY.size))
        .line_height(px(theme::BODY.leading))
        .font_weight(FontWeight::NORMAL)
        .text_color(theme::SECONDARY_LABEL)
        .child(text.into())
}

fn caption_text(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(theme::SUBHEADLINE.size))
        .line_height(px(16.))
        .text_color(theme::SECONDARY_LABEL)
        .child(text.into())
}

fn high_alert_rows(
    settings: &AppSettings,
    cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    let duration = settings.high_alert_default_duration_secs;
    let kind = settings.high_alert_kind;
    let battery = settings.low_battery_release_pct;
    vec![
        chip_row(
            "Default duration",
            &[
                ("15m", duration == 15 * 60),
                ("30m", duration == 30 * 60),
                ("1h", duration == 60 * 60),
                ("On", duration == 0),
            ],
            cx,
            |caption, s| {
                s.high_alert_default_duration_secs = match caption {
                    "15m" => 15 * 60,
                    "1h" => 60 * 60,
                    "On" => 0,
                    _ => 30 * 60,
                };
            },
        )
        .into_any_element(),
        chip_row(
            "Keep awake",
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
            "Release below",
            &[
                ("Off", battery == 0),
                ("10%", battery == 10),
                ("20%", battery == 20),
            ],
            cx,
            |caption, s| {
                s.low_battery_release_pct = match caption {
                    "Off" => 0,
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
    let work_name = settings
        .focus_shortcut_work
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("None")
        .to_string();
    let break_name = settings
        .focus_shortcut_break
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("None")
        .to_string();
    let listed: Vec<String> = catalog.to_vec();
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
            "Long break",
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
            "Auto-advance phases",
            settings.pomodoro_auto_advance,
            cx,
            |s| s.pomodoro_auto_advance = !s.pomodoro_auto_advance,
        )
        .into_any_element(),
        toggle_row(
            "Keep awake on work",
            settings.pomodoro_keep_awake,
            cx,
            |s| s.pomodoro_keep_awake = !s.pomodoro_keep_awake,
        )
        .into_any_element(),
        action_row(
            "focus-work",
            "Work shortcut",
            work_name,
            cx,
            move |_, _, cx| {
                let next = nook_core::focus::cycle_shortcut(
                    nook_core::settings::get_app_settings()
                        .focus_shortcut_work
                        .as_deref(),
                    &listed,
                );
                nook_core::settings::tweak_app_settings(|s| s.focus_shortcut_work = next);
                cx.notify();
            },
        )
        .into_any_element(),
        action_row(
            "focus-break",
            "Break shortcut",
            break_name,
            cx,
            {
                let listed = catalog.to_vec();
                move |_, _, cx| {
                    let next = nook_core::focus::cycle_shortcut(
                        nook_core::settings::get_app_settings()
                            .focus_shortcut_break
                            .as_deref(),
                        &listed,
                    );
                    nook_core::settings::tweak_app_settings(|s| s.focus_shortcut_break = next);
                    cx.notify();
                }
            },
        )
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

fn settings_group(rows: Vec<AnyElement>) -> impl IntoElement {
    let mut group = div()
        .flex()
        .flex_col()
        .rounded(px(theme::INNER_RADIUS))
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
        .child(label("Alert below", theme::BODY, true))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(stepper_btn("thr-dec", "−", cx, move |_, _, cx| {
                    nook_core::settings::tweak_app_settings(|s| {
                        s.battery_alert_threshold =
                            nook_core::power::clamp_alert_threshold(
                                s.battery_alert_threshold.saturating_sub(5),
                            );
                    });
                    cx.notify();
                }))
                .child(
                    div()
                        .w(px(44.))
                        .flex()
                        .justify_center()
                        .child(label(format!("{value}%"), theme::BODY, true)),
                )
                .child(stepper_btn("thr-inc", "+", cx, move |_, _, cx| {
                    nook_core::settings::tweak_app_settings(|s| {
                        s.battery_alert_threshold =
                            nook_core::power::clamp_alert_threshold(
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
        .rounded(px(6.))
        .bg(theme::FILL)
        .hover(|s| s.bg(theme::FILL_SECONDARY))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(label(caption, theme::BODY, true))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                on_click(this, window, cx);
            }),
        )
fn status_row(title: &'static str, value: &'static str) -> impl IntoElement {
    settings_row(title)
        .child(label(title, theme::BODY, true))
        .child(label(value, theme::SUBHEADLINE, false))
}

fn shortcut_row(title: &'static str, keys: String) -> impl IntoElement {
    settings_row(SharedString::from(title))
        .child(label(title, theme::BODY, true))
        .child(label(keys, theme::SUBHEADLINE, false))
fn permission_row(title: &'static str, status: nook_core::eventtap::PermissionStatus) -> impl IntoElement {
    let (text, color) = match status {
        nook_core::eventtap::PermissionStatus::Granted => ("Granted", theme::SUCCESS),
        nook_core::eventtap::PermissionStatus::Denied => ("Not granted", theme::DESTRUCTIVE),
        nook_core::eventtap::PermissionStatus::Unsupported => ("macOS only", theme::TERTIARY_LABEL),
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
        .child(label(label_text, theme::BODY, true))
        .child(toggle_knob(on))
}

fn module_toggle(
    on: bool,
    module: WidgetModule,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("tog-{}", module.name())))
        .min_h(px(theme::HIT_MIN))
        .flex()
        .items_center()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                let mut s = nook_core::settings::get_app_settings();
                module.set_enabled(&mut s);
                nook_core::settings::update_app_settings(s);
                cx.notify();
            }),
        )
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
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                nook_core::settings::tweak_app_settings(|s| s.island_color = swatch.rgb);
                cx.notify();
            }),
        )
        .child(
            div()
                .size(px(16.))
                .rounded_full()
                .bg(fill)
                .when(on, |d| d.border_2().border_color(theme::LABEL))
                .when(!on, |d| d.border_1().border_color(rgba(0xffffff33))),
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
        .child(div().size(px(18.)).rounded_full().bg(rgb(0xffffff)))
}

fn segmented_group() -> gpui::Div {
    div()
        .h(px(theme::HIT_MIN))
        .p(px(2.))
        .rounded(px(6.))
        .bg(theme::FILL)
        .flex()
        .items_center()
}

fn segment(
    caption: &'static str,
    selected: bool,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("seg-{caption}")))
        .h(px(24.))
        .px(px(10.))
        .rounded(px(4.))
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
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                on_click(this, window, cx);
            }),
        )
}

fn push_button(
    id: impl Into<SharedString>,
    caption: impl Into<SharedString>,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    let caption = caption.into();
    div()
        .id(id.into())
        .h(px(theme::HIT_MIN))
        .px(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .bg(theme::FILL)
        .hover(|s| s.bg(theme::FILL_SECONDARY))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(label(caption, theme::CALLOUT, true))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                on_click(this, window, cx);
            }),
        )
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
        .child(label(title, theme::BODY, true).w(px(56.)))
        .child(
            div()
                .id(SharedString::from(format!("{id}-field")))
                .track_focus(&focus)
                .flex_1()
                .min_w(px(0.))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(5.))
                .bg(theme::FILL)
                .when(focused, |d| d.border_1().border_color(theme::accent()))
                .flex()
                .items_center()
                .cursor(CursorStyle::IBeam)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _, window, cx| {
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
                        .w_full()
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
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_token_is_masked_by_default() {
        assert_eq!(token_text("secret-value", false), "••••••••");
        assert_eq!(token_text("secret-value", true), "secret-value");
    }

    #[test]
    fn settings_window_is_landscape() {
        let (w, h) = SETTINGS_SIZE;
        assert_eq!((w, h), (780.0, 560.0));
        assert!(w > h, "default size stays landscape");
        let (min_w, min_h) = SETTINGS_MIN;
        assert!(min_w > min_h, "min size stays landscape");
        assert!(min_w >= 680.0 && min_h >= 480.0);
        assert!(w > SIDEBAR_W + 400.0, "pane has room beside the sidebar");
    }

    #[test]
    fn weather_subtitle_uses_the_saved_city() {
        let mut settings = AppSettings::default();
        assert_eq!(
            WidgetModule::Weather.subtitle(&settings).as_ref(),
            "Open-Meteo"
        );
        settings.weather.location = nook_core::weather::WeatherLocationMode::Manual {
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
            "Alert at 20%"
        );
        settings.battery_alert_threshold = 5;
        assert_eq!(
            WidgetModule::Battery.subtitle(&settings).as_ref(),
            "Alert at 5%"
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
                "Speed Test",
                "Agents",
                "Mirror",
                "Battery",
                "Messages",
                "Obsidian",
                "Mixer",
                "Weather",
                "VPN",
                "High Alert",
                "Stats",
                "Voice",
            ]
        );
        assert!(!names
            .iter()
            .any(|n| n.contains("Shortcuts") || n.contains("Tencent") || n.contains("License")));
    }

    #[test]
    fn window_snap_hotkeys_are_listed() {
        let rows: Vec<_> = nook_core::hotkeys::default_bindings()
            .into_iter()
            .map(|(kind, hotkey)| (kind.label(), hotkey.display()))
            .collect();
        assert_eq!(rows.len(), 9);
        assert!(rows.iter().any(|(n, k)| *n == "Left half" && k.contains('←')));
    }

    #[test]
    fn nav_enums_round_trip() {
        assert_eq!(SettingsCategory::from_u8(1), SettingsCategory::Widgets);
        assert_eq!(SettingsCategory::from_u8(2), SettingsCategory::Keyboard);
        assert_eq!(SettingsCategory::from_u8(3), SettingsCategory::Scrolling);
        assert_eq!(SettingsCategory::from_u8(0), SettingsCategory::General);
        assert_eq!(SettingsCategory::from_u8(99), SettingsCategory::General);
        assert_eq!(SettingsCategory::Keyboard.title(), "Keyboard");
        assert_eq!(SettingsCategory::Scrolling.title(), "Scrolling");
        assert_eq!(WidgetModule::from_u8(0), WidgetModule::Calendar);
        assert_eq!(WidgetModule::from_u8(99), WidgetModule::Calendar);
    }

    #[test]
    fn hud_caption_explains_bezel_suppression() {
        let off = AppSettings::default();
        assert!(!off.replace_system_hud);
        assert!(hud_caption(&off).as_ref().contains("system bezel"));
        let mut on = AppSettings::default();
        on.replace_system_hud = true;
        assert!(hud_caption(&on).as_ref().contains("caps-lock"));
    }

    #[test]
    fn remaining_cells_saturate_at_zero() {
        let settings = AppSettings::default();
        assert_eq!(AppSettings::TOTAL_CELLS, 11);
        assert!(settings.used_cells() >= AppSettings::TOTAL_CELLS);
        assert_eq!(settings.remaining_cells(), 0);
    }
}
