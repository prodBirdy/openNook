//! Meeting card: mute + leave for Zoom / Teams / Google Meet.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_display, nook_empty, nook_pane};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, rgba, AnyElement, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, Rgba,
};
use nook_core::meetings::{MeetingApp, MeetingSnapshot};

pub(crate) fn compact_left(snap: &MeetingSnapshot) -> AnyElement {
    lucide_color(
        snap.app().map(MeetingApp::icon_name).unwrap_or("video"),
        theme::COMPACT_FACE,
        theme::LABEL,
    )
    .into_any_element()
}

pub(crate) fn compact_right(snap: &MeetingSnapshot, flash: f32) -> AnyElement {
    let (icon, color) = mic_face(snap);
    let flash = flash.clamp(0.0, 1.0);
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(theme::COMPACT_FACE))
        .rounded_full()
        .bg(rgba(0xffffff00 | ((flash * 72.0) as u32).min(72)))
        .child(lucide_color(icon, 16.0, color))
        .into_any_element()
}

pub(crate) fn meeting_card(snap: &MeetingSnapshot, cx: &mut Context<Island>) -> impl IntoElement {
    let Some(app) = snap.app().filter(|_| snap.in_meeting()) else {
        return nook_pane("nook-meeting")
            .w_full()
            .child(nook_empty("video", "No meeting"))
            .into_any_element();
    };
    let elapsed = format_elapsed(snap.elapsed_secs());
    let verified = snap.mute_verified();
    let muted = snap.muted();
    let (mic_icon, mic_color) = mic_face(snap);
    let mute_caption = match muted {
        Some(true) => "Unmute",
        Some(false) => "Mute",
        None => "Mute",
    };
    let state_line = if verified {
        if muted == Some(true) {
            "Muted"
        } else {
            "Live"
        }
    } else {
        "Unverified state"
    };

    nook_pane("nook-meeting")
        .w_full()
        .child(
            div()
                .flex()
                .items_end()
                .justify_between()
                .gap(px(10.))
                .child(nook_display(elapsed))
                .child(
                    div()
                        .pb(px(4.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(lucide_color(app.icon_name(), 14.0, theme::SECONDARY_LABEL))
                        .child(label(app.label(), theme::CALLOUT, true)),
                ),
        )
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if verified {
                    mic_color
                } else {
                    theme::TERTIARY_LABEL
                })
                .child(state_line),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .pt(px(4.))
                .child(action_btn(
                    "meeting-mute",
                    mic_icon,
                    mute_caption,
                    mic_color,
                    snap.accessibility_trusted || app == MeetingApp::Meet,
                    cx,
                    |this, cx| this.toggle_meeting_mute(cx),
                ))
                .child(action_btn(
                    "meeting-leave",
                    "phone-off",
                    "Leave",
                    theme::DESTRUCTIVE,
                    snap.accessibility_trusted || app == MeetingApp::Meet,
                    cx,
                    |this, cx| this.leave_meeting(cx),
                )),
        )
        .into_any_element()
}

fn action_btn(
    id: &'static str,
    icon: &'static str,
    caption: &'static str,
    color: Rgba,
    enabled: bool,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(theme::HIT_MIN))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(theme::CONTROL_RADIUS))
        .bg(theme::FILL)
        .opacity(if enabled { 1.0 } else { 0.4 })
        .when(enabled, |d| {
            d.hover(|s| s.bg(theme::FILL_SECONDARY))
                .active(|s| s.opacity(0.85))
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        on_click(this, cx);
                    }),
                )
        })
        .child(lucide_color(icon, 14.0, color))
        .child(label(caption, theme::CALLOUT, true))
}

fn mic_face(snap: &MeetingSnapshot) -> (&'static str, Rgba) {
    if snap.mute_verified() {
        if snap.muted() == Some(true) {
            ("mic-off", theme::WARNING)
        } else {
            ("mic", theme::SUCCESS)
        }
    } else {
        ("mic", theme::SECONDARY_LABEL)
    }
}

fn format_elapsed(secs: u32) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_pads_minutes() {
        assert_eq!(format_elapsed(0), "0:00");
        assert_eq!(format_elapsed(65), "1:05");
        assert_eq!(format_elapsed(3600), "1:00:00");
    }
}
