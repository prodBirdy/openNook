//! Reminders Nook pane — tap the circle to complete it.

use crate::island::ui::{nook_display, nook_empty, nook_icon_btn, nook_pane, nook_row};
use crate::island::Island;
use crate::theme;
use crate::widgets::QuickAdd;
use chrono::{Local, TimeZone};
use gpui::{
    div, prelude::*, px, rgba, Context, CursorStyle, Entity, FontWeight, MouseButton,
    MouseDownEvent, SharedString,
};
use nook_core::calendar::Reminder;

pub(crate) fn reminders_card(
    reminders: &[Reminder],
    quick_add: Option<Entity<QuickAdd>>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let open: Vec<_> = reminders.iter().filter(|r| !r.is_completed).collect();
    let count = open.len();

    let body = if open.is_empty() {
        div()
            .id("rem-empty")
            .flex_1()
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    nook_core::runtime().spawn(async {
                        let _ = nook_core::calendar::open_reminders_app().await;
                    });
                }),
            )
            .child(nook_empty("list-checks", "No reminders"))
            .into_any_element()
    } else {
        let mut list = div().flex().flex_col().flex_1();
        for reminder in open.into_iter().take(2) {
            list = list.child(reminder_row(reminder, cx));
        }
        list.into_any_element()
    };

    nook_pane("nook-reminders")
        .w_full()
        .when_some(quick_add, |d, field| d.child(field).child(div().h(px(6.))))
        .when(count > 0, |d| {
            d.child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(16.))
                    .flex_shrink_0()
                    .child(nook_display(count.to_string()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .pb(px(4.))
                            .child(nook_icon_btn("plus", "rem-add", cx, |_, _, _, _| {
                                nook_core::runtime().spawn(async {
                                    let _ = nook_core::calendar::open_reminders_app().await;
                                });
                            }))
                            .child(nook_icon_btn(
                                "rotate-ccw",
                                "rem-refresh",
                                cx,
                                |this, _, _, cx| {
                                    this.refresh_calendar(cx);
                                },
                            )),
                    ),
            )
        })
        .child(body)
}

fn reminder_row(reminder: &Reminder, cx: &mut Context<Island>) -> impl IntoElement {
    let id = reminder.id.clone();
    let color = theme::parse_hex(&reminder.list_color);
    let due = reminder.due_date.and_then(format_due);
    nook_row(SharedString::from(format!("rem-{id}")))
        .child(
            div()
                .id(SharedString::from(format!("rem-check-{id}")))
                .size(px(22.))
                .flex_shrink_0()
                .mr_3()
                .rounded_full()
                .border_2()
                .border_color(color)
                .flex()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::PointingHand)
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
                .child(div().size(px(8.)).rounded_full().bg(color).opacity(0.45)),
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
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::LABEL)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(reminder.title.clone()),
                )
                .when_some(due, |d, (text, overdue)| {
                    d.child(
                        div()
                            .text_size(px(11.))
                            .text_color(if overdue {
                                theme::DESTRUCTIVE
                            } else {
                                rgba(0xffffff80)
                            })
                            .mt(px(1.))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
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
