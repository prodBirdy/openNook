//! Compact Live Activity: left | notch gap | right, plus mode dots.

use super::media::{album_chip, visualizer};
use super::ui::{label, timer_text};
use super::{CompactMode, Island};
use crate::icons::lucide;
use crate::theme;
use crate::widgets;
use gpui::{
    div, prelude::*, px, AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent,
    SharedString,
};

impl Island {
    pub(super) fn render_compact(
        &self,
        mode: CompactMode,
        hovered: bool,
        notch_w: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Equal flex flanks keep the notch spacer on the camera; a fixed
        // side width plus px_3 used to overflow and shift the hole.
        div()
            .flex()
            .items_center()
            .justify_between()
            .size_full()
            .px_3()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_start()
                    .overflow_hidden()
                    .child(self.compact_left(mode, cx)),
            )
            .child(div().w(px(notch_w)).flex_shrink_0().h_full())
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .overflow_hidden()
                    .child(self.compact_right(mode, hovered, cx)),
            )
    }

    fn compact_left(&self, mode: CompactMode, cx: &mut Context<Self>) -> AnyElement {
        match mode {
            CompactMode::Media => {
                album_chip(&self.now_playing, self.overlay_fade.value, cx).into_any_element()
            }
            CompactMode::Agents => widgets::agents_compact_left(&self.agents, self.pixel_t),
            CompactMode::Files => super::files::compact_left(&self.files),
            CompactMode::Timer => widgets::timer_compact_left(self, cx),
            CompactMode::Observe => {
                lucide("triangle-alert", theme::COMPACT_FACE).into_any_element()
            }
            CompactMode::Onboard => label("openNook", theme::BODY, true).into_any_element(),
            CompactMode::Idle => div().into_any_element(),
        }
    }

    fn compact_right(
        &self,
        mode: CompactMode,
        hovered: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match mode {
            CompactMode::Media => visualizer(
                self.now_playing
                    .audio_levels
                    .as_deref()
                    .unwrap_or(&[0.2; 6]),
                self.now_playing.is_playing,
                self.visualizer_color,
            )
            .into_any_element(),
            CompactMode::Agents => widgets::agents_compact_right(&self.agents),
            CompactMode::Files => {
                label(self.files.len().to_string(), theme::BODY, true).into_any_element()
            }
            CompactMode::Timer => {
                let text = self
                    .face_timer()
                    .map(|t| super::ui::format_timer_compact(t.remaining))
                    .unwrap_or_else(|| "0s".into());
                timer_text(text, theme::BODY)
                    .min_w(px(40.))
                    .text_right()
                    .into_any_element()
            }
            CompactMode::Observe => {
                let text = match self.observe.alerts.as_slice() {
                    [one] => one.name.clone(),
                    _ => self.observe.firing_count().to_string(),
                };
                label(text, theme::BODY, true).into_any_element()
            }
            CompactMode::Onboard if hovered => div()
                .id("github")
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let _ = std::process::Command::new("/usr/bin/open")
                            .arg("https://github.com/prodBirdy/openNook-gpui")
                            .spawn();
                    }),
                )
                .child(lucide("github", theme::COMPACT_FACE))
                .into_any_element(),
            _ => div().into_any_element(),
        }
    }

    pub(super) fn mode_dots(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let modes = self.available_modes();
        let current = self.mode();
        if modes.len() <= 1 {
            return div().into_any_element();
        }
        let mut row = div()
            .absolute()
            .bottom(px(2.))
            .left_0()
            .right_0()
            .flex()
            .gap(px(6.))
            .items_center()
            .justify_center();
        for mode in modes {
            let active = mode == current;
            let name = match mode {
                CompactMode::Idle => "idle",
                CompactMode::Media => "media",
                CompactMode::Agents => "agents",
                CompactMode::Files => "files",
                CompactMode::Timer => "timer",
                CompactMode::Observe => "observe",
                CompactMode::Onboard => "onboard",
            };
            row = row.child(
                div()
                    .id(SharedString::from(format!("dot-{name}")))
                    .h(px(theme::HIT_MIN.min(16.0)))
                    .w(px(if active { 16. } else { 12. }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.preferred = Some(mode);
                            nook_core::haptics::trigger(None);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .h(px(6.))
                            .w(px(if active { 12. } else { 6. }))
                            .rounded_full()
                            .bg(if active {
                                theme::LABEL
                            } else {
                                theme::TERTIARY_LABEL
                            }),
                    ),
            );
        }
        row.into_any_element()
    }
}
