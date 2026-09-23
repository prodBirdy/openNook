//! Incoming iMessage / WhatsApp lockup with a quick reply.
//!
//! Expanded face matches the Pencil list mockup (dot + sender + snippet).
//! When a peek is active, the reply row stays so send/dismiss still work.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_empty, nook_pane, open_privacy_pane, text_btn};
use crate::island::Island;
use crate::theme;
use gpui::{
    canvas, div, prelude::*, px, Context, CursorStyle, FocusHandle, FontWeight, KeyDownEvent,
    MouseButton, MouseDownEvent, SharedString,
};
use nook_core::messages::{Conversation, FdaStatus, IncomingPeek, MessageService};
use std::cell::RefCell;

thread_local! {
    static SEND_FAILED: RefCell<bool> = const { RefCell::new(false) };
}

const ROW_H: f32 = 29.0;
const REPLY_H: f32 = 28.0;
const SEND: f32 = theme::HIT_MIN;
const CARET_W: f32 = 1.5;
const CARET_H: f32 = 14.0;

pub(crate) fn messages_card(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    if island.message_focus.is_none() {
        island.message_focus = Some(cx.focus_handle());
    }
    let snap = &island.messages;
    let draft = island.message_draft.clone();
    let focus = island.message_focus.clone();

    match snap.fda {
        FdaStatus::Denied => card_shell("nook-messages")
            .w_full()
            .child(
                div()
                    .id("msg-fda")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(theme::BODY.size))
                            .line_height(px(theme::BODY.leading))
                            .font_weight(theme::BODY.emphasized)
                            .text_color(theme::LABEL)
                            .child("Messages needs Full Disk Access"),
                    )
                    .child(
                        div()
                            .text_size(px(theme::FOOTNOTE.size))
                            .line_height(px(theme::FOOTNOTE.leading))
                            .text_color(theme::SECONDARY_LABEL)
                            .child("Reads your local iMessage database. Nothing leaves this Mac."),
                    )
                    .child(text_btn("Open Privacy Settings", cx, |_, _, _| {
                        open_privacy_pane("Privacy_AllFiles");
                    })),
            )
            .into_any_element(),
        FdaStatus::Unavailable => card_shell("nook-messages")
            .w_full()
            .child(nook_empty(
                "message-circle",
                "No Messages database on this Mac",
            ))
            .into_any_element(),
        FdaStatus::Granted => {
            let incoming = snap.incoming.clone();
            let conversations = snap.conversations.clone();
            if conversations.is_empty() && incoming.is_none() {
                return card_shell("nook-messages")
                    .w_full()
                    .child(nook_empty("message-circle", "No new messages"))
                    .into_any_element();
            }
            list_card(conversations, incoming, &draft, focus, cx).into_any_element()
        }
    }
}

fn list_card(
    conversations: Vec<Conversation>,
    incoming: Option<IncomingPeek>,
    draft: &str,
    focus: Option<FocusHandle>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let send_failed = SEND_FAILED.with(|f| *f.borrow());
    let empty = draft.is_empty();
    let shown = if empty { "Reply" } else { draft };
    let send_tint = incoming
        .as_ref()
        .map(|p| service_tint(p.service))
        .unwrap_or_else(theme::accent);

    let mut body = div()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.))
        .justify_center();

    if let Some(peek) = incoming.as_ref() {
        let snippet = if peek.snippet.is_empty() {
            "Attachment".into()
        } else {
            peek.snippet.clone()
        };
        body = body.child(msg_row(
            SharedString::from(format!("msg-peek-{}", peek.conversation_id)),
            peek.sender.clone(),
            snippet,
            true,
            None,
            cx,
        ));
    } else {
        for conv in conversations.iter().take(2) {
            let snippet = if conv.snippet.is_empty() {
                "Attachment".into()
            } else {
                conv.snippet.clone()
            };
            body = body.child(msg_row(
                SharedString::from(format!("msg-{}", conv.id)),
                conv.title.clone(),
                snippet,
                conv.unread,
                Some((conv.id.clone(), conv.last_rowid)),
                cx,
            ));
        }
    }

    card_shell("nook-messages").w_full().child(body).when(
        incoming.is_some(),
        |d| d.child(reply_row(shown, empty, focus, send_tint, send_failed, cx)),
    )
}

