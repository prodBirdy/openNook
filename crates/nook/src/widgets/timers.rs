//! Compact timer ring + expanded timers Nook pane (island + Apple Clock).

use crate::icons::lucide_color;
use crate::island::ui::{format_timer, label, nook_empty, nook_pane, scroll_body, timer_text};
use crate::island::{ClockTimerAction, FaceTimer, FaceTimerSource, Island, Timer, TimerKind};
use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, AnyElement, Context, FontWeight, MouseButton,
    MouseDownEvent, PathBuilder, Rgba, SharedString,
};
use nook_core::pomodoro::PomodoroPhase;
use nook_core::system_timers::{self, MTTimerState, SystemTimer};
use std::cell::{Cell, RefCell};

const PRESETS: [(&str, &str, u32); 4] = [
    ("5m", "5m", 300),
    ("15m", "15m", 900),
    ("25m", "25m", 1500),
    ("1h", "1h", 3600),
];

thread_local! {
    static PENDING_DELETE: Cell<Option<u64>> = const { Cell::new(None) };
    static PENDING_CLOCK_CANCEL: RefCell<Option<String>> = const { RefCell::new(None) };
    static SHOW_PRESETS: Cell<bool> = const { Cell::new(false) };
}

/// Mockup: 16px ring centered in a 22px leading box.
/// Clip-path donut: outer r=8, inner r=5.76 → stroke 2.24, flush to the 16px box.
const FACE_BOX: f32 = 22.0;
const FACE_RING: f32 = 16.0;
const FACE_STROKE: f32 = 2.24;
/// Ring track #FFFFFF29.
const RING_TRACK: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0x29 as f32 / 255.0,
};
/// Add Timer fill #FFFFFF14.
const ADD_FILL: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0x14 as f32 / 255.0,
};

/// Expanded remaining readout — mockup 26/30 semibold.
const REMAINING: theme::Text = theme::Text {
    size: 26.0,
    leading: 30.0,
    weight: FontWeight::SEMIBOLD,
    emphasized: FontWeight::SEMIBOLD,
};

pub(crate) fn compact_left(island: &Island, cx: &mut Context<Island>) -> AnyElement {
    let timer = island.face_timer();
    let Some(timer) = timer else {
        return div()
            .size(px(FACE_BOX))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(lucide_color("clock", 17.0, theme::secondary_label()))
            .into_any_element();
    };
    let total = timer.total.max(1);
    let progress = 1.0 - timer.remaining as f32 / total as f32;
    let done = timer.remaining == 0;
    div()
        .id("timer-ring")
        .size(px(FACE_BOX))
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
            FACE_RING,
            FACE_STROKE,
            if done {
                theme::DESTRUCTIVE
            } else {
                theme::LABEL
            },
            RING_TRACK,
        ))
        .into_any_element()
}

/// Compact trailing: `24:59` (BODY semibold, tabular). Mockup uses mm:ss,
/// not the shorter `24m59` compact HUD form.
#[allow(dead_code)]
pub(crate) fn compact_right(island: &Island) -> AnyElement {
    let text = island
        .face_timer()
        .map(|t| format_timer(t.remaining))
        .unwrap_or_else(|| "0:00".into());
    timer_text(text, theme::BODY)
        .min_w(px(40.))
        .text_right()
        .into_any_element()
}

