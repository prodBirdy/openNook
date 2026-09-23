//! Meeting card: mute + leave for Zoom / Teams / Google Meet.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_empty, nook_pane, open_privacy_pane, text_btn};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent, Rgba,
};
use nook_core::meetings::{MeetingApp, MeetingSnapshot};

pub(crate) fn compact_left(snap: &MeetingSnapshot) -> AnyElement {
    lucide_color(
        snap.app().map(MeetingApp::icon_name).unwrap_or("video"),
        17.0,
        theme::SECONDARY_LABEL,
    )
    .into_any_element()
}

pub(crate) fn compact_right(snap: &MeetingSnapshot, _flash: f32) -> AnyElement {
    let (icon, color) = mic_face(snap);
    div()
        .flex()
        .items_center()
        .justify_end()
        .size(px(22.))
        .flex_shrink_0()
        .child(lucide_color(icon, 16.0, color))
        .into_any_element()
}

pub(crate) fn meeting_card(snap: &MeetingSnapshot, cx: &mut Context<Island>) -> impl IntoElement {
    let Some(app) = snap.app().filter(|_| snap.in_meeting()) else {
        return card_shell("nook-meeting")
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
        "Meeting controls need Accessibility"
    };

    card_shell("nook-meeting")
        .w_full()
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().size(px(6.)).rounded_full().bg(theme::SYSTEM_ORANGE))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .min_w(px(0.))
                        .child(label(
                            format!("{} · Standup", app.label()),
                            theme::CALLOUT,
                            false,
                        )
                        .text_color(theme::LABEL))
                        .child(
                            label(
                                format!("{} · {} in", state_line.to_lowercase(), elapsed),
                                theme::FOOTNOTE,
                                false,
                            )
                            .text_color(theme::TERTIARY_LABEL),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .pt(px(4.))
                .when(
                    !snap.accessibility_trusted && app != MeetingApp::Meet,
                    |d| {
                        d.child(text_btn("Allow Accessibility", cx, |_, _, _| {
                            open_privacy_pane("Privacy_Accessibility");
                        }))
                    },
                )
                .when(snap.accessibility_trusted || app == MeetingApp::Meet, |d| {
                    d.child(action_btn(
                        "meeting-mute",
                        mic_icon,
                        mute_caption,
                        mic_color,
                        true,
                        cx,
                        |this, cx| this.toggle_meeting_mute(cx),
                    ))
                    .child(action_btn(
                        "meeting-leave",
                        "phone-off",
                        "Leave",
                        theme::DESTRUCTIVE,
                        true,
                        cx,
                        |this, cx| this.leave_meeting(cx),
                    ))
                }),
        )
        .into_any_element()
}

fn card_shell(id: impl Into<gpui::ElementId>) -> gpui::Stateful<gpui::Div> {
    nook_pane(id).p(px(16.)).gap(px(10.))
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
        .h(px(24.))
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
        .child(label(caption, theme::FOOTNOTE, true))
}

fn mic_face(snap: &MeetingSnapshot) -> (&'static str, Rgba) {
    if snap.mute_verified() {
        if snap.muted() == Some(true) {
            ("mic-off", theme::DESTRUCTIVE)
        } else {
            ("mic", theme::SUCCESS)
        }
    } else {
        ("mic", theme::SECONDARY_LABEL)
    }
}

/// Elapsed meeting time as the mockup shows it: whole minutes (`12 min`),
/// falling back to h:mm:ss past an hour.
fn format_elapsed(secs: u32) -> String {
    let h = secs / 3600;
    if h > 0 {
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{} min", secs.div_ceil(60).max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_reads_whole_minutes() {
        assert_eq!(format_elapsed(0), "1 min");
        assert_eq!(format_elapsed(45), "1 min");
        assert_eq!(format_elapsed(65), "2 min");
        assert_eq!(format_elapsed(720), "12 min");
        assert_eq!(format_elapsed(3600), "1:00:00");
    }
}
