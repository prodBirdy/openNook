//! Messages Nook pane — recent iMessage / WhatsApp threads and inline reply.

use crate::icons::lucide_color;
use crate::island::ui::{nook_empty, nook_icon_btn, nook_pane, nook_row};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, rgba, Context, CursorStyle, FocusHandle, FontWeight, KeyDownEvent,
    MouseButton, MouseDownEvent, SharedString,
};
use nook_core::messages::{
    relative_time, sender_avatar_rgb, sender_initials, Conversation, FdaStatus, IncomingPeek,
    MessageService, MessagesSnapshot,
};

const PEEK_AVATAR: f32 = 36.0;
const PEEK_SEND: f32 = 28.0;
const WA_GREEN: u32 = 0x25D366FF;
const IMESSAGE_BLUE: u32 = 0x0A84FFFF;

pub(crate) fn messages_card(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    if island.message_focus.is_none() {
        island.message_focus = Some(cx.focus_handle());
    }
    let snap = &island.messages;
    let selected = island.selected_conversation.clone();
    let draft = island.message_draft.clone();
    let focus = island.message_focus.clone();
    let count = snap
        .conversations
        .iter()
        .filter(|c| c.unread)
        .count();

    let body = match snap.fda {
        FdaStatus::Denied => div()
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
            .child(nook_empty("message-circle", "Grant Full Disk Access"))
            .into_any_element(),
        FdaStatus::Unavailable => div()
            .id("msg-unavailable")
            .flex_1()
            .child(nook_empty("message-circle", "Messages on this Mac"))
            .into_any_element(),
        FdaStatus::Granted if snap.conversations.is_empty() => div()
            .id("msg-empty")
            .flex_1()
            .child(nook_empty("message-circle", "No recent messages"))
            .into_any_element(),
        FdaStatus::Granted => {
            let mut list = div().flex().flex_col().flex_1();
            for conv in snap.conversations.iter().take(2) {
                list = list.child(conversation_row(conv, selected.as_deref(), cx));
            }
            list.child(composer(selected.as_deref(), &draft, focus, snap, cx))
                .into_any_element()
        }
    };

    nook_pane("nook-messages")
        .w_full()
        .when(count > 0, |d| {
            d.child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .flex_shrink_0()
                    .child(
                        div()
                            .text_size(px(32.))
                            .line_height(px(36.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::LABEL)
                            .child(count.to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::TERTIARY_LABEL)
                            .pb(px(4.))
                            .child("unread"),
                    ),
            )
        })
        .child(body)
}

fn conversation_row(
    conv: &Conversation,
    selected: Option<&str>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let id = conv.id.clone();
    let rowid = conv.last_rowid;
    let active = selected == Some(id.as_str());
    let service = conv.service;
    nook_row(SharedString::from(format!("msg-{id}")))
        .when(active, |d| d.bg(rgba(0xffffff12)))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.selected_conversation = Some(id.clone());
                nook_core::messages::mark_conversation_seen(&id, rowid);
                if let Some(c) = this
                    .messages
                    .conversations
                    .iter_mut()
                    .find(|c| c.id == id)
                {
                    c.unread = false;
                }
                if this
                    .messages
                    .incoming
                    .as_ref()
                    .is_some_and(|p| p.conversation_id == id)
                {
                    this.messages.incoming = None;
                }
                if service == MessageService::WhatsApp {
                    this.message_draft.clear();
                }
                cx.notify();
            }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .justify_center()
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::LABEL)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(conv.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(if conv.service == MessageService::IMessage {
                                    rgba(0x5AC8FAFF)
                                } else if conv.service == MessageService::WhatsApp {
                                    rgba(0x25D366FF)
                                } else {
                                    theme::TERTIARY_LABEL
                                })
                                .child(conv.service.label()),
                        )
                        .when(conv.unread, |d| {
                            d.child(
                                div()
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(theme::LABEL)
                                    .flex_shrink_0(),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgba(0xffffff80))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(if conv.snippet.is_empty() {
                            SharedString::from("Attachment")
                        } else {
                            SharedString::from(conv.snippet.clone())
                        }),
                ),
        )
}

