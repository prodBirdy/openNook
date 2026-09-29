//! Dev-only component gallery. Compiled only with `debug_assertions`.

use super::media::nook_media_pane;
use super::ui::{empty_state, label, nook_empty, nook_icon_btn, pill_btn, slide_label, text_btn};
use super::{CompactMode, Island, Tab, Timer, TimerKind};
use crate::theme;
use crate::widgets;
use gpui::{
    div, point, prelude::*, px, size, AnyElement, App, Context, CursorStyle, FontWeight,
    SharedString, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};
use nook_core::agents::{AgentKind, AgentSession, AgentStatus};
use nook_core::audio_devices::{OutputDevice, OutputTransport};
use nook_core::calendar::{CalendarEvent, Reminder};
use nook_core::files::FileTrayItem;
use nook_core::meetings::{MeetingApp, MeetingSnapshot, MeetingState};
use nook_core::messages::{IncomingPeek, MessageService};
use nook_core::models::{
    LyricLine, NowPlayingData, PlaybackQueue, QueueItem, QueueJump, QueueSource, RepeatMode,
    SyncedLyrics,
};
use nook_core::notifications::NotificationEvent;
use nook_core::observe::{
    ChartPoint, FiringAlert, MetricReading, ObserveChartKind, ObserveSnapshot, RangeSeries,
    SamplePoint, SeriesValue,
};
use nook_core::power::PowerSnapshot;
use nook_core::settings::{WidgetModule, WidgetSize};
use nook_core::share::SharePhase;
use nook_core::vpn::VpnSnapshot;
use nook_core::weather::{HourlyForecast, WeatherSnapshot, WeatherUnits};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

const GALLERY_SIZE: (f32, f32) = (1320.0, 1000.0);
const GALLERY_MIN: (f32, f32) = (1180.0, 640.0);
const MENU_BAR_STRIP: f32 = 6.0;
/// Gallery album art: a real 64x64 gradient PNG encoded in code (the old
/// hand-pasted base64 was corrupt, so every fixture fell back to the
/// music-note placeholder).
fn gallery_art_png() -> Vec<u8> {
    let mut img = image::RgbImage::new(64, 64);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let fx = x as f32 / 63.0;
        let fy = y as f32 / 63.0;
        *pixel = image::Rgb([
            (24.0 + 190.0 * fx) as u8,
            (60.0 + 110.0 * fy + 40.0 * fx) as u8,
            (110.0 + 120.0 * fy) as u8,
        ]);
    }
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("gallery art PNG encodes");
    encoded.into_inner()
}

fn gallery_art_base64() -> String {
    use base64::Engine;
    static CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| base64::engine::general_purpose::STANDARD.encode(gallery_art_png()))
        .clone()
}

const TAB_COMPACT: u8 = 0;
const TAB_MEDIA: u8 = 1;
const TAB_WIDGETS: u8 = 2;
const TAB_FILES: u8 = 3;
const TAB_PRIMITIVES: u8 = 4;

const GALLERY_TABS: &[(u8, &str)] = &[
    (TAB_COMPACT, "Compact"),
    (TAB_MEDIA, "Media"),
    (TAB_WIDGETS, "Widgets"),
    (TAB_FILES, "Files & Mirror"),
    (TAB_PRIMITIVES, "Primitives"),
];

fn tab_from_env(value: &str) -> u8 {
    match value {
        "compact" => TAB_COMPACT,
        "media" => TAB_MEDIA,
        "widgets" => TAB_WIDGETS,
        "files" => TAB_FILES,
        "primitives" => TAB_PRIMITIVES,
        _ => TAB_COMPACT,
    }
}

/// Launch options read once from the environment when opening the gallery.
struct GalleryLaunch {
    tab: u8,
    scroll_y: Option<f32>,
    motion: bool,
}

impl GalleryLaunch {
    fn from_env() -> Self {
        let tab = std::env::var("NOOK_GALLERY")
            .ok()
            .as_deref()
            .map(tab_from_env)
            .unwrap_or(TAB_COMPACT);
        let scroll_y = std::env::var("NOOK_GALLERY_SCROLL")
            .ok()
            .and_then(|s| s.parse::<f32>().ok());
        let motion = std::env::var("NOOK_GALLERY_MOTION").as_deref() == Ok("1");
        Self {
            tab,
            scroll_y,
            motion,
        }
    }
}

