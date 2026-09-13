//! Compact Live Activity: left | notch gap | right, plus mode dots.

use super::chrome::COMPACT_WING;
use super::media::{album_chip, visualizer};
use super::ui::{label, timer_text};
use super::{CompactMode, Island};
use crate::icons::{lucide, lucide_color};
use crate::theme;
use crate::widgets;
use gpui::{
    canvas, div, prelude::*, px, relative, AnyElement, Context, CursorStyle, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, SharedString,
};
use nook_core::sysvol::HudKind;
use std::cell::RefCell;
use std::rc::Rc;

impl Island {
    pub(super) fn render_compact(
        &self,
        mode: CompactMode,
        hovered: bool,
        notch_w: f32,
        glass: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Leading/trailing sit in the camera band, not the extra chin the
        // island grows on hover — centering in the full pill dropped the
        // glyphs below the housing. Equal flex flanks keep the spacer on
        // the camera; a fixed side width used to shift the hole.
        let notch_h = self.notch_height.max(theme::NOTCH_MIN_H);
        div().relative().size_full().child(
            div()
                .flex()
                .items_center()
                .w_full()
                .h(px(notch_h))
                .px(px(
                    theme::COMPACT_INSET + if glass { COMPACT_WING } else { 0. }
                ))
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
                            d.child(div().ml(px(4.)).flex_shrink_0().child(lucide_color(
                                "sun",
                                theme::COMPACT_BADGE,
                                theme::SUCCESS,
                            )))
                        }),
                )
                .child(
                    div()
                        .w(px(notch_w + 2.0 * self.glass_notch_gap()))
                        .flex_shrink_0()
                        .h_full(),
                )
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
                ),
        )
    }

    fn compact_left(&self, mode: CompactMode, cx: &mut Context<Self>) -> AnyElement {
        if self.hud_active() {
            let kind = self.hud.unwrap().kind;
            return lucide_color(hud_icon(kind), theme::COMPACT_FACE, hud_tint(kind))
                .into_any_element();
        }
        if let Some(name) = self.output_hud_label() {
            return label(name.to_string(), theme::BODY, true).into_any_element();
        }
        match mode {
            CompactMode::Media => album_chip(
                &self.now_playing,
                self.overlay_fade.value,
                self.reduce_motion,
                cx,
            )
            .into_any_element(),
            CompactMode::Agents => widgets::agents_compact_left(
                &self.agents,
                self.pixel_t,
                theme::island_fill(self.settings.island_color),
                self.size_morphing(),
            ),
            CompactMode::Files => super::files::compact_left(&self.files),
            CompactMode::Timer => widgets::timer_compact_left(self, cx),
            CompactMode::Observe => {
                lucide_color("triangle-alert", theme::COMPACT_FACE, theme::WARNING)
                    .into_any_element()
            }
            CompactMode::Battery => {
                let color = widgets::battery_tint(self.power);
                lucide_color(self.power.compact_icon(), theme::COMPACT_FACE, color)
                    .into_any_element()
            }
            CompactMode::Vpn => lucide_color(
                if self.vpn.connected {
                    "shield-check"
                } else {
                    "shield-off"
                },
                theme::COMPACT_FACE,
                if self.vpn.connected {
                    theme::SUCCESS
                } else {
                    theme::tertiary_label()
                },
            )
            .into_any_element(),
            CompactMode::Recording => widgets::recorder_compact_left(self),
            CompactMode::Meeting => widgets::meeting_compact_left(&self.meeting),
            CompactMode::Notifications => {
                widgets::notifications_compact_left(self.notifications.first())
            }
            CompactMode::Onboard => label("openNook", theme::BODY, true).into_any_element(),
            CompactMode::Messages => self
                .messages
                .incoming
                .as_ref()
                .map(widgets::messages_compact_left)
                .map(|el| el.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            CompactMode::Share => {
                lucide_color("share", theme::COMPACT_FACE, theme::ACCENT).into_any_element()
            }
            CompactMode::Idle => {
                if self.high_alert_active() {
                    lucide_color("sun", theme::COMPACT_FACE, theme::SUCCESS).into_any_element()
                } else {
                    widgets::compact_weather(self)
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
            CompactMode::Media => {
                visualizer(self.now_playing.is_playing, self.visualizer_color).into_any_element()
            }
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
            CompactMode::Battery => label(
                nook_core::power::format_percent(self.power.percent),
                theme::BODY,
                true,
            )
            .text_color(widgets::battery_tint(self.power))
            .into_any_element(),
            CompactMode::Idle => div().into_any_element(),
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
            }
            CompactMode::Recording => widgets::recorder_compact_right(self, cx),
            CompactMode::Meeting => {
                widgets::meeting_compact_right(&self.meeting, self.overlay_fade.value)
            }
            CompactMode::Notifications => widgets::notifications_compact_right(
                self.notification_unread,
                self.notifications.first(),
            ),
            CompactMode::Onboard => div()
                .flex()
                .items_center()
                .gap(px(2.))
                .opacity(if hovered { 1.0 } else { 0.7 })
                .child(
                    div()
                        .id("onboard-dismiss")
                        .size(px(theme::HIT_MIN))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(theme::FILL))
                        .active(|s| s.opacity(0.75))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.first_run = false;
                                nook_core::settings::mark_onboarded();
                                cx.notify();
                            }),
                        )
                        .child(lucide_color("x", theme::GLYPH_SM, theme::secondary_label())),
                )
                .child(hit_icon("github", "github", cx, |_, _, _| {
                    let _ = std::process::Command::new("/usr/bin/open")
                        .arg("https://github.com/prodBirdy/openNook")
                        .spawn();
                }))
                .into_any_element(),
        }
    }

    fn hud_slider(&self, cx: &mut Context<Self>) -> AnyElement {
        let fill = self.hud_fill.value.clamp(0.0, 1.0);
        let bounds: Rc<RefCell<Option<(f32, f32)>>> = Rc::new(RefCell::new(None));
        let fill_color = self
            .hud
            .map(|hud| hud_tint(hud.kind))
            .unwrap_or(theme::LABEL);
        let bounds_down = bounds.clone();
        let bounds_move = bounds.clone();
        div()
            .id("hud-slider")
            .w_full()
            .max_w(px(72.))
            .h(px(theme::HIT_MIN))
            .flex()
            .items_center()
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let Some((origin, width)) = *bounds_down.borrow() else {
                        return;
                    };
                    let ratio = media_scrubber_ratio(event.position.x.into(), origin, width);
                    this.apply_hud_slider(ratio, cx);
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if !this.hud_dragging {
                    return;
                }
                cx.stop_propagation();
                let Some((origin, width)) = *bounds_move.borrow() else {
                    return;
                };
                let ratio = media_scrubber_ratio(event.position.x.into(), origin, width);
                this.apply_hud_slider(ratio, cx);
            }))
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
                    .h(px(theme::TRACK_H))
                    .rounded(px(theme::TRACK_RADIUS))
                    .bg(theme::FILL_SECONDARY)
                    .child(
                        div()
                            .h_full()
                            .w(relative(fill))
                            .rounded(px(theme::TRACK_RADIUS))
                            .bg(fill_color),
                    )
                    .child(canvas(
                        {
                            let bounds = bounds.clone();
                            move |layout, _, _| {
                                let origin: f32 = layout.origin.x.into();
                                let width: f32 = layout.size.width.into();
                                *bounds.borrow_mut() = Some((origin, width));
                                layout
                            }
                        },
                        |_bounds, _, _, _| {},
                    )),
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
            .top(px(self.notch_height.max(theme::NOTCH_MIN_H)))
            .bottom_0()
            .left_0()
            .right_0()
            .flex()
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
                CompactMode::Recording => "recording",
                CompactMode::Meeting => "meeting",
                CompactMode::Notifications => "notify",
                CompactMode::Onboard => "onboard",
                CompactMode::Messages => "messages",
                CompactMode::Share => "share",
            };
            row = row.child(
                div()
                    .id(SharedString::from(format!("dot-{name}")))
                    .h(px(theme::HIT_MIN))
                    .mt(px(-theme::COMPACT_HOVER_CHIN))
                    .w(px(theme::HIT_MIN))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.user_preferred = Some(mode);
                            this.preferred = Some(mode);
                            this.alert_preferred = None;
                            nook_core::haptics::trigger(None);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("dot-glyph-{name}")))
                            .size(px(if active { 5.0 } else { 4.0 }))
                            .rounded_full()
                            .bg(if active {
                                theme::LABEL
                            } else {
                                theme::tertiary_label()
                            })
                            .hover(|s| s.opacity(0.7))
                            .active(|s| s.opacity(0.5)),
                    ),
            );
        }
        row.into_any_element()
    }
}

