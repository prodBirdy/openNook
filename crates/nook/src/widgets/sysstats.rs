//! System Stats Nook pane: CPU, memory, network, disk capacity.
//!
//! Sampling is spawned only while the expanded card is on screen and stops
//! on collapse — zero idle syscalls.

use crate::island::ui::{label, nook_empty, nook_pane};
use crate::island::{Island, Tab};
use crate::theme;
use gpui::{div, prelude::*, px, relative, rgba, Context, FontWeight};
use nook_core::sysstats;
use std::time::Duration;

const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) fn sysstats_card(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    island.ensure_sysstats(cx);
    let snap = &island.sysstats;
    let cfg = &island.settings.sysstats;
    let mut rows = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .justify_center()
        .gap(px(6.));
    let mut any = false;
    if cfg.show_cpu {
        any = true;
        let value = snap
            .cpu_pct
            .map(sysstats::format_pct)
            .unwrap_or_else(|| "—".into());
        let t = snap.cpu_pct.unwrap_or(0.0) / 100.0;
        rows = rows.child(stat_row("CPU", value, t));
        if !snap.per_core.is_empty() {
            rows = rows.child(core_strip(&snap.per_core));
        }
    }
    if cfg.show_mem {
        any = true;
        let value = if snap.mem_total == 0 {
            "—".into()
        } else {
            format!(
                "{} / {}",
                sysstats::format_bytes(snap.mem_used),
                sysstats::format_bytes(snap.mem_total)
            )
        };
        let t = ratio(snap.mem_used, snap.mem_total);
        rows = rows.child(stat_row("MEM", value, t));
    }
    if cfg.show_net {
        any = true;
        let down = snap
            .net_down_bps
            .map(sysstats::format_bps)
            .unwrap_or_else(|| "—".into());
        let up = snap
            .net_up_bps
            .map(sysstats::format_bps)
            .unwrap_or_else(|| "—".into());
        rows = rows.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(stat_label("NET"))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(10.))
                        .child(net_side("↓", down))
                        .child(net_side("↑", up)),
                ),
        );
    }
    if cfg.show_disk {
        any = true;
        let value = if snap.disk_total == 0 {
            "—".into()
        } else {
            format!(
                "{} / {}",
                sysstats::format_bytes(snap.disk_used),
                sysstats::format_bytes(snap.disk_total)
            )
        };
        let t = ratio(snap.disk_used, snap.disk_total);
        rows = rows.child(stat_row("DISK", value, t));
    }

    nook_pane("nook-sysstats").w_full().child(if any {
        rows.into_any_element()
    } else {
        nook_empty("gauge", "Enable a readout in Settings").into_any_element()
    })
}

fn stat_label(name: &'static str) -> impl IntoElement {
    div()
        .w(px(32.))
        .flex_shrink_0()
        .text_size(px(10.))
        .line_height(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::TERTIARY_LABEL)
        .child(name)
}

fn stat_row(name: &'static str, value: String, t: f32) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(3.))
        .child(
            div()
                .w_full()
                .flex()
                .items_baseline()
                .justify_between()
                .gap(px(8.))
                .child(stat_label(name))
                .child(label(value, theme::CALLOUT, true)),
        )
        .child(gauge(t))
}

fn gauge(t: f32) -> impl IntoElement {
    div()
        .w_full()
        .h(px(3.))
        .rounded_full()
        .overflow_hidden()
        .bg(rgba(0xffffff26))
        .child(
            div()
                .h_full()
                .w(relative(t.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(theme::accent()),
        )
}

fn core_strip(cores: &[f32]) -> impl IntoElement {
    let mut row = div().w_full().flex().items_end().gap(px(2.)).h(px(10.));
    for (i, pct) in cores.iter().enumerate() {
        let t = (*pct / 100.0).clamp(0.08, 1.0);
        row = row.child(
            div()
                .id(("sys-core", i))
                .flex_1()
                .h(relative(t))
                .rounded(px(1.))
                .bg(rgba(0xffffff40)),
        );
    }
    row
}

fn net_side(arrow: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .items_baseline()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(11.))
                .line_height(px(13.))
                .text_color(theme::SECONDARY_LABEL)
                .child(arrow),
        )
        .child(label(value, theme::CALLOUT, true))
}

fn ratio(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f32 / total as f32).clamp(0.0, 1.0)
    }
}

impl Island {
    pub(crate) fn ensure_sysstats(&mut self, cx: &mut Context<Self>) {
        if self.sysstats_sampling {
            return;
        }
        if !self.sysstats_should_sample() {
            return;
        }
        self.sysstats_sampling = true;
        let physical = self.settings.sysstats.physical_nics;
        cx.spawn(async move |this, cx| loop {
            let keep = this
                .update(cx, |this, _| this.sysstats_should_sample())
                .unwrap_or(false);
            if !keep {
                let _ = this.update(cx, |this, _| this.sysstats_sampling = false);
                break;
            }
            let physical = this
                .update(cx, |this, _| this.settings.sysstats.physical_nics)
                .unwrap_or(physical);
            let snap = cx
                .background_executor()
                .spawn(async move { sysstats::sample(physical) })
                .await;
            if this
                .update(cx, |this, cx| {
                    this.sysstats = snap;
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
            cx.background_executor().timer(SAMPLE_INTERVAL).await;
        })
        .detach();
    }

    fn sysstats_should_sample(&self) -> bool {
        self.expanded && self.tab == Tab::Widgets && self.settings.show_sysstats
    }
}
