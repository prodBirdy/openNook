//! Incoming iMessage / WhatsApp lockup with a quick reply.
//!
//! The pane is a communication-style Live Activity: it only appears when a
//! message arrives, shows the sender and snippet, and lets you reply without
//! opening the host app.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_empty, nook_pane};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, rgba, Context, CursorStyle, FocusHandle, FontWeight, KeyDownEvent,
    MouseButton, MouseDownEvent, SharedString,
};
use nook_core::messages::{FdaStatus, IncomingPeek, MessageService};
use nook_core::notifications::relative_age;
use std::time::{SystemTime, UNIX_EPOCH};

const AVATAR: f32 = 36.0;
const REPLY_H: f32 = 28.0;
const SEND: f32 = theme::HIT_MIN;
const CARET_W: f32 = 1.5;
const CARET_H: f32 = 14.0;
const BADGE: f32 = 14.0;

pub(crate) fn messages_card(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    if island.message_focus.is_none() {
        island.message_focus = Some(cx.focus_handle());
    }
    let snap = &island.messages;
    let draft = island.message_draft.clone();
    let focus = island.message_focus.clone();

    match snap.fda {
        FdaStatus::Denied => nook_pane("nook-messages")
            .w_full()
            .child(
                div()
                    .id("msg-fda")
                    .flex_1()
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let _ = nook_core::messages::open_fda_settings();
                        }),
                    )
                    .child(nook_empty("message-circle", "Grant Full Disk Access")),
            )
            .into_any_element(),
        FdaStatus::Unavailable => nook_pane("nook-messages")
            .w_full()
            .child(nook_empty("message-circle", "Messages on this Mac"))
            .into_any_element(),
        FdaStatus::Granted => match snap.incoming.clone() {
            Some(peek) => incoming_card(peek, &draft, focus, cx).into_any_element(),
            None => div().into_any_element(),
        },
    }
}

fn incoming_card(
    peek: IncomingPeek,
    draft: &str,
    focus: Option<FocusHandle>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let age = peek_age(peek.last_date);
    let snippet = if peek.snippet.is_empty() {
        SharedString::from("Attachment")
    } else {
        SharedString::from(peek.snippet.clone())
    };
    let send_tint = service_tint(peek.service);
    let empty = draft.is_empty();
    let shown = if empty { "Reply" } else { draft };

    nook_pane("nook-messages")
        .w_full()
        .justify_between()
        .child(
            div()
                .w_full()
                .flex()
                .items_start()
                .gap(px(10.))
                .child(avatar(&peek.sender, peek.service, AVATAR, true))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(8.))
                                .child(
                                    label(peek.sender.clone(), theme::BODY, true)
                                        .flex_1()
                                        .min_w(px(0.)),
                                )
                                .child(label(age, theme::SUBHEADLINE, false).flex_shrink_0()),
                        )
                        .child(
                            div()
                                .text_color(theme::LABEL)
                                .text_size(px(theme::CALLOUT.size))
                                .line_height(px(theme::CALLOUT.leading))
                                .font_weight(FontWeight::MEDIUM)
                                .line_clamp(2)
                                .child(snippet),
                        ),
                ),
        )
        .child(reply_row(shown, empty, focus, send_tint, cx))
}

fn reply_row(
    shown: &str,
    empty: bool,
    focus: Option<FocusHandle>,
    send_tint: gpui::Rgba,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let mut row = div()
        .id("msg-reply")
        .w_full()
        .pt(px(8.))
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
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        dismiss_incoming(this, cx);
                        return;
                    }
                    if event.keystroke.key == "enter" {
                        send_incoming(this, cx);
                        return;
                    }
                    if apply_draft_key(&mut this.message_draft, event, cx) {
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
                        .child(
                            div()
                                .w(px(CARET_W))
                                .h(px(CARET_H))
                                .ml(px(1.))
                                .flex_shrink_0()
                                .rounded(px(1.))
                                .bg(theme::accent()),
                        ),
                ),
        );
    }

    row.child(send_btn(empty, send_tint, cx))
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

