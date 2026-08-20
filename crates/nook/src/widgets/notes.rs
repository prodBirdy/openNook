//! Scratchpad notes card.

use crate::island::ui::{label, text_btn, widget_shell};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, Context};

pub(crate) fn notes_card(notes: &str, cx: &mut Context<Island>) -> impl IntoElement {
    let preview = if notes.trim().is_empty() {
        "Nothing here yet. Edit notes to add a scratchpad.".to_string()
    } else {
        notes.chars().take(120).collect()
    };
    widget_shell(
        "notebook",
        "Notes",
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(label(preview, theme::CALLOUT, false))
            .child(text_btn("Edit Notes…", cx, |_, _, _| {
                if let Err(err) = nook_core::notes::open_notes_editor() {
                    log::warn!("open notes: {err}");
                }
            })),
    )
}
