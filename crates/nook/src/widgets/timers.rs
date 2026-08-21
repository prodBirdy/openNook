//! Compact timer ring + expanded timers card.

use crate::icons::lucide;
use crate::island::ui::{format_timer, icon_btn, label, text_btn, timer_text, widget_shell};
use crate::island::{Island, Timer};
use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, rgb, rgba, AnyElement, Context, MouseButton,
    MouseDownEvent, PathBuilder, SharedString,
};

pub(crate) fn compact_left(island: &Island, cx: &mut Context<Island>) -> AnyElement {
    let timer = island.running_timer().or_else(|| island.timers.first());
    let Some(timer) = timer else {
        return lucide("clock", theme::COMPACT_FACE).into_any_element();
    };
    let total = timer.total.max(1);
    let progress = 1.0 - timer.remaining as f32 / total as f32;
    let id = timer.id;
    div()
        .id("timer-ring")
        .size(px(theme::COMPACT_FACE))
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
        .child(timer_ring(progress.clamp(0.0, 1.0), theme::COMPACT_FACE))
        .into_any_element()
}

/// Original compact timer: 24px ring, progress from 12 o'clock.
pub(crate) fn timer_ring(progress: f32, size: f32) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let cx: f32 = bounds.center().x.into();
            let cy: f32 = bounds.center().y.into();
            let dim: f32 = bounds.size.width.into();
            let radius = dim * 0.36;
            let stroke = dim * 0.14;
            let p = |x: f32, y: f32| point(px(x), px(y));

            let mut circle = |color: gpui::Rgba, start: f32, end: f32| {
                let steps = ((end - start).abs() * 24.0).ceil().max(2.0) as i32;
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

            // SVG circles start at 3 o'clock; rotate so 0 is 12 o'clock.
            let start = -std::f32::consts::FRAC_PI_2;
            circle(rgba(0xffffff40), start, start + std::f32::consts::TAU);
            if progress > 0.01 {
                circle(
                    rgb(0xffffff),
                    start,
                    start + progress * std::f32::consts::TAU,
                );
            }
        },
    )
    .w(px(size))
    .h(px(size))
}

pub(crate) fn timer_card(timers: &[Timer], cx: &mut Context<Island>) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_2();
    if timers.is_empty() {
        body = body.child(label(
            "Start a 5, 15, or 25 minute timer.",
            theme::SUBHEADLINE,
            false,
        ));
    } else {
        for t in timers {
            let id = t.id;
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(timer_text(format_timer(t.remaining), theme::TITLE_2))
                    .child(icon_btn(
                        if t.running { "pause" } else { "play" },
                        SharedString::from(format!("ibtn-timer-{id}")),
                        cx,
                        move |this, _, cx| {
                            if let Some(timer) = this.timers.iter_mut().find(|x| x.id == id) {
                                timer.running = !timer.running;
                            }
                            cx.notify();
                        },
                    )),
            );
        }
    }
    body = body.child(
        div()
            .flex()
            .gap_1()
            .child(text_btn("5m", cx, |this, _, cx| {
                this.add_timer(300);
                cx.notify();
            }))
            .child(text_btn("15m", cx, |this, _, cx| {
                this.add_timer(900);
                cx.notify();
            }))
            .child(text_btn("25m", cx, |this, _, cx| {
                this.add_timer(1500);
                cx.notify();
            })),
    );
    widget_shell("timers-scroll", body)
}
