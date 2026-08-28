//! Battery Nook pane: percent, time remaining, charging state, LPM toggle.

use crate::icons::lucide_color;
use crate::island::ui::{nook_display, nook_pane};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, rgba, Context, CursorStyle, FontWeight, MouseButton, MouseDownEvent};
use nook_core::power::{self, PowerSnapshot};

pub(crate) fn battery_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let snap = island.power;
    let pending = island.lpm_pending;
    let error = island.lpm_error.clone();

    nook_pane("nook-battery")
        .w_full()
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(gauge(snap))
                .child(lpm_btn(snap.low_power_mode, pending, cx)),
        )
        .child(status_line(snap, error.as_deref()))
}

fn gauge(snap: PowerSnapshot) -> impl IntoElement {
    if !snap.has_battery {
        return div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(nook_display("AC"))
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::SECONDARY_LABEL)
                    .child("No battery"),
            );
    }

    let tint = if snap.is_alerting(20) {
        theme::DESTRUCTIVE
    } else if snap.is_charging {
        theme::SUCCESS
    } else {
        theme::LABEL
    };

    div()
        .flex()
        .items_end()
        .gap(px(8.))
        .child(
            nook_display(power::format_percent(snap.percent)).text_color(tint),
        )
        .child(
            div()
                .pb(px(4.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::SECONDARY_LABEL)
                        .child(if snap.is_charging {
                            "Charging"
                        } else if snap.on_ac {
                            "On AC"
                        } else {
                            "On battery"
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .text_color(theme::TERTIARY_LABEL)
                        .child(power::format_time_remaining(snap.time_to_empty_min)),
                ),
        )
}

fn status_line(snap: PowerSnapshot, error: Option<&str>) -> impl IntoElement {
    let text = if let Some(err) = error {
        err.to_string()
    } else if snap.low_power_mode {
        "Low Power Mode on".into()
    } else if !snap.has_battery {
        "Low Power Mode still works on this Mac.".into()
    } else {
        String::new()
    };
    div()
        .w_full()
        .text_size(px(11.))
        .line_height(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if error.is_some() {
            theme::DESTRUCTIVE
        } else {
            theme::TERTIARY_LABEL
        })
        .child(text)
}

fn lpm_btn(on: bool, pending: bool, cx: &mut Context<Island>) -> impl IntoElement {
    let label = if pending {
        "…"
    } else if on {
        "LPM on"
    } else {
        "LPM"
    };
    div()
        .id("battery-lpm")
        .h(px(theme::HIT_MIN))
        .px(px(10.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(6.))
        .bg(if on {
            rgba(0xf59e0b33)
        } else {
            rgba(0xffffff14)
        })
        .opacity(if pending { 0.7 } else { 1.0 })
        .hover(|s| if pending { s } else { s.bg(rgba(0xffffff22)) })
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            "zap",
            14.0,
            if on {
                theme::SYSTEM_ORANGE
            } else {
                theme::LABEL
            },
        ))
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if on {
                    theme::SYSTEM_ORANGE
                } else {
                    theme::LABEL
                })
                .child(label),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.toggle_low_power_mode(cx);
            }),
        )
}
