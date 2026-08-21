//! Compact timer ring + expanded timers card.
//! Layout matches the React `TimerWidget` / `CompactTimer`.

use crate::icons::{lucide, lucide_color};
use crate::island::ui::{
    empty_state, format_timer, header_icon_btn, pill_btn, text_btn, timer_text,
    widget_shell_actions,
};
use crate::island::{Island, Timer};
use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, rgb, rgba, AnyElement, Context, FontWeight, MouseButton,
    MouseDownEvent, PathBuilder, Rgba, SharedString,
};

const PRESETS: [(&str, u32); 4] = [("5m", 300), ("15m", 900), ("25m", 1500), ("1h", 3600)];
const COMPACT_RING: f32 = 24.0;
const EXPANDED_RING: f32 = 44.0;

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
                if let Some(t) = this.timers.iter_mut().find(|t| t.id == id) {
                    t.running = !t.running;
                }
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
                rgb(0xffffff)
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
    composer: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let add = header_icon_btn("plus", "timer-add", cx, |this, _, _, cx| {
        this.timer_composer = !this.timer_composer;
        cx.notify();
    });

    let body = if timers.is_empty() && !composer {
        empty_state(
            "No active timers",
            pill_btn("Create Timer", cx, |this, _, cx| {
                this.timer_composer = true;
                cx.notify();
            }),
        )
        .into_any_element()
    } else {
        let mut list = div().flex().flex_col().gap_1().flex_1().min_h(px(0.));
        if composer {
            list = list.child(preset_row(cx));
        }
        for t in timers {
            list = list.child(timer_row(t, cx));
        }
        list.into_any_element()
    };

    widget_shell_actions("timers-scroll", "Timers", add, body)
}

fn preset_row(cx: &mut Context<Island>) -> impl IntoElement {
    let mut row = div().flex().gap_2().flex_wrap().px(px(4.)).py(px(4.));
    for (label, seconds) in PRESETS {
        row = row.child(text_btn(label, cx, move |this, _, cx| {
            this.add_timer(seconds);
            this.timer_composer = false;
            cx.notify();
        }));
    }
    row
}

fn timer_row(timer: &Timer, cx: &mut Context<Island>) -> impl IntoElement {
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
        theme::accent()
    };
    let play_icon = if timer.running {
        "pause-fill"
    } else {
        "play-fill"
    };

    div()
        .id(SharedString::from(format!("timer-row-{id}")))
        .flex()
        .items_center()
        .gap_2()
        .px(px(16.))
        .py(px(12.))
        .rounded(px(theme::ROW_RADIUS))
        .when(done, |d| {
            d.bg(rgba(0xFF453A1A))
                .border_1()
                .border_color(rgba(0xff453a33))
        })
        .when(!done, |d| d.hover(|s| s.bg(rgba(0xFFFFFF0D))))
        .child(
            div()
                .id(SharedString::from(format!("timer-toggle-{id}")))
                .relative()
                .size(px(EXPANDED_RING))
                .flex_shrink_0()
                .cursor(gpui::CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        if let Some(t) = this.timers.iter_mut().find(|t| t.id == id) {
                            t.running = !t.running;
                        }
                        cx.notify();
                    }),
                )
                .child(timer_ring(
                    progress.clamp(0.0, 1.0),
                    EXPANDED_RING,
                    20.0,
                    3.0,
                    ring_color,
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
                            .when(!timer.running, |d| d.pl(px(1.)).opacity(0.5))
                            .child(lucide_color(play_icon, 16.0, rgb(0xffffff))),
                    )
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .justify_center()
                .child(
                    timer_text(format_timer(timer.remaining), theme::TITLE_2)
                        .text_size(px(26.))
                        .line_height(px(26.)),
                )
                .when(!timer.name.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgba(0xffffff66))
                            .mt(px(2.))
                            .child(timer.name.clone()),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(round_tool(
                    "rotate-ccw",
                    format!("timer-reset-{id}"),
                    false,
                    cx,
                    move |this, _, cx| {
                        this.reset_timer(id);
                        cx.notify();
                    },
                ))
                .child(round_tool(
                    "trash-2",
                    format!("timer-del-{id}"),
                    true,
                    cx,
                    move |this, _, cx| {
                        this.remove_timer(id);
                        cx.notify();
                    },
                )),
        )
}

fn round_tool(
    icon: &'static str,
    elem_id: String,
    destructive: bool,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(elem_id))
        .size(px(32.))
        .rounded_full()
        .bg(rgba(0xFFFFFF1A))
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| {
            if destructive {
                s.bg(rgba(0xff453a33)).text_color(theme::DESTRUCTIVE)
            } else {
                s.bg(rgba(0xffffff33))
            }
        })
        .cursor(gpui::CursorStyle::PointingHand)
        .child(lucide_color(icon, 16.0, theme::LABEL))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, event, cx);
            }),
        )
}
