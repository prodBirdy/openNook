//! Compact Live Activity: left | notch gap | right.

use super::media::{album_chip, visualizer};
use super::ui::{label, slide_label, timer_text};
use super::{CompactMode, Island};
use crate::icons::{lucide, lucide_color};
use crate::theme;
use crate::widgets;
use gpui::{
    canvas, div, prelude::*, px, relative, AnyElement, Context, CursorStyle, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent,
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

/// BODY 13pt semibold width estimate, capped for ellipsising flank labels.
fn approx_label_w(text: &str) -> f32 {
    (text.chars().count() as f32 * theme::COMPACT_BODY_CHAR_W + theme::COMPACT_LABEL_SLACK)
        .min(theme::COMPACT_LABEL_MAX)
}

/// Compact face VPN trailing string: timer only while connected (name+timer
/// overflows the narrow flank and clips on the left under `justify_end`).
fn vpn_face_text(vpn: &nook_core::vpn::VpnSnapshot, show_timer: bool) -> String {
    let now = std::time::SystemTime::now();
    if !vpn.connected {
        let name = vpn.display_name();
        return if name.is_empty() {
            "Disconnected".into()
        } else {
            format!("{name} · off")
        };
    }
    if show_timer {
        if let Some(elapsed) = vpn.elapsed_label(now) {
            return elapsed;
        }
    }
    vpn.display_name()
}

/// Trailing/leading compact label. Width/`min_w(0)` live on a flex_col
/// *container* (leaf is a plain `label` — no `w`/`min_w`/`overflow_hidden` on
/// the text itself). GPUI ellipsizes when the leaf stretches to that width;
/// an `overflow_hidden` wrapper hard-clips mid-glyph instead.
fn flank_text(text: impl Into<gpui::SharedString>) -> gpui::Div {
    flank_label(text, None)
}

fn flank_label(text: impl Into<gpui::SharedString>, color: Option<gpui::Rgba>) -> gpui::Div {
    let text = text.into();
    let w = approx_label_w(text.as_ref());
    let mut leaf = label(text, theme::BODY, true);
    if let Some(c) = color {
        leaf = leaf.text_color(c);
    }
    div()
        .w(px(w))
        .min_w(px(0.))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .justify_center()
        .child(leaf)
}

/// Long flank title in a flex ROW: fill the flank and ellipsize (vpn.rs S).
fn flank_title(text: impl Into<gpui::SharedString>) -> gpui::Div {
    label(text, theme::BODY, true).flex_1().min_w(px(0.))
}

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
    /// Extra width so each equal flank fits its label/icon (capped).
    pub(super) fn compact_content_extra(&self) -> f32 {
        let need = self.compact_flank_need();
        if need <= 0.0 {
            return 0.0;
        }
        (2.0 * need + 2.0 * theme::COMPACT_FLANK_PAD).min(theme::COMPACT_CONTENT_EXTRA_MAX)
    }

    fn compact_flank_need(&self) -> f32 {
        let mode = self.mode();
        let (lead, trail) = match mode {
            CompactMode::Idle if self.mirror_on => (LEADING_SLOT, MIRROR_LIVE_DOT),
            CompactMode::Idle if self.high_alert_active() => {
                let text = self
                    .high_alert_remaining_secs()
                    .map(super::ui::format_timer_compact)
                    .unwrap_or_else(|| "On".into());
                (LEADING_SLOT, approx_label_w(&text))
            }
            CompactMode::Idle => {
                if let Some(snap) = self.weather_compact_snap() {
                    let temp = nook_core::weather::format_temp(snap.temperature);
                    (LEADING_SLOT, approx_label_w(&temp))
                } else {
                    (0.0, 0.0)
                }
            }
            CompactMode::Media => (LEADING_SLOT, 18.0),
            CompactMode::Agents => {
                let title = self.agent_face_title().unwrap_or_default();
                (LEADING_SLOT, approx_label_w(&title))
            }
            CompactMode::Files => (LEADING_SLOT, approx_label_w(&self.files.len().to_string())),
            CompactMode::Timer => {
                let text = self
                    .face_timer()
                    .map(|t| super::ui::format_timer_compact(t.remaining))
                    .unwrap_or_else(|| "0s".into());
                (LEADING_SLOT, approx_label_w(&text).max(40.0))
            }
            CompactMode::Observe => {
                let text = match self.observe.alerts.as_slice() {
                    [one] => one.name.clone(),
                    _ => self.observe.firing_count().to_string(),
                };
                (LEADING_SLOT, approx_label_w(&text))
            }
            CompactMode::Battery => (
                BATTERY_SHELL_W + BATTERY_NUB_W,
                approx_label_w(&nook_core::power::format_percent(self.power.percent)),
            ),
            CompactMode::Vpn => {
                let text = vpn_face_text(&self.vpn, self.settings.vpn_show_timer);
                (LEADING_SLOT, approx_label_w(&text).max(40.0))
            }
            CompactMode::Notifications => {
                let text = if self.notification_unread > 0 {
                    format!("{} new", self.notification_unread)
                } else if let Some(event) = self.notifications.first() {
                    if event.title.is_empty() {
                        event.app_name.clone()
                    } else {
                        event.title.clone()
                    }
                } else {
                    String::new()
                };
                (LEADING_SLOT, approx_label_w(&text))
            }
            CompactMode::Onboard => {
                // Dismiss + github hit targets.
                let right = theme::HIT_MIN * 2.0 + 2.0;
                (approx_label_w("openNook"), right)
            }
            CompactMode::Messages => {
                let sender = self
                    .messages
                    .incoming
                    .as_ref()
                    .map(|p| p.sender.as_str())
                    .unwrap_or("");
                (LEADING_SLOT, approx_label_w(sender))
            }
            CompactMode::Share => {
                let text = self.share.compact_label();
                (LEADING_SLOT, approx_label_w(&text))
            }
            CompactMode::Recording | CompactMode::Meeting => (0.0, 0.0),
        };
        lead.max(trail)
    }

    fn weather_compact_snap(&self) -> Option<&nook_core::weather::WeatherSnapshot> {
        let snap = self.weather.as_ref()?;
        if !self.settings.weather.enabled || !self.settings.weather.show_on_compact_face {
            return None;
        }
        Some(snap)
    }

    fn agent_face_title(&self) -> Option<String> {
        let n = self.agents.len();
        if n == 0 {
            return None;
        }
        let waiting = self.agents.iter().filter(|a| !a.status.is_working());
        waiting
            .chain(self.agents.iter().filter(|a| a.status.is_working()))
            .nth(self.agent_rotation % n)
            .map(|a| a.title().to_string())
    }

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
                // Mockup compact: padding 0 9.
                .px(px(theme::COMPACT_FLANK_PAD))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_start()
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
            return flank_text(name.to_string()).into_any_element();
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
                theme::island_fill(theme::island_color(&self.settings)),
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
            CompactMode::Onboard => flank_text("openNook").into_any_element(),
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
                } else if let Some(snap) = self.weather_compact_snap() {
                    // Icon left, temperature on the right flank so both fit.
                    let tint = if snap.is_day && matches!(snap.wmo_code, 0 | 1 | 2) {
                        theme::SYSTEM_ORANGE
                    } else {
                        theme::LABEL
                    };
                    leading_icon(snap.icon(), tint)
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
            CompactMode::Media => {
                visualizer(self.now_playing.is_playing, self.visualizer_color).into_any_element()
            }
            // Title fills the right flank and ellipsizes (vpn.rs S row pattern).
            CompactMode::Agents => match self.agent_face_title() {
                Some(title) => flank_title(title).into_any_element(),
                None => div().into_any_element(),
            },
            CompactMode::Files => {
                flank_text(self.files.len().to_string()).into_any_element()
            }
            CompactMode::Timer => {
                let text = self
                    .face_timer()
                    .map(|t| super::ui::format_timer_compact(t.remaining))
                    .unwrap_or_else(|| "0s".into());
                timer_text(text, theme::BODY)
                    .flex_shrink_0()
                    .min_w(px(40.))
                    .text_right()
                    .into_any_element()
            }
            CompactMode::Observe => {
                let text = match self.observe.alerts.as_slice() {
                    [one] => one.name.clone(),
                    _ => self.observe.firing_count().to_string(),
                };
                flank_label(text, Some(theme::DESTRUCTIVE)).into_any_element()
            }
            // Mockup: the percent stays white; the level fill carries the tint.
            CompactMode::Battery => flank_text(nook_core::power::format_percent(self.power.percent))
                .into_any_element(),
            // Mirror compact (mockup): webcam leading, green live dot trailing.
            CompactMode::Idle if self.mirror_on => div()
                .size(px(MIRROR_LIVE_DOT))
                .rounded_full()
                .flex_shrink_0()
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
                    .flex_shrink_0()
                    .text_color(ALERT_ORANGE)
                    .into_any_element()
            }
            CompactMode::Idle => {
                if let Some(snap) = self.weather_compact_snap() {
                    flank_text(nook_core::weather::format_temp(snap.temperature)).into_any_element()
                } else {
                    div().into_any_element()
                }
            }
            CompactMode::Messages => match self.messages.incoming.as_ref() {
                Some(peek) => flank_text(peek.sender.clone()).into_any_element(),
                None => div().into_any_element(),
            },
            CompactMode::Share => flank_text(self.share.compact_label()).into_any_element(),
            CompactMode::Vpn => {
                let text = vpn_face_text(&self.vpn, self.settings.vpn_show_timer);
                let color = if self.vpn.connected {
                    theme::SUCCESS
                } else {
                    theme::tertiary_label()
                };
                flank_label(text, Some(color))
                    .text_right()
                    .into_any_element()
            }
            CompactMode::Recording => widgets::recorder_compact_right(self, cx),
            CompactMode::Meeting => {
                widgets::meeting_compact_right(&self.meeting, self.overlay_fade.value)
            }
            CompactMode::Notifications => {
                let text = if self.notification_unread > 0 {
                    Some(format!("{} new", self.notification_unread))
                } else {
                    self.notifications.first().map(|event| {
                        if event.title.is_empty() {
                            event.app_name.clone()
                        } else {
                            event.title.clone()
                        }
                    })
                };
                match text {
                    Some(t) => flank_text(t).into_any_element(),
                    None => div().into_any_element(),
                }
            }
            CompactMode::Onboard => div()
                .flex()
                .items_center()
                .gap(px(2.))
                .flex_shrink_0()
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

    /// Now-playing title · artist in the media hover chin.
    pub(super) fn compact_media_hover_title(&self, fade: f32) -> AnyElement {
        let Some(title) = self
            .now_playing
            .title
            .as_ref()
            .filter(|t| !t.is_empty())
        else {
            return div().into_any_element();
        };
        if self.mode() != CompactMode::Media || !self.has_media() {
            return div().into_any_element();
        }
        let artist = self
            .now_playing
            .artist
            .as_ref()
            .filter(|a| !a.is_empty());
        let notch_h = self.notch_height.max(theme::NOTCH_MIN_H);
        let combined = match artist {
            Some(a) => format!("{title} · {a}"),
            None => title.clone(),
        };
        // Vertically center the title line in the chin; rise tracks the fade.
        let title_top = notch_h
            + (theme::COMPACT_MEDIA_HOVER_CHIN - theme::SUBHEADLINE.leading) / 2.0
            - 1.0;
        div()
            .absolute()
            .top(px(title_top))
            .left_0()
            .right_0()
            .px(px(14.0))
            .child(
                div()
                    .relative()
                    .top(px((1.0 - fade) * 4.0))
                    .w_full()
                    .overflow_hidden()
                    .child(slide_label(combined, theme::SUBHEADLINE, true).w_full()),
            )
            .into_any_element()
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
