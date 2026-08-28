//! Compact timer ring + expanded timers Nook pane.

use crate::icons::{lucide, lucide_color};
use crate::island::ui::{format_timer, nook_display, nook_empty, nook_icon_btn, nook_pane};
use crate::island::{Island, Timer, TimerKind};
use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, rgb, rgba, AnyElement, Context, FontWeight, MouseButton,
    MouseDownEvent, PathBuilder, Rgba, SharedString,
};

const PRESETS: [(&str, &str, &str, u32); 4] = [
    ("5m", "M", "05", 300),
    ("15m", "M", "15", 900),
    ("25m", "M", "25", 1500),
    ("1h", "H", "01", 3600),
];
const COMPACT_RING: f32 = 24.0;
const FEATURED_RING: f32 = 56.0;

pub(crate) fn compact_left(island: &Island, cx: &mut Context<Island>) -> AnyElement {
    let timer = island.face_timer();
    let Some(timer) = timer else {
        return lucide("clock", theme::COMPACT_FACE).into_any_element();
    };
    let total = timer.total.max(1);
    let progress = 1.0 - timer.remaining as f32 / total as f32;
    let done = timer.remaining == 0;
    let id = timer.id;
    div()
        .id("timer-ring")
        .size(px(COMPACT_RING))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.toggle_timer(id);
                cx.notify();
            }),
        )
        .child(timer_ring(
            progress.clamp(0.0, 1.0),
            COMPACT_RING,
            10.0,
            4.0,
            if done {
                theme::DESTRUCTIVE
            } else {
                phase_color(timer)
            },
            rgba(0xffffff40),
        ))
        .into_any_element()
}

/// SVG-style ring: progress from 12 o'clock, round caps.
pub(crate) fn timer_ring(
    progress: f32,
    size: f32,
    radius: f32,
    stroke: f32,
    color: Rgba,
    track: Rgba,
) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let cx: f32 = bounds.center().x.into();
            let cy: f32 = bounds.center().y.into();
            let p = |x: f32, y: f32| point(px(x), px(y));

            let mut circle = |color: gpui::Rgba, start: f32, end: f32| {
                let steps = ((end - start).abs() * 32.0).ceil().max(2.0) as i32;
                let mut path = PathBuilder::stroke(px(stroke));
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let a = start + (end - start) * t;
                    let x = cx + radius * a.cos();
                    let y = cy + radius * a.sin();
                    if i == 0 {
                        path.move_to(p(x, y));
                    } else {
                        path.line_to(p(x, y));
                    }
                }
                if let Ok(built) = path.build() {
                    window.paint_path(built, color);
                }
            };

            let start = -std::f32::consts::FRAC_PI_2;
            circle(track, start, start + std::f32::consts::TAU);
            if progress > 0.01 {
                circle(color, start, start + progress * std::f32::consts::TAU);
            }
        },
    )
    .w(px(size))
    .h(px(size))
}

pub(crate) fn timer_card(
    timers: &[Timer],
    _composer: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let mut week = div().flex().items_end().gap(px(10.));
    for (id, unit, num, seconds) in PRESETS {
        week = week.child(preset_col(id, unit, num, seconds, cx));
    }
    week = week.child(pomodoro_col(cx));

    let remaining = timers.first().map(|t| format_timer(t.remaining));
    let body = if timers.is_empty() {
        nook_empty("clock", "No timers").into_any_element()
    } else {
        let mut col = div().flex().flex_col().flex_1().min_w(px(0.));
        if let Some(first) = timers.first() {
            col = col.child(featured_timer(first, cx));
        }
        col.into_any_element()
    };

    nook_pane("nook-timers")
        .w_full()
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(16.))
                .flex_shrink_0()
                .when_some(remaining, |d, text| d.child(nook_display(text)))
                .child(week),
        )
        .child(body)
}

