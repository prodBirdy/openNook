//! Compact album chip, visualizer, and expanded Now Playing pane.

use super::ui::{
    card_chrome, label, scroll_body, slide_label, timer_text, MEDIA_ART, MEDIA_ART_RADIUS,
    MEDIA_PLAY, MEDIA_PROGRESS_HIT, MEDIA_TIME_PAD_GAP, MEDIA_TIME_PAD_TOP,
};
use super::Island;
use nook_core::audio_devices::{OutputDevice, OutputTransport};
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    canvas, div, img, linear_color_stop, linear_gradient, point, prelude::*, px, relative,
    AnyElement, BoxShadow, Context, CursorStyle, Font, FontFeatures, FontStyle, FontWeight,
    Image, MouseButton, MouseDownEvent, Rgba, SharedString,
};
use nook_core::models::{NowPlayingData, PlaybackQueue, QueueItem, RepeatMode};
use std::sync::{Mutex, OnceLock};

const MAX_ARTWORK_BYTES: usize = 5 * 1024 * 1024;
const MAX_ARTWORK_DIMENSION: u32 = 4096;

/// Compact album art — mockup 22×22, radius 6, 1px white 12% border.
const COMPACT_ART: f32 = 22.0;
const COMPACT_ART_RADIUS: f32 = 6.0;
const COMPACT_ART_BORDER: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.12,
};
const ART: f32 = MEDIA_ART;
const ART_RADIUS: f32 = MEDIA_ART_RADIUS;
const PLAY: f32 = MEDIA_PLAY;
const SKIP_GAP: f32 = 36.0;
/// Room for ~15 title glyphs at Title 2, beside the artwork.
const TITLE_COL: f32 = 120.0;
/// Compact waveform: 5 bars × 2pt, gap 2, max height 14 (`#FF7A4D`).
const VIS_BAR_W: f32 = 2.0;
const VIS_BAR_GAP: f32 = 2.0;
const VIS_H: f32 = 14.0;
const VIS_BARS: usize = 5;
/// Shortest a playing bar may shrink to.
const VIS_MIN_H: f32 = 2.0;
/// Resting heights as fractions of [`VIS_H`] (7 / 12 / 9 / 14 / 8).
const VIS_REST: [f32; 5] = [0.5, 12.0 / 14.0, 9.0 / 14.0, 1.0, 8.0 / 14.0];
const VIS_DEFAULT: Rgba = Rgba {
    r: 1.0,
    g: 122.0 / 255.0,
    b: 77.0 / 255.0,
    a: 1.0,
};
const ART_PLACEHOLDER: (Rgba, Rgba) = (
    Rgba {
        r: 0.165,
        g: 0.165,
        b: 0.165,
        a: 1.0,
    },
    Rgba {
        r: 0.067,
        g: 0.067,
        b: 0.067,
        a: 1.0,
    },
);

thread_local! {
    static REDUCE_MOTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Called from pane entry points so the compact visualizer (owned call site
/// in compact.rs) can honour Accessibility › Reduce Motion.
pub(super) fn set_reduce_motion(on: bool) {
    REDUCE_MOTION.set(on);
}

fn reduce_motion() -> bool {
    REDUCE_MOTION.get() || crate::platform::reduce_motion()
}

/// Target opacity for the compact play/pause scrim; `Island::overlay_fade`
/// springs to it on `motion::REVEAL`. GPUI `.hover()` / `on_hover` stick
/// after the full-screen overlay goes click-through (no MouseMove, so no
/// MouseLeave) — drive this from the polled island hover pad instead.
pub(super) fn album_overlay_target(island_hovered: bool) -> f32 {
    if island_hovered {
        1.0
    } else {
        0.0
    }
}

pub(super) fn album_chip(
    np: &NowPlayingData,
    overlay_alpha: f32,
    reduce_motion: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    set_reduce_motion(reduce_motion);
    let playing = np.is_playing;
    let art = np
        .artwork_base64
        .as_deref()
        .and_then(|b64| artwork_element(b64, COMPACT_ART, COMPACT_ART_RADIUS));
    let overlay_icon = if playing { "media-pause" } else { "media-play" };

    div()
        .id("album-hit")
        .size(px(theme::HIT_MIN))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.note_media_play_pause(cx);
                if this.gallery_mode {
                    return;
                }
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_play_pause().await;
                });
            }),
        )
        .child(
            div()
                .id("album")
                .relative()
                .size(px(COMPACT_ART))
                .rounded(px(COMPACT_ART_RADIUS))
                .overflow_hidden()
                .child(art.unwrap_or_else(placeholder_art))
                // The ring sits above the artwork: a border on this box is
                // painted before its children, so the image covered it.
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(COMPACT_ART_RADIUS))
                        .border_1()
                        .border_color(COMPACT_ART_BORDER),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(COMPACT_ART_RADIUS))
                        .bg(theme::with_alpha(theme::SCRIM, 0.30))
                        .flex()
                        .items_center()
                        .justify_center()
                        .opacity(overlay_alpha.clamp(0.0, 1.0))
                        .child(lucide_color(overlay_icon, 12.0, theme::LABEL)),
                ),
        )
}

fn placeholder_art() -> AnyElement {
    div()
        .size(px(COMPACT_ART))
        .rounded(px(COMPACT_ART_RADIUS))
        .bg(linear_gradient(
            135.0,
            linear_color_stop(ART_PLACEHOLDER.0, 0.0),
            linear_color_stop(ART_PLACEHOLDER.1, 1.0),
        ))
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color("music", 14.0, theme::SECONDARY_LABEL))
        .into_any_element()
}

pub(crate) fn artwork_element(b64: &str, size: f32, radius: f32) -> Option<AnyElement> {
    let bytes = artwork_bytes(b64)?;
    let image = std::sync::Arc::new(Image::from_bytes(gpui_format(&bytes), bytes));
    // Overflow clip is a rect; the sprite only rounds via its own corner_radii.
    // Fill keeps the painted quad equal to `size` so those radii land on the
    // visible box (Cover can paint a larger quad and leave square corners).
    Some(
        img(image)
            .size(px(size))
            .rounded(px(radius))
            .object_fit(gpui::ObjectFit::Fill)
            .into_any_element(),
    )
}

fn artwork_bytes(b64: &str) -> Option<Vec<u8>> {
    use std::hash::{DefaultHasher, Hash, Hasher};

    if b64.len() > MAX_ARTWORK_BYTES.div_ceil(3) * 4 {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    b64.hash(&mut hasher);
    let key = hasher.finish();
    static CACHE: OnceLock<Mutex<(u64, Option<Vec<u8>>)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new((0, None)));
    if let Ok(guard) = cache.lock() {
        if guard.0 == key {
            return guard.1.clone();
        }
    }
    let loaded = decode_artwork(b64);
    if let Ok(mut guard) = cache.lock() {
        *guard = (key, loaded.clone());
    }
    loaded
}

