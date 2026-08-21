//! Scratchpad notes card.

use crate::island::ui::{text_btn, widget_shell, wrapping_label};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, Context, CursorStyle, MouseButton, MouseDownEvent};

fn open_notes() {
    if let Err(err) = nook_core::notes::open_notes_editor() {
        log::warn!("open notes: {err}");
    }
}

pub(crate) fn notes_card(notes: &str, cx: &mut Context<Island>) -> impl IntoElement {
    let empty = notes.trim().is_empty();
    let preview = if empty {
        "Click to add notes".to_string()
    } else {
        notes.to_string()
    };
    widget_shell(
        "notes-scroll",
        div()
            .id("notes-hit")
            .flex()
            .flex_col()
            .gap_2()
            .size_full()
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    open_notes();
                }),
            )
            .child(wrapping_label(preview, theme::CALLOUT, !empty))
            .child(text_btn("Edit Notes…", cx, |_, _, _| {
                open_notes();
            })),
    )
}
