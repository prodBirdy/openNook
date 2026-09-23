//! System Stats Nook pane: CPU, memory, network, disk capacity.
//!
//! Sampling is spawned only while the expanded card is on screen and stops
//! on collapse — zero idle syscalls.

use crate::island::ui::{nook_empty, nook_pane};
use crate::island::{Island, Tab};
use crate::theme;
use gpui::{div, prelude::*, px, relative, Context, FontWeight, SharedString};
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
        .gap(px(9.));
    let mut any = false;
    if cfg.show_cpu {
        any = true;
        let value = snap
            .cpu_pct
            .map(sysstats::format_pct)
            .unwrap_or_else(|| "—".into());
        let t = snap.cpu_pct.unwrap_or(0.0) / 100.0;
        rows = rows.child(stat_row("CPU", value, t, theme::LABEL));
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
        rows = rows.child(stat_row("MEM", value, t, theme::LABEL));
    }
    if cfg.show_net {
        any = true;
        let down = snap
            .net_down_bps
            .map(sysstats::format_bps)
            .unwrap_or_else(|| "—".into());
        let t = snap
            .net_down_bps
            .map(|v| (v / 50_000_000.0) as f32)
            .unwrap_or(0.0);
        rows = rows.child(stat_row("NET", down, t, theme::accent()));
    }

    card_shell("nook-sysstats").w_full().child(if any {
        rows.into_any_element()
    } else {
        nook_empty("gauge", "Enable a readout in Settings").into_any_element()
    })
}

fn card_shell(id: impl Into<gpui::ElementId>) -> gpui::Stateful<gpui::Div> {
    nook_pane(id).p(px(16.)).gap(px(10.))
}

fn stat_label(name: &'static str) -> impl IntoElement {
    div()
        .w(px(26.))
        .flex_shrink_0()
        .text_size(px(9.))
        .line_height(px(12.))
        .font_weight(FontWeight::NORMAL)
        .text_color(theme::TERTIARY_LABEL)
        .child(name)
}

fn stat_row(name: &'static str, value: String, t: f32, color: gpui::Rgba) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(stat_label(name))
        .child(gauge(t, color))
        .child(
            div()
                .w(px(62.))
                .flex_shrink_0()
                .text_size(px(9.))
                .line_height(px(12.))
                .font_weight(FontWeight::NORMAL)
                .text_color(theme::SECONDARY_LABEL)
                .text_right()
                .whitespace_nowrap()
                .child(SharedString::from(value)),
        )
}

fn gauge(t: f32, color: gpui::Rgba) -> impl IntoElement {
    div()
        .w_full()
        .h(px(theme::TRACK_H))
        .rounded(px(theme::TRACK_RADIUS))
        .overflow_hidden()
        .bg(theme::FILL)
        .child(
            div()
                .h_full()
                .w(relative(t.clamp(0.0, 1.0)))
                .rounded(px(theme::TRACK_RADIUS))
                .bg(color),
        )
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