fn decode_artwork(b64: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    use std::io::Cursor;

    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    if bytes.is_empty() || bytes.len() > MAX_ARTWORK_BYTES {
        return None;
    }
    let (width, height) = image::ImageReader::new(Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if width > MAX_ARTWORK_DIMENSION
        || height > MAX_ARTWORK_DIMENSION
        || u64::from(width) * u64::from(height)
            > u64::from(MAX_ARTWORK_DIMENSION) * u64::from(MAX_ARTWORK_DIMENSION)
    {
        return None;
    }
    // MediaRemote often labels Safari/YouTube frames `image/jpeg` while the
    // bytes are TIFF (`MM\0*` / `II*\0`). GPUI then feeds them to the JPEG
    // decoder and errors (or crashes) on 4D4D.
    if is_png(&bytes) || is_jpeg(&bytes) {
        return Some(bytes);
    }
    let mut png = Cursor::new(Vec::new());
    image::load_from_memory(&bytes)
        .ok()?
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    let png = png.into_inner();
    if png.is_empty() || png.len() > MAX_ARTWORK_BYTES {
        None
    } else {
        Some(png)
    }
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

fn gpui_format(bytes: &[u8]) -> gpui::ImageFormat {
    if is_png(bytes) {
        gpui::ImageFormat::Png
    } else {
        gpui::ImageFormat::Jpeg
    }
}

pub(super) fn visualizer(playing: bool, color: Option<Rgba>) -> impl IntoElement {
    // Gallery mockup locks the waveform to coral; live art tint still wins
    // when the island has extracted one.
    let color = color.unwrap_or(VIS_DEFAULT);
    let still = reduce_motion() || !playing;
    // Clock-driven bars via request_animation_frame — only while
    // this element is on screen (Media compact face). No island-tick dirties.
    canvas(
        move |_, _, _| (),
        move |bounds, _, window, _cx| {
            let levels: [f64; VIS_BARS] = if still {
                VIS_REST.map(|r| r as f64)
            } else {
                // Core glides its noise between steps, so sample the live
                // clock. Six bands; the face shows the first five.
                let raw = nook_core::audio::visualizer_levels_at(vis_clock());
                [raw[0], raw[1], raw[2], raw[3], raw[4]]
            };
            let bar_w = px(VIS_BAR_W);
            let gap = px(VIS_BAR_GAP);
            let total_w = VIS_BAR_W * VIS_BARS as f32 + VIS_BAR_GAP * (VIS_BARS as f32 - 1.0);
            let mut x =
                bounds.origin.x + px(((f32::from(bounds.size.width) - total_w) * 0.5).max(0.0));
            // Mockup row is `align-items: center`: bars grow and shrink
            // symmetrically about the row's middle, not up from the bottom.
            let mid = bounds.origin.y + bounds.size.height * 0.5;
            // Paused shows the resting 7/12/9/14/8 at full colour, as drawn.
            let fill: gpui::Hsla = color.into();
            for (i, level) in levels.iter().enumerate() {
                let scale = visualizer_scale(*level, !still, i);
                let h = px(VIS_H * scale);
                let bar = gpui::Bounds {
                    origin: gpui::point(x, mid - h * 0.5),
                    size: gpui::size(bar_w, h),
                };
                window.paint_quad(gpui::fill(bar, fill).corner_radii(px(1.0)));
                x = x + bar_w + gap;
            }
            if !still {
                window.request_animation_frame();
            }
        },
    )
    .h(px(VIS_H))
    .w(px(
        VIS_BAR_W * VIS_BARS as f32 + VIS_BAR_GAP * (VIS_BARS as f32 - 1.0)
    ))
}

fn vis_clock() -> f64 {
    static ORIGIN: OnceLock<std::time::Instant> = OnceLock::new();
    ORIGIN
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
}

/// Bar height as a fraction of [`VIS_H`]. Playing bars use the whole
/// 2..14 px range (no per-bar floor, so every band visibly moves); paused and
/// Reduce Motion show the mockup's resting 7/12/9/14/8.
fn visualizer_scale(level: f64, playing: bool, index: usize) -> f32 {
    if playing {
        let floor = VIS_MIN_H / VIS_H;
        floor + (1.0 - floor) * (level as f32).clamp(0.0, 1.0)
    } else {
        VIS_REST[index]
    }
}

/// Gallery artwork 52×52 (the Now Playing row is 52 tall).
const NOOK_ART: f32 = 52.0;
/// Mockup artwork radius 13.
pub(crate) const NOOK_ART_RADIUS: f32 = 13.0;
const APP_BADGE: f32 = 22.0;
const APP_BADGE_RADIUS: f32 = 5.0;
const NOOK_PLAY_HIT: f32 = 34.0;
const NOOK_SKIP_HIT: f32 = 32.0;
const NOOK_PLAY_GLYPH: f32 = 26.0;
const NOOK_SKIP_GLYPH: f32 = 19.0;
/// Lyrics panel current line — Pencil export 17/20 bold.
const LYRIC_CURRENT: crate::theme::Text = crate::theme::Text {
    size: 17.0,
    leading: 20.0,
    weight: FontWeight::NORMAL,
    emphasized: FontWeight::BOLD,
};
/// Gap between stacked lyric lines (Pencil Lines `gap: 5`).
const LYRIC_GAP: f32 = 5.0;
/// Synced-line step: neighbour leading + gap (current top moves by this).
pub(super) const LYRIC_LINE_PITCH: f32 = theme::BODY.leading + LYRIC_GAP;
/// Sequential travel when the outgoing line is still current-sized (20+5).
const LYRIC_STEP: f32 = LYRIC_CURRENT.leading + LYRIC_GAP;
/// Per-line delay from the top of the stack (Apple Music wave).
const LYRIC_STAGGER_MS: f32 = 35.0;
/// `Spring::smooth` duration sampled for each line; 4×35ms + 0.5s ≤ 650ms.
const LYRIC_SPRING_S: f32 = 0.5;
/// Lines box in the 128 Pencil card: pad 8×2 + header 28 → 84.
const LYRIC_LINES_H: f32 = 84.0;
/// At-rest three-line stack (past + gap + current + gap + next).
const LYRIC_REST_STACK: f32 =
    theme::BODY.leading + LYRIC_GAP + LYRIC_CURRENT.leading + LYRIC_GAP + theme::BODY.leading;
/// Incoming next2 peek while the wave is in flight (hidden at rest).
const LYRIC_NEXT_FAR_ALPHA: f32 = 0.22;
#[allow(dead_code)]
const NOOK_PROGRESS_H: f32 = 6.0;
/// Pencil pad is 12/8 border-box (1 px cell border eats into it); app border is outside.
const NOOK_PAD_X: f32 = 11.0;
const NOOK_PAD_Y: f32 = 7.0;
/// Scrubber: Pencil `padding-top: 10` + 16-tall track/labels row → 26.
const NOOK_SCRUBBER_ROW: f32 = 26.0;
/// Transport row height (Pencil `Transport` 34: play hit 34, skips 32, slots 28).
const NOOK_TRANSPORT_ROW: f32 = 34.0;
/// AirPlay / Up Next / Lyrics header row height (Pencil).
const PANEL_ROW_H: f32 = 28.0;
/// Lyrics header thumb (Pencil 20 r5).
const QUEUE_ART: f32 = 20.0;
const QUEUE_ART_RADIUS: f32 = 5.0;
/// Up Next row thumb (Pencil 24 r6).
const QUEUE_ROW_ART: f32 = 24.0;
const QUEUE_ROW_ART_RADIUS: f32 = 6.0;
/// Pencil track label 13/16 bold (700) — not the old 16/20 title.
const TRACK_LABEL: crate::theme::Text = crate::theme::Text {
    size: 13.0,
    leading: 16.0,
    weight: FontWeight::NORMAL,
    emphasized: FontWeight::BOLD,
};
/// Artwork ring `#FFFFFF1A` (shadow `0 3 10 #00000066` is inline below).
const NOOK_ART_RING: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0x1A as f32 / 255.0,
};
/// Scrubber track fill `#FFFFFF24`.
const NOOK_SCRUB_TRACK: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0x24 as f32 / 255.0,
};

/// Expanded Now Playing: artwork + title row, flanked scrubber, filled
/// transport. With no track loaded the same chrome stays up — "Not
/// Playing", the last player's name, zeroed scrubber, dimmed controls —
/// instead of a separate empty state.
/// AirPlay / Up Next / Lyrics each replace the body inside the same pane.
pub(crate) fn nook_media_pane(island: &Island, cx: &mut Context<Island>) -> AnyElement {
    set_reduce_motion(island.reduce_motion);
    let has = island.has_media();
    let np = &island.now_playing;
    let title: SharedString = if has {
        np.title
            .clone()
            .unwrap_or_else(|| "Unknown Title".into())
            .into()
    } else {
        SharedString::from("")
    };
    let artist: SharedString = if has {
        np.artist
            .clone()
            .unwrap_or_else(|| "Unknown Artist".into())
            .into()
    } else {
        np.app_name.clone().unwrap_or_default().into()
    };
    let playing = np.is_playing;
    let duration = np.duration.unwrap_or(0.0);
    let elapsed = displayed_elapsed(np, island.scrubber_drag);
    let progress = if duration > 0.0 {
        (elapsed / duration) as f32
    } else {
        0.0
    };
    let seekable = duration > 0.0;
    let art = np
        .artwork_base64
        .as_deref()
        .and_then(|b64| artwork_element(b64, NOOK_ART, NOOK_ART_RADIUS));
    let queue_open = island.queue_panel_visible();
    let show_picker = island.output_picker_enabled();
    let picker_open = island.output_picker_open && show_picker;
    let lyrics_open = island.lyrics_panel_visible();
    let show_lyrics_btn = island.lyrics_panel_available();
    // The list slot is always drawn (gallery); it only works where the
    // player exposes a local queue.
    let queue_enabled = island.settings.show_media_queue
        && nook_core::queue::supports_local_queue(np.app_name.as_deref(), np.bundle_id.as_deref());
    // Gallery draws AirPlay here whatever the current route is.
    let picker_icon = "airplay";

    // Pencil Now Playing: art 52 r13, gap 12; title 13/16 bold, artist 12/15, gap 2.
    let header = div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(nook_art_frame(art))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .overflow_hidden()
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(2.))
                .child(if has {
                    slide_label(title.clone(), TRACK_LABEL, true)
                        .w_full()
                        .into_any_element()
                } else {
                    div()
                        .w_full()
                        .text_size(px(TRACK_LABEL.size))
                        .line_height(px(TRACK_LABEL.leading))
                        .font_weight(TRACK_LABEL.emphasized)
                        .text_color(theme::secondary_label())
                        .whitespace_nowrap()
                        .child("Not Playing")
                        .into_any_element()
                })
                .child(slide_label(artist.clone(), theme::CALLOUT, false).w_full()),
        );

    let body = if picker_open {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .overflow_hidden()
            .child(output_picker_list(island, &title, cx))
            .into_any_element()
    } else if queue_open {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .overflow_hidden()
            .child(up_next_panel(island, cx))
            .into_any_element()
    } else if lyrics_open {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .overflow_hidden()
            .child(lyrics_panel(island, &title, &artist, cx))
            .into_any_element()
    } else {
        // Pencil rows 52 + 26 + 34 = 112 pane height exactly.
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .overflow_hidden()
            .child(header.flex_shrink_0().h(px(NOOK_ART)))
            .child(
                div()
                    .w_full()
                    .h(px(NOOK_SCRUBBER_ROW))
                    .flex_shrink_0()
                    .min_h(px(0.))
                    .child(nook_progress(
                        island, progress, elapsed, duration, seekable, cx,
                    )),
            )
            .child(
                div()
                    .w_full()
                    .h(px(NOOK_TRANSPORT_ROW))
                    .flex_shrink_0()
                    .min_h(px(0.))
                    .opacity(if has { 1.0 } else { theme::DISABLED_OPACITY })
                    .child(nook_transport(
                        playing,
                        queue_enabled,
                        queue_open,
                        show_lyrics_btn,
                        show_picker,
                        picker_icon,
                        cx,
                    )),
            )
            .into_any_element()
    };

    let fade = island.media_view_fade.value.clamp(0.0, 1.0);
    let shift = island.media_view_shift.value;
    let body = div()
        .relative()
        .top(px(shift))
        .opacity(fade)
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .h_full()
        .min_w(px(0.))
        .min_h(px(0.))
        .child(body);

    div()
        .id("nook-media-col")
        .w_full()
        .h_full()
        .overflow_hidden()
        .flex()
        .child(
            div()
                .id("nook-media")
                .relative()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(theme::NOOK_BODY - 4.0))
                .h_full()
                .px(px(NOOK_PAD_X))
                .py(px(NOOK_PAD_Y))
                .overflow_hidden()
                .flex()
                .child(body),
        )
        .into_any_element()
}

