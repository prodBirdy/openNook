//! Shared island controls: labels, buttons, formatters.

use super::Island;
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    div, prelude::*, px, AnyElement, App, Context, CursorStyle, Div, ElementId, FontFeatures,
    MouseButton, MouseDownEvent, ScrollHandle, ScrollWheelEvent, SharedString, Stateful, Window,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) fn format_timer(seconds: u32) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Compact island face: `5m00`, `1h05`, `45s` — same as the React CompactTimer.
pub(crate) fn format_timer_compact(seconds: u32) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        if m > 0 {
            format!("{h}h{m:02}")
        } else {
            format!("{h}h")
        }
    } else if m > 0 {
        format!("{m}m{s:02}")
    } else {
        format!("{s}s")
    }
}

/// `strong` picks the style's emphasized weight and the primary label color;
/// otherwise the style's own weight and the secondary label color. HIG ›
/// Typography: "Adjust font weight, size, and color as needed to emphasize
/// important information and help people visualize hierarchy."
pub(crate) fn label(text: impl Into<SharedString>, style: theme::Text, strong: bool) -> Div {
    div()
        .text_color(if strong {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        })
        .text_size(px(style.size))
        .line_height(px(style.leading))
        .font_weight(if strong {
            style.emphasized
        } else {
            style.weight
        })
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(text.into())
}

fn tabular_features() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

/// Timer / countdown text. Tabular figures keep the compact pill from
/// shifting as digits change (HIG › Typography: use tabular numbers for
/// values that update in place).
pub(crate) fn timer_text(text: impl Into<SharedString>, style: theme::Text) -> Div {
    div()
        .font({
            let mut font = theme::mono_font(style.emphasized);
            font.features = tabular_features();
            font
        })
        .text_color(theme::TEXT)
        .text_size(px(style.size))
        .line_height(px(style.leading))
        .whitespace_nowrap()
        .child(text.into())
}

pub(crate) use super::marquee::slide_label;

pub(crate) fn text_btn(
    caption: impl Into<SharedString>,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    let caption = caption.into();
    div()
        .id(caption.clone())
        .h(px(theme::HIT_MIN))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::CONTROL_RADIUS))
        .bg(theme::FILL)
        .hover(|s| s.bg(theme::FILL_SECONDARY))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(label(caption, theme::CALLOUT, true))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, event, cx);
            }),
        )
}

/// Empty-state CTA (`Create Timer`, `Create Reminder`): `rounded-[20px]`.
pub(crate) fn pill_btn(
    caption: impl Into<SharedString>,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    let caption = caption.into();
    div()
        .id(caption.clone())
        .px(px(16.))
        .py(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::ROW_RADIUS))
        .bg(theme::FILL)
        .hover(|s| s.bg(theme::FILL_SECONDARY))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(label(caption, theme::BODY, true))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, event, cx);
            }),
        )
}

pub(crate) fn empty_state(
    message: impl Into<SharedString>,
    action: impl IntoElement,
) -> impl IntoElement {
    div()
        .flex_1()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .child(label(message, theme::TITLE_3, false).text_color(theme::TERTIARY_LABEL))
        .child(action)
}

/// Full-height Nook pane. Same chrome as Now Playing and Calendar: no fill,
/// no card radius — content sits in the scrolling row behind a 1px divider.
pub(crate) fn nook_pane(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_shrink_0()
        .h_full()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .overflow_hidden()
}

/// Calendar empty copy: 16px glyph + 12pt medium tertiary label, centered.
pub(crate) fn nook_empty(icon: &'static str, message: impl Into<SharedString>) -> impl IntoElement {
    nook_empty_column(icon, message)
}

pub(crate) fn nook_empty_with(
    icon: &'static str,
    message: impl Into<SharedString>,
    action: AnyElement,
) -> impl IntoElement {
    nook_empty_column(icon, message).child(action)
}