/// Open the gallery as its own normal window. Does not touch the live island.
pub(crate) fn open(cx: &mut App) {
    let launch = GalleryLaunch::from_env();
    let (w, h) = GALLERY_SIZE;
    let (min_w, min_h) = GALLERY_MIN;
    let bounds = gpui::Bounds::centered(None, size(px(w), px(h)), cx);
    let background = if crate::platform::reduce_transparency() {
        WindowBackgroundAppearance::Opaque
    } else {
        WindowBackgroundAppearance::Blurred
    };
    let _ = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("openNook Gallery".into()),
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
        move |_, cx| {
            cx.new(|cx| {
                let mut island = Island::mock();
                island.gallery_mode = true;
                island.reduce_motion = !launch.motion;
                island.focus = Some(cx.focus_handle());
                island.expanded = true;
                island.tab = Tab::Widgets;
                island.gallery_tab = launch.tab;
                fill_fixtures(&mut island);
                if let Some(value) = launch.scroll_y {
                    island
                        .gallery_scroll
                        .set_offset(point(px(0.), px(-value)));
                }
                island
            })
        },
    );
}

pub(crate) fn render(island: &mut Island, cx: &mut Context<Island>) -> AnyElement {
    super::marquee::set_animate(!island.reduce_motion);
    super::media::set_reduce_motion(island.reduce_motion);
    let selected = island.gallery_tab;
    let body = match selected {
        TAB_MEDIA => div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .items_start()
            .gap(px(28.))
            .child(section_title("Media card"))
            .child(media_section(island, cx)),
        TAB_WIDGETS => div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .items_start()
            .gap(px(28.))
            .child(section_title("Widgets"))
            .child(widgets_section(island, cx)),
        TAB_FILES => div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .items_start()
            .gap(px(28.))
            .child(section_title("Files & Mirror"))
            .child(files_section(island, cx)),
        TAB_PRIMITIVES => div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .items_start()
            .gap(px(28.))
            .child(section_title("Shared primitives"))
            .child(primitives_section(island, cx)),
        _ => div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .items_start()
            .gap(px(28.))
            .child(section_title("Compact faces"))
            .child(compact_section(island, cx)),
    };
    div()
        .id("gallery-root")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme::WINDOW_BG)
        .text_color(theme::LABEL)
        .child(
            div()
                .id("gallery-scroll")
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .overflow_y_scroll()
                .overflow_x_scroll()
                .track_scroll(&island.gallery_scroll)
                .p(px(24.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_shrink_0()
                        .items_start()
                        .gap(px(28.))
                        .child(header(island, cx))
                        .child(body),
                ),
        )
        .into_any_element()
}

fn header(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let motion_label = if island.reduce_motion {
        "Motion: Off"
    } else {
        "Motion: On"
    };
    let selected = island.gallery_tab;
    let mut tabs = segmented_group();
    for &(id, caption) in GALLERY_TABS {
        tabs = tabs.child(gallery_tab(caption, selected == id, id, cx));
    }
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .flex_shrink_0()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(label("openNook Gallery", theme::TITLE_2, true))
                        .child(
                            label(
                                "Dev-only. Transport clicks do not control the real player.",
                                theme::BODY,
                                false,
                            )
                            .text_color(theme::TEXT_MUTED),
                        ),
                )
                .child(text_btn(motion_label, cx, |this, _, cx| {
                    this.reduce_motion = !this.reduce_motion;
                    cx.notify();
                })),
        )
        .child(tabs)
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

fn gallery_tab(
    caption: &'static str,
    selected: bool,
    tab: u8,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("gallery-tab-{caption}")))
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
            gpui::MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                if this.gallery_tab != tab {
                    this.gallery_tab = tab;
                    this.gallery_scroll.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                }
                cx.stop_propagation();
            }),
        )
}

fn section_title(title: &'static str) -> impl IntoElement {
    label(title, theme::TITLE_2, true)
}

fn caption(text: impl Into<SharedString>) -> impl IntoElement {
    label(text, theme::FOOTNOTE, false).text_color(theme::TEXT_MUTED)
}

/// Real Nook cell chrome: ROW_RADIUS, tertiary border, NOOK_BODY tall.
fn nook_cell(w: f32, child: impl IntoElement) -> impl IntoElement {
    nook_cell_h(w, theme::NOOK_BODY, child)
}

fn nook_cell_h(w: f32, h: f32, child: impl IntoElement) -> impl IntoElement {
    div()
        .w(px(w))
        .h(px(h))
        .flex_shrink_0()
        .rounded(px(theme::ROW_RADIUS))
        .overflow_hidden()
        .border_1()
        .border_color(theme::FILL_TERTIARY)
        .child(child)
}

/// Expanded-island backdrop: black fill + NOOK_INSET padding around cells.
fn island_panel(child: impl IntoElement) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .rounded(px(theme::ROW_RADIUS))
        .bg(theme::ISLAND)
        .p(px(theme::NOOK_INSET))
        .child(child)
}

