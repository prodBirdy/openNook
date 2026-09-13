//! Reminders Nook pane — tap the circle to complete it.

use crate::island::ui::{
    label, nook_display, nook_empty, nook_icon_btn, nook_pane, nook_row, open_privacy_pane,
    scroll_body, slide_label, text_btn,
};
use crate::island::Island;
use crate::theme;
use crate::widgets::QuickAdd;
use chrono::{Local, TimeZone};
use gpui::{
    div, prelude::*, px, Context, CursorStyle, Entity, MouseButton, MouseDownEvent, SharedString,
};
use nook_core::calendar::Reminder;
use std::cell::RefCell;
use std::time::{Duration, Instant};

thread_local! {
    static PENDING_COMPLETE: RefCell<Option<String>> = const { RefCell::new(None) };
    static COMPLETE_ERROR: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

pub(crate) fn reminders_card(
    reminders: &[Reminder],
    quick_add: Option<Entity<QuickAdd>>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let open: Vec<_> = reminders.iter().filter(|r| !r.is_completed).collect();
    let count = open.len();

    let error = COMPLETE_ERROR.with(|e| {
        e.borrow()
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(4))
            .map(|(msg, _)| msg.clone())
    });
    let denied = nook_core::calendar::reminders_authorized() == Some(false);
    let body = if denied {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(nook_empty("calendar-x", "Reminders access is off"))
            .child(text_btn("Open Privacy Settings", cx, |_, _, _| {
                open_privacy_pane("Privacy_Reminders");
            }))
            .into_any_element()
    } else if open.is_empty() {
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
            .child(nook_empty("list-checks", "All clear"))
            .into_any_element()
    } else {
        let mut list = div().flex().flex_col().w_full().flex_shrink_0();
        for reminder in open {
            list = list.child(reminder_row(reminder, cx));
        }
        scroll_body("rem-list", list).into_any_element()
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
        .when_some(error, |d, msg| {
            d.child(
                label(msg, theme::FOOTNOTE, false)
                    .text_color(theme::DESTRUCTIVE)
                    .pt(px(4.)),
            )
        })
}

fn reminder_row(reminder: &Reminder, cx: &mut Context<Island>) -> impl IntoElement {
    let id = reminder.id.clone();
    let color = theme::parse_hex(&reminder.list_color);
    let due = reminder.due_date.and_then(format_due);
    let filling = PENDING_COMPLETE.with(|p| p.borrow().as_ref() == Some(&id));
    nook_row(SharedString::from(format!("rem-{id}")))
        .child(
            div()
                .id(SharedString::from(format!("rem-check-{id}")))
                .size(px(theme::HIT_MIN))
                .flex_shrink_0()
                .mr_1()
                .flex()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.opacity(0.85))
                .active(|s| s.opacity(0.75))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let id = id.clone();
                        PENDING_COMPLETE.with(|p| *p.borrow_mut() = Some(id.clone()));
                        cx.notify();
                        cx.spawn(async move |this, cx| {
                            let result = nook_core::runtime()
                                .spawn({
                                    let id = id.clone();
                                    async move { nook_core::calendar::complete_reminder(id).await }
                                })
                                .await
                                .unwrap_or_else(|e| Err(e.to_string()));
                            let failed = !matches!(result, Ok(true));
                            let _ = this.update(cx, |this, cx| {
                                PENDING_COMPLETE.with(|p| {
                                    if p.borrow().as_ref() == Some(&id) {
                                        *p.borrow_mut() = None;
                                    }
                                });
                                if matches!(result, Ok(true)) {
                                    this.reminders.retain(|r| r.id != id);
                                } else {
                                    COMPLETE_ERROR.with(|e| {
                                        *e.borrow_mut() = Some((
                                            "Couldn't complete reminder.".into(),
                                            Instant::now(),
                                        ));
                                    });
                                }
                                cx.notify();
                            });
                            if failed {
                                cx.background_executor().timer(Duration::from_secs(4)).await;
                                let _ = this.update(cx, |_, cx| {
                                    COMPLETE_ERROR.with(|e| *e.borrow_mut() = None);
                                    cx.notify();
                                });
                            }
                        })
                        .detach();
                    }),
                )
                .child(
                    div()
                        .size(px(22.))
                        .rounded_full()
                        .border_2()
                        .border_color(color)
                        .when(filling, |d| d.bg(color))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(if filling { 12. } else { 8. }))
                                .rounded_full()
                                .bg(color)
                                .opacity(if filling { 1.0 } else { 0.45 }),
                        ),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .justify_center()
                .overflow_hidden()
                .child(slide_label(reminder.title.clone(), theme::TITLE_3, true).w_full())
                .when_some(due, |d, (text, overdue)| {
                    d.child(
                        label(text, theme::SUBHEADLINE, false)
                            .text_color(if overdue {
                                theme::DESTRUCTIVE
                            } else {
                                theme::SECONDARY_LABEL
                            })
                            .mt(px(1.)),
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
        dt.format("%b %-d, %-I:%M %p").to_string()
    };
    Some((text, overdue))
}