fn nook_empty_column(icon: &'static str, message: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .child(lucide_color(icon, theme::GLYPH_SM, theme::TERTIARY_LABEL))
        .child(
            div()
                .text_size(px(theme::CALLOUT.size))
                .line_height(px(theme::CALLOUT.leading))
                .font_weight(theme::CALLOUT.weight)
                .text_color(theme::TERTIARY_LABEL)
                .child(message.into()),
        )
}

/// Calendar month numeral: 32/36 bold primary label.
pub(crate) fn nook_display(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::DISPLAY.size))
        .line_height(px(theme::DISPLAY.leading))
        .font_weight(theme::DISPLAY.emphasized)
        .text_color(theme::LABEL)
        .child(text.into())
}

/// Title row: body label on the left, trailing control on the right.
pub(crate) fn nook_header(
    title: impl Into<SharedString>,
    trailing: impl IntoElement,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .flex_shrink_0()
        .pb(px(4.))
        .child(label(title, theme::BODY, true))
        .child(trailing)
}

/// Open a Privacy & Security pane in System Settings via `/usr/bin/open`.
pub(crate) fn open_privacy_pane(anchor: &'static str) {
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
    let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
}

/// Calendar event row: hairline, vertical padding, no card fill.
pub(crate) fn nook_row(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .py_2()
        .min_h(px(theme::HIT_MIN))
        .flex_shrink_0()
        .border_b_1()
        .border_color(theme::HAIRLINE)
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(theme::FILL_TERTIARY))
        .active(|s| s.bg(theme::FILL_SECONDARY))
}

/// 3×32pt accent rail used beside Calendar event titles.
pub(crate) fn nook_accent_bar(color: gpui::Rgba) -> Div {
    div()
        .w(px(3.))
        .h(px(32.))
        .rounded(px(2.))
        .mr_3()
        .flex_shrink_0()
        .bg(color)
}

/// Now Playing skip/play glyph: 22pt face, opacity press, no fill.
pub(crate) fn nook_icon_btn(
    name: &'static str,
    elem_id: impl Into<SharedString>,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Window, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(elem_id.into())
        .size(px(theme::HIT_MIN))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(theme::FILL))
        .active(|s| s.opacity(0.75))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(name, theme::GLYPH_SM, theme::LABEL))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                on_click(this, event, window, cx);
            }),
        )
}

// Live scroll handles for the expanded cards, keyed by element id.
//
// A card may only claim a gesture when it has somewhere left to scroll, and
// that means reading `max_offset` while the wheel event is in flight -- which
// only a tracked `ScrollHandle` exposes. The cards are built by free functions
// (one per widget) rather than by `Island`, so the handles live beside the
// shell that installs them. GPUI draws on the main thread, so thread-local is
// as wide as this needs to be.
thread_local! {
    static CARD_SCROLLS: RefCell<HashMap<ElementId, ScrollHandle>> = RefCell::new(HashMap::new());
}

fn card_scroll(id: &ElementId) -> ScrollHandle {
    CARD_SCROLLS.with_borrow_mut(|handles| handles.entry(id.clone()).or_default().clone())
}

pub(crate) fn scroll_body(id: impl Into<ElementId>, child: impl IntoElement) -> impl IntoElement {
    let id = id.into();
    let scroll = card_scroll(&id);
    let mut body = div()
        .id(id)
        .track_scroll(&scroll)
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .overflow_x_hidden()
        .overflow_y_scroll()
        .on_scroll_wheel({
            let scroll = scroll.clone();
            move |event: &ScrollWheelEvent, window: &mut Window, cx: &mut App| {
                let delta = event.delta.pixel_delta(window.line_height());
                if delta.y.abs() > delta.x.abs() && scroll.max_offset().height > px(0.5) {
                    cx.stop_propagation();
                }
            }
        })
        .child(child);
    body.style().restrict_scroll_to_axis = Some(false);
    body
}