fn nook_art_frame(art: Option<AnyElement>) -> impl IntoElement {
    div().relative().size(px(NOOK_ART)).flex_shrink_0().child(
        div()
            .relative()
            .size(px(NOOK_ART))
            .rounded(px(NOOK_ART_RADIUS))
            .overflow_hidden()
            .shadow(vec![BoxShadow {
                color: gpui::hsla(0.0, 0.0, 0.0, 0.40),
                offset: point(px(0.), px(3.)),
                blur_radius: px(10.),
                spread_radius: px(0.),
            }])
            .bg(linear_gradient(
                135.0,
                linear_color_stop(ART_PLACEHOLDER.0, 0.0),
                linear_color_stop(ART_PLACEHOLDER.1, 1.0),
            ))
            .child(art.unwrap_or_else(|| {
                div()
                    .size(px(NOOK_ART))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(lucide_color("music", 20.0, theme::SECONDARY_LABEL))
                    .into_any_element()
            }))
            // Ring above the artwork — a border on this box paints under it.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(NOOK_ART_RADIUS))
                    .border_1()
                    .border_color(NOOK_ART_RING),
            )
            .child(
                canvas(
                    |bounds, _, _| {
                        let x: f32 = bounds.origin.x.into();
                        let y: f32 = bounds.origin.y.into();
                        let w: f32 = bounds.size.width.into();
                        let h: f32 = bounds.size.height.into();
                        report_art_bounds(x, y, w, h);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            ),
    )
}

/// Thumb tile with overlay ring so `border_1` does not shrink the content box.
fn thumb_frame(art: AnyElement, size: f32, radius: f32) -> impl IntoElement {
    div()
        .relative()
        .size(px(size))
        .rounded(px(radius))
        .overflow_hidden()
        .flex_shrink_0()
        .child(art)
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(radius))
                .border_1()
                .border_color(NOOK_ART_RING),
        )
}

/// Placeholder art for Up Next 24 r6 thumbs — same gradient as `placeholder_art`.
fn queue_placeholder_art() -> AnyElement {
    div()
        .size(px(QUEUE_ROW_ART))
        .rounded(px(QUEUE_ROW_ART_RADIUS))
        .bg(linear_gradient(
            135.0,
            linear_color_stop(ART_PLACEHOLDER.0, 0.0),
            linear_color_stop(ART_PLACEHOLDER.1, 1.0),
        ))
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color("music", 12.0, theme::SECONDARY_LABEL))
        .into_any_element()
}

fn nook_transport(
    playing: bool,
    queue_enabled: bool,
    queue_open: bool,
    show_lyrics_btn: bool,
    show_picker: bool,
    picker_icon: &'static str,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    // List flush left; lyrics (when present) sits immediately left of AirPlay.
    div()
        .w_full()
        .flex()
        .items_center()
        .child(
            div()
                .w(px(theme::HIT_MIN))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_start()
                .child(queue_toggle_btn(queue_open, queue_enabled, cx)),
        )
        .child(nook_skip(
            "media-skip-back",
            "nook-skip-back",
            cx,
            |this, _, cx| {
                this.note_media_skip(cx);
                if this.gallery_mode {
                    return;
                }
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_previous_track().await;
                });
            },
        ))
        .child(div().flex_1())
        .child(nook_play(playing, cx))
        .child(div().flex_1())
        .child(nook_skip(
            "media-skip-forward",
            "nook-skip-fwd",
            cx,
            |this, _, cx| {
                this.note_media_skip(cx);
                if this.gallery_mode {
                    return;
                }
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_next_track().await;
                });
            },
        ))
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_end()
                .when(show_lyrics_btn, |d| d.child(lyrics_toggle_btn(false, cx)))
                .when(show_picker, |d| {
                    d.child(output_picker_btn(picker_icon, false, cx))
                }),
        )
}

/// Gallery list slot: 15pt glyph flush left in a 28pt slot. Active state is
/// the Pencil filled chip (FILL + accent). Always drawn; without a local
/// queue it is dimmed and inert.
fn queue_toggle_btn(open: bool, enabled: bool, cx: &mut Context<Island>) -> impl IntoElement {
    let btn = div()
        .id("nook-queue-toggle")
        .size(px(theme::HIT_MIN))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .when(open, |d| d.rounded(px(14.)).bg(theme::FILL))
        .when(!open, |d| d.justify_start())
        .child(lucide_color(
            "list",
            15.0,
            if open {
                theme::accent()
            } else {
                theme::tertiary_label()
            },
        ));
    btn.when(!enabled, |d| d.opacity(theme::DISABLED_OPACITY))
        .when(enabled, |d| {
            d.hover(|s| s.opacity(0.85))
                .active(|s| s.opacity(0.75))
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.toggle_queue_panel(cx);
                    }),
                )
        })
}

fn output_picker_btn(icon: &'static str, open: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("nook-output-picker")
        .size(px(theme::HIT_MIN))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .when(open, |d| d.rounded(px(14.)).bg(theme::FILL))
        .when(!open, |d| d.justify_end())
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            icon,
            if open { 15.0 } else { 16.0 },
            if open {
                theme::accent()
            } else {
                theme::tertiary_label()
            },
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.toggle_output_picker(cx);
            }),
        )
}

/// True when the current track has showable lyrics (synced window or plain lines).
pub(crate) fn lyrics_available(island: &Island) -> bool {
    if !island.settings.show_lyrics {
        return false;
    }
    let Some(lyrics) = island.lyrics.as_ref() else {
        return false;
    };
    if lyrics.instrumental {
        return false;
    }
    if lyrics.has_synced() {
        return true;
    }
    lyrics
        .plain
        .as_deref()
        .is_some_and(|p| p.lines().any(|line| !line.trim().is_empty()))
}

fn lyrics_toggle_btn(open: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("nook-lyrics-toggle")
        .size(px(theme::HIT_MIN))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .when(open, |d| d.rounded(px(14.)).bg(theme::FILL))
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            "mic-vocal",
            15.0,
            if open {
                theme::accent()
            } else {
                theme::tertiary_label()
            },
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.toggle_lyrics_panel(cx);
            }),
        )
}

/// Pop Out control: 28×28 r14 FILL (accent-tinted while the PiP is open).
pub(crate) fn pip_popout_btn(
    kind: crate::pip::PipKind,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    pip_popout_btn_sized(kind, theme::HIT_MIN, 14.0, 14.0, cx)
}

/// Sized Pop Out control (Observe card uses 24×24 r12 / 13 px icon).
pub(crate) fn pip_popout_btn_sized(
    kind: crate::pip::PipKind,
    size: f32,
    radius: f32,
    icon: f32,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let open = crate::pip::is_open(kind);
    let id = match kind {
        crate::pip::PipKind::Lyrics => "pip-popout-lyrics",
        crate::pip::PipKind::Observe => "pip-popout-observe",
    };
    // `#0A84FF33` — active Pop Out fill while the window is open.
    const ACTIVE_BG: Rgba = Rgba {
        r: 0.039,
        g: 0.518,
        b: 1.0,
        a: 51.0 / 255.0,
    };
    div()
        .id(id)
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(radius))
        .bg(if open { ACTIVE_BG } else { theme::FILL })
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .child(lucide_color(
            "picture-in-picture-2",
            icon,
            if open {
                theme::accent()
            } else {
                theme::LABEL
            },
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if crate::pip::is_open(kind) {
                    crate::pip::close(kind, cx);
                } else {
                    crate::pip::open(kind, cx.entity().downgrade(), cx);
                    if kind == crate::pip::PipKind::Lyrics {
                        if this.lyrics_open {
                            this.begin_media_view_transition(false);
                            this.lyrics_open = false;
                        }
                        this.arm_lyrics_line_timer(cx);
                    }
                }
                cx.notify();
            }),
        )
}

fn lyrics_panel(
    island: &Island,
    title: &SharedString,
    artist: &SharedString,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    // Compact header + five-line window; current sits centred in the lines box.
    let thumb = island
        .now_playing
        .artwork_base64
        .as_deref()
        .and_then(|b64| artwork_element(b64, QUEUE_ART, QUEUE_ART_RADIUS));

    div()
        .id("nook-lyrics")
        .flex_1()
        .w_full()
        .h_full()
        .min_w(px(0.))
        .min_h(px(0.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .h(px(PANEL_ROW_H))
                .flex_shrink_0()
                .min_h(px(0.))
                .overflow_hidden()
                .flex()
                .items_center()
                .gap(px(8.))
                .when_some(thumb, |d, art| {
                    d.child(thumb_frame(art, QUEUE_ART, QUEUE_ART_RADIUS))
                })
                .child(
                    // Pencil lyrics header: title 13/16 bold, artist 12/15, align end, gap 6.
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .min_h(px(0.))
                        .overflow_hidden()
                        .flex()
                        .items_end()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex_shrink_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(px(TRACK_LABEL.size))
                                .line_height(px(TRACK_LABEL.leading))
                                .font_weight(TRACK_LABEL.emphasized)
                                .text_color(theme::LABEL)
                                .child(title.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(px(theme::CALLOUT.size))
                                .line_height(px(theme::CALLOUT.leading))
                                .text_color(theme::secondary_label())
                                .child(artist.clone()),
                        ),
                )
                .child(pip_popout_btn(crate::pip::PipKind::Lyrics, cx))
                .child(lyrics_toggle_btn(true, cx)),
        )
        .child(lyrics_lines_block(island, LYRIC_LINES_H))
}

/// Animated lyric stack shared by the island Lyrics pane and the Lyrics PiP.
/// `box_h` is the lines viewport height (island 84, PiP 66) used to centre the
/// rest stack; the element itself fills its parent (flex_1) like the pre-extract
/// lines box so gallery/island height never collapses to zero.
pub(crate) fn lyrics_lines_block(island: &Island, box_h: f32) -> AnyElement {
    let [past2, past1, cur, next1, next2] = lyrics_five_lines(island);
    let slots = lyric_motion_slots(island);
    let wrap = lyrics_wrap_offset(island);
    let current_top =
        (box_h - LYRIC_REST_STACK) * 0.5 + theme::BODY.leading + LYRIC_GAP;
    let stack_y = current_top - 2.0 * LYRIC_LINE_PITCH + wrap;

    div()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(box_h))
        .w_full()
        .h(px(box_h))
        .flex_shrink_0()
        .overflow_hidden()
        .relative()
        .child(
            div()
                .id("nook-lyrics-lines")
                .absolute()
                .inset_0()
                .w_full()
                .px(px(2.))
                .overflow_hidden()
                .child(
                    div()
                        .w_full()
                        .relative()
                        .top(px(stack_y))
                        .flex()
                        .flex_col()
                        .gap(px(LYRIC_GAP))
                        .child(lyric_slot_el(
                            past2.as_deref().unwrap_or(""),
                            &slots[0],
                            false,
                        ))
                        .child(lyric_slot_el(
                            past1.as_deref().unwrap_or(""),
                            &slots[1],
                            false,
                        ))
                        .child(lyric_slot_el(
                            cur.as_deref().unwrap_or(""),
                            &slots[2],
                            true,
                        ))
                        .child(lyric_slot_el(
                            next1.as_deref().unwrap_or(""),
                            &slots[3],
                            false,
                        ))
                        .child(lyric_slot_el(
                            next2.as_deref().unwrap_or(""),
                            &slots[4],
                            false,
                        )),
                ),
        )
        .into_any_element()
}

