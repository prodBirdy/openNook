//! Upcoming calendar events card.

use crate::island::ui::{format_ts, label, text_btn, widget_shell};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, Context};
use nook_core::calendar::CalendarEvent;

pub(crate) fn calendar_card(
    events: &[CalendarEvent],
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_1();
    if events.is_empty() {
        body = body
            .child(label("No upcoming events", theme::CALLOUT, true))
            .child(label(
                "Open Calendar to add one.",
                theme::SUBHEADLINE,
                false,
            ));
    } else {
        for event in events.iter().take(4) {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .child(label(event.title.clone(), theme::CALLOUT, true).w_full())
                    .child(label(format_ts(event.start_date), theme::SUBHEADLINE, false).w_full()),
            );
        }
    }
    widget_shell(
        "calendar",
        "Calendar",
        div().flex().flex_col().gap_2().child(body).child(text_btn(
            "Open Calendar",
            cx,
            |_, _, _| {
                nook_core::runtime().spawn(async {
                    let _ = nook_core::calendar::open_calendar_app().await;
                });
            },
        )),
    )
}