/// Compact face frame: flat top (menu-bar hang), bottom COMPACT_RADIUS only.
fn compact_frame(w: f32, h: f32, child: impl IntoElement) -> impl IntoElement {
    let r = theme::COMPACT_RADIUS.min(h * 0.5);
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .child(
            div()
                .w(px(w.max(1.0)))
                .h(px(MENU_BAR_STRIP))
                .bg(theme::FILL_TERTIARY),
        )
        .child(
            div()
                .w(px(w.max(1.0)))
                .h(px(h.max(1.0)))
                .overflow_hidden()
                .rounded_bl(px(r))
                .rounded_br(px(r))
                .bg(theme::ISLAND)
                .child(child),
        )
}

fn labelled(name: impl Into<SharedString>, child: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .flex_shrink_0()
        .child(caption(name))
        .child(child)
        .into_any_element()
}

fn wrap_row(children: Vec<AnyElement>) -> impl IntoElement {
    let mut row = div().flex().flex_row().flex_wrap().gap(px(16.));
    for child in children {
        row = row.child(child);
    }
    row
}

fn compact_section(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    let modes = [
        CompactMode::Idle,
        CompactMode::Media,
        CompactMode::Agents,
        CompactMode::Files,
        CompactMode::Timer,
        CompactMode::Observe,
        CompactMode::Battery,
        CompactMode::Vpn,
        CompactMode::Recording,
        CompactMode::Meeting,
        CompactMode::Notifications,
        CompactMode::Onboard,
        CompactMode::Messages,
        CompactMode::Share,
    ];
    let mut items = Vec::new();
    for mode in modes {
        items.push(compact_face(island, mode, false, cx));
        items.push(compact_face(island, mode, true, cx));
    }
    wrap_row(items)
}

fn compact_face(
    island: &mut Island,
    mode: CompactMode,
    hovered: bool,
    cx: &mut Context<Island>,
) -> AnyElement {
    let prev_expanded = island.expanded;
    let prev_alert = island.alert_preferred;
    let prev_user = island.user_preferred;
    let prev_pref = island.preferred;
    let prev_hover = island.hovered;

    island.expanded = false;
    island.hovered = hovered;
    island.alert_preferred = Some(mode);
    island.user_preferred = Some(mode);
    island.preferred = Some(mode);

    let (w, h) = island.target_size();
    let notch_w = island.notch_width.max(1.0);
    let name = format!(
        "{mode:?} · {}",
        if hovered { "hover" } else { "rest" }
    );
    let show_hover_title = hovered && mode == CompactMode::Media && island.has_media();
    let face = compact_frame(w.max(1.0), h.max(1.0), {
        let compact = island.render_compact(mode, hovered, notch_w, cx);
        if show_hover_title {
            // The live island paints this line in render.rs, outside
            // render_compact — mirror it so the hover face matches.
            div()
                .relative()
                .size_full()
                .child(compact)
                .child(island.compact_media_hover_title(1.0))
                .into_any_element()
        } else {
            compact.into_any_element()
        }
    });

    island.expanded = prev_expanded;
    island.alert_preferred = prev_alert;
    island.user_preferred = prev_user;
    island.preferred = prev_pref;
    island.hovered = prev_hover;

    labelled(name, face)
}

