//! Compact album chip, visualizer, and expanded Now Playing pane.

use super::ui::{
    card_chrome, scroll_body, slide_label, timer_text, MEDIA_ART, MEDIA_ART_RADIUS, MEDIA_PLAY,
    MEDIA_PROGRESS_HIT, MEDIA_TIME_PAD_GAP, MEDIA_TIME_PAD_TOP,
};
use super::{Island, QUEUE_PANEL_W, QUEUE_ROW_H};
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    canvas, div, img, linear_color_stop, linear_gradient, point, prelude::*, px, relative,
    AnyElement, BoxShadow, Context, CursorStyle, FontWeight, Image, MouseButton, MouseDownEvent,
    Rgba, SharedString,
};
use nook_core::models::{NowPlayingData, PlaybackQueue, QueueItem};
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

fn artwork_element(b64: &str, size: f32, radius: f32) -> Option<AnyElement> {
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
    // Clock-driven bars at ~15 fps via request_animation_frame — only while
    // this element is on screen (Media compact face). No island-tick dirties.
    canvas(
        move |_, _, _| (),
        move |bounds, _, window, _cx| {
            let levels: [f64; VIS_BARS] = if still {
                VIS_REST.map(|r| r as f64)
            } else {
                // Quantize to 15 Hz so bar heights hold between paints.
                // Core still samples 6 bands; take the first five for the face.
                let t = (vis_clock() * 15.0).floor() / 15.0;
                let raw = nook_core::audio::visualizer_levels_at(t);
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

fn visualizer_scale(level: f64, playing: bool, index: usize) -> f32 {
    let resting = VIS_REST[index];
    if playing {
        (level as f32).clamp((resting * 0.5).max(VIS_MIN_H / VIS_H), 1.0)
    } else {
        resting
    }
}

/// Gallery artwork 52×52 (the Now Playing row is 52 tall).
const NOOK_ART: f32 = 52.0;
/// Mockup artwork radius 13.
pub(crate) const NOOK_ART_RADIUS: f32 = 13.0;
const APP_BADGE: f32 = 22.0;
const APP_BADGE_RADIUS: f32 = 5.0;
const NOOK_PLAY_HIT: f32 = 40.0;
const NOOK_SKIP_HIT: f32 = 32.0;
const NOOK_PLAY_GLYPH: f32 = 26.0;
const NOOK_SKIP_GLYPH: f32 = 19.0;
/// Expanded track title: mockup 16/20 semibold (between TITLE_3 and TITLE_2).
const MEDIA_TITLE: crate::theme::Text = crate::theme::Text {
    size: 16.0,
    leading: 20.0,
    weight: FontWeight::NORMAL,
    emphasized: FontWeight::SEMIBOLD,
};
#[allow(dead_code)]
const NOOK_PROGRESS_H: f32 = 6.0;
/// Gallery pane pad `8 12`. Vertical is 7 here: the rows (52 + 22 + 40)
/// need 114 of the 128pt body, which the mockup's 8 overshoots by 2.
const NOOK_PAD_X: f32 = 12.0;
const NOOK_PAD_Y: f32 = 7.0;
/// Artwork ring `#FFFFFF1A` (shadow `0 3 10 #00000066` is inline below).
const NOOK_ART_RING: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.10,
};

/// Expanded Now Playing: artwork + title row, flanked scrubber, filled
/// transport. With no track loaded the same chrome stays up — "Not
/// Playing", the last player's name, zeroed scrubber, dimmed controls —
/// instead of a separate empty state.
/// Lyrics sit beside the player when enabled.
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
    // Queue claims the trailing column; lyrics stay hidden while it is open.
    let lyrics = if queue_open {
        None
    } else {
        lyrics_pane(island)
    };
    let show_picker = island.output_picker_enabled();
    let picker_open = island.output_picker_open && show_picker;
    // The list slot is always drawn (gallery); it only works where the
    // player exposes a local queue.
    let queue_enabled = island.settings.show_media_queue
        && nook_core::queue::supports_local_queue(np.app_name.as_deref(), np.bundle_id.as_deref());
    // Gallery draws AirPlay here whatever the current route is.
    let picker_icon = "airplay";

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
                .overflow_hidden()
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(2.))
                .child(if has {
                    slide_label(title, MEDIA_TITLE, true)
                        .w_full()
                        .into_any_element()
                } else {
                    div()
                        .w_full()
                        .text_size(px(MEDIA_TITLE.size))
                        .line_height(px(MEDIA_TITLE.leading))
                        .font_weight(MEDIA_TITLE.emphasized)
                        .text_color(theme::secondary_label())
                        .whitespace_nowrap()
                        .child("Not Playing")
                        .into_any_element()
                })
                .child(slide_label(artist, theme::BODY, false).w_full()),
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
            .child(header)
            .child(output_picker_list(island, cx))
            .into_any_element()
    } else {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .justify_between()
            .overflow_hidden()
            .child(header.flex_shrink_0().h(px(NOOK_ART)))
            .child(
                div()
                    .w_full()
                    .h(px(22.))
                    .flex_shrink_0()
                    .child(nook_progress(
                        island, progress, elapsed, duration, seekable, cx,
                    )),
            )
            .child(
                div()
                    .w_full()
                    .h(px(40.))
                    .flex_shrink_0()
                    .opacity(if has { 1.0 } else { theme::DISABLED_OPACITY })
                    .child(nook_transport(
                        playing,
                        queue_enabled,
                        queue_open,
                        show_picker,
                        picker_icon,
                        cx,
                    )),
            )
            .into_any_element()
    };

    let player = div()
        .id("nook-media")
        .relative()
        .h_full()
        .min_h(px(theme::NOOK_BODY - 4.0))
        .px(px(NOOK_PAD_X))
        .py(px(NOOK_PAD_Y))
        .overflow_hidden()
        .flex()
        .gap(px(12.))
        .child(body)
        .when_some(lyrics, |d, pane| d.child(pane));
    // When the queue is open, the pane is often wider than the nominal cell
    // width (flex-grown Nook column). Let the player absorb the leftover so
    // the fixed-width Up Next panel does not leave a dead strip on the right.
    let player = if queue_open {
        player
            .flex_1()
            .min_w(px(island.music_player_width()))
            .into_any_element()
    } else {
        player.flex_1().min_w(px(0.)).into_any_element()
    };

    div()
        .id("nook-media-col")
        .w_full()
        .h_full()
        .overflow_hidden()
        .flex()
        .gap(px(12.))
        .child(player)
        .when(queue_open, |d| {
            d.child(
                div()
                    .w(px(1.))
                    .h_full()
                    .flex_shrink_0()
                    .my(px(4.))
                    .bg(theme::SEPARATOR),
            )
            .child(up_next_panel(&island.queue, cx))
        })
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

fn nook_transport(
    playing: bool,
    queue_enabled: bool,
    queue_open: bool,
    show_picker: bool,
    picker_icon: &'static str,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    // Equal leading/trailing slots keep play geometrically centered while
    // the list / output glyphs sit on the edges.
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
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_next_track().await;
                });
            },
        ))
        .child(
            div()
                .w(px(theme::HIT_MIN))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_end()
                .when(show_picker, |d| {
                    d.child(output_picker_btn(picker_icon, false, cx))
                }),
        )
}