fn lyrics_five_lines(island: &Island) -> [Option<String>; 5] {
    let Some(lyrics) = island.lyrics.as_ref() else {
        return [None, None, None, None, None];
    };
    if lyrics.has_synced() {
        // Motion-committed index once bound (avoids a one-frame rest flash).
        // Gallery / first paint never run `sync_lyrics_line_motion`, so fall
        // back to the live playhead — `None` is "before the first line", not
        // "index unset", and would park "Short" in the next slot at the bottom.
        let idx = island
            .lyrics_line_idx
            .or_else(|| lyrics.active_index(island.lyrics_position_ms()));
        let [a, b, c, d, e] = lyrics.highlight_at_5(idx);
        return [
            a.map(str::to_string),
            b.map(str::to_string),
            c.map(str::to_string),
            d.map(str::to_string),
            e.map(str::to_string),
        ];
    }
    let Some(plain) = lyrics.plain.as_deref() else {
        return [None, None, None, None, None];
    };
    let collected: Vec<String> = plain
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(5)
        .map(|s| s.trim().to_string())
        .collect();
    match collected.as_slice() {
        [a, b, c, d, e] => [
            Some(a.clone()),
            Some(b.clone()),
            Some(c.clone()),
            Some(d.clone()),
            Some(e.clone()),
        ],
        [a, b, c, d] => [
            None,
            Some(a.clone()),
            Some(b.clone()),
            Some(c.clone()),
            Some(d.clone()),
        ],
        [a, b, c] => [
            None,
            Some(a.clone()),
            Some(b.clone()),
            Some(c.clone()),
            None,
        ],
        [a, b] => [None, None, Some(a.clone()), Some(b.clone()), None],
        [a] => [None, None, Some(a.clone()), None, None],
        _ => [None, None, None, None, None],
    }
}

