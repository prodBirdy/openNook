//! Compact Live Activity: left | notch gap | right, plus mode dots.

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

/// Mockup compact faces: a 22pt leading slot holding a 17pt glyph.
const LEADING_SLOT: f32 = 22.0;
const LEADING_GLYPH: f32 = 17.0;
/// Mirror compact: 8pt live dot trailing.
const MIRROR_LIVE_DOT: f32 = 8.0;
/// Battery compact glyph: 25×13 shell, 1pt 36% ring, 2pt inner pad, 1.5pt nub.
const BATTERY_SHELL_W: f32 = 25.0;
const BATTERY_SHELL_H: f32 = 13.0;
const BATTERY_SHELL_RADIUS: f32 = 4.3;
const BATTERY_LEVEL_RADIUS: f32 = 2.5;
const BATTERY_PAD: f32 = 2.0;
const BATTERY_NUB_W: f32 = 1.5;
const BATTERY_NUB_H: f32 = 4.5;
const BATTERY_RING_ALPHA: f32 = 0.36;
const BATTERY_LEVEL_RUN: f32 = 21.0;
/// Mockup High Alert orange `#FF9F0A` (systemOrange dark).
const ALERT_ORANGE: gpui::Rgba = gpui::Rgba {
    r: 1.0,
    g: 0.624,
    b: 0.039,
    a: 1.0,
};

/// Leading glyph centred in the mockup's 22pt slot.
fn leading_icon(name: &'static str, color: gpui::Rgba) -> AnyElement {
    div()
        .size(px(LEADING_SLOT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color(name, LEADING_GLYPH, color))
        .into_any_element()
}

/// Drawn battery (shell + level + nub) — the mockup face, not a line icon.
fn battery_glyph(percent: Option<u8>, tint: gpui::Rgba) -> AnyElement {
    let ring = theme::with_alpha(theme::LABEL, BATTERY_RING_ALPHA);
    // Gallery sizes the level off a 21pt run (82% → 17pt) though ring + pad
    // leave 19pt inside the shell; clamp so a full battery still fits.
    let inner = BATTERY_SHELL_W - 2.0 * (BATTERY_PAD + 1.0);
    let level = percent.map(|p| p.min(100) as f32 / 100.0).unwrap_or(1.0);
    let level_w = (BATTERY_LEVEL_RUN * level).min(inner);
    div()
        .h(px(LEADING_SLOT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(BATTERY_NUB_W))
        .child(
            div()
                .w(px(BATTERY_SHELL_W))
                .h(px(BATTERY_SHELL_H))
                .p(px(BATTERY_PAD))
                .rounded(px(BATTERY_SHELL_RADIUS))
                .border_1()
                .border_color(ring)
                .flex()
                .items_center()
                .child(
                    div()
                        .h_full()
                        .w(px(level_w.max(BATTERY_LEVEL_RADIUS)))
                        .rounded(px(BATTERY_LEVEL_RADIUS))
                        .bg(tint),
                ),
        )
        .child(
            div()
                .w(px(BATTERY_NUB_W))
                .h(px(BATTERY_NUB_H))
                .rounded(px(1.0))
                .bg(ring),
        )
        .into_any_element()
}

impl Island {
    pub(super) fn render_compact(
        &self,
        mode: CompactMode,
        hovered: bool,
        notch_w: f32,
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
                // Mockup compact: padding 0 9 (theme::COMPACT_INSET is still 8 for
                // chrome math elsewhere).
                .px(px(9.0))
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
                                ALERT_ORANGE,
                            )))
                        }),
                )
                .child(
                    div()
                        .w(px(notch_w))
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
            return leading_icon(hud_icon(kind), hud_tint(kind));
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
                self.agent_rotation,
                self.pixel_t,
                theme::island_fill(self.settings.island_color),
                self.size_morphing(),
            ),
            CompactMode::Files => super::files::compact_left(&self.files),
            CompactMode::Timer => widgets::timer_compact_left(self, cx),
            // Mockup V7Snx: radar in systemRed.
            CompactMode::Observe => leading_icon("radar", theme::DESTRUCTIVE),
            CompactMode::Battery => {
                battery_glyph(self.power.percent, widgets::battery_tint(self.power))
            }
            CompactMode::Vpn => leading_icon(
                if self.vpn.connected {
                    "shield-check"
                } else {
                    "shield-off"
                },
                if self.vpn.connected {
                    theme::SUCCESS
                } else {
                    theme::tertiary_label()
                },
            ),
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
            CompactMode::Share => leading_icon("share", theme::ACCENT),
            CompactMode::Idle => {
                if self.mirror_on {
                    leading_icon("webcam", theme::LABEL)
                } else if self.high_alert_active() {
                    leading_icon("sun", ALERT_ORANGE)
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
            // Pencil iZPif / jRPai: session title + slide dots while the face rotates.
            CompactMode::Agents => widgets::agents_compact_right(&self.agents, self.agent_rotation),
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
                label(text, theme::BODY, true)
                    .text_color(theme::DESTRUCTIVE)
                    .into_any_element()
            }
            // Mockup: the percent stays white; the level fill carries the tint.
            CompactMode::Battery => label(
                nook_core::power::format_percent(self.power.percent),
                theme::BODY,
                true,
            )
            .into_any_element(),
            // Mirror compact (mockup): webcam leading, green live dot trailing.
            CompactMode::Idle if self.mirror_on => div()
                .size(px(MIRROR_LIVE_DOT))
                .rounded_full()
                .bg(theme::SUCCESS)
                .into_any_element(),
            // High Alert compact (mockup): orange sun leading, orange value
            // trailing — here the keep-awake time left, or "On" when open-ended.
            CompactMode::Idle if self.high_alert_active() => {
                let text = self
                    .high_alert_remaining_secs()
                    .map(super::ui::format_timer_compact)
                    .unwrap_or_else(|| "On".into());
                timer_text(text, theme::BODY)
                    .text_color(ALERT_ORANGE)
                    .into_any_element()
            }
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
                    .text_color(if self.vpn.connected {
                        theme::SUCCESS
                    } else {
                        theme::tertiary_label()
                    })
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
