//! Cloudflare speed-test card.

use crate::island::ui::{label, text_btn, widget_shell};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, relative, Context};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) fn speed_card(
    mbps: Option<f64>,
    progress: f64,
    running: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let status = if running {
        format!("{:.0}%", progress)
    } else if let Some(v) = mbps {
        format!("{v:.1} Mbps")
    } else {
        "Tap to test.".into()
    };
    widget_shell(
        "speed-scroll",
        div()
            .id("speed-hit")
            .flex()
            .flex_col()
            .gap_2()
            .cursor(gpui::CursorStyle::PointingHand)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    if !this.speed_running {
                        this.begin_speed_test(cx);
                    }
                }),
            )
            .child(label(status, theme::TITLE_2, true))
            .when(running, |d| {
                d.child(
                    div()
                        .w_full()
                        .h(px(4.))
                        .rounded_full()
                        .bg(theme::FILL_SECONDARY)
                        .child(
                            div()
                                .h_full()
                                .w(relative((progress as f32 / 100.0).clamp(0.0, 1.0)))
                                .rounded_full()
                                .bg(theme::LABEL),
                        ),
                )
            })
            .child(text_btn(
                if running { "Stop" } else { "Run Test" },
                cx,
                |this, _, cx| {
                    cx.stop_propagation();
                    if this.speed_running {
                        this.stop_speed_test(cx);
                    } else {
                        this.begin_speed_test(cx);
                    }
                },
            )),
    )
}

impl Island {
    pub(crate) fn begin_speed_test(&mut self, cx: &mut Context<Self>) {
        if self.speed_running {
            return;
        }
        self.speed_gen = self.speed_gen.wrapping_add(1);
        let gen = self.speed_gen;
        self.speed_running = true;
        self.speed_progress = 0.0;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let progress = Arc::new(AtomicU64::new(0));
            let done = Arc::new(AtomicBool::new(false));
            let slot: Arc<Mutex<Option<Result<f64, String>>>> = Arc::new(Mutex::new(None));
            let p2 = progress.clone();
            let done2 = done.clone();
            let slot2 = slot.clone();
            cx.background_executor()
                .spawn(async move {
                    let result = nook_core::runtime().block_on(nook_core::widgets::run_speed_test(
                        move |sample| {
                            p2.store(sample.progress.to_bits(), Ordering::Relaxed);
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
                        this.speed_progress = f64::from_bits(progress.load(Ordering::Relaxed));
                        cx.notify();
                        !done.load(Ordering::SeqCst)
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }
            this.update(cx, |this, cx| {
                if this.speed_gen != gen {
                    return;
                }
                this.speed_running = false;
                this.speed_progress = 100.0;
                if let Ok(mut guard) = slot.lock() {
                    this.speed_mbps = guard.take().and_then(Result::ok);
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
        self.speed_gen = self.speed_gen.wrapping_add(1);
        self.speed_running = false;
        cx.notify();
    }
}
