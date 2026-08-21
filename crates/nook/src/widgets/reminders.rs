//! Reminders card — tap the circle to complete it.
//! Layout matches the React `RemindersWidget`.

use crate::island::ui::{empty_state, header_icon_btn, pill_btn, widget_shell_actions};
use crate::island::Island;
use crate::theme;
use chrono::{Local, TimeZone};
use gpui::{
    div, prelude::*, px, rgba, Context, FontWeight, MouseButton, MouseDownEvent, SharedString,
};
use nook_core::calendar::Reminder;

pub(crate) fn reminders_card(reminders: &[Reminder], cx: &mut Context<Island>) -> impl IntoElement {
    let actions = div()
        .flex()
        .gap_1()
        .child(header_icon_btn("plus", "rem-add", cx, |_, _, _, _| {
            nook_core::runtime().spawn(async {
                let _ = nook_core::calendar::open_reminders_app().await;
            });
        }))
        .child(header_icon_btn(
            "rotate-ccw",
            "rem-refresh",
            cx,
            |this, _, _, cx| {
                this.refresh_calendar(cx);
            },
        ));

    let open: Vec<_> = reminders
        .iter()
        .filter(|r| !r.is_completed)
        .take(10)
        .collect();

    let body = if open.is_empty() {
        empty_state(
            "No reminders",
            pill_btn("Create Reminder", cx, |_, _, _| {
                nook_core::runtime().spawn(async {
                    let _ = nook_core::calendar::open_reminders_app().await;
                });
            }),
        )
        .into_any_element()
    } else {
        let mut list = div().flex().flex_col().gap_1();
        for reminder in open {
            list = list.child(reminder_row(reminder, cx));
        }
        list.into_any_element()
    };

    widget_shell_actions("reminders-scroll", "Reminders", actions, body)
}

fn reminder_row(reminder: &Reminder, cx: &mut Context<Island>) -> impl IntoElement {
    let id = reminder.id.clone();
    let color = theme::parse_hex(&reminder.list_color);
    let due = reminder.due_date.and_then(format_due);
    div()
        .id(SharedString::from(format!("rem-{id}")))
        .flex()
        .items_center()
        .gap_3()
        .px(px(16.))
        .py(px(12.))
        .rounded(px(theme::ROW_RADIUS))
        .hover(|s| s.bg(rgba(0xFFFFFF0D)))
        .child(
            div()
                .id(SharedString::from(format!("rem-check-{id}")))
                .size(px(32.))
                .flex_shrink_0()
                .rounded_full()
                .border_2()
                .border_color(color)
                .flex()
                .items_center()
                .justify_center()
                .cursor(gpui::CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let id = id.clone();
                        this.reminders.retain(|r| r.id != id);
                        nook_core::runtime().spawn(async move {
                            let _ = nook_core::calendar::complete_reminder(id).await;
                        });
                        cx.notify();
                    }),
                )
                .child(div().size(px(16.)).rounded_full().bg(color).opacity(0.4)),
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
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(17.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgba(0xFFFFFFF2))
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(reminder.title.clone()),
                        )
                        .when(!reminder.list_name.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(color)
                                    .opacity(0.6)
                                    .child(reminder.list_name.clone()),
                            )
                        }),
                )
                .when_some(due, |d, (text, overdue)| {
                    d.child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if overdue {
                                theme::DESTRUCTIVE
                            } else {
                                rgba(0xffffff66)
                            })
                            .mt(px(2.))
                            .child(text),
                    )
                }),
        )
}

fn format_due(ts: f64) -> Option<(String, bool)> {
    let dt = Local.timestamp_opt(ts as i64, 0).single()?;
    let overdue = dt < Local::now();
    let today = dt.date_naive() == Local::now().date_naive();
    let text = if today {
        format!("Today, {}", dt.format("%I:%M %p"))
    } else {
        dt.format("%b %e, %I:%M %p").to_string()
    };
    Some((text, overdue))
}
