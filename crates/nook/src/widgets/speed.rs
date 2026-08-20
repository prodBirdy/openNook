//! Cloudflare speed-test card.

use crate::island::ui::{label, text_btn, widget_shell};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, relative, Context};
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
        "Not tested".into()
    };
    widget_shell(
        "gauge",
        "Speed",
        div()
            .flex()
            .flex_col()
            .gap_2()
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
                if running { "Testing…" } else { "Run Test" },
                cx,
                |this, _, cx| {
                    cx.stop_propagation();
                    if this.speed_running {
                        return;
                    }
                    this.speed_running = true;
                    this.speed_progress = 0.0;
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
                        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                        let slot: std::sync::Arc<std::sync::Mutex<Option<Result<f64, String>>>> =
                            std::sync::Arc::new(std::sync::Mutex::new(None));
                        let p2 = progress.clone();
                        let done2 = done.clone();
                        let slot2 = slot.clone();
                        cx.background_executor()
                            .spawn(async move {
                                let result = nook_core::runtime().block_on(
                                    nook_core::widgets::run_speed_test(move |sample| {
                                        p2.store(
                                            sample.progress.to_bits(),
                                            std::sync::atomic::Ordering::Relaxed,
                                        );
                                    }),
                                );
                                if let Ok(mut guard) = slot2.lock() {
                                    *guard = Some(result);
                                }
                                done2.store(true, std::sync::atomic::Ordering::SeqCst);
                            })
                            .detach();
                        loop {
                            let running = this
                                .update(cx, |this, cx| {
                                    this.speed_progress = f64::from_bits(
                                        progress.load(std::sync::atomic::Ordering::Relaxed),
                                    );
                                    cx.notify();
                                    !done.load(std::sync::atomic::Ordering::SeqCst)
                                })
                                .unwrap_or(false);
                            if !running {
                                break;
                            }
                            cx.background_executor()
                                .timer(Duration::from_millis(100))
                                .await;
                        }
                        this.update(cx, |this, cx| {
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
                },
            )),
    )
}