fn avatar(name: &str, service: MessageService, size: f32, badge: bool) -> impl IntoElement {
    let initials = sender_initials(name);
    let face = div()
        .size(px(size))
        .rounded_full()
        .bg(theme::FILL_SECONDARY)
        .border_1()
        .border_color(rgba(0xffffff1A))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(px((size * 0.38).max(10.0)))
                .line_height(px((size * 0.42).max(12.0)))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child(SharedString::from(initials)),
        );

    if !badge {
        return face.into_any_element();
    }

    div()
        .relative()
        .size(px(size))
        .flex_shrink_0()
        .child(face)
        .child(
            div()
                .absolute()
                .bottom(px(0.))
                .right(px(0.))
                .size(px(BADGE))
                .rounded_full()
                .bg(service_tint(service))
                .border_2()
                .border_color(theme::ISLAND)
                .flex()
                .items_center()
                .justify_center()
                .child(lucide_color("message-circle", 8.0, theme::LABEL)),
        )
        .into_any_element()
}

fn service_tint(service: MessageService) -> gpui::Rgba {
    match service {
        MessageService::IMessage => theme::accent(),
        MessageService::WhatsApp => theme::SUCCESS,
        MessageService::Sms => theme::SYSTEM_ORANGE,
    }
}

fn sender_initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .filter_map(|part| part.chars().find(|c| c.is_alphabetic()))
        .take(2)
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .collect();
    if !letters.is_empty() {
        return letters;
    }
    let digits: String = name.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 2 {
        return digits[digits.len() - 2..].to_string();
    }
    name.chars()
        .next()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "·".into())
}

fn peek_age(last_date: f64) -> String {
    if last_date < 1_000_000.0 {
        return "now".into();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    relative_age(last_date as i64, now)
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
    island.message_draft.clear();
    island.messages.incoming = None;
    let auto = island.settings.experimental_whatsapp_autosend;
    let id = conv.id.clone();
    let rowid = conv.last_rowid;
    nook_core::runtime().spawn(async move {
        let result = match conv.service {
            MessageService::WhatsApp => {
                let phone = conv.handle.as_deref().unwrap_or(&conv.title);
                nook_core::messages::reply_whatsapp(phone, &text, auto)
            }
            MessageService::IMessage | MessageService::Sms => {
                let Some(guid) = conv.chat_guid.as_deref() else {
                    return;
                };
                nook_core::messages::send_imessage(guid, &text)
            }
        };
        if let Err(err) = result {
            log::warn!("messages send: {err}");
        } else {
            nook_core::messages::mark_conversation_seen(&id, rowid);
            nook_core::messages::request_refresh();
        }
    });
    cx.notify();
}

fn dismiss_incoming(island: &mut Island, cx: &mut Context<Island>) {
    let Some(peek) = island.messages.incoming.take() else {
        return;
    };
    island.message_draft.clear();
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
            *draft = text.trim().to_string();
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

pub(crate) fn compact_left(peek: &IncomingPeek) -> impl IntoElement {
    avatar(&peek.sender, peek.service, theme::COMPACT_FACE, false)
}

pub(crate) fn compact_right(peek: &IncomingPeek) -> impl IntoElement {
    crate::island::ui::slide_label(peek.sender.clone(), theme::BODY, true)
}

#[cfg(test)]
mod tests {
    use super::sender_initials;

    #[test]
    fn initials_from_given_name() {
        assert_eq!(sender_initials("Carmen"), "C");
    }

    #[test]
    fn initials_from_two_words() {
        assert_eq!(sender_initials("Ada Lovelace"), "AL");
    }

    #[test]
    fn initials_from_phone_digits() {
        assert_eq!(sender_initials("+49 170 123456"), "56");
    }
}
