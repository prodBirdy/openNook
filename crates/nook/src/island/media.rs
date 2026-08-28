//! Compact album chip, visualizer, and expanded Now Playing card.
//! Layout matches the React island (`CompactMedia` / `ExpandedMedia`).

use super::ui::{
    card_chrome, slide_label, MEDIA_ART, MEDIA_ART_RADIUS, MEDIA_PLAY, MEDIA_PROGRESS_HIT,
    MEDIA_TIME_PAD_GAP, MEDIA_TIME_PAD_TOP,
};
use super::Island;
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    canvas, div, img, linear_color_stop, linear_gradient, prelude::*, px, relative, rgb, rgba,
    AnyElement, Context, CursorStyle, Image, MouseButton, MouseDownEvent, Rgba, SharedString,
};
use nook_core::models::NowPlayingData;
use std::sync::{Mutex, OnceLock};

const MAX_ARTWORK_BYTES: usize = 5 * 1024 * 1024;
const MAX_ARTWORK_DIMENSION: u32 = 4096;

const COMPACT_ART: f32 = theme::COMPACT_FACE;
const COMPACT_ART_RADIUS: f32 = 5.0;
const ART: f32 = MEDIA_ART;
const ART_RADIUS: f32 = MEDIA_ART_RADIUS;
const PLAY: f32 = MEDIA_PLAY;
const SKIP_GAP: f32 = 36.0;
/// Room for ~15 title glyphs at Title 2, beside the artwork.
const TITLE_COL: f32 = 120.0;
const VIS_BAR_W: f32 = 3.5;
const VIS_BAR_GAP: f32 = 2.5;
const VIS_H: f32 = 20.0;
const VIS_DEFAULT: Rgba = Rgba {
    r: 0.882,
    g: 0.882,
    b: 0.882,
    a: 1.0,
};

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
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let playing = np.is_playing;
    let art = np
        .artwork_base64
        .as_deref()
        .and_then(|b64| artwork_element(b64, COMPACT_ART, COMPACT_ART_RADIUS));
    let overlay_icon = if playing { "pause-fill" } else { "play-fill" };

    div()
        .id("album")
        .relative()
        .size(px(COMPACT_ART))
        .flex_shrink_0()
        .rounded(px(COMPACT_ART_RADIUS))
        .overflow_hidden()
        .shadow_sm()
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.now_playing.is_playing = !this.now_playing.is_playing;
                cx.notify();
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_play_pause().await;
                });
            }),
        )
        .child(art.unwrap_or_else(placeholder_art))
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(COMPACT_ART_RADIUS))
                .bg(rgba(0x0000004D))
                .flex()
                .items_center()
                .justify_center()
                .opacity(overlay_alpha.clamp(0.0, 1.0))
                .when(!playing, |d| d.pl(px(1.)))
                .child(lucide_color(overlay_icon, 14.0, rgb(0xffffff))),
        )
}

fn placeholder_art() -> AnyElement {
    div()
        .size(px(COMPACT_ART))
        .rounded(px(COMPACT_ART_RADIUS))
        .bg(linear_gradient(
            135.0,
            linear_color_stop(rgb(0x333333), 0.0),
            linear_color_stop(rgb(0x111111), 1.0),
        ))
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color("music", 14.0, rgba(0xffffff80)))
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

pub(super) fn visualizer(levels: &[f64], playing: bool, color: Option<Rgba>) -> impl IntoElement {
    let color = color.unwrap_or(VIS_DEFAULT);
    let mut row = div()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(VIS_BAR_GAP))
        .h(px(VIS_H))
        .pr_1();
    for (i, level) in levels.iter().take(6).enumerate() {
        let scale = if playing {
            (*level as f32).clamp(0.15, 1.0)
        } else {
            0.15
        };
        row = row.child(
            div()
                .id(SharedString::from(format!("bar-{i}")))
                .w(px(VIS_BAR_W))
                .h(px(VIS_H * scale))
                .rounded_full()
                .bg(color),
        );
    }
    row
}