fn media_section(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    let cell_w = island.nook_cell_width();
    let pane_w = 5.0 * cell_w;
    let mut items = Vec::new();

    items.push(media_variant(
        island,
        cx,
        "Now Playing · playing",
        pane_w,
        |i| {
            i.now_playing = playing_track(false);
            i.queue_open = false;
            i.lyrics_open = false;
            i.output_picker_open = false;
        },
    ));
    items.push(media_variant(
        island,
        cx,
        "Now Playing · paused",
        pane_w,
        |i| {
            i.now_playing = playing_track(true);
            i.now_playing.is_playing = false;
            i.queue_open = false;
            i.lyrics_open = false;
            i.output_picker_open = false;
        },
    ));
    items.push(media_variant(
        island,
        cx,
        "Now Playing · Not Playing",
        pane_w,
        |i| {
            i.now_playing = NowPlayingData {
                app_name: Some("Music".into()),
                ..Default::default()
            };
            i.queue_open = false;
            i.lyrics_open = false;
            i.output_picker_open = false;
        },
    ));
    items.push(media_variant(
        island,
        cx,
        "Up Next · items",
        pane_w,
        |i| {
            i.now_playing = playing_track(false);
            i.queue = filled_queue();
            i.queue_open = true;
            i.lyrics_open = false;
            i.output_picker_open = false;
        },
    ));
    items.push(media_variant(
        island,
        cx,
        "Up Next · empty",
        pane_w,
        |i| {
            i.now_playing = playing_track(false);
            i.queue = PlaybackQueue {
                source: Some(QueueSource::MusicPlaylist),
                label: "Up Next".into(),
                items: Vec::new(),
                hidden: None,
                context_uri: None,
            };
            i.queue_open = true;
            i.lyrics_open = false;
            i.output_picker_open = false;
        },
    ));
    items.push(media_variant(
        island,
        cx,
        "Up Next · streaming",
        pane_w,
        |i| {
            i.now_playing = playing_track(false);
            i.queue = PlaybackQueue {
                source: Some(QueueSource::MusicPlaylist),
                label: "Up Next".into(),
                items: Vec::new(),
                hidden: Some(nook_core::models::QueueHidden::Streaming),
                context_uri: None,
            };
            i.queue_open = true;
            i.lyrics_open = false;
            i.output_picker_open = false;
        },
    ));
    items.push(media_variant(island, cx, "Lyrics", pane_w, |i| {
        i.now_playing = playing_track(false);
        i.lyrics = Some(Arc::new(gallery_lyrics()));
        // Anchor on line 3 (0-based index 2) so past + next lines show.
        i.lyrics_anchor_elapsed = 4.5;
        i.lyrics_anchor_at = Instant::now();
        i.now_playing.is_playing = false;
        i.now_playing.elapsed_time = Some(4.5);
        i.lyrics_open = true;
        i.queue_open = false;
        i.output_picker_open = false;
    }));
    items.push(media_variant(island, cx, "AirPlay", pane_w, |i| {
        i.now_playing = playing_track(false);
        i.output_picker_open = true;
        i.queue_open = false;
        i.lyrics_open = false;
    }));

    island_panel(wrap_row(items))
}

fn media_variant(
    island: &mut Island,
    cx: &mut Context<Island>,
    name: &str,
    pane_w: f32,
    setup: impl FnOnce(&mut Island),
) -> AnyElement {
    let snap = MediaSnap::capture(island);
    setup(island);
    island.media_view_fade.set(1.0);
    island.media_view_shift.set(0.0);
    let pane = nook_cell(pane_w, nook_media_pane(island, cx));
    snap.restore(island);
    labelled(name.to_string(), pane)
}

struct MediaSnap {
    now_playing: NowPlayingData,
    queue: PlaybackQueue,
    lyrics: Option<Arc<SyncedLyrics>>,
    lyrics_anchor_elapsed: f64,
    lyrics_anchor_at: Instant,
    queue_open: bool,
    lyrics_open: bool,
    output_picker_open: bool,
}

impl MediaSnap {
    fn capture(island: &Island) -> Self {
        Self {
            now_playing: island.now_playing.clone(),
            queue: island.queue.clone(),
            lyrics: island.lyrics.clone(),
            lyrics_anchor_elapsed: island.lyrics_anchor_elapsed,
            lyrics_anchor_at: island.lyrics_anchor_at,
            queue_open: island.queue_open,
            lyrics_open: island.lyrics_open,
            output_picker_open: island.output_picker_open,
        }
    }

    fn restore(self, island: &mut Island) {
        island.now_playing = self.now_playing;
        island.queue = self.queue;
        island.lyrics = self.lyrics;
        island.lyrics_anchor_elapsed = self.lyrics_anchor_elapsed;
        island.lyrics_anchor_at = self.lyrics_anchor_at;
        island.queue_open = self.queue_open;
        island.lyrics_open = self.lyrics_open;
        island.output_picker_open = self.output_picker_open;
    }
}

fn widgets_section(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    let cell_w = island.nook_cell_width();
    // Music is covered by the media section; Files / Mirror / Observe-big live
    // on the Files & Mirror tab.
    let modules = [
        WidgetModule::Calendar,
        WidgetModule::Agents,
        WidgetModule::Meeting,
        WidgetModule::Observe,
        WidgetModule::Reminders,
        WidgetModule::Timers,
        WidgetModule::Notes,
        WidgetModule::Obsidian,
        WidgetModule::Speed,
        WidgetModule::Battery,
        WidgetModule::Messages,
        WidgetModule::Weather,
        WidgetModule::Vpn,
        WidgetModule::HighAlert,
        WidgetModule::SysStats,
        WidgetModule::Recorder,
        WidgetModule::Notifications,
    ];
    let mut col = div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .items_start()
        .gap(px(20.));
    for module in modules {
        let sizes = island.settings.distinct_sizes(module);
        let mut items = Vec::new();
        for size in sizes {
            let cells = match size {
                WidgetSize::Small => module.min_cells(),
                WidgetSize::Medium => module.default_cells(),
                WidgetSize::Large => island.settings.max_cells_for(module),
            };
            let w = cells as f32 * cell_w;
            let name = format!("{module:?} · {}", size.label());
            items.push(labelled(
                name,
                nook_cell(w, widget_card(island, module, cx)),
            ));
        }
        col = col.child(island_panel(wrap_row(items)));
    }
    col
}

