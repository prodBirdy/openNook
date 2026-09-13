//! Speed-test Nook pane: tap the ring gauge to run or stop.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_pane, timer_text};
use crate::island::Island;
use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, Context, CursorStyle, MouseButton, MouseDownEvent,
    PathBuilder,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

thread_local! {
    static SPEED_ERROR: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

const DIAL: f32 = 88.0;
const STROKE: f32 = 6.0;
/// Path radius so a 6px stroke sits inside the 88px face.
const RADIUS: f32 = (DIAL - STROKE) / 2.0;
const ARC_STEPS: i32 = 64;

pub(crate) fn speed_card(
    mbps: Option<f64>,
    progress: f64,
    running: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let error = SPEED_ERROR.with(|e| {
        let mut slot = e.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= Duration::from_secs(6))
        {
            *slot = None;
        }
        slot.as_ref().map(|(msg, _)| msg.clone())
    });
    let idle = !running && mbps.is_none() && error.is_none();
    nook_pane("nook-speed").w_full().child(
        div()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(dial(mbps, progress, running, cx))
            .when(idle, |d| {
                d.child(
                    label("Tap to test", theme::SUBHEADLINE, false)
                        .text_color(theme::TERTIARY_LABEL),
                )
            })
            .when_some(error, |d, msg| {
                d.child(label(msg, theme::FOOTNOTE, false).text_color(theme::DESTRUCTIVE))
            }),
    )
}

fn dial(
    mbps: Option<f64>,
    progress: f64,
    running: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let idle = !running && mbps.is_none();
    // Time-based samples can end mid-arc on a fast link; a finished result
    // always owns the full ring.
    let fill = if running {
        (progress as f32 / 100.0).clamp(0.0, 1.0)
    } else if mbps.is_some() {
        1.0
    } else {
        0.0
    };

    div()
        .id("speed-run")
        .relative()
        .size(px(DIAL))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.8))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if this.speed_running {
                    this.stop_speed_test(cx);
                } else {
                    this.begin_speed_test(cx);
                }
            }),
        )
        .child(speed_ring(fill))
        .when(idle, |d| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(lucide_color("play", 18.0, theme::LABEL)),
            )
        })
        .when(!idle, |d| d.child(readout(mbps.unwrap_or(0.0), running)))
}

/// Control Center–style ring: progress from 12 o'clock, round caps.
fn speed_ring(progress: f32) -> impl IntoElement {
    let accent = theme::accent();
    let track = theme::FILL_SECONDARY;
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let cx: f32 = bounds.center().x.into();
            let cy: f32 = bounds.center().y.into();
            let p = |x: f32, y: f32| point(px(x), px(y));

            let mut arc = |color: gpui::Rgba, start: f32, end: f32| {
                let span = (end - start).abs();
                if span < 0.001 {
                    return;
                }
                let steps = ((span / std::f32::consts::TAU) * ARC_STEPS as f32)
                    .ceil()
                    .max(2.0) as i32;
                let mut path = PathBuilder::stroke(px(STROKE));
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let a = start + (end - start) * t;
                    let x = cx + RADIUS * a.cos();
                    let y = cy + RADIUS * a.sin();
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
            arc(track, start, start + std::f32::consts::TAU);
            if progress > 0.001 {
                arc(accent, start, start + progress * std::f32::consts::TAU);
            }
        },
    )
    .size(px(DIAL))
    .flex_shrink_0()
}

fn readout(mbps: f64, running: bool) -> impl IntoElement {
    let (value, unit) = format_speed(mbps);
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(timer_text(value, theme::TITLE_2))
        .child(label(unit, theme::FOOTNOTE, true).text_color(if running {
            theme::TERTIARY_LABEL
        } else {
            theme::SECONDARY_LABEL
        }))
        .when(running, |d| {
            d.child(label("Testing…", theme::FOOTNOTE, false).text_color(theme::SECONDARY_LABEL))
        })
}

fn short_speed_error(err: &str) -> String {
    let lower = err.to_lowercase();
    if lower.contains("connect")
        || lower.contains("network")
        || lower.contains("dns")
        || lower.contains("offline")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("unreachable")
    {
        "No connection".into()
    } else {
        "Test failed".into()
    }
}

pub(crate) fn format_speed(val: f64) -> (String, &'static str) {
    if val >= 1000.0 {
        (format!("{:.2}", val / 1000.0), "Gbps")
    } else if val > 0.0 && val < 1.0 {
        (format!("{:.0}", val * 1000.0), "Kbps")
    } else {
        (format!("{val:.1}"), "Mbps")
    }
}

