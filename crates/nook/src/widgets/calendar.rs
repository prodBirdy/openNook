//! Date head + event list for the Nook calendar pane (Pencil mockup).

use crate::icons::lucide_color;
use crate::island::ui::{
    label, nook_empty, open_privacy_pane, scroll_body, text_btn, timer_text,
};
use crate::island::Island;
use crate::theme;
use chrono::{Datelike, Local, TimeZone};
use gpui::{
    div, prelude::*, px, AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent,
    SharedString,
};
use nook_core::calendar::CalendarEvent;

/// Mockup event bars: systemBlue / systemOrange (dark), fixed — not the user accent.
const BAR_BLUE: u32 = 0x0A84FF;
const BAR_ORANGE: u32 = 0xFF9F0A;

/// Compact leading: red calendar glyph in a 22pt box.
#[allow(dead_code)]
pub(crate) fn compact_left() -> AnyElement {
    div()
        .size(px(22.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color("calendar", 17.0, theme::DESTRUCTIVE))
        .into_any_element()
}

/// Compact trailing: next upcoming event start time (BODY semibold).
#[allow(dead_code)]
pub(crate) fn compact_right(events: &[CalendarEvent]) -> AnyElement {
    let now = Local::now().timestamp() as f64;
    let text = events
        .iter()
        .filter(|e| !e.is_all_day && e.start_date >= now)
        .min_by(|a, b| {
            a.start_date
                .partial_cmp(&b.start_date)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|e| format_event_time(e.start_date))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "—".into());
    timer_text(text, theme::BODY).into_any_element()
}

pub(crate) fn calendar_card(
    events: &[CalendarEvent],
    selected_day: u8,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let today = Local::now().date_naive();
    let selected = selected_day.min(6);
    let selected_date = today + chrono::Duration::days(selected as i64 - 3);
    let is_today = selected == 3;
    let weekday = selected_date.format("%A").to_string().to_uppercase();
    let day_num = selected_date.day().to_string();

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
        label(empty_copy, theme::CALLOUT, false)
            .text_color(theme::TERTIARY_LABEL)
            .into_any_element()
    } else {
        let mut col = div().flex().flex_col().w_full().flex_shrink_0().gap(px(8.));
        for (i, event) in filtered.into_iter().enumerate() {
            col = col.child(event_row(event, i, cx));
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
        .gap(px(10.))
        .p(px(16.))
        .overflow_hidden()
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .justify_center()
                .child(date_head(&weekday, &day_num, selected, cx))
                .child(body),
        )
}

fn date_head(
    weekday: &str,
    day_num: &str,
    selected: u8,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .flex_shrink_0()
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(7.))
                .child(
                    label(weekday.to_string(), theme::FOOTNOTE, true)
                        .text_color(theme::DESTRUCTIVE)
                        .line_height(px(12.)),
                )
                .child(
                    div()
                        .text_size(px(theme::TITLE_2.size))
                        .line_height(px(20.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme::LABEL)
                        .child(day_num.to_string()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(day_chevron("cal-prev", "chevron-left", selected, -1, cx))
                .child(day_chevron("cal-next", "chevron-right", selected, 1, cx)),
        )
}

fn day_chevron(
    id: &'static str,
    icon: &'static str,
    selected: u8,
    delta: i8,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let next = (selected as i8 + delta).clamp(0, 6) as u8;
    div()
        .id(id)
        .size(px(20.))
        .rounded(px(10.))
        .bg(theme::FILL_TERTIARY)
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.calendar_day = next;
                cx.notify();
            }),
        )
        .child(lucide_color(icon, 12.0, theme::secondary_label()))
}

fn same_day(ts: f64, day: chrono::NaiveDate) -> bool {
    Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .is_some_and(|dt| dt.date_naive() == day)
}

fn event_row(event: &CalendarEvent, index: usize, cx: &mut Context<Island>) -> impl IntoElement {
    let id = event.id.clone();
    let date = event.start_date;
    let bar = theme::rgba_from_u32(if index % 2 == 0 { BAR_BLUE } else { BAR_ORANGE }, 1.0);
    let time = format_event_range(event);
    div()
        .id(SharedString::from(format!("cal-ev-{id}")))
        .w_full()
        .flex()
        .items_center()
        .gap(px(9.))
        .flex_shrink_0()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.9))
        .active(|s| s.opacity(0.8))
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
                .w(px(3.))
                .h(px(29.))
                .rounded(px(1.5))
                .flex_shrink_0()
                .bg(bar),
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
                    label(event.title.clone(), theme::CALLOUT, false)
                        .text_color(theme::LABEL)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .w_full(),
                )
                .child(label(time, theme::FOOTNOTE, false).text_color(theme::secondary_label())),
        )
}

fn format_event_time(ts: f64) -> String {
    if let Some(dt) = Local.timestamp_opt(ts as i64, 0).single() {
        dt.format("%H:%M").to_string()
    } else {
        String::new()
    }
}

/// Expanded row: `10:30 – 11:00` (en dash, spaces). All-day stays `All day`.
fn format_event_range(event: &CalendarEvent) -> String {
    if event.is_all_day {
        return "All day".to_string();
    }
    let start = format_event_time(event.start_date);
    if start.is_empty() {
        return String::new();
    }
    let Some(end_ts) = event.end else {
        return start;
    };
    let end = format_event_time(end_ts);
    if end.is_empty() {
        start
    } else {
        format!("{start} – {end}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(start: f64, end: Option<f64>, all_day: bool) -> CalendarEvent {
        CalendarEvent {
            id: "e".into(),
            title: "Design sync".into(),
            start_date: start,
            end,
            location: None,
            is_all_day: all_day,
        }
    }

    #[test]
    fn event_time_formats_hh_mm() {
        // 2024-01-15 10:30 local — just assert non-empty for a known epoch.
        let s = format_event_time(1_705_311_000.0);
        assert!(!s.is_empty());
        assert!(s.contains(':'));
    }

    #[test]
    fn event_range_uses_en_dash_and_spaces() {
        let start = 1_705_311_000.0;
        let end = start + 30.0 * 60.0;
        let s = format_event_range(&event(start, Some(end), false));
        assert!(s.contains(" – "), "expected en dash with spaces, got {s:?}");
        let parts: Vec<_> = s.split(" – ").collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains(':'));
        assert!(parts[1].contains(':'));
        assert_eq!(format_event_range(&event(start, None, false)), format_event_time(start));
        assert_eq!(format_event_range(&event(start, Some(end), true)), "All day");
    }
}