fn files_section(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    let cell_w = island.nook_cell_width();
    let files_w = (island.expanded_width() - 2.0 * theme::NOOK_INSET).max(cell_w * 4.0);
    let files_h = super::files::files_pane_min_height(files_w).max(theme::NOOK_BODY);
    let observe_w = island.expanded_width() - 2.0 * theme::NOOK_INSET;
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .items_start()
        .gap(px(20.))
        .child(island_panel(wrap_row(vec![
            labelled(
                "Files · tray",
                nook_cell_h(files_w, files_h, island.render_files(cx)),
            ),
            labelled(
                "Mirror · off",
                nook_cell(
                    module_cells_w(WidgetModule::Mirror, cell_w, island),
                    super::expanded::mirror_pane(island, cx),
                ),
            ),
        ])))
        .child(island_panel(labelled(
            "Observe · big view",
            nook_cell_h(
                observe_w,
                widgets::OBSERVE_EXPANDED_BODY,
                widgets::observe_big_view(
                    &island.observe,
                    &island.settings,
                    island.observe_hover.as_ref(),
                    cx,
                ),
            ),
        )))
}

fn module_cells_w(module: WidgetModule, cell_w: f32, island: &Island) -> f32 {
    island.settings.max_cells_for(module).max(module.default_cells()) as f32 * cell_w
}

fn widget_card(island: &mut Island, module: WidgetModule, cx: &mut Context<Island>) -> AnyElement {
    match module {
        WidgetModule::Calendar => {
            widgets::calendar_card(&island.events, island.calendar_day, cx).into_any_element()
        }
        WidgetModule::Agents => widgets::agents_card(
            &island.agents,
            island.pixel_t,
            theme::island_fill(theme::island_color(&island.settings)),
            false,
            cx,
        )
        .into_any_element(),
        WidgetModule::Meeting => widgets::meeting_card(&island.meeting, cx).into_any_element(),
        WidgetModule::Observe => widgets::observe_card(
            &island.observe,
            &island.settings,
            island.observe_hover.as_ref(),
            cx,
        )
        .into_any_element(),
        WidgetModule::Reminders => {
            let qa = island.ensure_reminders_quick_add(cx);
            widgets::reminders_card(&island.reminders, Some(qa), cx).into_any_element()
        }
        WidgetModule::Timers => widgets::timer_card(island, cx).into_any_element(),
        WidgetModule::Notes => widgets::notes_card(island, cx).into_any_element(),
        WidgetModule::Obsidian => widgets::obsidian_card(island, cx).into_any_element(),
        WidgetModule::Speed => {
            widgets::speed_card(island.speed_mbps, island.speed_progress, island.speed_running, cx)
                .into_any_element()
        }
        WidgetModule::Battery => widgets::battery_card(island, cx).into_any_element(),
        WidgetModule::Messages => widgets::messages_card(island, cx).into_any_element(),
        WidgetModule::Weather => widgets::weather_card(island, cx).into_any_element(),
        WidgetModule::Vpn => widgets::vpn_card(&island.vpn).into_any_element(),
        WidgetModule::HighAlert => widgets::high_alert_card(island, cx).into_any_element(),
        WidgetModule::SysStats => widgets::sysstats_card(island, cx).into_any_element(),
        WidgetModule::Recorder => widgets::recorder_card(island, cx).into_any_element(),
        WidgetModule::Notifications => {
            widgets::notifications_card(&island.notifications, cx).into_any_element()
        }
        WidgetModule::Music | WidgetModule::Files | WidgetModule::Mirror => {
            // Handled outside widget_card (media section / dedicated panes).
            div().into_any_element()
        }
    }
}