fn lyric_lerp(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn lyric_dim(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

#[derive(Clone, Copy)]
struct LyricPaint {
    offset: f32,
    size: f32,
    leading: f32,
    bold: bool,
    wrap: bool,
    color: Rgba,
}

thread_local! {
    static LYRIC_CURRENT_H: std::cell::Cell<f32> =
        const { std::cell::Cell::new(LYRIC_CURRENT.leading) };
}

fn report_lyric_current_h(h: f32) -> bool {
    if !h.is_finite() || h <= 0.0 {
        return false;
    }
    // Canvas `inset_0` can report the whole lines box on the first layout.
    // Current wraps at most two leadings; anything larger wrecks wrap-shift.
    let cap = LYRIC_CURRENT.leading * 2.0;
    let h = h.min(cap);
    let prev = LYRIC_CURRENT_H.get();
    if prev.is_finite() && (prev - h).abs() < 0.5 {
        return false;
    }
    LYRIC_CURRENT_H.set(h);
    true
}

pub(super) fn lyric_wrap_target() -> f32 {
    let extra = lyric_wrap_extra();
    if extra > 0.0 {
        -extra / 2.0
    } else {
        0.0
    }
}

/// Extra height a wrapped current line needs above one leading, capped at
/// one extra row (20pt). Shared by the island wrap-shift and the Lyrics PiP
/// window, which grows by this amount instead of clipping past/next.
pub(crate) fn lyric_wrap_extra() -> f32 {
    let h = LYRIC_CURRENT_H.get();
    if !h.is_finite() {
        return 0.0;
    }
    if h > LYRIC_CURRENT.leading + 0.5 {
        (h - LYRIC_CURRENT.leading).min(LYRIC_CURRENT.leading)
    } else {
        0.0
    }
}

fn lyrics_wrap_offset(island: &Island) -> f32 {
    // Gallery (and Reduce Motion) never step the wrap spring, so sample the
    // measured target directly. Live motion uses the sprung value.
    let v = if island.reduce_motion {
        lyric_wrap_target()
    } else {
        island.lyrics_wrap_shift.value
    };
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

/// Shared clock: one `Spring::smooth(0.5)` sampled at `elapsed − 35ms·line` (line 0 leads).
pub(super) fn lyric_stagger_progress(elapsed_ms: f32, line: usize) -> f32 {
    let delay = (line.min(4) as f32) * LYRIC_STAGGER_MS;
    let local_s = (elapsed_ms - delay) / 1000.0;
    lyric_spring_sample(local_s)
}

fn lyric_spring_sample(elapsed_s: f32) -> f32 {
    const SPRING: crate::motion::Spring = crate::motion::Spring::smooth(LYRIC_SPRING_S);
    if elapsed_s <= 0.0 {
        return 0.0;
    }
    // Cap so line 4 (delay 140ms) finishes by ~650ms.
    if elapsed_s >= 0.51 {
        return 1.0;
    }
    let mut v = crate::motion::SpringValue::at(0.0);
    let mut left = elapsed_s;
    while left > 0.0 {
        let dt = left.min(0.25);
        let moving = v.step(SPRING, 1.0, dt, crate::motion::REST_ALPHA);
        if v.value >= 1.0 {
            return 1.0;
        }
        if !moving {
            return 1.0;
        }
        left -= dt;
    }
    v.value.clamp(0.0, 1.0)
}

pub(super) fn lyrics_retarget_from(from: [f32; 5], elapsed_ms: f32) -> [f32; 5] {
    let mut next = [0.0f32; 5];
    for i in 0..4 {
        let visual = from[i + 1] * (1.0 - lyric_stagger_progress(elapsed_ms, i + 1));
        next[i] = visual + LYRIC_STEP;
    }
    next[4] = LYRIC_STEP;
    next
}

fn lyric_motion_slots(island: &Island) -> [LyricPaint; 5] {
    let synced = island
        .lyrics
        .as_ref()
        .is_some_and(|lyrics| lyrics.has_synced());
    let sliding = synced
        && !island.reduce_motion
        && island
            .lyrics_line_from
            .iter()
            .any(|&y| y.abs() > crate::motion::REST_PX);
    let elapsed_ms = island.lyrics_wave_at.elapsed().as_secs_f32() * 1000.0;
    lyric_motion_paint(
        sliding,
        elapsed_ms,
        &island.lyrics_line_from,
        island.lyrics_fade.value,
        synced,
    )
}

fn lyric_motion_paint(
    sliding: bool,
    elapsed_ms: f32,
    from: &[f32; 5],
    fade: f32,
    synced: bool,
) -> [LyricPaint; 5] {
    let past_color = theme::tertiary_label();
    let next_color = theme::secondary_label();
    let fade = fade.clamp(0.0, 1.0);

    let mut sizes = [theme::BODY.size; 5];
    let mut leadings = [theme::BODY.leading; 5];
    sizes[2] = LYRIC_CURRENT.size;
    leadings[2] = LYRIC_CURRENT.leading;
    let mut bolds = [false; 5];
    bolds[2] = true;
    let mut wraps = [false; 5];
    wraps[2] = true;
    let mut colors = [
        theme::with_alpha(theme::LABEL, 0.0),
        past_color,
        theme::LABEL,
        next_color,
        theme::with_alpha(theme::LABEL, 0.0),
    ];
    let mut offsets = [0.0f32; 5];

    if sliding {
        for i in 0..5 {
            let t = lyric_stagger_progress(elapsed_ms, i);
            offsets[i] = from[i] * (1.0 - t);
            match i {
                0 => {
                    colors[0] = lyric_lerp(past_color, theme::with_alpha(past_color, 0.0), t);
                }
                1 => {
                    sizes[1] = lyric_dim(LYRIC_CURRENT.size, theme::BODY.size, t);
                    leadings[1] = lyric_dim(LYRIC_CURRENT.leading, theme::BODY.leading, t);
                    bolds[1] = t < 0.5;
                    wraps[1] = t < 0.5;
                    colors[1] = lyric_lerp(theme::LABEL, past_color, t);
                }
                2 => {
                    // Incoming current: next-style at t=0 (never LABEL/bold).
                    sizes[2] = lyric_dim(theme::BODY.size, LYRIC_CURRENT.size, t);
                    leadings[2] = lyric_dim(theme::BODY.leading, LYRIC_CURRENT.leading, t);
                    bolds[2] = t >= 0.5;
                    wraps[2] = true;
                    colors[2] = lyric_lerp(next_color, theme::LABEL, t);
                }
                3 => {
                    colors[3] = lyric_lerp(theme::with_alpha(next_color, 0.0), next_color, t);
                }
                _ => {
                    let peak = 4.0 * t * (1.0 - t);
                    colors[4] = lyric_lerp(
                        theme::with_alpha(theme::LABEL, 0.0),
                        theme::with_alpha(theme::LABEL, LYRIC_NEXT_FAR_ALPHA),
                        peak,
                    );
                }
            }
        }
    } else if synced && (1.0 - fade).abs() > crate::motion::REST_ALPHA {
        let hide = |c: Rgba| theme::with_alpha(c, 0.0);
        colors = [
            hide(theme::LABEL),
            lyric_lerp(hide(past_color), past_color, fade),
            lyric_lerp(hide(next_color), theme::LABEL, fade),
            lyric_lerp(hide(next_color), next_color, fade),
            hide(theme::LABEL),
        ];
    }

    std::array::from_fn(|i| LyricPaint {
        offset: offsets[i],
        size: sizes[i],
        leading: leadings[i],
        bold: bolds[i],
        wrap: wraps[i],
        color: colors[i],
    })
}

fn lyric_slot_el(text: &str, slot: &LyricPaint, measure: bool) -> AnyElement {
    div()
        .relative()
        .w_full()
        .flex_shrink_0()
        .top(px(slot.offset))
        .child(lyric_line(
            text,
            slot.size,
            slot.leading,
            slot.bold,
            slot.wrap,
            slot.color,
        ))
        .when(measure, |d| {
            d.child(
                canvas(
                    |bounds, window, cx| {
                        let h: f32 = bounds.size.height.into();
                        if report_lyric_current_h(h) {
                            // Measure lands after this frame's height math.
                            // Defer a notify so PiP can grow/shrink next cycle
                            // (request_animation_frame during prepaint borrows
                            // Window::next_frame_callbacks).
                            let id = window.current_view();
                            window.defer(cx, move |_, cx| cx.notify(id));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
        })
        .into_any_element()
}

fn lyric_line(
    text: &str,
    size: f32,
    leading: f32,
    bold: bool,
    wrap: bool,
    color: Rgba,
) -> AnyElement {
    let size = size.max(1.0);
    let leading = leading.max(1.0);
    let weight = if bold {
        LYRIC_CURRENT.emphasized
    } else {
        theme::BODY.weight
    };
    if text.is_empty() {
        return div()
            .h(px(leading))
            .w_full()
            .min_h(px(0.))
            .flex_shrink_0()
            .into_any_element();
    }
    if wrap {
        let max_h = leading * 2.0;
        return div()
            .w_full()
            .min_w(px(0.))
            .flex_shrink_0()
            .max_h(px(max_h))
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .line_clamp(2)
                    .text_size(px(size))
                    .line_height(px(leading))
                    .font_weight(weight)
                    .text_color(color)
                    .child(text.to_string()),
            )
            .into_any_element();
    }
    div()
        .w_full()
        .h(px(leading))
        .min_w(px(0.))
        .flex_shrink_0()
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .text_size(px(size))
        .line_height(px(leading))
        .font_weight(weight)
        .text_color(color)
        .child(text.to_string())
        .into_any_element()
}

fn output_picker_list(
    island: &Island,
    track_title: &SharedString,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    // Pencil "Media — AirPlay": header + device rows with kind + radio trailing.
    let mut list = div()
        .id("nook-output-list")
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .w_full()
        .h_full()
        .overflow_hidden();

    list = list.child(
        div()
            .w_full()
            .h(px(PANEL_ROW_H))
            .flex_shrink_0()
            .min_h(px(0.))
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(theme::LABEL)
                    .text_size(px(theme::BODY.size))
                    .line_height(px(theme::BODY.leading))
                    .font_weight(theme::BODY.emphasized)
                    .whitespace_nowrap()
                    .child("AirPlay"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(theme::tertiary_label())
                    .text_size(px(theme::SUBHEADLINE.size))
                    .line_height(px(theme::SUBHEADLINE.leading))
                    .child(track_title.clone()),
            )
            .child(output_picker_btn("airplay", true, cx)),
    );

    if island.output_devices.is_empty() {
        list = list.child(
            div()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .w_full()
                .overflow_hidden()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .child(
                    div()
                        .text_color(theme::secondary_label())
                        .text_size(px(theme::BODY.size))
                        .line_height(px(theme::BODY.leading))
                        .child("No output devices"),
                )
                .child(
                    div()
                        .text_color(theme::tertiary_label())
                        .text_size(px(theme::SUBHEADLINE.size))
                        .line_height(px(theme::SUBHEADLINE.leading))
                        .child("AirPlay starts from Control Center."),
                ),
        );
    } else {
        let mut rows = div().flex().flex_col().min_w(px(0.)).min_h(px(0.));
        for device in &island.output_devices {
            rows = rows.child(output_device_row(device, cx));
        }
        list = list.child(
            div()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .child(scroll_body("nook-output-rows", rows)),
        );
    }
    list
}

fn output_device_row(device: &OutputDevice, cx: &mut Context<Island>) -> impl IntoElement {
    let id = device.id;
    let name = device.name.clone();
    let icon = device_row_icon(device);
    let kind = device_kind(device);
    let selected = device.is_default;
    div()
        .id(SharedString::from(format!("out-dev-{id}")))
        .w_full()
        .h(px(PANEL_ROW_H))
        .flex_shrink_0()
        .min_w(px(0.))
        .min_h(px(0.))
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(10.))
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.select_output_device(id, cx);
            }),
        )
        .child(
            div()
                .size(px(20.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(lucide_color(
                    icon,
                    15.0,
                    if selected {
                        theme::LABEL
                    } else {
                        theme::secondary_label()
                    },
                )),
        )
        // Pencil AirPlay row: name 13/16 (selected semibold white / else regular
        // secondary), kind 11/14 tertiary flex 1, trailing 28×20.
        .child(
            label(name, theme::BODY, selected)
                .flex_shrink_0()
                .text_color(if selected {
                    theme::LABEL
                } else {
                    theme::secondary_label()
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_color(theme::tertiary_label())
                .text_size(px(theme::SUBHEADLINE.size))
                .line_height(px(theme::SUBHEADLINE.leading))
                .when_some(kind, |d, k| d.child(k)),
        )
        .child(
            div()
                .w(px(28.))
                .h(px(20.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(if selected {
                    lucide_color("check", 14.0, theme::accent()).into_any_element()
                } else {
                    div()
                        .size(px(12.))
                        .rounded_full()
                        .border_1()
                        .border_color(theme::tertiary_label())
                        .into_any_element()
                }),
        )
}

fn device_row_icon(device: &OutputDevice) -> &'static str {
    match device.transport {
        OutputTransport::BuiltIn => "laptop",
        OutputTransport::Hdmi | OutputTransport::DisplayPort => "tv",
        _ => device.icon(),
    }
}

/// Kind caption from HAL transport only — no HomePod / Apple TV invention.
fn device_kind(device: &OutputDevice) -> Option<&'static str> {
    match device.transport {
        OutputTransport::BuiltIn => Some("This Mac"),
        OutputTransport::Unknown => None,
        other => Some(other.label()),
    }
}

#[allow(dead_code)]
fn app_badge(bundle_id: Option<&str>, app_name: Option<&str>) -> AnyElement {
    if let Some(image) = app_icon_image(bundle_id, app_name) {
        return img(image)
            .size(px(APP_BADGE))
            .rounded(px(APP_BADGE_RADIUS))
            .object_fit(gpui::ObjectFit::Fill)
            .into_any_element();
    }
    div()
        .size(px(APP_BADGE))
        .rounded(px(APP_BADGE_RADIUS))
        .bg(theme::SCRIM)
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color("music", 11.0, theme::LABEL))
        .into_any_element()
}

pub(crate) fn app_icon_image(
    bundle_id: Option<&str>,
    app_name: Option<&str>,
) -> Option<std::sync::Arc<Image>> {
    let key = bundle_id
        .filter(|s| !s.is_empty())
        .or(app_name)
        .filter(|s| !s.is_empty())?
        .to_string();
    static CACHE: OnceLock<
        Mutex<std::collections::HashMap<String, Option<std::sync::Arc<Image>>>>,
    > = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
    }
    let loaded = crate::platform::app_icon_png(bundle_id, app_name).and_then(|png| {
        if png.is_empty() {
            None
        } else {
            Some(std::sync::Arc::new(Image::from_bytes(
                gpui::ImageFormat::Png,
                png,
            )))
        }
    });
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, loaded.clone());
    }
    loaded
}

fn displayed_elapsed(np: &NowPlayingData, drag: Option<f32>) -> f64 {
    if let Some(ratio) = drag {
        return np.duration.unwrap_or(0.0) * ratio as f64;
    }
    np.elapsed_time.unwrap_or(0.0)
}

pub(crate) fn scrubber_ratio(x: f32, origin: f32, width: f32) -> f32 {
    if width < 1.0 {
        return 0.0;
    }
    ((x - origin) / width).clamp(0.0, 1.0)
}

fn nook_progress(
    island: &Island,
    progress: f32,
    elapsed: f64,
    duration: f64,
    seekable: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let dragging = island.scrubber_drag.is_some();
    let progress = island.scrubber_drag.unwrap_or(progress).clamp(0.0, 1.0);
    let bounds = island.scrubber_bounds.clone();
    // Pencil Scrubber: pt 10 under art, then 16-tall row (labels 11/14 + track
    // area share a midline); bar 6 r3, played #FFFFFF, track #FFFFFF24.
    div()
        .w_full()
        .h(px(NOOK_SCRUBBER_ROW))
        .pt(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        // No dimming when unseekable: the gallery track stays #FFFFFF24 and
        // the stamps read 0:00 / -0:00.
        .cursor(if seekable {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Arrow
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if !seekable {
                    return;
                }
                cx.stop_propagation();
                this.update_scrubber_from_x(event.position.x.into());
                cx.notify();
            }),
        )
        .child(
            media_time_label(if duration > 0.0 {
                format_time(elapsed)
            } else {
                "0:00".into()
            })
            .w(px(30.))
            .flex_shrink_0(),
        )
        .child(
            div()
                .relative()
                .flex_1()
                .min_w(px(24.))
                .h(px(16.))
                .flex()
                .items_center()
                .group("nook-scrub")
                .child(
                    canvas(
                        {
                            let bounds = bounds.clone();
                            move |layout, _, _| {
                                let origin: f32 = layout.origin.x.into();
                                let width: f32 = layout.size.width.into();
                                *bounds.borrow_mut() = Some((origin, width));
                                layout
                            }
                        },
                        |_bounds, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(6.))
                        .rounded(px(3.))
                        .bg(NOOK_SCRUB_TRACK)
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress))
                                .rounded(px(3.))
                                .bg(theme::LABEL),
                        ),
                )
                // Gallery shows a bare bar; the thumb appears on hover or drag.
                .when(seekable, |d| {
                    d.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .when(!dragging, |d| {
                                d.opacity(0.0).group_hover("nook-scrub", |s| s.opacity(1.0))
                            })
                            .child(scrubber_thumb(progress)),
                    )
                }),
        )
        .child(
            media_time_label(if duration > 0.0 {
                format_remaining(elapsed, duration)
            } else {
                "-0:00".into()
            })
            .w(px(34.))
            .flex_shrink_0()
            .text_right(),
        )
}

fn scrubber_thumb(progress: f32) -> impl IntoElement {
    div()
        .absolute()
        .left(relative(progress.clamp(0.0, 1.0)))
        .ml(px(-5.))
        .size(px(10.))
        .rounded_full()
        .bg(theme::LABEL)
        .shadow_sm()
}