fn msg_row(
    id: SharedString,
    title: String,
    snippet: String,
    unread: bool,
    mark_seen: Option<(String, i64)>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    div()
        .id(id)
        .w_full()
        .h(px(ROW_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .when_some(mark_seen, |d, (conv_id, rowid)| {
            d.cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(theme::FILL))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        nook_core::messages::mark_conversation_seen(&conv_id, rowid);
                        nook_core::messages::request_refresh();
                        cx.notify();
                    }),
                )
        })
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .flex_shrink_0()
                .bg(if unread {
                    theme::accent()
                } else {
                    theme::TERTIARY_LABEL
                }),
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
                    label(snippet, theme::FOOTNOTE, false)
                        .text_color(theme::TERTIARY_LABEL)
                        .overflow_hidden()
                        .text_ellipsis(),
                ),
        )
}

fn card_shell(id: impl Into<gpui::ElementId>) -> gpui::Stateful<gpui::Div> {
    nook_pane(id).p(px(16.)).gap(px(10.))
}

fn reply_row(
    shown: &str,
    empty: bool,
    focus: Option<FocusHandle>,
    send_tint: gpui::Rgba,
    send_failed: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let mut row = div()
        .id("msg-reply")
        .w_full()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(8.));

    if let Some(focus) = focus {
        let focus_input = focus.clone();
        row = row.child(
            div()
                .id("msg-draft")
                .track_focus(&focus)
                .flex_1()
                .min_w(px(0.))
                .h(px(REPLY_H))
                .px(px(12.))
                .rounded(px(REPLY_H / 2.0))
                .bg(theme::FILL)
                .flex()
                .items_center()
                .overflow_hidden()
                .cursor(CursorStyle::IBeam)
                .hover(|s| s.bg(theme::FILL_SECONDARY))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.focus(&focus_input);
                        cx.notify();
                    }),
                )
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        if !this.message_draft.is_empty() {
                            this.message_draft.clear();
                            SEND_FAILED.with(|f| *f.borrow_mut() = false);
                            cx.notify();
                        } else if this.messages.incoming.is_some() {
                            dismiss_incoming(this, cx);
                        } else {
                            window.blur();
                        }
                        cx.stop_propagation();
                        return;
                    }
                    if event.keystroke.key == "enter" {
                        send_incoming(this, cx);
                        return;
                    }
                    if apply_draft_key(&mut this.message_draft, event, cx) {
                        SEND_FAILED.with(|f| *f.borrow_mut() = false);
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .child(
                            div()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(px(theme::BODY.size))
                                .line_height(px(theme::BODY.leading))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if empty {
                                    theme::TERTIARY_LABEL
                                } else {
                                    theme::LABEL
                                })
                                .child(SharedString::from(shown.to_string())),
                        )
                        .child({
                            let focus_caret = focus.clone();
                            canvas(
                                |_, _, _| {},
                                move |bounds, _, window, _| {
                                    if focus_caret.is_focused(window) {
                                        window.paint_quad(gpui::fill(bounds, theme::accent()));
                                    }
                                },
                            )
                            .w(px(CARET_W))
                            .h(px(CARET_H))
                            .ml(px(1.))
                            .flex_shrink_0()
                            .rounded(px(1.))
                        }),
                ),
        );
    }

    row.when(send_failed, |d| {
        d.child(
            label("Couldn't send", theme::FOOTNOTE, false)
                .text_color(theme::DESTRUCTIVE)
                .flex_shrink_0(),
        )
    })
    .child(send_btn(empty, send_tint, cx))
}