/// Progress ring from 12 o'clock. Stroke is centered so the outer edge
/// lands on `size` (mockup donut flush to the 16px box).
pub(crate) fn timer_ring(
    progress: f32,
    size: f32,
    stroke: f32,
    color: Rgba,
    track: Rgba,
) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let cx: f32 = bounds.center().x.into();
            let cy: f32 = bounds.center().y.into();
            let radius = (size - stroke) * 0.5;
            if radius < 1.0 {
                return;
            }
            let p = |x: f32, y: f32| point(px(x), px(y));

            let mut circle = |color: gpui::Rgba, start: f32, end: f32| {
                let sweep = end - start;
                let steps = (sweep.abs() * 48.0).ceil().max(2.0) as i32;
                let mut path = PathBuilder::stroke(px(stroke));
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let a = start + sweep * t;
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

    let empty = timers.is_empty() && clock.is_empty();
    let show_presets = SHOW_PRESETS.get();
    let face = island.face_timer();

    let body = if empty {
        nook_empty("clock", "No timers").into_any_element()
    } else if let Some(face) = face {
        let caption = face_caption(island, &face);
        let done = face.remaining == 0;
        let skip_local = match &face.source {
            FaceTimerSource::Local(id) => Some(*id),
            _ => None,
        };
        let skip_clock = match &face.source {
            FaceTimerSource::Clock(id) => Some(id.clone()),
            _ => None,
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .gap(px(8.))
            .justify_center()
            .child(
                div()
                    .id("timer-face-toggle")
                    .cursor(gpui::CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            clear_pending(cx);
                            this.toggle_face_timer();
                            cx.notify();
                        }),
                    )
                    .child(
                        timer_text(format_timer(face.remaining), REMAINING).text_color(if done {
                            theme::DESTRUCTIVE
                        } else {
                            theme::TEXT
                        }),
                    ),
            )
            .child(label(caption, theme::FOOTNOTE, false).text_color(theme::tertiary_label()))
            .when(timers.len() + clock.len() > 1, |d| {
                let mut col = div().flex().flex_col().w_full().gap(px(6.)).pt(px(4.));
                for timer in timers.iter().rev() {
                    if skip_local == Some(timer.id) {
                        continue;
                    }
                    col = col.child(local_row_compact(timer, cx));
                }
                for timer in &clock {
                    if skip_clock.as_deref() == Some(timer.id.as_str()) {
                        continue;
                    }
                    col = col.child(clock_row_compact(timer, cx));
                }
                d.child(scroll_body("timers-extra", col))
            })
            .into_any_element()
    } else {
        let mut col = div().flex().flex_col().w_full().flex_shrink_0().gap(px(8.));
        for timer in timers.iter().rev() {
            col = col.child(local_row(timer, cx));
        }
        if !clock.is_empty() {
            col = col.child(clock_section(&clock, cx));
        }
        scroll_body("timers-scroll", col).into_any_element()
    };

    nook_pane("nook-timers")
        .w_full()
        .p(px(16.))
        .gap(px(10.))
        .child(body)
        .child(new_timer_btn(cx))
        .when(show_presets, |d| d.child(preset_grid(cx)))
}

fn new_timer_btn(cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("timer-new")
        .w_full()
        .h(px(28.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .rounded(px(theme::CONTROL_RADIUS))
        .bg(ADD_FILL)
        .hover(|s| s.bg(theme::FILL))
        .active(|s| s.opacity(0.85))
        .cursor(gpui::CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                SHOW_PRESETS.set(!SHOW_PRESETS.get());
                cx.notify();
            }),
        )
        .child(lucide_color("plus", 13.0, theme::secondary_label()))
        .child(label("New timer", theme::SUBHEADLINE, true).text_color(theme::secondary_label()))
}

fn preset_grid(cx: &mut Context<Island>) -> impl IntoElement {
    let mut top = div().flex().gap(px(4.)).w_full();
    for (id, caption, seconds) in PRESETS.into_iter().take(3) {
        top = top.child(preset_chip(id, caption, seconds, cx));
    }
    let (hour_id, hour_caption, hour_secs) = PRESETS[3];
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .w_full()
        .flex_shrink_0()
        .child(top)
        .child(
            div()
                .flex()
                .gap(px(4.))
                .w_full()
                .child(preset_chip(hour_id, hour_caption, hour_secs, cx))
                .child(pomodoro_chip(cx)),
        )
}

fn preset_chip(
    id: &'static str,
    caption: &'static str,
    seconds: u32,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    chip(
        format!("timer-preset-{id}"),
        caption,
        cx,
        move |this, cx| {
            clear_pending(cx);
            SHOW_PRESETS.set(false);
            this.add_timer(seconds);
            cx.notify();
        },
    )
}

fn pomodoro_chip(cx: &mut Context<Island>) -> impl IntoElement {
    chip("timer-preset-pomo", "Pomo", cx, |this, cx| {
        clear_pending(cx);
        SHOW_PRESETS.set(false);
        this.add_pomodoro();
        cx.notify();
    })
}

fn chip(
    id: impl Into<SharedString>,
    caption: &'static str,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(id.into())
        .flex_1()
        .min_w(px(0.))
        .h(px(theme::HIT_MIN))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::CONTROL_RADIUS))
        .hover(|s| s.bg(theme::FILL))
        .active(|s| s.opacity(0.7))
        .cursor(gpui::CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, cx);
            }),
        )
        .child(label(caption, theme::FOOTNOTE, true))
}

