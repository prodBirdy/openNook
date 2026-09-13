//! Week strip + empty/event state for the Nook calendar pane.

use crate::island::ui::{
    label, nook_accent_bar, nook_display, nook_empty, nook_row, open_privacy_pane, scroll_body,
    slide_label, text_btn, timer_text,
};
use crate::island::Island;
use crate::theme;
use chrono::{Datelike, Local, TimeZone, Weekday};
use gpui::{div, prelude::*, px, Context, MouseButton, MouseDownEvent, SharedString};
use nook_core::calendar::CalendarEvent;

pub(crate) fn calendar_card(
    events: &[CalendarEvent],
    selected_day: u8,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let today = Local::now().date_naive();
    let selected = selected_day.min(6);
    let selected_date = today + chrono::Duration::days(selected as i64 - 3);
    let month = selected_date.format("%b").to_string();
    let is_today = selected == 3;

    let mut week = div().flex().items_end().gap(px(10.));
    for index in 0..7u8 {
        let date = today + chrono::Duration::days(index as i64 - 3);
        week = week.child(day_col(
            index,
            date.day(),
            weekday_label(date, index == selected),
            index == selected,
            is_weekend(date.weekday()),
            cx,
        ));
    }

    let filtered: Vec<_> = events
        .iter()
        .filter(|e| same_day(e.start_date, selected_date))
        .collect();

    let denied = nook_core::calendar::calendar_authorized() == Some(false);
    let empty_copy = if is_today {
        "Nothing today"
    } else {
        "No events"
    };
    let body = if denied {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(nook_empty("calendar-x", "Calendar access is off"))
            .child(text_btn("Open Privacy Settings", cx, |_, _, _| {
                open_privacy_pane("Privacy_Calendars");
            }))
            .into_any_element()
    } else if filtered.is_empty() {
        nook_empty("calendar", empty_copy).into_any_element()
    } else {
        let mut col = div().flex().flex_col().flex_shrink_0().gap_2().pt(px(8.));
        for event in filtered {
            col = col.child(event_row(event, cx));
        }
        scroll_body("cal-events", col).into_any_element()
    };

    div()
        .id("nook-calendar")
        .w_full()
        .h_full()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .flex_shrink_0()
                .child(nook_display(month))
                .child(week),
        )
        .child(body)
}

fn weekday_label(date: chrono::NaiveDate, selected: bool) -> String {
    let short = date.format("%a").to_string().to_uppercase();
    if selected {
        short.chars().take(3).collect()
    } else {
        short.chars().next().unwrap_or('?').to_string()
    }
}

fn is_weekend(day: Weekday) -> bool {
    matches!(day, Weekday::Sat | Weekday::Sun)
}

fn same_day(ts: f64, day: chrono::NaiveDate) -> bool {
    Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .is_some_and(|dt| dt.date_naive() == day)
}

fn day_col(
    index: u8,
    day: u32,
    weekday: String,
    selected: bool,
    weekend: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let number_color = if weekend {
        theme::SECONDARY_LABEL
    } else {
        theme::LABEL
    };
    let label_color = theme::SECONDARY_LABEL;
    div()
        .id(SharedString::from(format!("cal-day-{index}")))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .min_h(px(theme::HIT_MIN))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.calendar_day = index;
                cx.notify();
            }),
        )
        .child(label(weekday, theme::FOOTNOTE, true).text_color(label_color))
        .child(
            div()
                .id(SharedString::from(format!("day-pill-{index}")))
                .px(px(6.))
                .py(px(2.))
                .rounded_full()
                .active(|s| s.bg(theme::FILL))
                .when(selected, |d| d.bg(theme::FILL_SECONDARY))
                .child(label(format!("{day:02}"), theme::TITLE_3, true).text_color(number_color)),
        )
}

fn event_row(event: &CalendarEvent, cx: &mut Context<Island>) -> impl IntoElement {
    let id = event.id.clone();
    let date = event.start_date;
    let time = if event.is_all_day {
        "All day".to_string()
    } else {
        format_event_time(event.start_date)
    };
    nook_row(SharedString::from(format!("cal-ev-{id}")))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let id = id.clone();
                nook_core::runtime().spawn(async move {
                    let _ = nook_core::calendar::open_calendar_event(id, date).await;
                });
            }),
        )
        .child(
            div()
                .w(px(48.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .pr_2()
                .child(if event.is_all_day {
                    label(time, theme::FOOTNOTE, true)
                        .text_color(theme::SECONDARY_LABEL)
                        .into_any_element()
                } else {
                    timer_text(time, theme::BODY).into_any_element()
                }),
        )
        .child(nook_accent_bar(theme::accent()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .justify_center()
                .overflow_hidden()
                .child(slide_label(event.title.clone(), theme::TITLE_3, true).w_full())
                .when_some(event.location.clone(), |d, loc| {
                    d.child(
                        label(loc, theme::SUBHEADLINE, false)
                            .text_color(theme::SECONDARY_LABEL)
                            .mt(px(1.)),
                    )
                }),
        )
}

fn format_event_time(ts: f64) -> String {
    if let Some(dt) = Local.timestamp_opt(ts as i64, 0).single() {
        dt.format("%H:%M").to_string()
    } else {
        String::new()
    }
}