fn nook_skip(
    icon: &'static str,
    elem_id: &'static str,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(elem_id)
        .size(px(NOOK_SKIP_HIT))
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(icon, NOOK_SKIP_GLYPH, theme::LABEL))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, event, cx);
            }),
        )
}

fn nook_play(playing: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("nook-playpause")
        .size(px(NOOK_PLAY_HIT))
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            if playing { "media-pause" } else { "media-play" },
            NOOK_PLAY_GLYPH,
            theme::LABEL,
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.note_media_play_pause(cx);
                if this.gallery_mode {
                    return;
                }
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_play_pause().await;
                });
            }),
        )
}

#[allow(dead_code)]
pub(crate) fn media_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let np = &island.now_playing;
    let title = np.title.clone().unwrap_or_else(|| "Unknown Title".into());
    let artist = np.artist.clone().unwrap_or_else(|| "Unknown Artist".into());
    let playing = np.is_playing;
    let duration = np.duration.unwrap_or(0.0);
    let elapsed = displayed_elapsed(np, island.scrubber_drag);
    let progress = if duration > 0.0 {
        (elapsed / duration) as f32
    } else {
        0.0
    };
    let seekable = duration > 0.0;
    let art = np
        .artwork_base64
        .as_deref()
        .and_then(|b64| artwork_element(b64, ART, ART_RADIUS));

    let header = ART + theme::CONTENT_INSET + TITLE_COL;
    let transport = theme::HIT_MIN + SKIP_GAP + PLAY + SKIP_GAP + theme::HIT_MIN;
    let card_w =
        (theme::WIDGET_PAD * 2.0 + header.max(transport)).max(super::ui::WIDGET_CARD_WIDTH);

    card_chrome(card_w)
        .gap(px(12.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(theme::CONTENT_INSET))
                .child(
                    div()
                        .size(px(ART))
                        .rounded(px(ART_RADIUS))
                        .overflow_hidden()
                        .shadow_md()
                        .flex_shrink_0()
                        .bg(linear_gradient(
                            135.0,
                            linear_color_stop(ART_PLACEHOLDER.0, 0.0),
                            linear_color_stop(ART_PLACEHOLDER.1, 1.0),
                        ))
                        .child(art.unwrap_or_else(|| div().size(px(ART)).into_any_element())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .overflow_hidden()
                        .child(slide_label(title, theme::TITLE_2, true).w_full())
                        .child(slide_label(artist, theme::BODY, false).w_full()),
                ),
        )
        .child(progress_block(
            island, progress, elapsed, duration, seekable, cx,
        ))
        .child(transport_row(playing, cx))
}

fn progress_block(
    island: &Island,
    progress: f32,
    elapsed: f64,
    duration: f64,
    seekable: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let progress = island.scrubber_drag.unwrap_or(progress).clamp(0.0, 1.0);
    let bounds = island.scrubber_bounds.clone();
    div()
        .flex()
        .flex_col()
        .w_full()
        .opacity(if seekable {
            1.0
        } else {
            theme::DISABLED_OPACITY
        })
        .cursor(if seekable {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Arrow
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if !seekable {
                    return;
                }
                cx.stop_propagation();
                this.update_scrubber_from_x(event.position.x.into());
                cx.notify();
            }),
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(MEDIA_PROGRESS_HIT))
                .flex()
                .items_center()
                .child(
                    canvas(
                        {
                            let bounds = bounds.clone();
                            move |layout, _, _| {
                                let origin: f32 = layout.origin.x.into();
                                let width: f32 = layout.size.width.into();
                                *bounds.borrow_mut() = Some((origin, width));
                                layout
                            }
                        },
                        |_bounds, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(theme::TRACK_H))
                        .rounded(px(theme::TRACK_RADIUS))
                        .bg(theme::FILL_SECONDARY)
                        .hover(|s| s.h(px(theme::TRACK_H + 2.0)))
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress))
                                .rounded(px(theme::TRACK_RADIUS))
                                .bg(theme::LABEL),
                        ),
                )
                .when(seekable, |d| d.child(scrubber_thumb(progress))),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .pt(px(MEDIA_TIME_PAD_TOP))
                .mt(px(MEDIA_TIME_PAD_GAP))
                .px(px(1.))
                .child(time_label(if duration > 0.0 {
                    format_time(elapsed)
                } else {
                    "–:––".into()
                }))
                .child(time_label(format_remaining(elapsed, duration))),
        )
}

fn up_next_panel(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    // Pencil "Media — Up Next": active list toggle + title; shuffle/repeat
    // only when MediaRemote reports the modes.
    use nook_core::models::QueueHidden;
    let queue = &island.queue;
    let shuffle = island.now_playing.shuffle;
    let repeat = island.now_playing.repeat;
    let streaming = matches!(queue.hidden, Some(QueueHidden::Streaming));
    let mut rows = div().flex().flex_col().min_h(px(0.));
    if queue.items.is_empty() {
        rows = rows.child(queue_empty_state(queue, streaming, cx));
    } else {
        for (i, item) in queue.items.iter().enumerate() {
            rows = rows.child(queue_row(i, item, cx));
        }
    }
    div()
        .id("up-next")
        .flex_1()
        .w_full()
        .h_full()
        .min_w(px(0.))
        .min_h(px(0.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .h(px(PANEL_ROW_H))
                .flex_shrink_0()
                .min_h(px(0.))
                .overflow_hidden()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(queue_toggle_btn(true, true, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .min_h(px(0.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(px(theme::BODY.size))
                        .line_height(px(theme::BODY.leading))
                        .font_weight(theme::BODY.emphasized)
                        .text_color(theme::LABEL)
                        .child("Up Next"),
                )
                .when_some(shuffle, |d, on| d.child(shuffle_btn(on, cx)))
                .when_some(repeat, |d, mode| d.child(repeat_btn(mode, cx))),
        )
        .child(scroll_body("nook-up-next-rows", rows))
}

fn queue_empty_state(
    queue: &PlaybackQueue,
    streaming: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    if streaming {
        return div()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .w_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(6.))
            .pt(px(4.))
            .child(
                div()
                    .text_size(px(theme::SUBHEADLINE.size))
                    .line_height(px(theme::SUBHEADLINE.leading))
                    .text_color(theme::secondary_label())
                    .child("Up Next isn't available for streamed songs"),
            )
            .child(
                div()
                    .id("nook-open-music")
                    .flex_shrink_0()
                    .text_size(px(theme::CALLOUT.size))
                    .line_height(px(theme::CALLOUT.leading))
                    .text_color(theme::accent())
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.opacity(0.85))
                    .active(|s| s.opacity(0.75))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let _ = std::process::Command::new("open")
                                .args(["-b", "com.apple.Music"])
                                .spawn();
                        }),
                    )
                    .child("Open in Music"),
            )
            .into_any_element();
    }
    div()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .w_full()
        .pt(px(8.))
        .text_size(px(theme::SUBHEADLINE.size))
        .line_height(px(theme::SUBHEADLINE.leading))
        .text_color(theme::secondary_label())
        .child(queue_empty_message(queue))
        .into_any_element()
}

fn shuffle_btn(on: bool, cx: &mut Context<Island>) -> impl IntoElement {
    // Pencil: 20×24, icon right-aligned.
    div()
        .id("nook-shuffle")
        .w(px(20.))
        .h(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            "shuffle",
            14.0,
            if on {
                theme::accent()
            } else {
                theme::secondary_label()
            },
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.note_media_toggle_shuffle(cx);
                if this.gallery_mode {
                    return;
                }
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_toggle_shuffle().await;
                });
            }),
        )
}

fn repeat_btn(mode: RepeatMode, cx: &mut Context<Island>) -> impl IntoElement {
    let (icon, active) = match mode {
        RepeatMode::Off => ("repeat", false),
        RepeatMode::One => ("repeat-1", true),
        RepeatMode::All => ("repeat", true),
    };
    // Pencil: 20×24, icon right-aligned.
    div()
        .id("nook-repeat")
        .w(px(20.))
        .h(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            icon,
            14.0,
            if active {
                theme::accent()
            } else {
                theme::tertiary_label()
            },
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.note_media_toggle_repeat(cx);
                if this.gallery_mode {
                    return;
                }
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_toggle_repeat().await;
                });
            }),
        )
}

fn queue_empty_message(queue: &PlaybackQueue) -> SharedString {
    use nook_core::models::QueueHidden;
    SharedString::from(match queue.hidden {
        Some(QueueHidden::NeedsSpotifyAuth) | Some(QueueHidden::SpotifyUnavailable) => {
            "Not available in Spotify"
        }
        Some(QueueHidden::PremiumRequired) => "Not available in Spotify",
        Some(QueueHidden::AutomationDenied) => "Needs Music automation",
        Some(QueueHidden::Shuffle) => "Hidden while shuffling",
        Some(QueueHidden::Radio) => "Not available for radio",
        Some(QueueHidden::Streaming) => "Up Next isn't available for streamed songs",
        Some(QueueHidden::Idle) | None => "No upcoming tracks",
    })
}