fn local_row_compact(timer: &Timer, cx: &mut Context<Island>) -> impl IntoElement {
    let id = timer.id;
    let done = timer.remaining == 0;
    div()
        .id(SharedString::from(format!("timer-mini-{id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .cursor(gpui::CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                clear_pending(cx);
                this.toggle_timer(id);
                cx.notify();
            }),
        )
        .child(
            timer_text(format_timer(timer.remaining), theme::BODY).text_color(if done {
                theme::DESTRUCTIVE
            } else {
                theme::TEXT
            }),
        )
        .child(
            label(local_caption(timer), theme::FOOTNOTE, false).text_color(theme::tertiary_label()),
        )
}

fn clock_row_compact(timer: &SystemTimer, cx: &mut Context<Island>) -> impl IntoElement {
    let id = timer.id.clone();
    let now = system_timers::unix_now();
    let remaining = timer.remaining_secs(now);
    let done = timer.state == MTTimerState::Fired || remaining == 0;
    let running = timer.state.is_running();
    let can = nook_core::shortcuts::cached_shortcuts().can_control();
    div()
        .id(SharedString::from(format!("clock-mini-{id}")))
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .when(can, |d| {
            d.cursor(gpui::CursorStyle::PointingHand).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    clear_pending(cx);
                    this.control_clock_timer(if running {
                        ClockTimerAction::Pause(id.clone())
                    } else {
                        ClockTimerAction::Resume(id.clone())
                    });
                    cx.notify();
                }),
            )
        })
        .child(
            timer_text(format_timer(remaining), theme::BODY).text_color(if done {
                theme::DESTRUCTIVE
            } else {
                theme::TEXT
            }),
        )
        .child(
            label(clock_caption(timer, done), theme::FOOTNOTE, false)
                .text_color(theme::tertiary_label()),
        )
}

fn local_row(timer: &Timer, cx: &mut Context<Island>) -> impl IntoElement {
    let id = timer.id;
    let done = timer.remaining == 0;
    let progress = if timer.total > 0 {
        1.0 - timer.remaining as f32 / timer.total as f32
    } else {
        0.0
    };
    let pending = PENDING_DELETE.get() == Some(id);
    div()
        .id(SharedString::from(format!("timer-row-{id}")))
        .flex()
        .flex_col()
        .gap(px(2.))
        .w_full()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                if pending_active() {
                    clear_pending(cx);
                }
            }),
        )
        .child(
            timer_text(format_timer(timer.remaining), REMAINING).text_color(if done {
                theme::DESTRUCTIVE
            } else {
                theme::TEXT
            }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .min_w(px(0.))
                .child(
                    label(local_caption(timer), theme::FOOTNOTE, false)
                        .text_color(theme::tertiary_label()),
                )
                .when_some(cycle_dots(timer), |row, dots| row.child(dots)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .pt(px(2.))
                .child(timer_face(
                    id,
                    progress.clamp(0.0, 1.0),
                    if done {
                        theme::DESTRUCTIVE
                    } else {
                        phase_color(timer)
                    },
                    if timer.running {
                        "pause-fill"
                    } else {
                        "play-fill"
                    },
                    done,
                    cx,
                ))
                .child(icon_btn(
                    "rotate-ccw",
                    format!("timer-reset-{id}"),
                    theme::LABEL,
                    cx,
                    move |this, cx| {
                        clear_pending(cx);
                        this.reset_timer(id);
                        cx.notify();
                    },
                ))
                .child(icon_btn(
                    "trash-2",
                    format!("timer-del-{id}"),
                    if pending {
                        theme::DESTRUCTIVE
                    } else {
                        theme::LABEL
                    },
                    cx,
                    move |this, cx| {
                        if PENDING_DELETE.get() == Some(id) {
                            PENDING_DELETE.set(None);
                            this.remove_timer(id);
                        } else {
                            PENDING_DELETE.set(Some(id));
                            PENDING_CLOCK_CANCEL.with(|p| *p.borrow_mut() = None);
                        }
                        cx.notify();
                    },
                )),
        )
}

fn face_caption(island: &Island, face: &FaceTimer) -> String {
    match &face.source {
        FaceTimerSource::Local(id) => island
            .timers
            .iter()
            .find(|t| t.id == *id)
            .map(local_caption)
            .unwrap_or_else(|| clock_face_caption(face)),
        FaceTimerSource::Clock(_) => clock_face_caption(face),
    }
}

fn clock_face_caption(face: &FaceTimer) -> String {
    if !face.name.is_empty() {
        return face.name.clone();
    }
    if face.remaining == 0 {
        "Done".to_string()
    } else if face.running {
        "Running".to_string()
    } else {
        "Paused".to_string()
    }
}

fn local_caption(timer: &Timer) -> String {
    if timer.remaining == 0 {
        return "Done".to_string();
    }
    match timer.kind {
        TimerKind::Pomodoro(spec) => {
            let phase = match spec.phase {
                PomodoroPhase::Work => "Focus",
                PomodoroPhase::ShortBreak => "Break",
                PomodoroPhase::LongBreak => "Long break",
            };
            format!("{phase} · pomodoro")
        }
        TimerKind::Countdown if !timer.name.is_empty() => timer.name.clone(),
        TimerKind::Countdown if !timer.running => "Paused".to_string(),
        TimerKind::Countdown => "Running".to_string(),
    }
}

fn timer_face(
    id: u64,
    progress: f32,
    color: Rgba,
    play_icon: &'static str,
    done: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("timer-toggle-{id}")))
        .relative()
        .size(px(theme::HIT_MIN))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(theme::FILL))
        .active(|s| s.opacity(0.8))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                clear_pending(cx);
                this.toggle_timer(id);
                cx.notify();
            }),
        )
        .child(timer_ring(
            progress,
            FACE_RING,
            FACE_STROKE,
            color,
            theme::FILL_SECONDARY,
        ))
        .when(!done, |d| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    // Play is a right-pointing triangle; a 1px nudge centers it.
                    .when(play_icon == "play-fill", |d| d.pl(px(1.)))
                    .child(lucide_color(play_icon, 12.0, theme::LABEL)),
            )
        })
}