const NOOK_ART: f32 = 84.0;
pub(crate) const NOOK_ART_RADIUS: f32 = 14.0;
const APP_BADGE: f32 = 22.0;
const APP_BADGE_RADIUS: f32 = 5.0;

/// Horizontal Now Playing strip for the Nook tab (art + metadata + transport).
pub(crate) fn nook_media_pane(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let np = &island.now_playing;
    let title = np.title.clone().unwrap_or_else(|| "Unknown Title".into());
    let artist = np.artist.clone().unwrap_or_else(|| "Unknown Artist".into());
    let album = np.album.clone().filter(|s| !s.is_empty());
    let playing = np.is_playing;
    let aura = media_aura(island);
    let elapsed = np.elapsed_time.unwrap_or(0.0);
    let duration = np.duration.unwrap_or(0.0);
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

    div()
        .id("nook-media")
        .relative()
        .w_full()
        .h_full()
        .overflow_hidden()
        .when_some(aura, |d, aura| d.child(aura))
        .flex()
        .items_center()
        .gap(px(14.))
        .pr(px(4.))
        .child(
            div()
                .relative()
                .size(px(NOOK_ART))
                .flex_shrink_0()
                .child(
                    div()
                        .size(px(NOOK_ART))
                        .rounded(px(NOOK_ART_RADIUS))
                        .overflow_hidden()
                        .shadow_md()
                        .bg(linear_gradient(
                            135.0,
                            linear_color_stop(rgb(0x2a2a2a), 0.0),
                            linear_color_stop(rgb(0x1a1a1a), 1.0),
                        ))
                        .child(art.unwrap_or_else(|| {
                            div()
                                .size(px(NOOK_ART))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(lucide_color("music", 28.0, rgba(0xffffff66)))
                                .into_any_element()
                        }))
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
                .child(
                    div()
                        .absolute()
                        .right(px(4.))
                        .bottom(px(4.))
                        .shadow_sm()
                        .child(app_badge(np.bundle_id.as_deref(), np.app_name.as_deref())),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .justify_center()
                .min_w(px(0.))
                .w(px(160.))
                .overflow_hidden()
                .child(slide_label(title, theme::TITLE_3, true).w_full())
                .when_some(album, |d, album| {
                    d.child(slide_label(album, theme::CALLOUT, false).w_full())
                })
                .child(slide_label(artist, theme::CALLOUT, false).w_full())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(18.))
                        .mt(px(6.))
                        .child(nook_skip("skip-back", "nook-skip-back", cx, |_, _, _| {
                            nook_core::runtime().spawn(async {
                                let _ = nook_core::audio::media_previous_track().await;
                            });
                        }))
                        .child(nook_play(playing, cx))
                        .child(nook_skip("skip-forward", "nook-skip-fwd", cx, |_, _, _| {
                            nook_core::runtime().spawn(async {
                                let _ = nook_core::audio::media_next_track().await;
                            });
                        })),
                )
                .child(nook_progress(progress, elapsed, duration, seekable, cx)),
        )
}

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
        .bg(rgba(0x00000099))
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color("music", 11.0, rgb(0xffffff)))
        .into_any_element()
}

fn app_icon_image(
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

fn nook_progress(
    progress: f32,
    elapsed: f64,
    duration: f64,
    seekable: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let progress = progress.clamp(0.0, 1.0);
    div()
        .w_full()
        .mt(px(6.))
        .opacity(if seekable { 1.0 } else { 0.45 })
        .cursor(if seekable {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Arrow
        })
        .child(
            div()
                .relative()
                .w_full()
                .h(px(10.))
                .flex()
                .items_center()
                .child(
                    div()
                        .w_full()
                        .h(px(3.))
                        .rounded(px(2.))
                        .bg(rgba(0xffffff26))
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress))
                                .rounded(px(2.))
                                .bg(rgb(0xffffff)),
                        ),
                )
                .when(seekable, |d| d.child(nook_seek_hits(cx))),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .pt(px(2.))
                .child(time_label(format_time(elapsed)))
                .child(time_label(format_time(duration))),
        )
}

