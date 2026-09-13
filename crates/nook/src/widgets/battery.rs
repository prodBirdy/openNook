//! Battery Nook pane: percent, time remaining, charging state, LPM toggle.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_display, nook_pane};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, Context, CursorStyle, MouseButton, MouseDownEvent};
use nook_core::power::{self, BatteryWarning, PowerSnapshot};

/// Status-bar battery colors: green while charging, yellow in Low Power Mode,
/// red at the OS low-battery warnings, white otherwise.
pub(crate) fn tint(snap: PowerSnapshot) -> gpui::Rgba {
    if snap.is_charging {
        theme::SUCCESS
    } else if snap.low_power_mode {
        theme::SYSTEM_YELLOW
    } else if snap.warning_level != BatteryWarning::None
        || snap.percent.is_some_and(|percent| percent <= 20)
    {
        theme::DESTRUCTIVE
    } else {
        theme::LABEL
    }
}

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
            .child(label("Plugged in", theme::TITLE_2, true))
            .child(
                label("No battery", theme::SUBHEADLINE, false).text_color(theme::TERTIARY_LABEL),
            );
    }

    let color = tint(snap);

    div()
        .flex()
        .items_end()
        .gap(px(8.))
        .child(nook_display(power::format_percent(snap.percent)).text_color(color))
        .child(
            div()
                .pb(px(4.))
                .flex()
                .flex_col()
                .child(label(
                    if snap.is_charging {
                        "Charging"
                    } else if snap.on_ac {
                        "Plugged in"
                    } else {
                        "On battery"
                    },
                    theme::SUBHEADLINE,
                    true,
                ))
                .child(
                    label(
                        power::format_time_remaining(snap.time_to_empty_min),
                        theme::SUBHEADLINE,
                        false,
                    )
                    .text_color(theme::TERTIARY_LABEL),
                ),
        )
}

fn status_line(snap: PowerSnapshot, error: Option<&str>) -> impl IntoElement {
    let text = if let Some(err) = error {
        err.to_string()
    } else if snap.low_power_mode {
        "Low Power Mode is on".into()
    } else {
        "Low Power Mode is off".into()
    };
    label(text, theme::SUBHEADLINE, false).text_color(if error.is_some() {
        theme::DESTRUCTIVE
    } else {
        theme::TERTIARY_LABEL
    })
}

fn lpm_btn(on: bool, pending: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("battery-lpm")
        .h(px(theme::HIT_MIN))
        .px(px(10.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(6.))
        .bg(if on {
            gpui::Rgba {
                a: 0.2,
                ..theme::SYSTEM_YELLOW
            }
        } else {
            theme::FILL_TERTIARY
        })
        .opacity(if pending { 0.7 } else { 1.0 })
        .hover(|s| if pending { s } else { s.bg(theme::FILL) })
        .active(|s| s.opacity(0.85))
        .cursor(if pending {
            CursorStyle::Arrow
        } else {
            CursorStyle::PointingHand
        })
        .child(lucide_color(
            "zap",
            14.0,
            if on {
                theme::SYSTEM_YELLOW
            } else {
                theme::LABEL
            },
        ))
        .child(
            label("Low Power Mode", theme::CALLOUT, true).text_color(if on {
                theme::SYSTEM_YELLOW
            } else {
                theme::LABEL
            }),
        )
        .when(!pending, |d| {
            d.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.toggle_low_power_mode(cx);
                }),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> PowerSnapshot {
        PowerSnapshot {
            percent: Some(80),
            is_charging: false,
            on_ac: false,
            time_to_empty_min: None,
            warning_level: BatteryWarning::None,
            low_power_mode: false,
            has_battery: true,
        }
    }

    #[test]
    fn charging_is_green() {
        let mut snap = snap();
        snap.is_charging = true;
        assert_eq!(tint(snap), theme::SUCCESS);
    }

    #[test]
    fn low_power_mode_is_yellow() {
        let mut snap = snap();
        snap.low_power_mode = true;
        assert_eq!(tint(snap), theme::SYSTEM_YELLOW);
    }

    #[test]
    fn charging_wins_over_low_power_mode() {
        let mut snap = snap();
        snap.is_charging = true;
        snap.low_power_mode = true;
        assert_eq!(tint(snap), theme::SUCCESS);
    }

    #[test]
    fn early_warning_is_red() {
        let mut snap = snap();
        snap.percent = Some(18);
        snap.warning_level = BatteryWarning::Early;
        assert_eq!(tint(snap), theme::DESTRUCTIVE);
    }

    #[test]
    fn final_warning_is_red() {
        let mut snap = snap();
        snap.percent = Some(5);
        snap.warning_level = BatteryWarning::Final;
        assert_eq!(tint(snap), theme::DESTRUCTIVE);
    }

    #[test]
    fn healthy_discharging_is_white() {
        assert_eq!(tint(snap()), theme::LABEL);
    }
}