fn clock_section(timers: &[&SystemTimer], cx: &mut Context<Island>) -> impl IntoElement {
    let can_control = nook_core::shortcuts::cached_shortcuts().can_control();
    let now = system_timers::unix_now();
    let mut list = div().flex().flex_col().gap(px(8.));
    list = list.child(label("Clock", theme::FOOTNOTE, true).text_color(theme::tertiary_label()));
    for (index, timer) in timers.iter().enumerate() {
        list = list.child(clock_row(timer, now, can_control && index == 0, cx));
    }
    list
}

fn clock_row(
    timer: &SystemTimer,
    now: f64,
    can_control: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let id = timer.id.clone();
    let running = timer.state.is_running();
    let remaining = timer.remaining_secs(now);
    let total = timer.total_secs().max(1);
    let progress = 1.0 - remaining as f32 / total as f32;
    let done = timer.state == MTTimerState::Fired || remaining == 0;
    let pending = PENDING_CLOCK_CANCEL.with(|p| p.borrow().as_ref() == Some(&id));
    div()
        .id(SharedString::from(format!("clock-timer-{id}")))
        .flex()
        .flex_col()
        .gap(px(2.))
        .w_full()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, _: &MouseDownEvent, _, cx| {
                if pending_active() {
                    clear_pending(cx);
                }
            }),
        )
        .child(
            timer_text(format_timer(remaining), REMAINING).text_color(if done {
                theme::DESTRUCTIVE
            } else {
                theme::TEXT
            }),
        )
        .child(
            label(clock_caption(timer, done), theme::FOOTNOTE, false)
                .text_color(theme::tertiary_label()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .pt(px(2.))
                .child(clock_toggle(
                    &id,
                    progress.clamp(0.0, 1.0),
                    done,
                    running,
                    can_control,
                    cx,
                ))
                .when(!can_control, |d| {
                    let id = id.clone();
                    d.child(icon_btn(
                        "clock",
                        format!("clock-open-{id}"),
                        theme::LABEL,
                        cx,
                        move |this, cx| {
                            clear_pending(cx);
                            this.control_clock_timer(ClockTimerAction::Open(id.clone()));
                            cx.notify();
                        },
                    ))
                })
                .when(can_control, |d| {
                    let id = id.clone();
                    d.child(icon_btn(
                        "x",
                        format!("clock-cancel-{id}"),
                        if pending {
                            theme::DESTRUCTIVE
                        } else {
                            theme::LABEL
                        },
                        cx,
                        move |this, cx| {
                            let armed =
                                PENDING_CLOCK_CANCEL.with(|p| p.borrow().as_ref() == Some(&id));
                            if armed {
                                PENDING_CLOCK_CANCEL.with(|p| *p.borrow_mut() = None);
                                this.control_clock_timer(ClockTimerAction::Cancel(id.clone()));
                            } else {
                                PENDING_DELETE.set(None);
                                PENDING_CLOCK_CANCEL.with(|p| *p.borrow_mut() = Some(id.clone()));
                            }
                            cx.notify();
                        },
                    ))
                }),
        )
}