fn nook_seek_hits(cx: &mut Context<Island>) -> impl IntoElement {
    const SEGMENTS: u32 = 24;
    let mut row = div()
        .absolute()
        .inset_0()
        .flex()
        .cursor(CursorStyle::PointingHand);
    for i in 0..SEGMENTS {
        let ratio = (i as f32 + 0.5) / SEGMENTS as f32;
        row = row.child(
            div()
                .id(SharedString::from(format!("nook-seek-{i}")))
                .flex_1()
                .h_full()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let Some(duration) = this.now_playing.duration.filter(|d| *d > 0.0) else {
                            return;
                        };
                        let position = duration * ratio as f64;
                        this.now_playing.elapsed_time = Some(position);
                        cx.notify();
                        nook_core::runtime().spawn(async move {
                            let _ = nook_core::audio::media_seek(position).await;
                        });
                    }),
                ),
        );
    }
    row
}

fn nook_skip(
    icon: &'static str,
    elem_id: &'static str,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(elem_id)
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .opacity(0.9)
        .hover(|s| s.opacity(1.0))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(icon, 16.0, rgb(0xffffff)))
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
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.7))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            if playing { "pause" } else { "play" },
            16.0,
            rgb(0xffffff),
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.now_playing.is_playing = !this.now_playing.is_playing;
                cx.notify();
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_play_pause().await;
                });
            }),
        )
}

#[allow(dead_code)]
pub(crate) fn media_card(np: &NowPlayingData, cx: &mut Context<Island>) -> impl IntoElement {
    let title = np.title.clone().unwrap_or_else(|| "Unknown Title".into());
    let artist = np.artist.clone().unwrap_or_else(|| "Unknown Artist".into());
    let playing = np.is_playing;
    let elapsed = np.elapsed_time.unwrap_or(0.0);
    let duration = np.duration.unwrap_or(0.0);
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
                            linear_color_stop(rgb(0x2a2a2a), 0.0),
                            linear_color_stop(rgb(0x1a1a1a), 1.0),
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
                        // Track and artist are the two strings most likely to
                        // outrun the card, so they slide rather than ellipsis.
                        .child(slide_label(title, theme::TITLE_2, true).w_full())
                        .child(slide_label(artist, theme::BODY, false).w_full()),
                ),
        )
        .child(progress_block(progress, elapsed, duration, seekable, cx))
        .child(transport_row(playing, cx))
}

fn progress_block(
    progress: f32,
    elapsed: f64,
    duration: f64,
    seekable: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let progress = progress.clamp(0.0, 1.0);
    div()
        .flex()
        .flex_col()
        .w_full()
        .opacity(if seekable { 1.0 } else { 0.5 })
        .cursor(if seekable {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Arrow
        })
        .child(
            div()
                .relative()
                .w_full()
                .h(px(MEDIA_PROGRESS_HIT))
                .flex()
                .items_center()
                .child(
                    div()
                        .w_full()
                        .h(px(4.))
                        .rounded(px(2.))
                        .bg(rgba(0xffffff26))
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress))
                                .rounded(px(2.))
                                .bg(rgb(0xffffff)),
                        ),
                )
                .when(seekable, |d| d.child(seek_hits(cx))),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .pt(px(MEDIA_TIME_PAD_TOP))
                .mt(px(MEDIA_TIME_PAD_GAP))
                .px(px(1.))
                .child(time_label(format_time(elapsed)))
                .child(time_label(format_time(duration))),
        )
}

