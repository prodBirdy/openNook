//! Compact timer ring + expanded timers Nook pane (island + Apple Clock).

use crate::icons::{lucide, lucide_color};
use crate::island::ui::{
    format_timer, label, nook_empty, nook_icon_btn, nook_pane, scroll_body, timer_text,
};
use crate::island::{ClockTimerAction, Island, Timer, TimerKind};
use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, AnyElement, Context, MouseButton, MouseDownEvent,
    PathBuilder, Rgba, SharedString,
};
use nook_core::system_timers::{self, MTTimerState, SystemTimer};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

const PRESETS: [(&str, &str, u32); 4] = [
    ("5m", "5 min", 300),
    ("15m", "15 min", 900),
    ("25m", "25 min", 1500),
    ("1h", "1 hour", 3600),
];

thread_local! {
    static PENDING_DELETE: Cell<Option<u64>> = const { Cell::new(None) };
    static PENDING_CLOCK_CANCEL: RefCell<Option<String>> = const { RefCell::new(None) };
    static HIGHLIGHT_AT: Cell<Option<Instant>> = const { Cell::new(None) };
}
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
                this.toggle_face_timer();
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
                theme::LABEL
            },
            theme::FILL_SECONDARY,
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

pub(crate) fn timer_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let timers = &island.timers;
    let clock: Vec<&SystemTimer> = if island.settings.sync_clock_timers {
        island
            .system_timers
            .iter()
            .filter(|t| t.state.is_active())
            .collect()
    } else {
        Vec::new()
    };

    let mut presets = div()
        .flex()
        .flex_wrap()
        .items_end()
        .gap(px(6.))
        .min_w(px(0.))
        .flex_1();
    for (id, caption, seconds) in PRESETS {
        presets = presets.child(preset_col(id, caption, seconds, cx));
    }
    presets = presets.child(pomodoro_col(cx));

    let remaining = island
        .face_timer()
        .map(|t| format_timer(t.remaining))
        .or_else(|| timers.first().map(|t| format_timer(t.remaining)));
    let empty = timers.is_empty() && clock.is_empty();
    let body = if empty {
        nook_empty("clock", "No timers").into_any_element()
    } else {
        let mut col = div().flex().flex_col().w_full().flex_shrink_0().gap(px(4.));
        if let Some(first) = timers.first() {
            col = col.child(featured_timer(first, cx));
        }
        if !clock.is_empty() {
            col = col.child(clock_section(&clock, cx));
        }
        scroll_body("timers-scroll", col).into_any_element()
    };

    nook_pane("nook-timers")
        .w_full()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(8.))
                .flex_shrink_0()
                .min_w(px(0.))
                .w_full()
                .when_some(remaining, |d, text| {
                    d.child(timer_text(text, theme::DISPLAY).flex_shrink_0())
                })
                .child(presets),
        )
        .child(body)
}

fn clock_section(timers: &[&SystemTimer], cx: &mut Context<Island>) -> impl IntoElement {
    let can_control = nook_core::shortcuts::cached_shortcuts().can_control();
    let now = system_timers::unix_now();
    let mut list = div().flex().flex_col().gap(px(4.));
    list = list.child(
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .px(px(5.))
                    .py(px(1.))
                    .rounded(px(4.))
                    .bg(theme::FILL)
                    .child(label("Clock", theme::FOOTNOTE, true).text_color(theme::accent())),
            )
            .child(label(
                if can_control {
                    "Shortcuts controls the most recent timer"
                } else {
                    "Open Clock to control"
                },
                theme::SUBHEADLINE,
                false,
            )),
    );
    let extra = timers.len().saturating_sub(3);
    for (index, timer) in timers.iter().take(3).enumerate() {
        list = list.child(clock_row(timer, now, can_control, index == 0, cx));
    }
    if extra > 0 {
        list = list.child(
            label(format!("+{extra} more"), theme::FOOTNOTE, false)
                .text_color(theme::tertiary_label()),
        );
    }
    list
}

