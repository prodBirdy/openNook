//! High Alert keep-awake card: toggle + duration chips + remaining readout.

use crate::icons::lucide_color;
use crate::island::ui::{format_timer, label, nook_pane};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, Context, CursorStyle, MouseButton, MouseDownEvent, SharedString};
use nook_core::settings::{WidgetModule, WidgetSize};
use std::cell::RefCell;

const CHIPS: [(&str, Option<u32>); 4] = [
    ("15m", Some(15 * 60)),
    ("30m", Some(30 * 60)),
    ("1h", Some(60 * 60)),
    ("On", None),
];

/// S only has room for two chips; prefer 30m + On.
const CHIPS_S: [(&str, Option<u32>); 2] = [("30m", Some(30 * 60)), ("On", None)];

thread_local! {
    /// Duration chip the active session was started with (`None` = "On").
    static ACTIVE_DURATION_SECS: RefCell<Option<Option<u32>>> = const { RefCell::new(None) };
}

pub(crate) fn high_alert_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let size = resolve_size(island, WidgetModule::HighAlert);
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
    let chips: &[(&str, Option<u32>)] = if size == WidgetSize::Small {
        &CHIPS_S
    } else {
        &CHIPS
    };
    // M (~3 cells) needs compact chips so 15m/30m/1h/On all fit.
    let (chip_gap, chip_px) = match size {
        WidgetSize::Medium => (px(3.), px(5.)),
        _ => (px(6.), px(8.)),
    };

    card_shell("nook-high-alert")
        .w_full()
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .min_w(px(0.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(big_label(
                            if active {
                                remaining.clone()
                            } else {
                                "Off".into()
                            },
                            if active {
                                theme::SYSTEM_ORANGE
                            } else {
                                theme::SECONDARY_LABEL
                            },
                        ))
                        .child(
                            label(
                                if active {
                                    "High alert active"
                                } else {
                                    "High alert paused"
                                },
                                theme::FOOTNOTE,
                                false,
                            )
                            .text_color(theme::TERTIARY_LABEL),
                        ),
                )
                .child(toggle_btn(active, cx)),
        )
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(chip_gap)
                .overflow_hidden()
                .children(
                    chips
                        .iter()
                        .map(|(name, secs)| chip(name, *secs, selected == *secs, chip_px, cx)),
                ),
        )
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

fn toggle_btn(active: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("high-alert-toggle")
        .size(px(theme::HIT_MIN))
        .flex_shrink_0()
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
            if active {
                theme::SYSTEM_ORANGE
            } else {
                theme::LABEL
            },
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
    pad_x: gpui::Pixels,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("high-alert-chip-{name}")))
        .h(px(24.))
        .px(pad_x)
        .rounded(px(8.))
        .flex_shrink_0()
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
        .child(label(name, theme::FOOTNOTE, true).text_color(if selected {
            theme::LABEL
        } else {
            theme::SECONDARY_LABEL
        }))
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

fn resolve_size(island: &Island, module: WidgetModule) -> WidgetSize {
    #[cfg(debug_assertions)]
    {
        if island.gallery_mode {
            let sizes = island.settings.distinct_sizes(module);
            if !sizes.is_empty() {
                return sizes[gallery_call_idx(module as u8) % sizes.len()];
            }
        }
    }
    island.settings.size_for(module)
}

#[cfg(debug_assertions)]
fn gallery_call_idx(module: u8) -> usize {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};
    thread_local! {
        static STATE: RefCell<HashMap<u8, (Instant, usize)>> =
            RefCell::new(HashMap::new());
    }
    STATE.with(|state| {
        let mut map = state.borrow_mut();
        let now = Instant::now();
        let entry = map.entry(module).or_insert((now, 0));
        if now.duration_since(entry.0) > Duration::from_millis(32) {
            entry.1 = 0;
        }
        entry.0 = now;
        let idx = entry.1;
        entry.1 = idx + 1;
        idx
    })
}
