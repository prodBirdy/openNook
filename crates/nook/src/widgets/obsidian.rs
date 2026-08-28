//! Obsidian vault Nook pane: daily-note capture, recent notes, deep links.

use super::notes::markdown_preview;
use crate::island::ui::{nook_empty, nook_icon_btn, nook_pane, nook_row, scroll_body};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, rgba, Context, CursorStyle, FontWeight, KeyDownEvent, MouseButton,
    MouseDownEvent, SharedString,
};
use nook_core::obsidian::{self, NoteEntry};

pub(crate) fn obsidian_card(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    island.flush_obsidian_dirty(cx);
    let focus = island.obsidian_capture_focus(cx);
    let capturing = island.obsidian_typing;
    let capture = island.obsidian_capture.clone();
    let flash = island.obsidian_flash.clone();
    let selected = island.obsidian_selected.clone();
    let body = island.obsidian_body.clone();
    let vault = island.settings.obsidian_vault.clone();
    let notes = island.obsidian_notes.clone();

    let mut pane = nook_pane("nook-obsidian").relative().w_full();
    pane = pane.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .flex_shrink_0()
            .pb(px(4.))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::LABEL)
                    .child("Obsidian"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(nook_icon_btn(
                        "calendar",
                        "obs-daily",
                        cx,
                        |this, _, window, cx| {
                            this.open_obsidian_daily(window, cx);
                        },
                    ))
                    .child(nook_icon_btn(
                        "rotate-ccw",
                        "obs-refresh",
                        cx,
                        |this, _, _, cx| {
                            this.obsidian_dirty = true;
                            this.flush_obsidian_dirty(cx);
                        },
                    )),
            ),
    );

    if vault.is_none() {
        return pane.child(nook_empty("book", "Choose a vault in Settings"));
    }

    pane.child(capture_field(
        &capture,
        capturing,
        flash.as_deref(),
        &focus,
        cx,
    ))
    .child(scroll_body(
        "obsidian-scroll",
        note_list(&notes, selected.as_deref(), body.as_deref(), cx),
    ))
}

fn capture_field(
    capture: &str,
    focused: bool,
    flash: Option<&str>,
    focus: &gpui::FocusHandle,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let placeholder = capture.is_empty();
    let shown = if placeholder {
        flash.unwrap_or("Capture to today…").to_string()
    } else {
        capture.to_string()
    };
    let focus = focus.clone();
    div()
        .id("obs-capture")
        .track_focus(&focus)
        .w_full()
        .h(px(26.))
        .px(px(8.))
        .mb(px(6.))
        .rounded(px(6.))
        .bg(rgba(0xffffff14))
        .when(focused, |d| d.border_1().border_color(theme::accent()))
        .flex()
        .items_center()
        .cursor(CursorStyle::IBeam)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.focus_obsidian_capture(window, cx);
            }),
        )
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            this.on_obsidian_capture_key(event, window, cx);
        }))
        .child(
            div()
                .w_full()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(12.))
                .text_color(if placeholder {
                    theme::TERTIARY_LABEL
                } else {
                    theme::LABEL
                })
                .child(SharedString::from(shown)),
        )
}

fn note_list(
    notes: &[NoteEntry],
    selected: Option<&str>,
    body: Option<&str>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    if notes.is_empty() {
        return nook_empty("book", "No markdown notes").into_any_element();
    }
    let mut list = div().flex().flex_col().w_full();
    for note in notes.iter().take(8) {
        let rel = note.rel_path.clone();
        let title = note.title.clone();
        let folder = note
            .rel_path
            .rsplit_once('/')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_default();
        let is_sel = selected == Some(note.rel_path.as_str());
        list = list.child(
            nook_row(SharedString::from(format!("obs-{}", note.rel_path)))
                .gap(px(8.))
                .when(is_sel, |d| d.bg(rgba(0xffffff14)))
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener({
                        let rel = rel.clone();
                        move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.select_obsidian_note(rel.clone(), cx);
                        }
                    }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::LABEL)
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(SharedString::from(title)),
                        )
                        .when(!folder.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(theme::TERTIARY_LABEL)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(SharedString::from(folder)),
                            )
                        }),
                )
                .child(nook_icon_btn(
                    "files",
                    SharedString::from(format!("obs-open-{}", note.rel_path)),
                    cx,
                    {
                        let rel = rel.clone();
                        move |this, _, _, cx| {
                            this.open_obsidian_note(&rel, cx);
                        }
                    },
                )),
        );
        if is_sel {
            if let Some(body) = body {
                list = list.child(
                    div()
                        .w_full()
                        .max_h(px(72.))
                        .overflow_hidden()
                        .pt(px(4.))
                        .child(markdown_preview(body)),
                );
            }
        }
    }
    list.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_url_uses_core_builder() {
        let url = obsidian::open_file_url("Vault", "a/b.md");
        assert_eq!(url, "obsidian://open?vault=Vault&file=a/b");
    }
}
