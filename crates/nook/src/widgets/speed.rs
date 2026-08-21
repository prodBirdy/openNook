//! Cloudflare speed-test Nook pane.

use crate::icons::lucide_color;
use crate::island::ui::{nook_display, nook_pane};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, relative, rgba, Context, CursorStyle, FontWeight};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) fn speed_card(
    mbps: Option<f64>,
    progress: f64,
    running: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let (value, unit) = format_speed(mbps.unwrap_or(0.0));
    let t = (progress as f32 / 100.0).clamp(0.0, 1.0);
    let show_bar = running || progress > 0.0;

    nook_pane("nook-speed")
        .w_full()
        .pr(px(4.))
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(px(8.))
                        .child(nook_display(value))
                        .child(
                            div()
                                .pb(px(4.))
                                .text_size(px(11.))
                                .line_height(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::SECONDARY_LABEL)
                                .child(unit),
                        ),
                )
                .child(run_btn(running, cx)),
        )
        .child(
            div()
                .w_full()
                .h(px(3.))
                .flex_shrink_0()
                .rounded_full()
                .overflow_hidden()
                .bg(rgba(0xffffff26))
                .opacity(if show_bar { 1.0 } else { 0.45 })
                .child(
                    div()
                        .h_full()
                        .w(relative(t))
                        .rounded_full()
                        .bg(theme::accent()),
                ),
        )
}

fn format_speed(val: f64) -> (String, &'static str) {
    if val >= 1000.0 {
        (format!("{:.2}", val / 1000.0), "GBPS")
    } else if val > 0.0 && val < 1.0 {
        (format!("{:.0}", val * 1000.0), "KBPS")
    } else {
        (format!("{val:.1}"), "MBPS")
    }
}

fn run_btn(running: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("speed-run")
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .opacity(0.9)
        .hover(|s| s.opacity(1.0))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            if running { "pause" } else { "play" },
            16.0,
            if running {
                theme::DESTRUCTIVE
            } else {
                theme::LABEL
            },
        ))
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                if this.speed_running {
                    this.stop_speed_test(cx);
                } else {
                    this.begin_speed_test(cx);
                }
            }),
        )
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
                this.speed_progress = 100.0;
                if let Ok(mut guard) = slot.lock() {
                    if let Some(Ok(mbps)) = guard.take() {
                        this.speed_mbps = Some(mbps);
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
        self.speed_gen
    }

    pub(crate) fn cancel_speed_test(&mut self) {
        self.speed_gen = self.speed_gen.wrapping_add(1);
        self.speed_running = false;
        self.speed_progress = 0.0;
    }

    pub(crate) fn apply_speed_sample(&mut self, speed: f64, progress: f64) {
        self.speed_mbps = Some(speed);
        self.speed_progress = progress;
    }
}