impl Island {
    pub(crate) fn begin_speed_test(&mut self, cx: &mut Context<Self>) {
        if self.speed_running {
            return;
        }
        let gen = self.arm_speed_test();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let progress = Arc::new(AtomicU64::new(0));
            let speed = Arc::new(AtomicU64::new(0));
            let done = Arc::new(AtomicBool::new(false));
            let slot: Arc<Mutex<Option<Result<f64, String>>>> = Arc::new(Mutex::new(None));
            let p2 = progress.clone();
            let s2 = speed.clone();
            let done2 = done.clone();
            let slot2 = slot.clone();
            cx.background_executor()
                .spawn(async move {
                    let result = nook_core::runtime().block_on(nook_core::widgets::run_speed_test(
                        move |sample| {
                            p2.store(sample.progress.to_bits(), Ordering::Relaxed);
                            s2.store(sample.speed.to_bits(), Ordering::Relaxed);
                        },
                    ));
                    if let Ok(mut guard) = slot2.lock() {
                        *guard = Some(result);
                    }
                    done2.store(true, Ordering::SeqCst);
                })
                .detach();
            loop {
                let keep_going = this
                    .update(cx, |this, cx| {
                        if this.speed_gen != gen {
                            return false;
                        }
                        this.apply_speed_sample(
                            f64::from_bits(speed.load(Ordering::Relaxed)),
                            f64::from_bits(progress.load(Ordering::Relaxed)),
                        );
                        cx.notify();
                        !done.load(Ordering::SeqCst)
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
            }
            this.update(cx, |this, cx| {
                if this.speed_gen != gen {
                    return;
                }
                this.speed_running = false;
                if let Ok(mut guard) = slot.lock() {
                    match guard.take() {
                        Some(Ok(mbps)) => {
                            this.speed_progress = 100.0;
                            this.speed_mbps = Some(mbps);
                            SPEED_ERROR.with(|e| *e.borrow_mut() = None);
                        }
                        Some(Err(err)) => {
                            this.speed_progress = 0.0;
                            this.speed_mbps = None;
                            SPEED_ERROR.with(|e| {
                                *e.borrow_mut() = Some((short_speed_error(&err), Instant::now()));
                            });
                            cx.spawn(async move |this, cx| {
                                cx.background_executor().timer(Duration::from_secs(6)).await;
                                let _ = this.update(cx, |_, cx| {
                                    SPEED_ERROR.with(|e| {
                                        if e.borrow().as_ref().is_some_and(|(_, at)| {
                                            at.elapsed() >= Duration::from_secs(6)
                                        }) {
                                            *e.borrow_mut() = None;
                                        }
                                    });
                                    cx.notify();
                                });
                            })
                            .detach();
                        }
                        None => {
                            this.speed_progress = 100.0;
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn stop_speed_test(&mut self, cx: &mut Context<Self>) {
        if !self.speed_running {
            return;
        }
        self.cancel_speed_test();
        cx.notify();
    }

    /// Zero the readout so Mbps streams from 0 instead of holding the last result.
    pub(crate) fn arm_speed_test(&mut self) -> u64 {
        self.speed_gen = self.speed_gen.wrapping_add(1);
        self.speed_running = true;
        self.speed_progress = 0.0;
        self.speed_mbps = Some(0.0);
        SPEED_ERROR.with(|e| *e.borrow_mut() = None);
        self.speed_gen
    }

    pub(crate) fn cancel_speed_test(&mut self) {
        self.speed_gen = self.speed_gen.wrapping_add(1);
        self.speed_running = false;
        self.speed_progress = 0.0;
        self.speed_mbps = None;
    }

    pub(crate) fn apply_speed_sample(&mut self, speed: f64, progress: f64) {
        self.speed_mbps = Some(speed);
        self.speed_progress = progress;
    }
}

#[cfg(test)]
mod tests {
    use super::format_speed;

    #[test]
    fn format_speed_uses_conventional_unit_names() {
        assert_eq!(format_speed(0.0), ("0.0".into(), "Mbps"));
        assert_eq!(format_speed(0.4), ("400".into(), "Kbps"));
        assert_eq!(format_speed(14.2), ("14.2".into(), "Mbps"));
        assert_eq!(format_speed(1500.0), ("1.50".into(), "Gbps"));
    }
}