fn seek_hits(cx: &mut Context<Island>) -> impl IntoElement {
    const SEGMENTS: u32 = 32;
    let mut row = div()
        .absolute()
        .inset_0()
        .flex()
        .cursor(CursorStyle::PointingHand);
    for i in 0..SEGMENTS {
        let ratio = (i as f32 + 0.5) / SEGMENTS as f32;
        row = row.child(
            div()
                .id(SharedString::from(format!("seek-{i}")))
                .flex_1()
                .h_full()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let Some(duration) = this.now_playing.duration.filter(|d| *d > 0.0) else {
                            return;
                        };
                        let position = duration * ratio as f64;
                        this.now_playing.elapsed_time = Some(position);
                        cx.notify();
                        nook_core::runtime().spawn(async move {
                            let _ = nook_core::audio::media_seek(position).await;
                        });
                    }),
                ),
        );
    }
    row
}

fn transport_row(playing: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(SKIP_GAP))
        .child(skip_btn(
            "skip-back-fill",
            "ibtn-skip-back",
            cx,
            |_, _, _| {
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_previous_track().await;
                });
            },
        ))
        .child(play_btn(playing, cx))
        .child(skip_btn(
            "skip-forward-fill",
            "ibtn-skip-forward",
            cx,
            |_, _, _| {
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
        .child(lucide_color(icon, 24.0, rgb(0xffffff)))
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
        .bg(rgb(0xffffff))
        .flex()
        .items_center()
        .justify_center()
        .when(!playing, |d| d.pl(px(1.5)))
        .hover(|s| s.bg(rgb(0xf5f5f5)))
        .active(|s| s.opacity(0.95))
        .cursor(CursorStyle::PointingHand)
        .shadow_sm()
        .child(lucide_color(
            if playing { "pause-fill" } else { "play-fill" },
            22.0,
            rgb(0x000000),
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.now_playing.is_playing = !this.now_playing.is_playing;
                cx.notify();
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_play_pause().await;
                });
            }),
        )
}

fn time_label(text: String) -> impl IntoElement {
    div()
        .text_color(rgba(0xffffff66))
        .text_size(px(theme::SUBHEADLINE.size))
        .line_height(px(theme::SUBHEADLINE.leading))
        .font_weight(theme::SUBHEADLINE.emphasized)
        .font_family("SF Mono")
        .child(text)
}

fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0) as u32;
    format!("{}:{:02}", total / 60, total % 60)
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
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
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
        let base = picked.first().copied().or_else(|| sample_dominant_color(b64))?;
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

fn media_aura(island: &Island) -> Option<AnyElement> {
    if !island.settings.ambient_art_glow || island.reduce_motion {
        return None;
    }
    let palette = island.aura_palette?;
    let t = if island.now_playing.is_playing {
        island.aura_t
    } else {
        0.0
    };
    let mut layer = div()
        .absolute()
        .inset_0()
        .overflow_hidden()
        .opacity(0.9);
    for (i, color) in palette.iter().enumerate() {
        let (dx, dy) = crate::motion::aura_blob_offset(i, t);
        let faded = Rgba {
            r: color.r,
            g: color.g,
            b: color.b,
            a: 0.32,
        };
        layer = layer.child(
            div()
                .absolute()
                .left(px(8.0 + dx))
                .top(px(-16.0 + dy))
                .size(px(140.0))
                .rounded_full()
                .bg(faded),
        );
    }
    Some(layer.into_any_element())
}

static ART_BOUNDS: OnceLock<Mutex<(u64, f32, f32, f32, f32)>> = OnceLock::new();

fn report_art_bounds(x: f32, y: f32, w: f32, h: f32) {
    let cache = ART_BOUNDS.get_or_init(|| Mutex::new((0, 0.0, 0.0, 0.0, 0.0)));
    let Ok(mut guard) = cache.lock() else {
        return;
    };
    let (gen, ox, oy, ow, oh) = *guard;
    if (ox - x).abs() < 0.5 && (oy - y).abs() < 0.5 && (ow - w).abs() < 0.5 && (oh - h).abs() < 0.5 {
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
    fn compact_play_overlay_tracks_polled_island_hover() {
        // GPUI `.hover()` stays true after click-through; this must not.
        assert_eq!(album_overlay_target(false), 0.0);
        assert_eq!(album_overlay_target(true), 1.0);
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
