//! Lyrics floating window: full card and mini pill.

use super::chrome;
use super::PipView;
use crate::icons::lucide_color;
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, relative, AnyElement, Context, CursorStyle, Entity, FontWeight,
    MouseButton, MouseDownEvent, Rgba, Window,
};
use std::cell::Cell;

pub use super::chrome::{LYRICS_W, MINI_H, MINI_W};

const HEADER_H: f32 = 28.0;
const GAP: f32 = 6.0;
const LINES_PAD: f32 = 2.0;
const LINE_GAP: f32 = 5.0;
const PAST_H: f32 = 16.0;
const CURRENT_H: f32 = 20.0;
const NEXT_H: f32 = 16.0;
const LINES_H: f32 = LINES_PAD * 2.0 + PAST_H + LINE_GAP + CURRENT_H + LINE_GAP + NEXT_H;
const TRANSPORT_H: f32 = 28.0;
const PROGRESS_H: f32 = 3.0;
const PAD_TOP: f32 = 10.0;
const PAD_BOTTOM: f32 = 12.0;
const ART: f32 = 20.0;
const ART_RADIUS: f32 = 5.0;
const MINI_ART_RADIUS: f32 = 10.0;

thread_local! {
    static MEASURE_FOLLOWUP: Cell<bool> = const { Cell::new(false) };
}

pub fn render(
    view: &PipView,
    island: &Entity<Island>,
    window: &mut Window,
    cx: &mut Context<PipView>,
) -> AnyElement {
    let height = chrome::desired_height(view, island.read(cx));
    // Hover fade + first-frame skip live in chrome; wrap extra can already
    // be in `height` on the first PiP paint (gallery measured first), so
    // also resize when it disagrees with the window's last applied size.
    if (view.content_height - height).abs() >= 0.5 {
        cx.defer_in(window, move |view, window, cx| {
            view.set_content_height(height, window, cx);
        });
    } else if !MEASURE_FOLLOWUP.get() {
        // Canvas wrap measure runs in this frame's prepaint; wake once so
        // a wrapping current line can grow the window on the next frame.
        MEASURE_FOLLOWUP.set(true);
        window.request_animation_frame();
    }
    chrome::note_content_height(view, height, window, cx);
    if chrome::is_mini(view) {
        return mini(island, cx);
    }
    full(view, island, cx)
}

fn lines_box_h() -> f32 {
    LINES_H + crate::island::media::lyric_wrap_extra()
}

pub fn content_height(view: &PipView, _island: &Island) -> f32 {
    if chrome::is_mini(view) {
        return MINI_H;
    }
    let mut height = PAD_TOP + HEADER_H + GAP + lines_box_h() + GAP + PROGRESS_H + PAD_BOTTOM;
    if view.hovered {
        height += TRANSPORT_H + GAP;
    }
    height
}

pub fn desired_height(view: &PipView, island: &Island) -> f32 {
    content_height(view, island)
}

fn full(view: &PipView, island: &Entity<Island>, cx: &mut Context<PipView>) -> AnyElement {
    let (title, artist, np) = {
        let island = island.read(cx);
        let np = island.now_playing.clone();
        (
            np.title.clone().unwrap_or_default(),
            np.artist.clone().unwrap_or_default(),
            np,
        )
    };
    let header_el = header(view, island, &title, &artist, cx);
    let lines_el = lines(island.read(cx));
    let mut column = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .child(header_el)
        .child(lines_el);
    if view.hovered {
        column = column.child(transport(island, cx));
    }
    column.child(progress(&np)).into_any_element()
}

fn header(
    view: &PipView,
    island: &Entity<Island>,
    title: &str,
    artist: &str,
    cx: &mut Context<PipView>,
) -> impl IntoElement {
    let artwork_b64 = island.read(cx).now_playing.artwork_base64.clone();
    let reduce_motion = island.read(cx).reduce_motion;
    div()
        .w_full()
        .h(px(HEADER_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(artwork(artwork_b64.as_deref(), ART, ART_RADIUS, true))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .flex()
                .items_end()
                .gap(px(6.))
                .child(track_text(
                    title,
                    13.0,
                    16.0,
                    FontWeight::BOLD,
                    theme::LABEL,
                ))
                .child(track_text(
                    artist,
                    12.0,
                    15.0,
                    FontWeight::NORMAL,
                    theme::secondary_label(),
                )),
        )
        .when(view.hovered, |row| {
            row.child(chrome::hover_controls(view, reduce_motion, cx))
        })
}

fn track_text(
    text: &str,
    size: f32,
    leading: f32,
    weight: FontWeight,
    color: Rgba,
) -> impl IntoElement {
    div()
        .min_w(px(0.))
        .flex_shrink()
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .text_size(px(size))
        .line_height(px(leading))
        .font_weight(weight)
        .text_color(color)
        .child(text.to_string())
}

fn lines(island: &Island) -> impl IntoElement {
    // P3: shared animated stack (box_h matches this window's lines area,
    // grown by the measured wrap extra so past/current/next stay unclipped).
    crate::island::media::lyrics_lines_block(island, lines_box_h())
}

fn transport(island: &Entity<Island>, cx: &mut Context<PipView>) -> impl IntoElement {
    let playing = island.read(cx).now_playing.is_playing;
    div()
        .w_full()
        .h(px(TRANSPORT_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(28.))
        .child(glyph_btn(
            "pip-skip-back",
            "skip-back",
            15.0,
            cx,
            |this, cx| {
                let _ = this
                    .island
                    .update(cx, |island, cx| island.note_media_skip(cx));
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_previous_track().await;
                });
            },
        ))
        .child(glyph_btn(
            "pip-play",
            if playing { "pause" } else { "play" },
            18.0,
            cx,
            |this, cx| {
                let _ = this
                    .island
                    .update(cx, |island, cx| island.note_media_play_pause(cx));
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_play_pause().await;
                });
            },
        ))
        .child(glyph_btn(
            "pip-skip-fwd",
            "skip-forward",
            15.0,
            cx,
            |this, cx| {
                let _ = this
                    .island
                    .update(cx, |island, cx| island.note_media_skip(cx));
                nook_core::runtime().spawn(async {
                    let _ = nook_core::audio::media_next_track().await;
                });
            },
        ))
}

