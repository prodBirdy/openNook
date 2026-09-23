//! Battery Nook pane: percent, time remaining, charging state, LPM toggle.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_pane};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent};
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
    let lpm_on = snap.low_power_mode;

    // Pencil uSzA5: percent + time only. LPM is essential but not in the
    // mockup — hidden at rest (opacity 0, group hover), always shown when on
    // as a small yellow zap chip.
    card_shell("nook-battery")
        .group("nook-battery")
        .relative()
        .w_full()
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .flex()
                .flex_col()
                .gap(px(8.))
                .justify_center()
                .child(gauge(snap)),
        )
        .child(
            div()
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .when(!lpm_on, |d| {
                    d.opacity(0.0)
                        .group_hover("nook-battery", |s| s.opacity(1.0))
                })
                .child(lpm_control(lpm_on, pending, cx)),
        )
        .when_some(error.as_deref(), |d, err| {
            d.child(
                div().absolute().bottom(px(8.)).left(px(16.)).child(
                    label(err.to_string(), theme::FOOTNOTE, false).text_color(theme::DESTRUCTIVE),
                ),
            )
        })
}

fn card_shell(id: impl Into<gpui::ElementId>) -> gpui::Stateful<gpui::Div> {
    nook_pane(id).p(px(16.)).gap(px(10.))
}

fn big_label(text: impl Into<gpui::SharedString>, color: gpui::Rgba) -> gpui::Div {
    div()
        .text_size(px(26.))
        .line_height(px(30.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(color)
        .whitespace_nowrap()
        .child(text.into())
}

fn gauge(snap: PowerSnapshot) -> impl IntoElement {
    if !snap.has_battery {
        return div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(big_label("Plugged in", theme::LABEL))
            .child(label("No battery", theme::FOOTNOTE, false).text_color(theme::TERTIARY_LABEL));
    }

    let color = tint(snap);
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(big_label(power::format_percent(snap.percent), color))
        .child(
            label(status_caption(snap), theme::FOOTNOTE, false).text_color(theme::TERTIARY_LABEL),
        )
}

fn status_caption(snap: PowerSnapshot) -> String {
    if snap.is_charging {
        match snap.time_to_empty_min {
            Some(0) => "Charging · Calculating…".into(),
            Some(m) => format!("Charging · {} to full", format_duration(m)),
            None => "Charging".into(),
        }
    } else {
        format_time_left(snap.time_to_empty_min)
    }
}

/// Pencil "1h 40m left" — minutes unpadded (core `format_time_remaining` zero-pads).
fn format_time_left(minutes: Option<u32>) -> String {
    match minutes {
        None => "—".into(),
        Some(0) => "Calculating…".into(),
        Some(m) => format!("{} left", format_duration(m)),
    }
}

fn format_duration(minutes: u32) -> String {
    if minutes < 60 {
        format!("{minutes}m")
    } else {
        format!("{}h {}m", minutes / 60, minutes % 60)
    }
}

/// Compact-face battery shell (Pencil b3w3FY). Package A draws this in
/// `compact.rs`; kept here for the same geometry constants.
#[allow(dead_code)]
pub(crate) fn battery_glyph(snap: PowerSnapshot, scale: f32) -> AnyElement {
    let percent = snap
        .percent
        .unwrap_or(if snap.has_battery { 0 } else { 100 });
    let fill_w = 21.0 * (percent as f32 / 100.0).clamp(0.0, 1.0);
    let color = tint(snap);
    let ring = theme::with_alpha(theme::LABEL, 0x5C as f32 / 255.0);
    div()
        .flex()
        .items_center()
        .gap(px(1.5 * scale))
        .child(
            div()
                .w(px(25.0 * scale))
                .h(px(13.0 * scale))
                .rounded(px(4.3 * scale))
                .border_1()
                .border_color(ring)
                .p(px(2.0 * scale))
                .flex()
                .items_center()
                .child(
                    div()
                        .w(px(fill_w * scale))
                        .h(px(9.0 * scale))
                        .rounded(px(2.5 * scale))
                        .bg(color),
                ),
        )
        .child(
            div()
                .w(px(1.5 * scale))
                .h(px(4.5 * scale))
                .rounded(px(1.0 * scale))
                .bg(ring),
        )
        .into_any_element()
}

/// When LPM is on: small yellow zap chip. When off: LPM pill (shown via group hover).
fn lpm_control(on: bool, pending: bool, cx: &mut Context<Island>) -> impl IntoElement {
    if on {
        return div()
            .id("battery-lpm")
            .size(px(theme::HIT_MIN))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::Rgba {
                a: 0.2,
                ..theme::SYSTEM_YELLOW
            })
            .opacity(if pending { 0.7 } else { 1.0 })
            .hover(|s| if pending { s } else { s.bg(theme::FILL) })
            .active(|s| s.opacity(0.85))
            .cursor(if pending {
                CursorStyle::Arrow
            } else {
                CursorStyle::PointingHand
            })
            .child(lucide_color("zap", 14.0, theme::SYSTEM_YELLOW))
            .when(!pending, |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.toggle_low_power_mode(cx);
                    }),
                )
            })
            .into_any_element();
    }

    div()
        .id("battery-lpm")
        .h(px(24.))
        .px(px(8.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(6.))
        .bg(theme::FILL_TERTIARY)
        .opacity(if pending { 0.7 } else { 1.0 })
        .hover(|s| if pending { s } else { s.bg(theme::FILL) })
        .active(|s| s.opacity(0.85))
        .cursor(if pending {
            CursorStyle::Arrow
        } else {
            CursorStyle::PointingHand
        })
        .child(lucide_color("zap", 14.0, theme::LABEL))
        .child(label("LPM", theme::FOOTNOTE, true).text_color(theme::LABEL))
        .when(!pending, |d| {
            d.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.toggle_low_power_mode(cx);
                }),
            )
        })
        .into_any_element()
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

    #[test]
    fn time_left_matches_pencil_unpadded_minutes() {
        assert_eq!(format_time_left(Some(100)), "1h 40m left");
        assert_eq!(format_time_left(Some(65)), "1h 5m left");
        assert_eq!(format_time_left(Some(40)), "40m left");
    }

    #[test]
    fn charging_caption_uses_time_to_full() {
        let mut snap = snap();
        snap.is_charging = true;
        snap.time_to_empty_min = Some(70);
        assert_eq!(status_caption(snap), "Charging · 1h 10m to full");
        snap.time_to_empty_min = None;
        assert_eq!(status_caption(snap), "Charging");
    }
}