fn composer(
    selected: Option<&str>,
    draft: &str,
    focus: Option<FocusHandle>,
    snap: &MessagesSnapshot,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let Some(id) = selected else {
        return div()
            .pt(px(8.))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::TERTIARY_LABEL)
                    .child("Select a thread to reply"),
            )
            .into_any_element();
    };
    let Some(conv) = snap.conversations.iter().find(|c| c.id == id) else {
        return div().into_any_element();
    };
    let whatsapp = conv.service == MessageService::WhatsApp;
    let placeholder = if whatsapp {
        "Prefill WhatsApp…"
    } else {
        "Reply…"
    };
    let empty = draft.is_empty();
    let shown = if empty { placeholder } else { draft };
    let mut row = div()
        .id("msg-composer")
        .pt(px(8.))
        .flex()
        .items_center()
        .gap(px(6.));
    if let Some(focus) = focus {
        let focus_input = focus.clone();
        row = row.child(
            div()
                .id("msg-draft")
                .track_focus(&focus)
                .flex_1()
                .min_w(px(0.))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(6.))
                .bg(rgba(0xffffff14))
                .flex()
                .items_center()
                .cursor(CursorStyle::IBeam)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.focus(&focus_input);
                        cx.notify();
                    }),
                )
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "enter" {
                        send_selected(this, cx);
                        return;
                    }
                    if apply_draft_key(&mut this.message_draft, event, cx) {
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .w_full()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(px(12.))
                        .text_color(if empty {
                            theme::TERTIARY_LABEL
                        } else {
                            theme::LABEL
                        })
                        .child(SharedString::from(shown.to_string())),
                ),
        );
    }
    row.child(nook_icon_btn(
        if whatsapp { "upload" } else { "send" },
        "msg-send",
        cx,
        |this, _, _, cx| send_selected(this, cx),
    ))
    .into_any_element()
}

fn send_selected(island: &mut Island, cx: &mut Context<Island>) {
    let Some(id) = island.selected_conversation.clone() else {
        return;
    };
    let Some(conv) = island
        .messages
        .conversations
        .iter()
        .find(|c| c.id == id)
        .cloned()
    else {
        return;
    };
    send_conversation(island, conv, cx);
}

fn send_incoming(island: &mut Island, cx: &mut Context<Island>) {
    let Some(id) = island
        .messages
        .incoming
        .as_ref()
        .map(|p| p.conversation_id.clone())
    else {
        send_selected(island, cx);
        return;
    };
    let Some(conv) = island
        .messages
        .conversations
        .iter()
        .find(|c| c.id == id)
        .cloned()
    else {
        return;
    };
    send_conversation(island, conv, cx);
}

fn send_conversation(island: &mut Island, conv: Conversation, cx: &mut Context<Island>) {
    let text = island.message_draft.trim().to_string();
    if text.is_empty() {
        return;
    }
    island.message_draft.clear();
    let id = conv.id.clone();
    let auto = island.settings.experimental_whatsapp_autosend;
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
            nook_core::messages::mark_conversation_seen(&id, conv.last_rowid);
            nook_core::messages::request_refresh();
        }
    });
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

pub(crate) fn compact_left(_peek: &IncomingPeek) -> impl IntoElement {
    crate::icons::lucide("message-circle", theme::COMPACT_FACE)
}

pub(crate) fn compact_right(peek: &IncomingPeek) -> impl IntoElement {
    crate::island::ui::slide_label(peek.sender.clone(), theme::BODY, true)
}

