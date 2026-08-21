//! Reminders card — tap a row to complete it.

use crate::island::ui::{card_row, label, slide_label, widget_shell};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, Context, MouseButton, MouseDownEvent, SharedString};
use nook_core::calendar::Reminder;

pub(crate) fn reminders_card(reminders: &[Reminder], cx: &mut Context<Island>) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_1();
    let open: Vec<_> = reminders
        .iter()
        .filter(|r| !r.is_completed)
        .take(4)
        .collect();
    if open.is_empty() {
        body = body
            .child(label("No reminders", theme::CALLOUT, true))
            .child(label(
                "Reminders you add on this Mac appear here.",
                theme::SUBHEADLINE,
                false,
            ));
    } else {
        for reminder in open {
            let id = reminder.id.clone();
            body = body.child(
                card_row(SharedString::from(format!("rem-{id}")))
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
                    // The row itself carries the 28 pt target now, so the
                    // circle no longer needs a hit-sized wrapper stealing width
                    // from the title in a 220 pt card.
                    .child(
                        div()
                            .flex_none()
                            .size(px(16.))
                            .rounded_full()
                            .border_1()
                            .border_color(theme::SECONDARY_LABEL),
                    )
                    .child(slide_label(reminder.title.clone(), theme::CALLOUT, true).w_full()),
            );
        }
    }
    widget_shell("reminders-scroll", body)
}