fn primitives_section(_island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    wrap_row(vec![
        labelled(
            "label · strong",
            primitive_frame(
                160.0,
                40.0,
                div()
                    .p(px(8.))
                    .child(label("Primary label", theme::BODY, true)),
            ),
        ),
        labelled(
            "label · muted",
            primitive_frame(
                160.0,
                40.0,
                div()
                    .p(px(8.))
                    .child(label("Secondary label", theme::BODY, false)),
            ),
        ),
        labelled(
            "slide_label",
            primitive_frame(
                180.0,
                40.0,
                div().p(px(8.)).child(slide_label(
                    "A long sliding title for the gallery",
                    theme::BODY,
                    true,
                )),
            ),
        ),
        labelled(
            "text_btn",
            primitive_frame(
                140.0,
                48.0,
                div()
                    .p(px(8.))
                    .child(text_btn("Text", cx, |_, _, _| {})),
            ),
        ),
        labelled(
            "pill_btn",
            primitive_frame(
                160.0,
                48.0,
                div()
                    .p(px(8.))
                    .child(pill_btn("Create Timer", cx, |_, _, _| {})),
            ),
        ),
        labelled(
            "nook_icon_btn",
            primitive_frame(
                56.0,
                56.0,
                div()
                    .p(px(8.))
                    .child(nook_icon_btn("play", "gallery-play", cx, |_, _, _, _| {})),
            ),
        ),
        labelled(
            "nook_empty",
            primitive_frame_padded(180.0, theme::NOOK_BODY, nook_empty("music", "Nothing here")),
        ),
        labelled(
            "empty_state",
            primitive_frame_padded(
                240.0,
                theme::NOOK_BODY,
                empty_state(
                    "No widgets enabled",
                    pill_btn("Customize", cx, |_, _, _| {}),
                ),
            ),
        ),
    ])
}

fn primitive_frame(w: f32, h: f32, child: impl IntoElement) -> impl IntoElement {
    div()
        .w(px(w))
        .h(px(h))
        .flex_shrink_0()
        .rounded(px(theme::ROW_RADIUS))
        .overflow_hidden()
        .bg(theme::ISLAND)
        .child(child)
}

/// Empty states live in NOOK_BODY-tall panes with 16pt insets — frame them
/// that way so the icon and copy sit where they do in the real island.
fn primitive_frame_padded(w: f32, h: f32, child: impl IntoElement) -> impl IntoElement {
    div()
        .w(px(w))
        .h(px(h))
        .flex_shrink_0()
        .rounded(px(theme::ROW_RADIUS))
        .overflow_hidden()
        .bg(theme::ISLAND)
        .flex()
        .flex_col()
        .p(px(16.))
        .child(child)
}

fn playing_track(long_title: bool) -> NowPlayingData {
    NowPlayingData {
        title: Some(if long_title {
            "A Very Long Title That Should Overflow The Compact Hover Line And The Media Pane"
                .into()
        } else {
            "Gallery Track".into()
        }),
        artist: Some("openNook".into()),
        album: Some("Fixtures".into()),
        artwork_base64: Some(gallery_art_base64()),
        duration: Some(240.0),
        elapsed_time: Some(72.0),
        is_playing: true,
        audio_levels: Some(vec![0.2, 0.5, 0.8, 0.4, 0.6, 0.3]),
        app_name: Some("Music".into()),
        bundle_id: Some("com.apple.Music".into()),
        motion_artwork_url: None,
        shuffle: Some(false),
        repeat: Some(RepeatMode::Off),
    }
}

fn filled_queue() -> PlaybackQueue {
    PlaybackQueue {
        source: Some(QueueSource::MusicPlaylist),
        label: "Up Next in playlist".into(),
        items: vec![
            QueueItem {
                id: "q1".into(),
                title: "Next Up".into(),
                artist: "Artist A".into(),
                artwork_url: None,
                artwork_base64: Some(gallery_art_base64()),
                duration: Some(200.0),
                source: QueueSource::MusicPlaylist,
                jump: QueueJump::MusicTrack { index: 2 },
            },
            QueueItem {
                id: "q2".into(),
                title: "After That".into(),
                artist: "Artist B".into(),
                artwork_url: None,
                artwork_base64: None,
                duration: Some(180.0),
                source: QueueSource::MusicPlaylist,
                jump: QueueJump::MusicTrack { index: 3 },
            },
        ],
        hidden: None,
        context_uri: None,
    }
}

fn gallery_lyrics() -> SyncedLyrics {
    SyncedLyrics {
        lines: vec![
            LyricLine {
                time_ms: 0,
                text: "Short".into(),
            },
            LyricLine {
                time_ms: 2_000,
                text: "Medium length line".into(),
            },
            LyricLine {
                time_ms: 4_000,
                text: "This is a deliberately very long lyric line that should wrap inside the pane"
                    .into(),
            },
            LyricLine {
                time_ms: 8_000,
                text: "Next line after".into(),
            },
            LyricLine {
                time_ms: 11_000,
                text: "Almost done".into(),
            },
            LyricLine {
                time_ms: 14_000,
                text: "Final line".into(),
            },
        ],
        plain: None,
        instrumental: false,
        source: "gallery".into(),
    }
}

