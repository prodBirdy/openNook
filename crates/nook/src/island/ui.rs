//! Shared island controls: labels, buttons, widget card chrome, formatters.

use super::Island;
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    div, prelude::*, px, rgba, App, Context, CursorStyle, Div, ElementId, Font, FontFeatures,
    FontStyle, FontWeight, MouseButton, MouseDownEvent, ScrollHandle, ScrollWheelEvent,
    SharedString, Stateful, Window,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) fn format_timer(seconds: u32) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{h}:{m:02}")
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
        .font(Font {
            family: "SF Pro".into(),
            features: tabular_features(),
            fallbacks: None,
            weight: style.emphasized,
            style: FontStyle::Normal,
        })
        .text_color(theme::TEXT)
        .text_size(px(style.size))
        .line_height(px(style.leading))
        .whitespace_nowrap()
        .child(text.into())
}

pub(crate) use super::marquee::slide_label;

/// A selectable row inside a card.
///
/// HIG › Accessibility gives macOS a 28×28 pt recommended hit target, so a row
/// holds that height even when its text is shorter. HIG › Lists and tables asks
/// for feedback on selection and HIG › Layout asks for alignment, so the
/// highlight is inset back out of the card's 12 pt content margin by 6 pt and
/// rounded concentrically with the card (14 pt outer − 6 pt gap = 8 pt inner)
/// rather than running square-cornered to the card's edges.
pub(crate) fn card_row(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .min_h(px(theme::HIT_MIN))
        .mx(px(-theme::ROW_INSET))
        .px(px(theme::ROW_INSET))
        .rounded(px(theme::ROW_RADIUS))
        .overflow_hidden()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(theme::FILL_TERTIARY))
        .active(|s| s.bg(theme::FILL_SECONDARY))
}

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

/// Header + / refresh on React widgets: 18px icon, round, white/40.
pub(crate) fn header_icon_btn(
    name: &'static str,
    elem_id: impl Into<SharedString>,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Window, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(elem_id.into())
        .size(px(28.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(rgba(0xFFFFFF1A)))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(name, 18.0, rgba(0xffffff66)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                on_click(this, event, window, cx);
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
        .bg(rgba(0xFFFFFF1A))
        .hover(|s| s.bg(rgba(0xffffff33)))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::LABEL)
                .child(caption),
        )
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
        .child(
            div()
                .text_size(px(14.))
                .text_color(rgba(0xFFFFFF4D))
                .child(message.into()),
        )
        .child(action)
}

pub(crate) fn widget_title(title: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(17.))
        .line_height(px(22.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgba(0xFFFFFFF2))
        .child(title.into())
}

/// React `WidgetWrapper`: `min-w-[300px]`.
pub(crate) const WIDGET_CARD_WIDTH: f32 = 300.0;

pub(crate) const MEDIA_ART: f32 = 52.0;
pub(crate) const MEDIA_ART_RADIUS: f32 = 12.0;
pub(crate) const MEDIA_PLAY: f32 = 40.0;
pub(crate) const MEDIA_PROGRESS_HIT: f32 = 12.0;
pub(crate) const MEDIA_TIME_PAD_TOP: f32 = 2.0;
pub(crate) const MEDIA_TIME_PAD_GAP: f32 = 6.0;

/// Expanded-card chrome matching React `WidgetWrapper`: 28px corners, 16px
/// pad, hairline, stretch to the row height.
pub(crate) fn card_chrome(width: f32) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .w(px(width))
        .h_full()
        .p(px(theme::WIDGET_PAD))
        .bg(theme::FILL)
        .border_1()
        .border_color(rgba(0xFFFFFF1A))
        .rounded(px(theme::WIDGET_RADIUS))
        .overflow_hidden()
        .shadow_md()
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

pub(crate) fn widget_shell_actions(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    actions: impl IntoElement,
    child: impl IntoElement,
) -> impl IntoElement {
    card_chrome(WIDGET_CARD_WIDTH)
        .gap(px(8.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .flex_shrink_0()
                .w_full()
                .child(widget_title(title))
                .child(div().flex().items_center().gap(px(4.)).child(actions)),
        )
        .child(scroll_body(id, child))
}

pub(crate) fn widget_shell_w(
    id: impl Into<ElementId>,
    width: f32,
    child: impl IntoElement,
) -> impl IntoElement {
    card_chrome(width).child(scroll_body(id, child))
}

fn scroll_body(id: impl Into<ElementId>, child: impl IntoElement) -> impl IntoElement {
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
                // GPUI runs its own scroll listener ahead of this one, so the
                // card has already moved by the time we get here: all this
                // decides is whether the row of cards behind the card gets the
                // gesture too. Keep it only when it is vertical *and* there is
                // overflow to move, so a card that fits its content never
                // blocks scrolling from card to card.
                let delta = event.delta.pixel_delta(window.line_height());
                if delta.y.abs() > delta.x.abs() && scroll.max_offset().height > px(0.5) {
                    cx.stop_propagation();
                }
            }
        })
        .child(child);
    // Sideways gestures have to reach the row untouched. Without this, GPUI
    // feeds a purely horizontal delta into whichever axis the element *can*
    // scroll, so swiping across the cards would drag each card's content
    // vertically on the way past.
    body.style().restrict_scroll_to_axis = Some(true);
    body
}