fn clock_toggle(
    id: &str,
    progress: f32,
    done: bool,
    running: bool,
    can_control: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let play_icon = if running { "pause-fill" } else { "play-fill" };
    let ident = id.to_string();
    div()
        .id(SharedString::from(format!("clock-toggle-{id}")))
        .relative()
        .size(px(theme::HIT_MIN))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(can_control, |d| {
            d.cursor(gpui::CursorStyle::PointingHand)
                .hover(|s| s.opacity(0.92))
                .active(|s| s.opacity(0.8))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        clear_pending(cx);
                        this.control_clock_timer(if running {
                            ClockTimerAction::Pause(ident.clone())
                        } else {
                            ClockTimerAction::Resume(ident.clone())
                        });
                        cx.notify();
                    }),
                )
        })
        .child(timer_ring(
            progress,
            FACE_RING,
            FACE_STROKE,
            if done {
                theme::DESTRUCTIVE
            } else {
                theme::accent()
            },
            theme::FILL_SECONDARY,
        ))
        .when(can_control && !done, |d| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(play_icon == "play-fill", |d| d.pl(px(1.)))
                    .child(lucide_color(play_icon, 12.0, theme::LABEL)),
            )
        })
}

fn clock_caption(timer: &SystemTimer, done: bool) -> String {
    if !timer.title.is_empty() {
        return timer.title.clone();
    }
    if done {
        "Done".to_string()
    } else if timer.state.is_running() {
        "Running".to_string()
    } else if timer.state == MTTimerState::Paused {
        "Paused".to_string()
    } else {
        "Timer".to_string()
    }
}

fn icon_btn(
    name: &'static str,
    id: impl Into<SharedString>,
    color: Rgba,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(id.into())
        .size(px(theme::HIT_MIN))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .hover(|s| s.bg(theme::FILL))
        .active(|s| s.opacity(0.75))
        .cursor(gpui::CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, cx);
            }),
        )
        .child(lucide_color(name, theme::GLYPH_SM, color))
}

fn pending_active() -> bool {
    PENDING_DELETE.get().is_some() || PENDING_CLOCK_CANCEL.with(|p| p.borrow().is_some())
}

fn clear_pending(cx: &mut Context<Island>) {
    let dirty = pending_active();
    PENDING_DELETE.set(None);
    PENDING_CLOCK_CANCEL.with(|p| *p.borrow_mut() = None);
    if dirty {
        cx.notify();
    }
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
    let mut row = div().flex().items_center().gap(px(3.)).flex_shrink_0();
    for i in 1..=spec.cycles_per_long {
        row = row.child(div().size(px(4.)).rounded_full().bg(if i <= filled {
            theme::LABEL
        } else {
            theme::TERTIARY_LABEL
        }));
    }
    Some(row)
}

#[cfg(test)]
mod tests {
    use super::{local_caption, FACE_BOX, FACE_RING, FACE_STROKE};
    use crate::island::{Timer, TimerKind};
    use nook_core::pomodoro::{PomodoroPhase, PomodoroSpec};

    #[test]
    fn compact_ring_sits_inside_the_leading_box() {
        assert_eq!(FACE_BOX, 22.0);
        assert_eq!(FACE_RING, 16.0);
        assert_eq!(FACE_STROKE, 2.24);
        // Centerline radius = (size - stroke) / 2; outer edge is size/2 = 8.
        assert!((FACE_RING - FACE_STROKE) * 0.5 > 1.0);
        assert!(FACE_RING <= FACE_BOX);
    }

    #[test]
    fn pomodoro_caption_matches_the_mockup() {
        let spec = PomodoroSpec {
            phase: PomodoroPhase::Work,
            cycle: 1,
            work_secs: 1500,
            break_secs: 300,
            long_break_secs: 900,
            cycles_per_long: 4,
            auto_advance: true,
        };
        let timer = Timer {
            id: 1,
            name: spec.label().to_string(),
            remaining: 1499,
            total: 1500,
            running: true,
            kind: TimerKind::Pomodoro(spec),
            ends_at: None,
        };
        assert_eq!(local_caption(&timer), "Focus · pomodoro");
    }
}
