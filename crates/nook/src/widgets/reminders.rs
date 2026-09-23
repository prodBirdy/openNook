//! Reminders Nook pane — tap the circle to complete it.

use crate::icons::lucide_color;
use crate::island::ui::{
    label, nook_empty, nook_pane, open_privacy_pane, scroll_body, text_btn,
};
use crate::island::Island;
use crate::theme;
use crate::widgets::QuickAdd;
use chrono::{Local, TimeZone};
use gpui::{
    div, prelude::*, px, AnyElement, Context, CursorStyle, Entity, MouseButton, MouseDownEvent,
    SharedString,
};
use nook_core::calendar::Reminder;
use std::cell::RefCell;
use std::time::{Duration, Instant};

thread_local! {
    static PENDING_COMPLETE: RefCell<Option<String>> = const { RefCell::new(None) };
    static COMPLETE_ERROR: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

/// Compact leading: list-checks glyph in a 22pt box.
#[allow(dead_code)]
pub(crate) fn compact_left() -> AnyElement {
    div()
        .size(px(22.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color("list-checks", 17.0, theme::secondary_label()))
        .into_any_element()
}

/// Compact trailing: "N due" (BODY semibold secondary, matching the mockup).
#[allow(dead_code)]
pub(crate) fn compact_right(reminders: &[Reminder]) -> AnyElement {
    let n = reminders.iter().filter(|r| !r.is_completed).count();
    let text = if n == 0 {
        "All clear".into()
    } else if n == 1 {
        "1 due".into()
    } else {
        format!("{n} due")
    };
    label(text, theme::BODY, true)
        .text_color(theme::secondary_label())
        .into_any_element()
}

pub(crate) fn reminders_card(
    reminders: &[Reminder],
    quick_add: Option<Entity<QuickAdd>>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let open: Vec<_> = reminders.iter().filter(|r| !r.is_completed).collect();

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
        let mut list = div()
            .flex()
            .flex_col()
            .w_full()
            .flex_1()
            .gap(px(8.))
            .justify_center()
            .flex_shrink_0();
        for reminder in open.into_iter().take(4) {
            list = list.child(reminder_row(reminder, cx));
        }
        scroll_body("rem-list", list).into_any_element()
    };

    nook_pane("nook-reminders")
        .w_full()
        .p(px(16.))
        .gap(px(10.))
        .when_some(quick_add, |d, field| d.child(field))
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
    let tap_id = reminder.id.clone();
    let due = reminder.due_date.and_then(format_due);
    let filling = PENDING_COMPLETE.with(|p| p.borrow().as_ref() == Some(&id));
    div()
        .id(SharedString::from(format!("rem-{id}")))
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.))
        .flex_shrink_0()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.8))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                complete_reminder(tap_id.clone(), cx);
            }),
        )
        .child(
            div()
                .id(SharedString::from(format!("rem-check-{id}")))
                .size(px(6.))
                .flex_shrink_0()
                .rounded_full()
                .border_1()
                .border_color(theme::tertiary_label())
                .when(filling, |d| {
                    d.bg(theme::tertiary_label()).border_color(theme::LABEL)
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(1.))
                .overflow_hidden()
                .child(
                    label(reminder.title.clone(), theme::CALLOUT, false)
                        .text_color(theme::LABEL)
                        .w_full(),
                )
                .when_some(due, |d, (text, overdue)| {
                    d.child(label(text, theme::FOOTNOTE, false).text_color(if overdue {
                        theme::DESTRUCTIVE
                    } else {
                        theme::tertiary_label()
                    }))
                }),
        )
}

fn complete_reminder(id: String, cx: &mut Context<Island>) {
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
                    *e.borrow_mut() = Some(("Couldn't complete reminder.".into(), Instant::now()));
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
}

fn format_due(ts: f64) -> Option<(String, bool)> {
    let dt = Local.timestamp_opt(ts as i64, 0).single()?;
    let overdue = dt < Local::now();
    let today = dt.date_naive() == Local::now().date_naive();
    let tomorrow = dt.date_naive() == Local::now().date_naive() + chrono::Duration::days(1);
    let text = if today {
        "Today".to_string()
    } else if tomorrow {
        "Tomorrow".to_string()
    } else {
        dt.format("%b %-d").to_string()
    };
    Some((text, overdue))
}

#[cfg(test)]
mod tests {
    use super::format_due;
    use chrono::Local;

    #[test]
    fn due_labels_today_and_tomorrow() {
        let now = Local::now();
        let (today, _) = format_due(now.timestamp() as f64).unwrap();
        assert_eq!(today, "Today");
        let tom = (now + chrono::Duration::days(1)).timestamp() as f64;
        let (label, _) = format_due(tom).unwrap();
        assert_eq!(label, "Tomorrow");
    }
}