fn fill_fixtures(island: &mut Island) {
    // Owner's MacBook notch so compact faces match real compact size.
    island.notch_width = 220.0;
    island.notch_height = 38.0;

    let settings = &mut island.settings;
    settings.show_media = true;
    settings.show_media_queue = true;
    settings.show_lyrics = true;
    settings.audio_output_picker = true;
    settings.show_calendar = true;
    settings.show_agents = true;
    settings.show_observe = true;
    settings.experimental_widgets = true;
    settings.show_reminders = true;
    settings.show_timers = true;
    settings.show_notes = true;
    settings.show_speed = true;
    settings.show_battery = true;
    settings.show_messages = true;
    settings.show_vpn = true;
    settings.show_sysstats = true;
    settings.show_recorder = true;
    settings.show_notifications = true;
    settings.show_files = true;
    settings.show_high_alert = true;
    settings.show_meetings = true;
    settings.show_obsidian = true;
    settings.show_mirror = true;
    settings.weather.enabled = true;
    settings.observe.prometheus_url = "http://127.0.0.1:9090".into();
    settings.battery_alert_threshold = 20;
    for module in WidgetModule::ALL {
        if module.occupies_nook_cells() {
            let _ = settings.set_enabled(module, true);
        }
    }

    island.now_playing = playing_track(false);
    island.queue = filled_queue();
    island.lyrics = Some(Arc::new(gallery_lyrics()));
    // Park on line 3 (index 2, time_ms 4000) so past + next lines are visible.
    island.lyrics_anchor_elapsed = 4.5;
    island.lyrics_anchor_at = Instant::now();

    island.output_devices = vec![
        OutputDevice {
            id: 1,
            name: "MacBook Pro Speakers".into(),
            transport: OutputTransport::BuiltIn,
            is_default: true,
        },
        OutputDevice {
            id: 2,
            name: "AirPods Pro".into(),
            transport: OutputTransport::Bluetooth,
            is_default: false,
        },
        OutputDevice {
            id: 3,
            name: "Living Room".into(),
            transport: OutputTransport::AirPlay,
            is_default: false,
        },
    ];

    // Real thumbnail bytes so the image tile renders instead of a dark box.
    let art_bytes = gallery_art_png();
    let art_path = std::env::temp_dir()
        .join("openNook-gallery")
        .join("shot.png");
    if let Some(dir) = art_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&art_path, &art_bytes);
    island.files = vec![
        FileTrayItem {
            name: "shot.png".into(),
            size: art_bytes.len() as i64,
            path: art_path.to_string_lossy().into_owned(),
            mime_type: "image/png".into(),
            last_modified: 0,
        },
        FileTrayItem {
            name: "notes.txt".into(),
            size: 420,
            path: "/tmp/notes.txt".into(),
            mime_type: "text/plain".into(),
            last_modified: 0,
        },
    ];

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    island.events = vec![CalendarEvent {
        id: "ev1".into(),
        title: "Design review".into(),
        start_date: now + 3600.0,
        end: Some(now + 5400.0),
        location: Some("HQ".into()),
        is_all_day: false,
    }];
    island.reminders = vec![Reminder {
        id: "r1".into(),
        title: "Ship gallery".into(),
        due_date: Some(now + 7200.0),
        is_completed: false,
        list_color: "#0A84FF".into(),
    }];
    island.agents = vec![AgentSession {
        kind: AgentKind::Claude,
        pid: 4242,
        project: "openNook".into(),
        cwd: "/tmp/openNook".into(),
        status: AgentStatus::Working,
        session_id: Some("sess".into()),
        name: Some("Gallery session".into()),
        model: Some("opus".into()),
    }];
    island.notes = "# Gallery\n\n- one\n- two".into();
    island.timers = vec![Timer {
        id: 1,
        name: "Focus".into(),
        remaining: 12 * 60 + 34,
        total: 25 * 60,
        running: true,
        kind: TimerKind::Countdown,
        ends_at: Some(SystemTime::now() + Duration::from_secs(12 * 60 + 34)),
    }];
    island.next_timer_id = 2;

    // Below alert threshold so Battery compact mode is available.
    island.power = PowerSnapshot {
        percent: Some(15),
        is_charging: false,
        on_ac: false,
        time_to_empty_min: Some(40),
        warning_level: nook_core::power::BatteryWarning::Early,
        low_power_mode: false,
        has_battery: true,
    };
    island.vpn = VpnSnapshot {
        connected: true,
        service_name: "WireGuard".into(),
        interface: "utun4".into(),
        since: Some(SystemTime::now() - Duration::from_secs(600)),
        since_estimated: false,
        tunnel_count: 1,
    };
    island.meeting = MeetingSnapshot {
        state: MeetingState::InMeeting {
            app: MeetingApp::Zoom,
            pid: 99,
            muted: Some(false),
            started: Instant::now() - Duration::from_secs(320),
        },
        accessibility_trusted: true,
    };
    island.notifications = vec![NotificationEvent::new(
        "com.apple.MobileSMS",
        "Messages",
        "Alex",
        "",
        "See you at 5?",
        now as i64 - 300,
    )];
    island.notification_unread = 1;
    island.recording = true;
    island.recording_started = Some(Instant::now() - Duration::from_secs(45));
    island.live_transcript = "Gallery transcript sample…".into();
    island.recorder_level = 0.55;
    island.recorder_wave = std::collections::VecDeque::from(vec![0.1, 0.4, 0.7, 0.3, 0.5, 0.2]);
    island.speed_mbps = Some(212.4);
    island.speed_progress = 1.0;

    island.messages.incoming = Some(IncomingPeek {
        conversation_id: "iMessage;-;+15551212".into(),
        sender: "Alex".into(),
        snippet: "See you at 5?".into(),
        service: MessageService::IMessage,
        last_date: now,
        last_rowid: 42,
    });

    island.share.phase = SharePhase::Transferring;
    island.share.status = "Sending".into();
    island.share.progress = 0.42;

    // Onboard compact face.
    island.first_run = true;

    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let points: Vec<SamplePoint> = (0..30)
        .map(|i| SamplePoint {
            ts: ts - (30 - i) as f64 * 30.0,
            value: 10.0 + (i as f64 * 1.7) % 40.0,
        })
        .collect();
    island.observe = ObserveSnapshot {
        connected: true,
        error: None,
        metrics: vec![
            MetricReading {
                label: "2xx".into(),
                query: "2xx".into(),
                chart: ObserveChartKind::Bars,
                values: vec![SeriesValue {
                    name: "2xx".into(),
                    value: 120.0,
                }],
                series: vec![RangeSeries {
                    name: "2xx".into(),
                    points: points.clone(),
                }],
                error: None,
                history: points
                    .iter()
                    .enumerate()
                    .map(|(i, p)| ChartPoint {
                        t: i as f32 / 29.0,
                        value: p.value,
                    })
                    .collect(),
                window_total: Some(1200.0),
            },
            MetricReading {
                label: "5xx".into(),
                query: "5xx".into(),
                chart: ObserveChartKind::Bars,
                values: vec![SeriesValue {
                    name: "5xx".into(),
                    value: 3.0,
                }],
                series: vec![RangeSeries {
                    name: "5xx".into(),
                    points: points
                        .iter()
                        .map(|p| SamplePoint {
                            ts: p.ts,
                            value: (p.value * 0.05).max(0.0),
                        })
                        .collect(),
                }],
                error: None,
                history: Vec::new(),
                window_total: Some(12.0),
            },
        ],
        alerts: vec![FiringAlert {
            name: "5xx".into(),
            severity: "critical".into(),
            summary: "2 > 0".into(),
        }],
    };

    island.weather = Some(WeatherSnapshot {
        location_name: "Cupertino".into(),
        latitude: 37.32,
        longitude: -122.03,
        units: WeatherUnits::Celsius,
        temperature: 22.0,
        feels_like: 21.0,
        wmo_code: 1,
        is_day: true,
        high: Some(26.0),
        low: Some(14.0),
        precip_probability: Some(10),
        wind_speed: Some(8.0),
        humidity: Some(55),
        uv_index: Some(4),
        hourly: (0..6)
            .map(|h| HourlyForecast {
                hour: format!("{h:02}:00"),
                temperature: 20.0 + h as f64,
                wmo_code: 1,
            })
            .collect(),
        fetched_at: Instant::now(),
    });

    island.sysstats = nook_core::sysstats::SysSnapshot {
        cpu_pct: Some(28.0),
        per_core: vec![20.0, 30.0, 25.0, 35.0],
        mem_used: 12_000_000_000,
        mem_total: 32_000_000_000,
        net_up_bps: Some(400_000.0),
        net_down_bps: Some(1_200_000.0),
        disk_used: 400_000_000_000,
        disk_total: 1_000_000_000_000,
    };

    // Keep gallery island expanded so media panel visibility helpers agree.
    island.expanded = true;
    island.tab = Tab::Widgets;
    island.media_view_fade.set(1.0);
}
