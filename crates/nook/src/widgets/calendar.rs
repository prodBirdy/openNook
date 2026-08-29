//! Week strip + empty/event state for the Nook calendar pane.

use crate::island::ui::{nook_accent_bar, nook_empty};
use crate::island::Island;
use crate::theme;
use chrono::{Datelike, Local, TimeZone, Weekday};
use gpui::{
    div, prelude::*, px, rgba, Context, FontWeight, MouseButton, MouseDownEvent, SharedString,
};
use nook_core::calendar::CalendarEvent;

const WEEKEND: gpui::Rgba = gpui::Rgba {
    r: 0.78,
    g: 0.42,
    b: 0.42,
    a: 1.0,
};

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

    let empty_copy = if is_today {
        "Nothing for today"
    } else {
        "No events"
    };
    let body = if filtered.is_empty() {
        nook_empty("calendar", empty_copy).into_any_element()
    } else {
        let mut col = div().flex().flex_col().gap_2().pt(px(8.));
        for event in filtered.into_iter().take(2) {
            col = col.child(event_row(event, cx));
        }
        col.into_any_element()
    };

    div()
        .id("nook-calendar")
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .text_size(px(32.))
                        .line_height(px(36.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::LABEL)
                        .child(month),
                )
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
    let number_color = if selected {
        theme::accent()
    } else if weekend {
        WEEKEND
    } else {
        theme::LABEL
    };
    let label_color = if selected {
        theme::accent()
    } else if weekend {
        WEEKEND
    } else {
        theme::SECONDARY_LABEL
    };
    div()
        .id(SharedString::from(format!("cal-day-{index}")))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.calendar_day = index;
                cx.notify();
            }),
        )
        .child(
            div()
                .text_size(px(9.))
                .line_height(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(label_color)
                .child(weekday),
        )
        .child(
            div()
                .text_size(px(15.))
                .line_height(px(18.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(number_color)
                .child(format!("{day:02}")),
        )
}

fn event_row(event: &CalendarEvent, cx: &mut Context<Island>) -> impl IntoElement {
    let id = event.id.clone();
    let date = event.start_date;
    let time = if event.is_all_day {
        "ALL DAY".to_string()
    } else {
        format_event_time(event.start_date)
    };
    div()
        .id(SharedString::from(format!("cal-ev-{id}")))
        .flex()
        .items_center()
        .py_2()
        .border_b_1()
        .border_color(rgba(0xFFFFFF0D))
        .cursor(gpui::CursorStyle::PointingHand)
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
                .child(
                    div()
                        .text_size(px(if event.is_all_day { 10. } else { 13. }))
                        .font_weight(if event.is_all_day {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(if event.is_all_day {
                            rgba(0xffffff99)
                        } else {
                            rgba(0xffffffe6)
                        })
                        .child(time),
                ),
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
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::LABEL)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(event.title.clone()),
                )
                .when_some(event.location.clone(), |d, loc| {
                    d.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgba(0xffffff80))
                            .mt(px(1.))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(loc),
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