/// Gallery list slot: 15pt glyph flush left in a 28pt slot. Always drawn;
/// without a local queue it is dimmed and inert.
fn queue_toggle_btn(open: bool, enabled: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("nook-queue-toggle")
        .size(px(theme::HIT_MIN))
        .flex()
        .items_center()
        .justify_start()
        .child(lucide_color(
            "list",
            15.0,
            if open {
                theme::LABEL
            } else {
                theme::tertiary_label()
            },
        ))
        .when(!enabled, |d| d.opacity(theme::DISABLED_OPACITY))
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
        .flex()
        .items_center()
        // Gallery: 16pt glyph flush right in the 28pt slot.
        .justify_end()
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            icon,
            16.0,
            if open {
                theme::LABEL
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

fn lyrics_pane(island: &Island) -> Option<AnyElement> {
    if !island.settings.show_lyrics {
        return None;
    }
    let lyrics = island.lyrics.as_ref()?;
    if lyrics.instrumental {
        return None;
    }
    if lyrics.has_synced() {
        let [prev, cur, next] = lyrics.highlight_window(island.lyrics_position_ms());
        return Some(lyrics_window(prev, cur, next));
    }
    let plain = lyrics.plain.as_deref()?;
    let mut lines = plain.lines().filter(|line| !line.trim().is_empty());
    let first = lines.next()?;
    Some(lyrics_window(
        None,
        Some(first.trim()),
        lines.next().map(str::trim),
    ))
}

fn lyrics_window(prev: Option<&str>, cur: Option<&str>, next: Option<&str>) -> AnyElement {
    div()
        .id("lyrics-pane")
        .flex_1()
        .min_w(px(72.))
        .max_w(px(220.))
        .h_full()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.))
        .overflow_hidden()
        .child(lyric_line(prev.unwrap_or(""), false))
        .child(lyric_line(cur.unwrap_or(""), true))
        .child(lyric_line(next.unwrap_or(""), false))
        .into_any_element()
}

