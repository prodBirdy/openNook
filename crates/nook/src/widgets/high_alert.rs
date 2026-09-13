//! High Alert keep-awake card: toggle + duration chips + remaining readout.

use crate::icons::lucide_color;
use crate::island::ui::{format_timer, label, nook_display, nook_pane};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, Context, CursorStyle, MouseButton, MouseDownEvent, SharedString};
use std::cell::RefCell;

const CHIPS: [(&str, Option<u32>); 4] = [
    ("15m", Some(15 * 60)),
    ("30m", Some(30 * 60)),
    ("1h", Some(60 * 60)),
    ("On", None),
];

thread_local! {
    /// Duration chip the active session was started with (`None` = "On").
    static ACTIVE_DURATION_SECS: RefCell<Option<Option<u32>>> = const { RefCell::new(None) };
}

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
        ACTIVE_DURATION_SECS
            .with(|d| *d.borrow())
            .unwrap_or_else(|| {
                Some(island.settings.high_alert_default_duration_secs).filter(|s| *s > 0)
            })
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
            div().flex().items_center().gap(px(6.)).children(
                CHIPS
                    .iter()
                    .map(|(name, secs)| chip(name, *secs, selected == *secs, cx)),
            ),
        )
        .child(
            label("Lid-close sleep is not prevented.", theme::FOOTNOTE, false)
                .text_color(theme::TERTIARY_LABEL),
        )
}

fn toggle_btn(active: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("high-alert-toggle")
        .size(px(theme::HIT_MIN))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::CONTROL_RADIUS))
        .hover(|s| s.bg(theme::FILL_SECONDARY))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            "sun",
            16.0,
            if active { theme::SUCCESS } else { theme::LABEL },
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if this.high_alert_active()
                    && nook_core::high_alert::is_held_by(
                        nook_core::high_alert::HighAlertOwner::Manual,
                    )
                {
                    this.set_high_alert(false, None);
                    ACTIVE_DURATION_SECS.with(|d| *d.borrow_mut() = None);
                } else {
                    let secs = this.settings.high_alert_default_duration_secs;
                    this.set_high_alert(true, Some(secs));
                    ACTIVE_DURATION_SECS.with(|d| {
                        *d.borrow_mut() = Some(if secs == 0 { None } else { Some(secs) });
                    });
                }
                cx.notify();
            }),
        )
}

fn chip(
    name: &'static str,
    secs: Option<u32>,
    selected: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("high-alert-chip-{name}")))
        .h(px(theme::HIT_MIN))
        .px(px(8.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .bg(if selected {
            theme::FILL_SECONDARY
        } else {
            theme::FILL_TERTIARY
        })
        .hover(|s| s.bg(theme::FILL_SECONDARY))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(
            label(name, theme::SUBHEADLINE, true).text_color(if selected {
                theme::LABEL
            } else {
                theme::SECONDARY_LABEL
            }),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.settings.high_alert_default_duration_secs = secs.unwrap_or(0);
                nook_core::settings::tweak_app_settings(|s| {
                    s.high_alert_default_duration_secs = secs.unwrap_or(0);
                });
                if this.high_alert_active() {
                    this.set_high_alert(true, Some(secs.unwrap_or(0)));
                    ACTIVE_DURATION_SECS.with(|d| *d.borrow_mut() = Some(secs));
                }
                cx.notify();
            }),
        )
}