fn send_btn(empty: bool, tint: gpui::Rgba, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("msg-send")
        .size(px(SEND))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(tint)
        .opacity(if empty { 0.4 } else { 1.0 })
        .when(!empty, |d| {
            d.hover(|s| s.opacity(0.92))
                .active(|s| s.opacity(0.8))
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        send_incoming(this, cx);
                    }),
                )
        })
        .child(lucide_color("send", 13.0, theme::LABEL))
}

fn service_tint(service: MessageService) -> gpui::Rgba {
    match service {
        MessageService::IMessage => theme::accent(),
        MessageService::WhatsApp => theme::SUCCESS,
        MessageService::Sms => theme::SYSTEM_ORANGE,
    }
}

fn send_incoming(island: &mut Island, cx: &mut Context<Island>) {
    let Some(peek) = island.messages.incoming.clone() else {
        return;
    };
    let Some(conv) = island
        .messages
        .conversations
        .iter()
        .find(|c| c.id == peek.conversation_id)
        .cloned()
    else {
        return;
    };
    let text = island.message_draft.trim().to_string();
    if text.is_empty() {
        return;
    }
    SEND_FAILED.with(|f| *f.borrow_mut() = false);
    let auto = island.settings.experimental_whatsapp_autosend;
    let id = conv.id.clone();
    let rowid = conv.last_rowid;
    let service = conv.service;
    let phone = conv.handle.clone().unwrap_or_else(|| conv.title.clone());
    let chat_guid = conv.chat_guid.clone();
    cx.spawn(async move |this, cx| {
        let result = cx
            .background_executor()
            .spawn(async move {
                match service {
                    MessageService::WhatsApp => {
                        nook_core::messages::reply_whatsapp(&phone, &text, auto)
                    }
                    MessageService::IMessage | MessageService::Sms => {
                        let Some(guid) = chat_guid.as_deref() else {
                            return Err("No chat GUID".into());
                        };
                        nook_core::messages::send_imessage(guid, &text)
                    }
                }
            })
            .await;
        let _ = this.update(cx, |this, cx| {
            match result {
                Ok(()) => {
                    this.message_draft.clear();
                    this.messages.incoming = None;
                    SEND_FAILED.with(|f| *f.borrow_mut() = false);
                    nook_core::messages::mark_conversation_seen(&id, rowid);
                    nook_core::messages::request_refresh();
                }
                Err(err) => {
                    log::warn!("messages send: {err}");
                    SEND_FAILED.with(|f| *f.borrow_mut() = true);
                }
            }
            cx.notify();
        });
    })
    .detach();
    cx.notify();
}

fn dismiss_incoming(island: &mut Island, cx: &mut Context<Island>) {
    let Some(peek) = island.messages.incoming.take() else {
        return;
    };
    island.message_draft.clear();
    SEND_FAILED.with(|f| *f.borrow_mut() = false);
    if island.preferred == Some(crate::island::CompactMode::Messages) {
        island.preferred = None;
    }
    nook_core::messages::mark_conversation_seen(&peek.conversation_id, peek.last_rowid);
    nook_core::messages::request_refresh();
    cx.notify();
}

fn apply_draft_key(draft: &mut String, event: &KeyDownEvent, cx: &Context<Island>) -> bool {
    let ks = &event.keystroke;
    if ks.modifiers.secondary() && ks.key == "v" {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            draft.push_str(text.trim());
            return true;
        }
        return false;
    }
    if ks.modifiers.platform || ks.modifiers.control {
        return false;
    }
    match ks.key.as_str() {
        "backspace" => {
            draft.pop();
            true
        }
        _ => {
            if let Some(ch) = &ks.key_char {
                if !ch.chars().any(|c| c.is_control()) {
                    draft.push_str(ch);
                    return true;
                }
            }
            false
        }
    }
}

pub(crate) fn compact_left(_peek: &IncomingPeek) -> impl IntoElement {
    div()
        .size(px(22.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color(
            "message-circle",
            17.0,
            theme::secondary_label(),
        ))
}

pub(crate) fn compact_right(peek: &IncomingPeek) -> impl IntoElement {
    crate::island::ui::slide_label(peek.sender.clone(), theme::BODY, true)
}
