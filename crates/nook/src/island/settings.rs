//! Settings window — separate from the island overlay.

use super::ui::label;
use crate::theme;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, CursorStyle, FontWeight, MouseButton};
use nook_core::settings::AppSettings;

pub(super) struct SettingsView;

impl gpui::Render for SettingsView {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = nook_core::settings::get_app_settings();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::WINDOW_BG)
            .text_color(theme::LABEL)
            .p_5()
            .gap_4()
            .child(
                div()
                    .text_size(px(theme::TITLE_2.size))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("openNook"),
            )
            .child(settings_group(vec![
                toggle_row("Show Now Playing", settings.show_media, cx, |s| {
                    s.show_media = !s.show_media;
                })
                .into_any_element(),
                toggle_row("Show Calendar", settings.show_calendar, cx, |s| {
                    s.show_calendar = !s.show_calendar;
                })
                .into_any_element(),
                toggle_row("Show Reminders", settings.show_reminders, cx, |s| {
                    s.show_reminders = !s.show_reminders;
                })
                .into_any_element(),
                toggle_row("Show Agents", settings.show_agents, cx, |s| {
                    s.show_agents = !s.show_agents;
                })
                .into_any_element(),
            ]))
            .child(settings_group(vec![
                toggle_row("Translucent island", settings.liquid_glass_mode, cx, |s| {
                    s.liquid_glass_mode = !s.liquid_glass_mode
                })
                .into_any_element(),
                toggle_row(
                    "Show island without a notch",
                    settings.non_notch_mode,
                    cx,
                    |s| {
                        s.non_notch_mode = !s.non_notch_mode;
                    },
                )
                .into_any_element(),
            ]))
            .child(
                div()
                    .id("edit-notes")
                    .h(px(theme::HIT_MIN + 8.0))
                    .px_3()
                    .rounded(px(theme::CONTROL_RADIUS))
                    .bg(theme::GROUPED_BG)
                    .flex()
                    .items_center()
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.bg(theme::FILL_SECONDARY))
                    .active(|s| s.opacity(0.85))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, _| {
                            if let Err(err) = nook_core::notes::open_notes_editor() {
                                log::warn!("open notes: {err}");
                            }
                        }),
                    )
                    .child(label("Edit Notes…", theme::BODY, true)),
            )
            .child(
                div()
                    .text_size(px(theme::SUBHEADLINE.size))
                    .text_color(theme::SECONDARY_LABEL)
                    .child(
                        "Hover the notch to expand. Settings and Quit live in the menu bar extra.",
                    ),
            )
    }
}

fn settings_group(rows: Vec<AnyElement>) -> impl IntoElement {
    let mut group = div()
        .flex()
        .flex_col()
        .rounded(px(theme::INNER_RADIUS))
        .bg(theme::GROUPED_BG)
        .px_3();
    for row in rows {
        group = group.child(row);
    }
    group
}

fn toggle_row(
    label_text: &'static str,
    on: bool,
    cx: &mut Context<SettingsView>,
    tweak: impl Fn(&mut AppSettings) + 'static,
) -> impl IntoElement {
    div()
        .id(label_text)
        .flex()
        .items_center()
        .justify_between()
        .h(px(36.))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                let mut s = nook_core::settings::get_app_settings();
                tweak(&mut s);
                nook_core::settings::update_app_settings(s);
                cx.notify();
            }),
        )
        .child(label(label_text, theme::BODY, true))
        .child(
            div()
                .w(px(40.))
                .h(px(24.))
                .rounded_full()
                .bg(if on {
                    theme::ACCENT
                } else {
                    theme::FILL_SECONDARY
                })
                .flex()
                .items_center()
                .when(on, |d| d.justify_end())
                .px(px(2.))
                .child(div().size(px(20.)).rounded_full().bg(rgb(0xffffff))),
        )
}