fn media_scrubber_ratio(x: f32, origin: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    ((x - origin) / width).clamp(0.0, 1.0)
}

fn hit_icon(
    id: &'static str,
    icon: &'static str,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &mut gpui::Window, &mut Context<Island>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(theme::HIT_MIN))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(theme::FILL))
        .active(|s| s.opacity(0.75))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                on_click(this, window, cx);
            }),
        )
        .child(lucide(icon, theme::COMPACT_FACE))
}

fn hud_icon(kind: HudKind) -> &'static str {
    match kind {
        HudKind::Volume => "volume-2",
        HudKind::Mute => "volume-x",
        HudKind::Brightness => "sun",
    }
}

fn hud_tint(kind: HudKind) -> gpui::Rgba {
    match kind {
        HudKind::Brightness => theme::SYSTEM_YELLOW,
        HudKind::Volume | HudKind::Mute => theme::LABEL,
    }
}

#[cfg(test)]
mod tests {
    use super::{hud_icon, hud_tint};
    use crate::theme;
    use nook_core::sysvol::HudKind;

    #[test]
    fn hud_icons_match_kind() {
        assert_eq!(hud_icon(HudKind::Volume), "volume-2");
        assert_eq!(hud_icon(HudKind::Mute), "volume-x");
        assert_eq!(hud_icon(HudKind::Brightness), "sun");
    }

    #[test]
    fn hud_tints_match_macos() {
        assert_eq!(hud_tint(HudKind::Brightness), theme::SYSTEM_YELLOW);
        assert_eq!(hud_tint(HudKind::Volume), theme::LABEL);
        assert_eq!(hud_tint(HudKind::Mute), theme::LABEL);
    }
}
