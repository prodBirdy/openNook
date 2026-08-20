//! Shared island controls: labels, buttons, widget card chrome, formatters.

use super::Island;
use crate::icons::lucide;
use crate::theme;
use gpui::{
    div, prelude::*, px, Context, CursorStyle, Div, ElementId, MouseButton, MouseDownEvent,
    SharedString, Stateful,
};

pub(crate) fn format_timer(seconds: u32) -> String {
    let m = seconds / 60;
    let s = seconds % 60;
    format!("{m}:{s:02}")
}

pub(crate) fn format_ts(ts: f64) -> String {
    use chrono::{Local, TimeZone};
    if let Some(dt) = Local.timestamp_opt(ts as i64, 0).single() {
        dt.format("%a %H:%M").to_string()
    } else {
        String::new()
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
        .rounded(px(theme::WIDGET_RADIUS - theme::ROW_INSET))
        .overflow_hidden()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(theme::FILL_TERTIARY))
        .active(|s| s.bg(theme::FILL_SECONDARY))
}

pub(crate) fn icon_btn(
    name: &'static str,
    elem_id: impl Into<SharedString>,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &MouseDownEvent, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    let play_optical = name == "play";
    div()
        .id(elem_id.into())
        .size(px(theme::HIT_MIN))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .when(play_optical, |d| d.pl(px(1.)))
        .hover(|s| s.bg(theme::FILL_SECONDARY))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(lucide(name, 14.0))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, event, cx);
            }),
        )
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

pub(crate) fn widget_shell(
    icon: &'static str,
    title: impl Into<SharedString>,
    child: impl IntoElement,
) -> impl IntoElement {
    use crate::icons::lucide_color;

    div()
        .flex()
        .flex_col()
        .w(px(220.))
        .h_full()
        .px_3()
        .pt_3()
        .pb_3()
        .bg(theme::FILL)
        .rounded(px(theme::WIDGET_RADIUS))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .mb_2()
                .child(lucide_color(icon, 13.0, theme::SECONDARY_LABEL))
                .child(
                    div()
                        .text_color(theme::SECONDARY_LABEL)
                        .text_size(px(theme::CALLOUT.size))
                        .line_height(px(theme::CALLOUT.leading))
                        .font_weight(theme::CALLOUT.emphasized)
                        .child(title.into()),
                ),
        )
        .child(div().flex_1().min_h(px(0.)).child(child))
}
