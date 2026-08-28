//! Compact Live Activity: left | notch gap | right, plus mode dots.

use super::media::{album_chip, visualizer};
use super::ui::{label, timer_text};
use super::{CompactMode, Island};
use crate::icons::{lucide, lucide_color};
use crate::theme;
use crate::widgets;
use gpui::{
    div, prelude::*, px, relative, rgb, rgba, AnyElement, Context, CursorStyle, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, SharedString,
};
use nook_core::sysvol::HudKind;

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
                    .child(self.compact_left(mode, cx))
                    .when(mode != CompactMode::Idle && self.high_alert_active(), |d| {
                        d.child(
                            div()
                                .ml(px(4.))
                                .flex_shrink_0()
                                .child(lucide_color("sun", 10.0, theme::SUCCESS)),
                        )
                    }),
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
        if let Some(text) = nook_core::window_snap::flash_label() {
            return label(text, theme::BODY, true).into_any_element();
        if self.hud_active() {
            return lucide(hud_icon(self.hud.unwrap().kind), theme::COMPACT_FACE)
                .into_any_element();
        if let Some(hud) = self.shell_hud.as_ref() {
            return label(hud.clone(), theme::BODY, true).into_any_element();
        if let Some(name) = self.output_hud_label() {
            return label(name.to_string(), theme::BODY, true).into_any_element();
        }
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
            CompactMode::Battery => {
                let critical = self.power.percent.map(|p| p <= 10).unwrap_or(false)
                    || self.power.warning_level == nook_core::power::BatteryWarning::Final;
                let color = if critical {
                    theme::DESTRUCTIVE
                } else {
                    theme::SYSTEM_ORANGE
                };
                lucide_color(self.power.compact_icon(), theme::COMPACT_FACE, color)
                    .into_any_element()
            }
            CompactMode::Vpn => lucide_color(
                "shield",
                theme::COMPACT_FACE,
                if self.vpn.connected {
                    theme::SUCCESS
                } else {
                    theme::TERTIARY_LABEL
                },
            )
            .into_any_element(),
            CompactMode::Shell => lucide("terminal", theme::COMPACT_FACE).into_any_element(),
            CompactMode::Onboard => label("openNook", theme::BODY, true).into_any_element(),
            CompactMode::Messages => self
                .messages
                .incoming
                .as_ref()
                .map(widgets::messages_compact_left)
                .map(|el| el.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            CompactMode::Share => lucide("share", theme::COMPACT_FACE).into_any_element(),
            CompactMode::Idle => div().into_any_element(),
            CompactMode::Idle => widgets::compact_weather(self),
            CompactMode::Idle => {
                if self.high_alert_active() {
                    lucide_color("sun", 12.0, theme::SUCCESS).into_any_element()
                } else {
                    div().into_any_element()
                }
            }
        }
    }

    fn compact_right(
        &self,
        mode: CompactMode,
        hovered: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.hud_active() {
            return self.hud_slider(cx);
        }
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
            CompactMode::Battery => {
                label(nook_core::power::format_percent(self.power.percent), theme::BODY, true)
                    .into_any_element()
            }
            CompactMode::Idle if self.settings.thaw_enabled => thaw_toggle(
                self.settings.thaw_hidden,
                cx,
            )
            .into_any_element(),
            CompactMode::Messages => self
                .messages
                .incoming
                .as_ref()
                .map(widgets::messages_compact_right)
                .map(|el| el.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            CompactMode::Share => {
                label(self.share.compact_label(), theme::BODY, true).into_any_element()
            }
            CompactMode::Vpn => {
                let text = self
                    .vpn
                    .compact_right(self.settings.vpn_show_timer, std::time::SystemTime::now());
                timer_text(text, theme::BODY)
                    .min_w(px(40.))
                    .text_right()
                    .into_any_element()
            CompactMode::Shell => {
                let frame = ((self.pixel_t * 8.0) as usize) % 4;
                let spin = ["⠋", "⠙", "⠹", "⠸"][frame];
                label(spin, theme::BODY, true).into_any_element()
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

    fn hud_slider(&self, cx: &mut Context<Self>) -> AnyElement {
        const SEGMENTS: u32 = 20;
        let fill = self.hud_fill.value.clamp(0.0, 1.0);
        let mut hits = div()
            .absolute()
            .inset_0()
            .flex()
            .cursor(CursorStyle::PointingHand);
        for i in 0..SEGMENTS {
            let ratio = (i as f32 + 0.5) / SEGMENTS as f32;
            hits = hits.child(
                div()
                    .id(SharedString::from(format!("hud-seg-{i}")))
                    .flex_1()
                    .h_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.apply_hud_slider(ratio, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                        if this.hud_dragging {
                            cx.stop_propagation();
                            this.apply_hud_slider(ratio, cx);
                        }
                    })),
            );
        }
        div()
            .id("hud-slider")
            .w_full()
            .max_w(px(72.))
            .h(px(theme::HIT_MIN.min(18.0)))
            .flex()
            .items_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.end_hud_drag();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(4.))
                    .rounded(px(2.))
                    .bg(rgba(0xffffff26))
                    .child(
                        div()
                            .h_full()
                            .w(relative(fill))
                            .rounded(px(2.))
                            .bg(rgb(0xffffff)),
                    )
                    .child(hits),
            )
            .into_any_element()
    }

    pub(super) fn mode_dots(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.hud_active() {
            return div().into_any_element();
        }
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
                CompactMode::Battery => "battery",
                CompactMode::Vpn => "vpn",
                CompactMode::Onboard => "onboard",
                CompactMode::Messages => "messages",
                CompactMode::Share => "share",
                CompactMode::Shell => "shell",
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

fn thaw_toggle(hidden: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("thaw-toggle")
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                nook_core::menubar::toggle();
            }),
        )
        .child(lucide("eye", theme::COMPACT_FACE))
        .when(hidden, |d| d.opacity(0.45))
}
fn hud_icon(kind: HudKind) -> &'static str {
    match kind {
        HudKind::Volume => "volume-2",
        HudKind::Mute => "volume-x",
        HudKind::Brightness => "sun",
    }
}

#[cfg(test)]
mod tests {
    use super::hud_icon;
    use nook_core::sysvol::HudKind;

    #[test]
    fn hud_icons_match_kind() {
        assert_eq!(hud_icon(HudKind::Volume), "volume-2");
        assert_eq!(hud_icon(HudKind::Mute), "volume-x");
        assert_eq!(hud_icon(HudKind::Brightness), "sun");
    }
}