fn preset_col(
    id: &'static str,
    unit: &'static str,
    num: &'static str,
    seconds: u32,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("timer-preset-{id}")))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.add_timer(seconds);
                this.timer_composer = false;
                cx.notify();
            }),
        )
        .child(
            div()
                .text_size(px(9.))
                .line_height(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::SECONDARY_LABEL)
                .child(unit),
        )
        .child(
            div()
                .text_size(px(15.))
                .line_height(px(18.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child(num),
        )
}

fn featured_timer(timer: &Timer, cx: &mut Context<Island>) -> impl IntoElement {
    let id = timer.id;
    let done = timer.remaining == 0;
    let progress = if timer.total > 0 {
        1.0 - timer.remaining as f32 / timer.total as f32
    } else {
        0.0
    };
    let ring_color = if done {
        theme::DESTRUCTIVE
    } else {
        phase_color(timer)
    };
    let play_icon = if timer.running {
        "pause-fill"
    } else {
        "play-fill"
    };

    div()
        .id(SharedString::from(format!("timer-featured-{id}")))
        .flex()
        .items_center()
        .gap(px(14.))
        .flex_1()
        .min_h(px(0.))
        .child(timer_face(
            id,
            progress,
            FEATURED_RING,
            24.0,
            3.5,
            ring_color,
            play_icon,
            done,
            timer.running,
            cx,
        ))
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
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::LABEL)
                        .child(if timer.name.is_empty() {
                            if done {
                                "Done"
                            } else if timer.running {
                                "Running"
                            } else {
                                "Paused"
                            }
                            .to_string()
                        } else {
                            timer.name.clone()
                        }),
                )
                .when_some(cycle_dots(timer), |d, dots| d.child(dots))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .mt(px(4.))
                        .child(nook_icon_btn(
                            "rotate-ccw",
                            format!("timer-reset-{id}"),
                            cx,
                            move |this, _, _, cx| {
                                this.reset_timer(id);
                                cx.notify();
                            },
                        ))
                        .child(nook_icon_btn(
                            "trash-2",
                            format!("timer-del-{id}"),
                            cx,
                            move |this, _, _, cx| {
                                this.remove_timer(id);
                                cx.notify();
                            },
                        )),
                ),
        )
}

fn timer_face(
    id: u64,
    progress: f32,
    size: f32,
    radius: f32,
    stroke: f32,
    color: Rgba,
    play_icon: &'static str,
    done: bool,
    running: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("timer-toggle-{id}-{size}")))
        .relative()
        .size(px(size))
        .flex_shrink_0()
        .cursor(gpui::CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.toggle_timer(id);
                cx.notify();
            }),
        )
        .child(timer_ring(
            progress.clamp(0.0, 1.0),
            size,
            radius,
            stroke,
            color,
            rgba(0xFFFFFF1A),
        ))
        .when(!done, |d| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(!running, |d| d.pl(px(1.)).opacity(0.5))
                    .child(lucide_color(play_icon, 16.0, rgb(0xffffff))),
            )
        })
}

fn pomodoro_col(cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("timer-preset-pomo")
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.add_pomodoro();
                cx.notify();
            }),
        )
        .child(
            div()
                .text_size(px(9.))
                .line_height(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::SECONDARY_LABEL)
                .child("P"),
        )
        .child(
            div()
                .text_size(px(15.))
                .line_height(px(18.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child("omo"),
        )
}

fn phase_color(timer: &Timer) -> Rgba {
    match timer.kind {
        TimerKind::Pomodoro(spec) if !spec.phase.is_work() => theme::SUCCESS,
        _ => theme::accent(),
    }
}

fn cycle_dots(timer: &Timer) -> Option<impl IntoElement> {
    let TimerKind::Pomodoro(spec) = timer.kind else {
        return None;
    };
    let filled = spec.filled_cycles();
    let mut row = div().flex().items_center().gap(px(4.)).mt(px(4.));
    for i in 1..=spec.cycles_per_long {
        row = row.child(
            div()
                .size(px(5.))
                .rounded_full()
                .bg(if i <= filled {
                    theme::LABEL
                } else {
                    theme::TERTIARY_LABEL
                }),
        );
    }
    Some(row)
}
