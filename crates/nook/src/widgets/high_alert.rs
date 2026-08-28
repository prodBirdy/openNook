//! High Alert keep-awake card: toggle + duration chips + remaining readout.

use crate::icons::lucide_color;
use crate::island::ui::{format_timer, nook_display, nook_pane};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, rgba, Context, CursorStyle, FontWeight, MouseButton, MouseDownEvent,
    SharedString,
};

const CHIPS: [(&str, Option<u32>); 4] = [
    ("15m", Some(15 * 60)),
    ("30m", Some(30 * 60)),
    ("1h", Some(60 * 60)),
    ("On", None),
];

pub(crate) fn high_alert_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let active = island.high_alert_active();
    let remaining = if active {
        island
            .high_alert_remaining_secs()
            .map(format_timer)
            .unwrap_or_else(|| "On".into())
    } else {
        "Off".into()
    };
    let selected = if active {
        island
            .awake_deadline
            .and_then(|_| island.high_alert_remaining_secs())
            .map(|secs| nearest_chip(secs, island.settings.high_alert_default_duration_secs))
            .unwrap_or(None)
    } else {
        Some(island.settings.high_alert_default_duration_secs).filter(|s| *s > 0)
    };

    nook_pane("nook-high-alert")
        .w_full()
        .child(
            div()
                .flex()
                .items_end()
                .justify_between()
                .gap(px(12.))
                .flex_shrink_0()
                .child(nook_display(remaining))
                .child(toggle_btn(active, cx)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .children(CHIPS.iter().map(|(label, secs)| {
                    chip(*label, *secs, selected == *secs && active, cx)
                })),
        )
        .child(
            div()
                .text_size(px(9.))
                .line_height(px(11.))
                .text_color(theme::TERTIARY_LABEL)
                .child("Lid-close sleep is not prevented."),
        )
}

fn nearest_chip(remaining: u32, default_secs: u32) -> Option<u32> {
    if remaining == 0 {
        return None;
    }
    CHIPS
        .iter()
        .filter_map(|(_, secs)| *secs)
        .min_by_key(|secs| secs.abs_diff(remaining.max(default_secs.min(remaining))))
}

fn toggle_btn(active: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("high-alert-toggle")
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .opacity(0.9)
        .hover(|s| s.opacity(1.0))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            "sun",
            16.0,
            if active {
                theme::SUCCESS
            } else {
                theme::LABEL
            },
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if this.high_alert_active() && nook_core::high_alert::is_held_by(
                    nook_core::high_alert::HighAlertOwner::Manual,
                ) {
                    this.set_high_alert(false, None);
                } else {
                    let secs = this.settings.high_alert_default_duration_secs;
                    this.set_high_alert(true, Some(secs));
                }
                cx.notify();
            }),
        )
}

fn chip(
    label: &'static str,
    secs: Option<u32>,
    selected: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("high-alert-chip-{label}")))
        .h(px(22.))
        .px(px(8.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .bg(if selected {
            theme::FILL_SECONDARY
        } else {
            rgba(0xffffff14)
        })
        .hover(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if selected {
                    theme::LABEL
                } else {
                    theme::SECONDARY_LABEL
                })
                .child(label),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.settings.high_alert_default_duration_secs = secs.unwrap_or(0);
                nook_core::settings::tweak_app_settings(|s| {
                    s.high_alert_default_duration_secs = secs.unwrap_or(0);
                });
                this.set_high_alert(true, Some(secs.unwrap_or(0)));
                cx.notify();
            }),
        )
}