fn queue_row(index: usize, item: &QueueItem, cx: &mut Context<Island>) -> impl IntoElement {
    if let Some(url) = item.artwork_url.as_ref() {
        if nook_core::queue::cached_artwork(&item.id).is_none() {
            nook_core::queue::request_artwork(item.id.clone(), url.clone());
        }
    }
    let thumb = item
        .artwork_base64
        .as_deref()
        .map(str::to_string)
        .or_else(|| nook_core::queue::cached_artwork(&item.id).and_then(|hit| hit));
    let art = thumb
        .as_deref()
        .and_then(|b64| artwork_element(b64, QUEUE_ROW_ART, QUEUE_ROW_ART_RADIUS))
        .unwrap_or_else(queue_placeholder_art);
    let title = item.title.clone();
    let artist = item.artist.clone();
    let clickable = !matches!(item.jump, nook_core::models::QueueJump::Unavailable);
    let duration = item.duration.filter(|d| *d > 0.0).map(format_time);
    // Pencil Up Next row: pl 2, 28 tall gap 10; thumb 24 r6; title 12/15
    // semibold; artist 12/15 secondary; duration 11/14 tertiary.
    div()
        .id(SharedString::from(format!("up-next-{index}")))
        .h(px(PANEL_ROW_H))
        .w_full()
        .pl(px(2.))
        .flex_shrink_0()
        .min_h(px(0.))
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(10.))
        .when(clickable, |d| {
            let jump = item.clone();
            d.cursor(CursorStyle::PointingHand)
                .hover(|s| s.opacity(0.85))
                .active(|s| s.opacity(0.75))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        if this.gallery_mode {
                            return;
                        }
                        let item = jump.clone();
                        let context = this.queue.context_uri.clone();
                        nook_core::runtime().spawn(async move {
                            let _ = nook_core::audio::media_jump_to_queue_item(item, context)
                                .await;
                        });
                    }),
                )
        })
        .child(thumb_frame(art, QUEUE_ROW_ART, QUEUE_ROW_ART_RADIUS))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .overflow_hidden()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(px(theme::CALLOUT.size))
                        .line_height(px(theme::CALLOUT.leading))
                        .font_weight(theme::CALLOUT.emphasized)
                        .text_color(theme::LABEL)
                        .child(title),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(px(theme::CALLOUT.size))
                        .line_height(px(theme::CALLOUT.leading))
                        .text_color(theme::secondary_label())
                        .child(artist),
                ),
        )
        .when_some(duration, |d, stamp| {
            d.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(theme::SUBHEADLINE.size))
                    .line_height(px(theme::SUBHEADLINE.leading))
                    .text_color(theme::tertiary_label())
                    .child(stamp),
            )
        })
}

fn transport_row(playing: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(SKIP_GAP))
        .child(skip_btn(
            "media-skip-back",
            "ibtn-skip-back",
            cx,
            |this, _, cx| {
                this.note_media_skip(cx);
                if this.gallery_mode {
                    return;
                }
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_previous_track().await;
                });
            },
        ))
        .child(play_btn(playing, cx))
        .child(skip_btn(
            "media-skip-forward",
            "ibtn-skip-forward",
            cx,
            |this, _, cx| {
                this.note_media_skip(cx);
                if this.gallery_mode {
                    return;
                }
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_next_track().await;
                });
            },
        ))
}

fn skip_btn(
    icon: &'static str,
    elem_id: &'static str,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(elem_id)
        .size(px(theme::HIT_MIN))
        .flex()
        .items_center()
        .justify_center()
        .opacity(0.9)
        .hover(|s| s.opacity(1.0))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(icon, 24.0, theme::LABEL))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, event, cx);
            }),
        )
}

fn play_btn(playing: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("ibtn-playpause")
        .size(px(PLAY))
        .rounded_full()
        .bg(theme::LABEL)
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(theme::LABEL))
        .active(|s| s.opacity(0.95))
        .cursor(CursorStyle::PointingHand)
        .shadow_sm()
        .child(lucide_color(
            if playing { "media-pause" } else { "media-play" },
            22.0,
            theme::ISLAND,
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.note_media_play_pause(cx);
                if this.gallery_mode {
                    return;
                }
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_play_pause().await;
                });
            }),
        )
}

fn time_label(text: String) -> gpui::Div {
    // Mockup scrubber stamps: 11px tabular, tertiary (#EBEBF54D).
    timer_text(text, theme::SUBHEADLINE).text_color(theme::tertiary_label())
}

/// Now Playing scrubber stamps: Pencil 11/14, regular weight (the mockup's
/// 400, not the compact timer's semibold), tabular figures, tertiary.
fn media_time_label(text: String) -> gpui::Div {
    div()
        .font(Font {
            family: "SF Pro".into(),
            features: FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        })
        .text_size(px(theme::SUBHEADLINE.size))
        .line_height(px(theme::SUBHEADLINE.leading))
        .text_color(theme::tertiary_label())
        .whitespace_nowrap()
        .child(text)
}

fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0) as u32;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn format_remaining(elapsed: f64, duration: f64) -> String {
    if duration <= 0.0 {
        return "–:––".into();
    }
    format!("-{}", format_time((duration - elapsed).max(0.0)))
}

pub(crate) fn visualizer_color_from_art(artwork_base64: Option<&str>) -> Option<Rgba> {
    let b64 = artwork_base64?;
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    b64.hash(&mut hasher);
    let key = hasher.finish();
    static CACHE: OnceLock<Mutex<(u64, Option<Rgba>)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new((0, None)));
    if let Ok(guard) = cache.lock() {
        if guard.0 == key {
            return guard.1;
        }
    }
    let color = sample_dominant_color(b64);
    if let Ok(mut guard) = cache.lock() {
        *guard = (key, color);
    }
    color
}

fn sample_dominant_color(b64: &str) -> Option<Rgba> {
    use image::GenericImageView;
    let bytes = artwork_bytes(b64)?;
    let img = image::load_from_memory(&bytes).ok()?.thumbnail(32, 32);
    let mut r_acc = 0u64;
    let mut g_acc = 0u64;
    let mut b_acc = 0u64;
    let mut n = 0u64;
    let mut r_all = 0u64;
    let mut g_all = 0u64;
    let mut b_all = 0u64;
    let mut n_all = 0u64;
    for (_, _, px) in img.pixels() {
        let [r, g, b, a] = px.0;
        if a < 200 {
            continue;
        }
        r_all += r as u64;
        g_all += g as u64;
        b_all += b as u64;
        n_all += 1;
        let brightness = (r as u16 + g as u16 + b as u16) / 3;
        if !(20..=230).contains(&brightness) {
            continue;
        }
        r_acc += r as u64;
        g_acc += g as u64;
        b_acc += b as u64;
        n += 1;
    }
    let (r, g, b, count) = if n > 0 {
        (r_acc, g_acc, b_acc, n)
    } else {
        (r_all, g_all, b_all, n_all)
    };
    if count == 0 {
        return None;
    }
    Some(Rgba {
        r: (r / count) as f32 / 255.0,
        g: (g / count) as f32 / 255.0,
        b: (b / count) as f32 / 255.0,
        a: 1.0,
    })
}

pub(crate) fn art_palette(artwork_base64: Option<&str>) -> Option<[Rgba; 3]> {
    let b64 = artwork_base64?;
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    b64.hash(&mut hasher);
    let key = hasher.finish();
    static CACHE: OnceLock<Mutex<(u64, Option<[Rgba; 3]>)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new((0, None)));
    if let Ok(guard) = cache.lock() {
        if guard.0 == key {
            return guard.1;
        }
    }
    let palette = sample_palette(b64);
    if let Ok(mut guard) = cache.lock() {
        *guard = (key, palette);
    }
    palette
}

fn sample_palette(b64: &str) -> Option<[Rgba; 3]> {
    use image::GenericImageView;
    use std::collections::HashMap;

    let bytes = artwork_bytes(b64)?;
    let img = image::load_from_memory(&bytes).ok()?.thumbnail(32, 32);
    let mut buckets: HashMap<(u8, u8, u8), u32> = HashMap::new();
    for (_, _, px) in img.pixels() {
        let [r, g, b, a] = px.0;
        if a < 200 {
            continue;
        }
        let brightness = (r as u16 + g as u16 + b as u16) / 3;
        if !(16..=240).contains(&brightness) {
            continue;
        }
        *buckets.entry((r >> 4, g >> 4, b >> 4)).or_insert(0) += 1;
    }
    if buckets.is_empty() {
        return sample_dominant_color(b64).map(|c| [c, shift_color(c, 0.08), shift_color(c, 0.16)]);
    }
    let mut ranked: Vec<_> = buckets.into_iter().collect();
    ranked.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let mut picked: Vec<Rgba> = Vec::new();
    for ((r, g, b), _) in ranked {
        let color = Rgba {
            r: (r as f32 + 0.5) * 16.0 / 255.0,
            g: (g as f32 + 0.5) * 16.0 / 255.0,
            b: (b as f32 + 0.5) * 16.0 / 255.0,
            a: 1.0,
        };
        if picked.iter().all(|p| color_dist(*p, color) > 0.18) {
            picked.push(color);
        }
        if picked.len() == 3 {
            break;
        }
    }
    while picked.len() < 3 {
        let base = picked
            .first()
            .copied()
            .or_else(|| sample_dominant_color(b64))?;
        picked.push(shift_color(base, 0.08 * picked.len() as f32));
    }
    Some([picked[0], picked[1], picked[2]])
}

fn shift_color(color: Rgba, amount: f32) -> Rgba {
    Rgba {
        r: (color.r + amount).clamp(0.0, 1.0),
        g: (color.g + amount * 0.5).clamp(0.0, 1.0),
        b: (color.b + amount * 0.75).clamp(0.0, 1.0),
        a: 1.0,
    }
}

fn color_dist(a: Rgba, b: Rgba) -> f32 {
    let dr = a.r - b.r;
    let dg = a.g - b.g;
    let db = a.b - b.b;
    (dr * dr + dg * dg + db * db).sqrt()
}

// NOTE (R2 media bleed): the ambient artwork wash / bloom behind the Nook
// media pane was removed — the mockup is a clean black card with no
// artwork-colour bleed. `art_palette` below stays: island/mod.rs still
// reads it for the aura state.

type ArtBoundsCache = (u64, f32, f32, f32, f32);
static ART_BOUNDS: OnceLock<Mutex<ArtBoundsCache>> = OnceLock::new();

fn report_art_bounds(x: f32, y: f32, w: f32, h: f32) {
    let cache = ART_BOUNDS.get_or_init(|| Mutex::new((0, 0.0, 0.0, 0.0, 0.0)));
    let Ok(mut guard) = cache.lock() else {
        return;
    };
    let (gen, ox, oy, ow, oh) = *guard;
    if (ox - x).abs() < 0.5 && (oy - y).abs() < 0.5 && (ow - w).abs() < 0.5 && (oh - h).abs() < 0.5
    {
        return;
    }
    *guard = (gen.wrapping_add(1), x, y, w, h);
}

