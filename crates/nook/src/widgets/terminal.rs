//! Termi-Notch expanded card: command input, mono scrollback, exit chip.
//!
//! Commands are typed here only. The URL scheme / CLI / Services paths never
//! call into `nook_core::shell`.

use crate::island::ui::{nook_icon_btn, nook_pane, scroll_body};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, rgba, Context, CursorStyle, FontWeight, KeyDownEvent, MouseButton,
    MouseDownEvent, SharedString, Window,
};

const MONO: &str = "SF Mono";

pub(crate) fn terminal_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let running = island.shell_running;
    let placeholder = island.shell_input.is_empty();
    let prompt = if placeholder {
        "type a command — never from a URL"
    } else {
        island.shell_input.as_str()
    };
    let chip = exit_chip(island);
    let body = if island.shell_output.is_empty() && !running {
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::TERTIARY_LABEL)
                    .child("One-shot login shell. Opt-in in Settings."),
            )
            .into_any_element()
    } else {
        scroll_body(
            "term-scroll",
            div()
                .w_full()
                .font_family(MONO)
                .text_size(px(11.))
                .line_height(px(14.))
                .text_color(theme::LABEL)
                .whitespace_nowrap()
                .child(SharedString::from(island.shell_output.clone())),
        )
        .into_any_element()
    };

    nook_pane("nook-terminal")
        .w_full()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .flex_shrink_0()
                .child(command_field(prompt, placeholder, island, cx))
                .child(if running {
                    nook_icon_btn("x", "term-stop", cx, |this, _, _, cx| {
                        this.cancel_shell();
                        cx.notify();
                    })
                    .into_any_element()
                } else {
                    nook_icon_btn("play", "term-run", cx, |this, _, window, cx| {
                        this.run_typed_shell(window, cx);
                    })
                    .into_any_element()
                })
                .when_some(chip, |d, chip| d.child(chip)),
        )
        .child(body)
}

fn command_field(
    prompt: &str,
    placeholder: bool,
    island: &Island,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let focus = island.shell_focus.clone();
    let mut field = div()
        .id("term-input")
        .flex_1()
        .min_w(px(0.))
        .h(px(26.))
        .px(px(8.))
        .rounded(px(6.))
        .bg(theme::FILL)
        .flex()
        .items_center()
        .cursor(CursorStyle::IBeam)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.shell_focused = true;
                if let Some(focus) = this.shell_focus.clone() {
                    window.focus(&focus);
                }
                cx.notify();
            }),
        )
        .on_key_down(cx.listener(Island::on_shell_key))
        .child(
            div()
                .w_full()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .font_family(MONO)
                .text_size(px(12.))
                .text_color(if placeholder {
                    theme::TERTIARY_LABEL
                } else {
                    theme::LABEL
                })
                .child(SharedString::from(prompt.to_string())),
        );
    if let Some(focus) = focus {
        field = field.track_focus(&focus);
    }
    field
}

fn exit_chip(island: &Island) -> Option<impl IntoElement> {
    if island.shell_running {
        return Some(
            div()
                .px(px(8.))
                .h(px(20.))
                .rounded_full()
                .bg(rgba(0xffffff18))
                .flex()
                .items_center()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::SECONDARY_LABEL)
                        .child("running"),
                ),
        );
    }
    let code = island.shell_exit?;
    let ok = code == 0;
    Some(
        div()
            .px(px(8.))
            .h(px(20.))
            .rounded_full()
            .bg(if ok {
                rgba(0x30d15833)
            } else {
                rgba(0xff453a33)
            })
            .flex()
            .items_center()
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if ok { theme::SUCCESS } else { theme::DESTRUCTIVE })
                    .child(format!("exit {code}")),
            ),
    )
}

impl Island {
    pub(crate) fn on_shell_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.settings.terminal_enabled {
            return;
        }
        let ks = &event.keystroke;
        if ks.modifiers.secondary() && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.shell_input.push_str(text.trim_end());
                cx.notify();
            }
            return;
        }
        if ks.modifiers.platform || ks.modifiers.control {
            return;
        }
        match ks.key.as_str() {
            "enter" => self.run_typed_shell(_window, cx),
            "escape" => {
                if self.shell_running {
                    self.cancel_shell();
                    cx.notify();
                }
            }
            "backspace" => {
                self.shell_input.pop();
                cx.notify();
            }
            "up" => {
                self.history_step(-1);
                cx.notify();
            }
            "down" => {
                self.history_step(1);
                cx.notify();
            }
            _ => {
                if let Some(ch) = &ks.key_char {
                    if !ch.chars().any(|c| c.is_control()) {
                        self.shell_input.push_str(ch);
                        cx.notify();
                    }
                }
            }
        }
    }
}
