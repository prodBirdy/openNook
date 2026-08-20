//! Compact album chip, visualizer, and expanded Now Playing card.
//! Layout matches the React island (`CompactMedia` / `ExpandedMedia`).

use super::Island;
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    div, img, linear_color_stop, linear_gradient, prelude::*, px, relative, rgb, rgba, AnyElement,
    Context, CursorStyle, Image, MouseButton, MouseDownEvent, Rgba, SharedString,
};
use nook_core::models::NowPlayingData;
use std::sync::{Mutex, OnceLock};

const COMPACT_ART: f32 = 26.0;
const COMPACT_ART_RADIUS: f32 = 5.0;
const VIS_BAR_W: f32 = 3.5;
const VIS_BAR_GAP: f32 = 2.5;
const VIS_H: f32 = 20.0;
const VIS_DEFAULT: Rgba = Rgba {
    r: 0.882,
    g: 0.882,
    b: 0.882,
    a: 1.0,
};

pub(super) fn album_chip(np: &NowPlayingData, cx: &mut Context<Island>) -> impl IntoElement {
    let playing = np.is_playing;
    let art = np
        .artwork_base64
        .as_deref()
        .and_then(|b64| artwork_element(b64, COMPACT_ART));
    let overlay_icon = if playing { "pause-fill" } else { "play-fill" };

    div()
        .id("album")
        .relative()
        .size(px(COMPACT_ART))
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
                .opacity(0.)
                .hover(|s| s.opacity(1.))
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

fn artwork_element(b64: &str, size: f32) -> Option<AnyElement> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let format = if bytes.len() >= 8 && bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        gpui::ImageFormat::Png
    } else {
        gpui::ImageFormat::Jpeg
    };
    let image = std::sync::Arc::new(Image::from_bytes(format, bytes));
    Some(
        img(image)
            .size(px(size))
            .object_fit(gpui::ObjectFit::Cover)
            .into_any_element(),
    )
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
        .and_then(|b64| artwork_element(b64, 52.0));

    div()
        .flex()
        .flex_col()
        .w(px(300.))
        .h_full()
        .p(px(16.))
        .gap_3()
        .bg(theme::FILL)
        .rounded(px(20.))
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .size(px(52.))
                        .rounded(px(12.))
                        .overflow_hidden()
                        .shadow_md()
                        .flex_shrink_0()
                        .bg(linear_gradient(
                            135.0,
                            linear_color_stop(rgb(0x2a2a2a), 0.0),
                            linear_color_stop(rgb(0x1a1a1a), 1.0),
                        ))
                        .child(art.unwrap_or_else(|| div().size(px(52.)).into_any_element())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .overflow_hidden()
                        .child(
                            div()
                                .text_color(theme::LABEL)
                                .text_size(px(theme::TITLE_2.size))
                                .line_height(px(theme::TITLE_2.leading))
                                .font_weight(theme::TITLE_2.emphasized)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(title),
                        )
                        .child(
                            div()
                                .text_color(theme::SECONDARY_LABEL)
                                .text_size(px(theme::BODY.size))
                                .line_height(px(theme::BODY.leading))
                                .font_weight(theme::BODY.weight)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(artist),
                        ),
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
                .h(px(12.))
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
                .pt(px(2.))
                .mt(px(6.))
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
        .gap(px(36.))
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
        .size(px(40.))
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
    static CACHE: OnceLock<Mutex<(String, Option<Rgba>)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new((String::new(), None)));
    if let Ok(guard) = cache.lock() {
        if guard.0 == b64 {
            return guard.1;
        }
    }
    let color = sample_dominant_color(b64);
    if let Ok(mut guard) = cache.lock() {
        *guard = (b64.to_string(), color);
    }
    color
}

fn sample_dominant_color(b64: &str) -> Option<Rgba> {
    use base64::Engine;
    use image::GenericImageView;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
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
        if brightness < 20 || brightness > 230 {
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