fn clock_row(
    timer: &SystemTimer,
    now: f64,
    can_control: bool,
    is_first: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let id = timer.id.clone();
    let running = timer.state.is_running();
    let remaining = timer.remaining_secs(now);
    let done = timer.state == MTTimerState::Fired || remaining == 0;
    let title = if timer.title.is_empty() {
        match timer.state {
            MTTimerState::Running => "Running",
            MTTimerState::Paused => "Paused",
            MTTimerState::Fired => "Done",
            _ => "Timer",
        }
        .to_string()
    } else {
        timer.title.clone()
    };
    let play_icon = if running { "pause-fill" } else { "play-fill" };
    let row_controls = can_control && is_first;
    let pending_cancel = PENDING_CLOCK_CANCEL.with(|p| p.borrow().as_ref() == Some(&id));
    div()
        .id(SharedString::from(format!("clock-timer-{id}")))
        .flex()
        .items_center()
        .gap(px(8.))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, _: &MouseDownEvent, _, cx| {
                if PENDING_CLOCK_CANCEL.with(|p| p.borrow().is_some()) {
                    PENDING_CLOCK_CANCEL.with(|p| *p.borrow_mut() = None);
                    cx.notify();
                }
            }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(label(title, theme::BODY, true))
                .child(
                    timer_text(format_timer(remaining), theme::CALLOUT).text_color(if done {
                        theme::DESTRUCTIVE
                    } else {
                        theme::SECONDARY_LABEL
                    }),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(nook_icon_btn(
                    if row_controls { play_icon } else { "clock" },
                    format!("clock-toggle-{id}"),
                    cx,
                    {
                        let id = id.clone();
                        move |this, _, _, cx| {
                            if row_controls {
                                this.control_clock_timer(if running {
                                    ClockTimerAction::Pause(id.clone())
                                } else {
                                    ClockTimerAction::Resume(id.clone())
                                });
                            } else {
                                this.control_clock_timer(ClockTimerAction::Open(id.clone()));
                            }
                            cx.notify();
                        }
                    },
                ))
                .when(row_controls, |d| {
                    d.child(if pending_cancel {
                        div()
                            .id(SharedString::from(format!("clock-cancel-{id}")))
                            .h(px(theme::HIT_MIN))
                            .px_3()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(theme::CONTROL_RADIUS))
                            .bg(theme::FILL)
                            .hover(|s| s.bg(theme::FILL_SECONDARY))
                            .active(|s| s.opacity(0.85))
                            .cursor(gpui::CursorStyle::PointingHand)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener({
                                    let id = id.clone();
                                    move |this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        PENDING_CLOCK_CANCEL.with(|p| *p.borrow_mut() = None);
                                        this.control_clock_timer(ClockTimerAction::Cancel(
                                            id.clone(),
                                        ));
                                        cx.notify();
                                    }
                                }),
                            )
                            .child(
                                label("Cancel", theme::CALLOUT, true)
                                    .text_color(theme::DESTRUCTIVE),
                            )
                            .into_any_element()
                    } else {
                        nook_icon_btn("x", format!("clock-cancel-{id}"), cx, {
                            let id = id.clone();
                            move |_, _, _, cx| {
                                cx.stop_propagation();
                                PENDING_CLOCK_CANCEL.with(|p| *p.borrow_mut() = Some(id.clone()));
                                cx.notify();
                            }
                        })
                        .into_any_element()
                    })
                }),
        )
}

fn flash_new_timer(cx: &mut Context<Island>) {
    HIGHLIGHT_AT.set(Some(Instant::now()));
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(Duration::from_secs(1)).await;
        let _ = this.update(cx, |_, cx| cx.notify());
    })
    .detach();
}

fn preset_col(
    id: &'static str,
    caption: &'static str,
    seconds: u32,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("timer-preset-{id}")))
        .flex()
        .flex_col()
        .items_center()
        .justify_end()
        .min_h(px(theme::HIT_MIN))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.add_timer(seconds);
                flash_new_timer(cx);
                cx.notify();
            }),
        )
        .child(label(caption, theme::CALLOUT, true))
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
    let pending = PENDING_DELETE.get() == Some(id);
    let highlight = HIGHLIGHT_AT
        .get()
        .is_some_and(|at| at.elapsed() < Duration::from_secs(1));

    div()
        .id(SharedString::from(format!("timer-featured-{id}")))
        .flex()
        .items_center()
        .gap(px(14.))
        .flex_1()
        .min_h(px(0.))
        .rounded(px(theme::CONTROL_RADIUS))
        .when(highlight, |d| d.bg(theme::FILL))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, _: &MouseDownEvent, _, cx| {
                if PENDING_DELETE.get().is_some() {
                    PENDING_DELETE.set(None);
                    cx.notify();
                }
            }),
        )
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
                .child(label(
                    if timer.name.is_empty() {
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
                    },
                    theme::TITLE_3,
                    true,
                ))
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
                        .child(if pending {
                            div()
                                .id(SharedString::from(format!("timer-del-{id}")))
                                .h(px(theme::HIT_MIN))
                                .px_3()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(theme::CONTROL_RADIUS))
                                .bg(theme::FILL)
                                .hover(|s| s.bg(theme::FILL_SECONDARY))
                                .active(|s| s.opacity(0.85))
                                .cursor(gpui::CursorStyle::PointingHand)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        PENDING_DELETE.set(None);
                                        this.remove_timer(id);
                                        cx.notify();
                                    }),
                                )
                                .child(
                                    label("Delete", theme::CALLOUT, true)
                                        .text_color(theme::DESTRUCTIVE),
                                )
                                .into_any_element()
                        } else {
                            nook_icon_btn(
                                "trash-2",
                                format!("timer-del-{id}"),
                                cx,
                                move |_, _, _, cx| {
                                    cx.stop_propagation();
                                    PENDING_DELETE.set(Some(id));
                                    cx.notify();
                                },
                            )
                            .into_any_element()
                        }),
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
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.8))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.toggle_local_timer(id);
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
            theme::FILL,
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
                    .child(lucide_color(play_icon, 16.0, theme::LABEL)),
            )
        })
}

fn pomodoro_col(cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("timer-preset-pomo")
        .flex()
        .flex_col()
        .items_center()
        .justify_end()
        .min_h(px(theme::HIT_MIN))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.add_pomodoro();
                flash_new_timer(cx);
                cx.notify();
            }),
        )
        .child(label("Pomodoro", theme::CALLOUT, true))
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
        row = row.child(div().size(px(5.)).rounded_full().bg(if i <= filled {
            theme::LABEL
        } else {
            theme::TERTIARY_LABEL
        }));
    }
    Some(row)
}