fn lyric_line(text: &str, current: bool) -> AnyElement {
    if text.is_empty() {
        return div()
            .h(px(theme::CALLOUT.leading))
            .w_full()
            .into_any_element();
    }
    slide_label(text.to_string(), theme::CALLOUT, current)
        .w_full()
        .into_any_element()
}

fn output_picker_list(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let mut list = div()
        .id("nook-output-list")
        .flex()
        .flex_col()
        .min_w(px(0.))
        .w(px(200.))
        .h_full()
        .overflow_hidden();
    // Styled after the Control Center Sound panel: a small section title,
    // rows with a circular icon chip, and a checkmark on the active device.
    list = list.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .mb(px(6.))
            .child(
                div()
                    .text_color(theme::SECONDARY_LABEL)
                    .text_size(px(theme::SUBHEADLINE.size))
                    .line_height(px(theme::SUBHEADLINE.leading))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Sound"),
            )
            .child(output_picker_btn("airplay", true, cx)),
    );
    if island.output_devices.is_empty() {
        list = list.child(
            div()
                .text_color(theme::SECONDARY_LABEL)
                .text_size(px(theme::SUBHEADLINE.size))
                .child("No output devices"),
        );
    } else {
        let mut rows = div().flex().flex_col().gap(px(1.));
        for device in &island.output_devices {
            let id = device.id;
            let name = device.name.clone();
            let icon = device.icon();
            let selected = device.is_default;
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("out-dev-{id}")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(30.))
                    .rounded(px(theme::CONTROL_RADIUS))
                    .px(px(4.))
                    .hover(|s| s.bg(theme::FILL_TERTIARY))
                    .active(|s| s.bg(theme::FILL_SECONDARY))
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.select_output_device(id, cx);
                        }),
                    )
                    .child(
                        div()
                            .size(px(theme::HIT_MIN))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(if selected {
                                theme::LABEL
                            } else {
                                theme::FILL_SECONDARY
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(lucide_color(
                                icon,
                                12.0,
                                if selected {
                                    theme::WINDOW_BG
                                } else {
                                    theme::LABEL
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(theme::LABEL)
                            .text_size(px(theme::SUBHEADLINE.size))
                            .line_height(px(theme::SUBHEADLINE.leading))
                            .child(name),
                    )
                    .when(selected, |d| {
                        d.child(lucide_color("check", 13.0, theme::LABEL))
                    }),
            );
        }
        list = list.child(scroll_body("nook-output-rows", rows));
    }
    list.child(
        div()
            .mt(px(4.))
            .text_color(theme::tertiary_label())
            .text_size(px(theme::FOOTNOTE.size))
            .line_height(px(theme::FOOTNOTE.leading))
            .child("AirPlay starts from Control Center."),
    )
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
    div()
        .w_full()
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
            time_label(if duration > 0.0 {
                format_time(elapsed)
            } else {
                "0:00".into()
            })
            .w(px(30.))
            .text_color(theme::tertiary_label()),
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
                        .bg(theme::with_alpha(theme::LABEL, 0.14))
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
            time_label(if duration > 0.0 {
                format_remaining(elapsed, duration)
            } else {
                "-0:00".into()
            })
            .w(px(34.))
            .text_right()
            .text_color(theme::tertiary_label()),
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

fn up_next_panel(queue: &PlaybackQueue, cx: &mut Context<Island>) -> impl IntoElement {
    let label = if queue.label.is_empty() {
        "Playing Next".to_string()
    } else {
        queue.label.clone()
    };
    let empty_msg = queue_empty_message(queue);
    let mut rows = div()
        .id("up-next-rows")
        .flex_1()
        .min_h(px(QUEUE_ROW_H))
        .overflow_y_scroll();
    if queue.items.is_empty() {
        rows = rows.child(
            div()
                .pt(px(4.))
                .pr(px(4.))
                .text_size(px(theme::FOOTNOTE.size))
                .line_height(px(theme::FOOTNOTE.leading))
                .text_color(theme::SECONDARY_LABEL)
                .child(empty_msg),
        );
    } else {
        for (i, item) in queue.items.iter().enumerate() {
            rows = rows.child(queue_row(i, item, cx));
        }
    }
    div()
        .id("up-next")
        .w(px(QUEUE_PANEL_W))
        .flex_shrink_0()
        .h_full()
        .min_h(px(theme::NOOK_BODY - 4.0))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .pb(px(6.))
                .text_size(px(theme::SUBHEADLINE.size))
                .line_height(px(theme::SUBHEADLINE.leading))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child(label),
        )
        .child(rows)
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
        Some(QueueHidden::Radio) => "Hidden during radio",
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
        .and_then(|b64| artwork_element(b64, 32.0, 6.0))
        .unwrap_or_else(|| {
            div()
                .size(px(32.))
                .rounded(px(theme::CONTROL_RADIUS))
                .bg(theme::FILL_TERTIARY)
                .flex()
                .items_center()
                .justify_center()
                .child(lucide_color("music", 12.0, theme::SECONDARY_LABEL))
                .into_any_element()
        });
    let title = item.title.clone();
    let artist = item.artist.clone();
    let jump = item.clone();
    div()
        .id(SharedString::from(format!("up-next-{index}")))
        .h(px(QUEUE_ROW_H))
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(theme::CONTROL_RADIUS))
        .hover(|s| s.bg(theme::FILL_TERTIARY))
        .active(|s| s.bg(theme::FILL_SECONDARY))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let item = jump.clone();
                let context = this.queue.context_uri.clone();
                nook_core::runtime().spawn(async move {
                    let _ = nook_core::audio::media_jump_to_queue_item(item, context).await;
                });
            }),
        )
        .child(art)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(theme::CALLOUT.size))
                        .font_weight(theme::CALLOUT.emphasized)
                        .text_color(theme::LABEL)
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(theme::FOOTNOTE.size))
                        .text_color(theme::SECONDARY_LABEL)
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(artist),
                ),
        )
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

static ART_BOUNDS: OnceLock<Mutex<(u64, f32, f32, f32, f32)>> = OnceLock::new();

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
        assert!(visualizer_scale(0.0, true, 3) > visualizer_scale(0.0, true, 0));
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
}