fn glyph_btn(
    id: &'static str,
    icon: &'static str,
    size: f32,
    cx: &mut Context<PipView>,
    on_click: impl Fn(&mut PipView, &mut Context<PipView>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_shrink_0()
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.75))
        .child(lucide_color(icon, size, theme::LABEL))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, cx);
            }),
        )
}

fn progress(np: &nook_core::models::NowPlayingData) -> impl IntoElement {
    let duration = np.duration.unwrap_or(0.0);
    let ratio = if duration > 0.0 {
        (np.elapsed_time.unwrap_or(0.0) / duration).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    div()
        .w_full()
        .h(px(PROGRESS_H))
        .flex_shrink_0()
        .rounded(px(1.5))
        .bg(theme::FILL_TERTIARY)
        .child(
            div()
                .h_full()
                .w(relative(ratio))
                .rounded(px(1.5))
                .bg(theme::LABEL),
        )
}

fn mini(island: &Entity<Island>, cx: &mut Context<PipView>) -> AnyElement {
    let (playing, art, line) = {
        let island = island.read(cx);
        (
            island.now_playing.is_playing,
            island.now_playing.artwork_base64.clone(),
            current_line(island),
        )
    };
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(artwork(art.as_deref(), ART, MINI_ART_RADIUS, false))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(13.))
                .line_height(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child(line),
        )
        .child(
            div()
                .id("pip-mini-play")
                .size(px(24.))
                .flex_shrink_0()
                .rounded(px(12.))
                .bg(theme::FILL)
                .flex()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::PointingHand)
                .active(|style| style.opacity(0.75))
                .child(lucide_color(
                    if playing { "pause" } else { "play" },
                    12.0,
                    theme::LABEL,
                ))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let _ = this
                            .island
                            .update(cx, |island, cx| island.note_media_play_pause(cx));
                        nook_core::runtime().spawn(async {
                            let _ = nook_core::audio::media_play_pause().await;
                        });
                    }),
                ),
        )
        .into_any_element()
}

fn artwork(b64: Option<&str>, size: f32, radius: f32, ring: bool) -> impl IntoElement {
    let image = b64.and_then(|raw| crate::island::media::artwork_element(raw, size, radius));
    div()
        .relative()
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(radius))
        .overflow_hidden()
        .bg(theme::FILL)
        .child(image.unwrap_or_else(|| {
            div()
                .size(px(size))
                .flex()
                .items_center()
                .justify_center()
                .child(lucide_color("music", 12.0, theme::secondary_label()))
                .into_any_element()
        }))
        .when(ring, |tile| {
            tile.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(radius))
                    .border_1()
                    .border_color(theme::FILL),
            )
        })
}

fn lyric_window(island: &Island) -> Option<[String; 3]> {
    let lyrics = island.lyrics.as_ref()?;
    if lyrics.instrumental {
        return None;
    }
    if lyrics.has_synced() {
        let [past, current, next] = lyrics.highlight_window(island.lyrics_position_ms());
        let lines = [
            past.unwrap_or("").to_string(),
            current.unwrap_or("").to_string(),
            next.unwrap_or("").to_string(),
        ];
        return lines
            .iter()
            .any(|line| !line.trim().is_empty())
            .then_some(lines);
    }
    let plain = lyrics.plain.as_deref()?;
    let collected: Vec<String> = plain
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .map(str::to_string)
        .collect();
    if collected.is_empty() {
        return None;
    }
    let mut lines = [String::new(), String::new(), String::new()];
    match collected.len() {
        1 => lines[1] = collected[0].clone(),
        2 => {
            lines[1] = collected[0].clone();
            lines[2] = collected[1].clone();
        }
        _ => {
            lines[0] = collected[0].clone();
            lines[1] = collected[1].clone();
            lines[2] = collected[2].clone();
        }
    }
    Some(lines)
}

fn current_line(island: &Island) -> String {
    let Some(lines) = lyric_window(island) else {
        return "No lyrics for this song".into();
    };
    if !lines[1].trim().is_empty() {
        return lines[1].clone();
    }
    lines
        .into_iter()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_else(|| "No lyrics for this song".into())
}
