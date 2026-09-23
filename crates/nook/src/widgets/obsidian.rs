//! Obsidian vault Nook pane: daily-note capture, recent notes, deep links.
//!
//! Expanded face is the Pencil list mockup (vault summary + daily/recent rows).
//! Capture / daily / refresh stay as corner controls so plumbing still works.

use crate::island::ui::{label, nook_empty, nook_icon_btn, nook_pane};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, Context, CursorStyle, KeyDownEvent, MouseButton, MouseDownEvent,
    SharedString,
};
use nook_core::obsidian::{CivilDate, NoteEntry};
use std::time::SystemTime;

const ROW_H: f32 = 29.0;

pub(crate) fn obsidian_card(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    island.flush_obsidian_dirty(cx);
    let focus = island.obsidian_capture_focus(cx);
    let capturing = island.obsidian_typing;
    let capture = island.obsidian_capture.clone();
    let flash = island.obsidian_flash.clone();
    let vault = island.settings.obsidian_vault.clone();
    let notes = island.obsidian_notes.clone();

    let mut pane = card_shell("nook-obsidian").relative().w_full();

    let Some(vault_path) = vault else {
        return pane.child(nook_empty(
            "book",
            "Choose a vault in Settings. openNook reads and writes Markdown in that folder.",
        ));
    };

    let vault_label = nook_core::obsidian::vault_name(&vault_path);
    let today_n = notes_modified_today(&notes);
    let today = CivilDate::today();
    let stamp = format!("{:04}-{:02}-{:02}", today.year, today.month, today.day);

    let mut body = div()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.))
        .justify_center();

    let today_sub = if today_n == 1 {
        "1 note today".into()
    } else {
        format!("{today_n} notes today")
    };
    body = body.child(obs_row(
        SharedString::from("obs-vault"),
        vault_label,
        today_sub,
        false,
        cx,
        None,
    ));

    if let Some(daily) = notes.iter().find(|n| {
        n.rel_path.contains(&stamp) || n.title.contains(&stamp) || n.title.eq_ignore_ascii_case("daily")
    }) {
        let rel = daily.rel_path.clone();
        body = body.child(obs_row(
            SharedString::from(format!("obs-{}", daily.rel_path)),
            format!("Daily · {stamp}"),
            edited_ago(daily.mtime),
            true,
            cx,
            Some(rel),
        ));
    } else if let Some(note) = notes.first() {
        let rel = note.rel_path.clone();
        body = body.child(obs_row(
            SharedString::from(format!("obs-{}", note.rel_path)),
            note.title.clone(),
            edited_ago(note.mtime),
            true,
            cx,
            Some(rel),
        ));
    } else {
        body = body.child(nook_empty("book", "No markdown notes"));
    }

    pane = pane.child(body).child(
        div()
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(nook_icon_btn(
                "plus",
                "obs-capture-btn",
                cx,
                |this, _, window, cx| {
                    this.focus_obsidian_capture(window, cx);
                },
            ))
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
    );

    if capturing || !capture.is_empty() || flash.is_some() {
        pane = pane.child(
            div()
                .absolute()
                .left(px(16.))
                .right(px(16.))
                .bottom(px(8.))
                .child(capture_field(
                    &capture,
                    capturing,
                    flash.as_deref(),
                    &focus,
                    cx,
                )),
        );
    }

    pane
}

fn card_shell(id: impl Into<gpui::ElementId>) -> gpui::Stateful<gpui::Div> {
    nook_pane(id).p(px(16.)).gap(px(10.))
}

fn obs_row(
    id: SharedString,
    title: String,
    subtitle: String,
    openable: bool,
    cx: &mut Context<Island>,
    rel: Option<String>,
) -> impl IntoElement {
    div()
        .id(id)
        .w_full()
        .h(px(ROW_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .overflow_hidden()
        .when(openable, |d| {
            d.cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(theme::FILL))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        if let Some(rel) = rel.as_ref() {
                            this.open_obsidian_note(rel, cx);
                        }
                    }),
                )
        })
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .flex_shrink_0()
                .bg(theme::TERTIARY_LABEL),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(1.))
                .overflow_hidden()
                .child(
                    label(title, theme::CALLOUT, false)
                        .text_color(theme::LABEL)
                        .overflow_hidden()
                        .text_ellipsis(),
                )
                .child(
                    label(subtitle, theme::FOOTNOTE, false)
                        .text_color(theme::TERTIARY_LABEL)
                        .overflow_hidden()
                        .text_ellipsis(),
                ),
        )
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
        .flex_shrink_0()
        .h(px(theme::HIT_MIN))
        .px(px(8.))
        .rounded(px(8.))
        .bg(theme::FILL_TERTIARY)
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
                .text_size(px(theme::CALLOUT.size))
                .line_height(px(theme::CALLOUT.leading))
                .text_color(if placeholder {
                    theme::TERTIARY_LABEL
                } else {
                    theme::LABEL
                })
                .child(SharedString::from(shown)),
        )
}

fn notes_modified_today(notes: &[NoteEntry]) -> usize {
    notes.iter().filter(|n| mtime_is_today(n.mtime)).count()
}

fn mtime_is_today(mtime: SystemTime) -> bool {
    let Ok(elapsed) = SystemTime::now().duration_since(mtime) else {
        return false;
    };
    elapsed.as_secs() < 86_400
}

fn edited_ago(mtime: SystemTime) -> String {
    let secs = SystemTime::now()
        .duration_since(mtime)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if secs < 60 {
        "edited just now".into()
    } else if secs < 3600 {
        format!("edited {}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("edited {}h ago", secs / 3600)
    } else {
        let days = secs / 86_400;
        if days == 1 {
            "edited 1d ago".into()
        } else {
            format!("edited {days}d ago")
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn open_url_uses_core_builder() {
        let url = nook_core::obsidian::open_file_url("Vault", "a/b.md");
        assert_eq!(url, "obsidian://open?vault=Vault&file=a/b");
    }
}