/// Droppy-style incoming HUD: avatar, sender + time, snippet, inline Reply.
pub(crate) fn messages_peek(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let Some(peek) = island.messages.incoming.as_ref() else {
        return div().into_any_element();
    };
    let notch_h = island.notch_height.max(32.0);
    let now = unix_now();
    let when = relative_time(peek.last_date, now);
    let whatsapp = peek.service == MessageService::WhatsApp;
    let send_fill = if whatsapp { WA_GREEN } else { IMESSAGE_BLUE };
    let draft = island.message_draft.clone();
    let focus = island.message_focus.clone();
    let empty = draft.is_empty();
    let shown = if empty { "Reply…" } else { draft.as_str() };

    div()
        .id("msg-peek")
        .size_full()
        .flex()
        .flex_col()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.activate_message_composer(window, cx);
            }),
        )
        .child(div().h(px(notch_h)).w_full().flex_shrink_0())
        .child(
            div()
                .flex_1()
                .w_full()
                .px(px(14.))
                .pb(px(10.))
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(8.))
                .child(peek_header(peek, &when))
                .child(peek_composer(
                    focus,
                    shown,
                    empty,
                    send_fill,
                    cx,
                )),
        )
        .into_any_element()
}

fn peek_header(peek: &IncomingPeek, when: &str) -> impl IntoElement {
    let snippet = if peek.snippet.is_empty() {
        SharedString::from("Attachment")
    } else {
        SharedString::from(peek.snippet.clone())
    };
    div()
        .flex()
        .items_start()
        .gap(px(10.))
        .child(peek_avatar(&peek.sender, peek.service))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .justify_center()
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(6.))
                        .child(
                            div()
                                .min_w(px(0.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::LABEL)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(peek.sender.clone()),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(11.))
                                .line_height(px(14.))
                                .text_color(theme::TERTIARY_LABEL)
                                .child(SharedString::from(when.to_string())),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(15.))
                        .h(px(30.))
                        .text_color(rgba(0xffffffB3))
                        .overflow_hidden()
                        .child(snippet),
                ),
        )
}

fn peek_avatar(sender: &str, service: MessageService) -> impl IntoElement {
    let initials = sender_initials(sender);
    let fill = theme::rgba_from_u32(sender_avatar_rgb(sender), 1.0);
    let badge = match service {
        MessageService::WhatsApp => WA_GREEN,
        MessageService::IMessage => IMESSAGE_BLUE,
        MessageService::Sms => 0x30D158FF,
    };
    div()
        .relative()
        .size(px(PEEK_AVATAR))
        .flex_shrink_0()
        .child(
            div()
                .size_full()
                .rounded_full()
                .bg(fill)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::LABEL)
                        .child(SharedString::from(initials)),
                ),
        )
        .child(
            div()
                .absolute()
                .bottom(px(-1.))
                .right(px(-1.))
                .size(px(14.))
                .rounded_full()
                .bg(rgba(badge))
                .border_1()
                .border_color(rgba(0x000000FF))
                .flex()
                .items_center()
                .justify_center()
                .child(lucide_color("message-circle", 8.0, theme::LABEL)),
        )
}

fn peek_composer(
    focus: Option<FocusHandle>,
    shown: &str,
    empty: bool,
    send_fill: u32,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let shown = SharedString::from(shown.to_string());
    let mut row = div()
        .id("msg-peek-composer")
        .flex()
        .items_center()
        .gap(px(8.));
    if let Some(focus) = focus {
        let focus_input = focus.clone();
        row = row.child(
            div()
                .id("msg-peek-draft")
                .track_focus(&focus)
                .flex_1()
                .min_w(px(0.))
                .h(px(PEEK_SEND))
                .px(px(12.))
                .rounded_full()
                .bg(rgba(0xffffff18))
                .flex()
                .items_center()
                .cursor(CursorStyle::IBeam)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.activate_message_composer(window, cx);
                        window.focus(&focus_input);
                    }),
                )
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
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
                        .w_full()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(px(12.))
                        .text_color(if empty {
                            theme::TERTIARY_LABEL
                        } else {
                            theme::LABEL
                        })
                        .child(shown),
                ),
        );
    }
    row.child(
        div()
            .id("msg-peek-send")
            .size(px(PEEK_SEND))
            .rounded_full()
            .bg(rgba(send_fill))
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    send_incoming(this, cx);
                }),
            )
            .child(lucide_color("send", 14.0, theme::LABEL)),
    )
    .into_any_element()
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