pub(crate) fn take_art_bounds(seen: &mut u64) -> Option<(f32, f32, f32, f32)> {
    let cache = ART_BOUNDS.get_or_init(|| Mutex::new((0, 0.0, 0.0, 0.0, 0.0)));
    let guard = cache.lock().ok()?;
    if guard.0 == *seen || guard.0 == 0 {
        return None;
    }
    *seen = guard.0;
    Some((guard.1, guard.2, guard.3, guard.4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_artwork_dimensions_are_rejected_before_decode() {
        use base64::Engine;
        use std::io::Cursor;

        let image = image::RgbaImage::new(MAX_ARTWORK_DIMENSION + 1, 1);
        let mut encoded = Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(encoded.into_inner());
        assert!(artwork_bytes(&b64).is_none());
    }

    #[test]
    fn scrubber_ratio_clamps_to_the_bar() {
        assert_eq!(scrubber_ratio(50.0, 0.0, 100.0), 0.5);
        assert_eq!(scrubber_ratio(-10.0, 0.0, 100.0), 0.0);
        assert_eq!(scrubber_ratio(200.0, 0.0, 100.0), 1.0);
        assert_eq!(scrubber_ratio(10.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn remaining_time_is_negative_and_clamped() {
        assert_eq!(format_remaining(50.0, 241.0), "-3:11");
        assert_eq!(format_remaining(0.0, 65.0), "-1:05");
        assert_eq!(format_remaining(0.0, 0.0), "–:––");
        assert_eq!(format_remaining(100.0, 90.0), "-0:00");
    }

    #[test]
    fn compact_play_overlay_tracks_polled_island_hover() {
        // GPUI `.hover()` stays true after click-through; this must not.
        assert_eq!(album_overlay_target(false), 0.0);
        assert_eq!(album_overlay_target(true), 1.0);
    }

    #[test]
    fn compact_visualizer_keeps_a_legible_waveform_at_rest() {
        let heights = (0..VIS_REST.len())
            .map(|i| visualizer_scale(0.0, false, i))
            .collect::<Vec<_>>();
        assert_eq!(heights, VIS_REST);
        // Playing: every bar spans the same 2..14 px range.
        for i in 0..VIS_BARS {
            assert_eq!(visualizer_scale(0.0, true, i), VIS_MIN_H / VIS_H);
            assert_eq!(visualizer_scale(1.0, true, i), 1.0);
        }
    }

    #[test]
    fn compact_visualizer_moves_every_bar() {
        // 10 s at the 30 Hz paint rate: each bar sweeps at least 40% of its height.
        let heights: Vec<[f32; VIS_BARS]> = (0..300)
            .map(|i| {
                let raw = nook_core::audio::visualizer_levels_at(i as f64 / 30.0);
                std::array::from_fn(|b| visualizer_scale(raw[b], true, b))
            })
            .collect();
        for bar in 0..VIS_BARS {
            let lo = heights.iter().map(|h| h[bar]).fold(f32::MAX, f32::min);
            let hi = heights.iter().map(|h| h[bar]).fold(f32::MIN, f32::max);
            assert!(hi - lo >= 0.4, "bar {bar} only spans {lo:.2}..{hi:.2}");
            assert!(lo >= VIS_MIN_H / VIS_H && hi <= 1.0);
        }
    }

    fn encode_rgba(w: u32, h: u32, format: image::ImageFormat) -> String {
        use base64::Engine;
        use std::io::Cursor;
        let mut encoded = Cursor::new(Vec::new());
        if format == image::ImageFormat::Jpeg {
            image::RgbImage::from_pixel(w, h, image::Rgb([0x20, 0x40, 0x80]))
                .write_to(&mut encoded, format)
                .unwrap();
        } else {
            image::RgbaImage::from_pixel(w, h, image::Rgba([0x20, 0x40, 0x80, 0xff]))
                .write_to(&mut encoded, format)
                .unwrap();
        }
        base64::engine::general_purpose::STANDARD.encode(encoded.into_inner())
    }

    #[test]
    fn tiff_artwork_is_not_handed_to_gpui_as_jpeg() {
        use base64::Engine;
        let b64 = encode_rgba(8, 8, image::ImageFormat::Tiff);
        let raw = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .unwrap();
        assert!(
            raw.starts_with(&[0x4D, 0x4D]) || raw.starts_with(&[0x49, 0x49]),
            "fixture must be TIFF, got {:02x?}",
            &raw[..4.min(raw.len())]
        );
        let bytes = artwork_bytes(&b64).expect("TIFF artwork should load");
        assert!(
            bytes.starts_with(&[0x89, b'P', b'N', b'G']),
            "MediaRemote YouTube/Safari art is TIFF (MM\\0*) mislabeled as jpeg; GPUI's JPEG decoder then dies on 4D4D. Expected PNG, got {:02x?}",
            &bytes[..4.min(bytes.len())]
        );
        assert_eq!(gpui_format(&bytes), gpui::ImageFormat::Png);
    }

    #[test]
    fn jpeg_and_png_artwork_keep_their_format() {
        let jpeg = artwork_bytes(&encode_rgba(8, 8, image::ImageFormat::Jpeg)).unwrap();
        assert_eq!(gpui_format(&jpeg), gpui::ImageFormat::Jpeg);
        let png = artwork_bytes(&encode_rgba(8, 8, image::ImageFormat::Png)).unwrap();
        assert_eq!(gpui_format(&png), gpui::ImageFormat::Png);
    }

    fn encode_two_tone() -> String {
        use base64::Engine;
        use std::io::Cursor;
        let mut img = image::RgbaImage::new(16, 16);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = if x < 8 {
                image::Rgba([0x20, 0x40, 0xC0, 0xff])
            } else if y < 8 {
                image::Rgba([0xC0, 0x30, 0x20, 0xff])
            } else {
                image::Rgba([0x20, 0xB0, 0x40, 0xff])
            };
        }
        let mut encoded = Cursor::new(Vec::new());
        img.write_to(&mut encoded, image::ImageFormat::Png).unwrap();
        base64::engine::general_purpose::STANDARD.encode(encoded.into_inner())
    }

    #[test]
    fn artwork_palette_returns_three_distinct_colors() {
        let palette = art_palette(Some(&encode_two_tone())).expect("palette");
        assert_eq!(palette.len(), 3);
        assert!(color_dist(palette[0], palette[1]) > 0.1);
        assert!(art_palette(None).is_none());
    }

    #[test]
    fn art_bounds_only_surface_when_the_rect_moves() {
        let mut seen = 0;
        assert!(take_art_bounds(&mut seen).is_none());
        report_art_bounds(10.0, 20.0, 84.0, 84.0);
        let first = take_art_bounds(&mut seen).expect("first report");
        assert_eq!(first, (10.0, 20.0, 84.0, 84.0));
        report_art_bounds(10.2, 20.1, 84.0, 84.0);
        assert!(take_art_bounds(&mut seen).is_none());
        report_art_bounds(40.0, 20.0, 84.0, 84.0);
        assert_eq!(
            take_art_bounds(&mut seen).expect("moved"),
            (40.0, 20.0, 84.0, 84.0)
        );
    }

    #[test]
    fn lyric_stagger_progress_is_monotonic_and_line_zero_leads() {
        let mut last = [0.0f32; 5];
        for ms in 0..=800 {
            let ms = ms as f32;
            let mut prev = 1.0;
            for line in 0..5 {
                let p = lyric_stagger_progress(ms, line);
                assert!(
                    (0.0..=1.0).contains(&p),
                    "p={p} line={line} ms={ms}"
                );
                assert!(
                    p + 1e-5 >= last[line],
                    "non-monotonic line={line} ms={ms}"
                );
                assert!(
                    p <= prev + 1e-5,
                    "line {line} led a higher line at {ms}"
                );
                last[line] = p;
                prev = p;
            }
        }
        assert_eq!(lyric_stagger_progress(0.0, 0), 0.0);
        assert_eq!(lyric_stagger_progress(0.0, 4), 0.0);
        assert!(lyric_stagger_progress(35.0, 0) > lyric_stagger_progress(35.0, 4));
        assert_eq!(lyric_stagger_progress(35.0, 4), 0.0);
        assert!(lyric_stagger_progress(650.0, 4) >= 0.999);
    }

    #[test]
    fn lyric_wrap_target_caps_oversized_measure() {
        report_lyric_current_h(LYRIC_CURRENT.leading);
        assert_eq!(lyric_wrap_target(), 0.0);
        assert_eq!(lyric_wrap_extra(), 0.0);
        report_lyric_current_h(LYRIC_CURRENT.leading * 2.0);
        assert!((lyric_wrap_target() - (-LYRIC_CURRENT.leading / 2.0)).abs() < 1e-3);
        assert!((lyric_wrap_extra() - LYRIC_CURRENT.leading).abs() < 1e-3);
        report_lyric_current_h(84.0);
        assert!((lyric_wrap_target() - (-LYRIC_CURRENT.leading / 2.0)).abs() < 1e-3);
        assert!((lyric_wrap_extra() - LYRIC_CURRENT.leading).abs() < 1e-3);
        report_lyric_current_h(f32::NAN);
        assert!((lyric_wrap_target() - (-LYRIC_CURRENT.leading / 2.0)).abs() < 1e-3);
        assert!((lyric_wrap_extra() - LYRIC_CURRENT.leading).abs() < 1e-3);
        report_lyric_current_h(LYRIC_CURRENT.leading);
        assert_eq!(lyric_wrap_target(), 0.0);
        assert_eq!(lyric_wrap_extra(), 0.0);
    }

    #[test]
    fn first_frame_after_advance_keeps_incoming_current_next_styled() {
        let from = lyrics_retarget_from([0.0; 5], 10_000.0);
        let cur = lyric_motion_paint(true, 0.0, &from, 0.0, true)[2];
        assert!(
            (cur.size - theme::BODY.size).abs() < 1e-3,
            "size {}",
            cur.size
        );
        assert!(
            (cur.leading - theme::BODY.leading).abs() < 1e-3,
            "leading {}",
            cur.leading
        );
        assert!(!cur.bold);
        let next = theme::secondary_label();
        assert!((cur.color.r - next.r).abs() < 1e-3);
        assert!((cur.color.g - next.g).abs() < 1e-3);
        assert!((cur.color.b - next.b).abs() < 1e-3);
        assert!((cur.color.a - next.a).abs() < 1e-3);
        assert!(
            (cur.color.r - theme::LABEL.r).abs() > 0.01
                || (cur.color.a - theme::LABEL.a).abs() > 0.01
        );
    }
}
