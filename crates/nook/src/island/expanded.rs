//! Expanded island: top bar (tabs + settings) and widget row vs files tab.

use super::media::media_card;
use super::{Island, Tab};
use crate::icons::lucide;
use crate::icons::lucide_color;
use crate::theme;
use crate::widgets::{
    agents_card, calendar_card, notes_card, observe_card, reminders_card, speed_card, timer_card,
};
use gpui::{
    div, prelude::*, px, Context, CursorStyle, MouseButton, MouseDownEvent, ScrollWheelEvent,
};

impl Island {
    pub(super) fn render_expanded(
        &mut self,
        notch_w: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tab = self.tab;
        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .child(self.render_topbar(notch_w, cx))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .px(px(theme::CONTENT_INSET))
                    // Breathing room under the tab switch / settings button so
                    // the cards don't read as part of the top row.
                    .pt_3()
                    .pb(px(theme::EXPANDED_RADIUS))
                    .overflow_hidden()
                    .child(if tab == Tab::Widgets {
                        self.render_widgets(cx).into_any_element()
                    } else {
                        self.render_files(cx).into_any_element()
                    }),
            )
    }

    fn render_topbar(&self, notch_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let widgets_active = self.tab == Tab::Widgets;
        div()
            .w_full()
            .flex_shrink_0()
            .h(px(self.notch_height.max(32.0)))
            .flex()
            .items_center()
            .justify_between()
            .px(px(theme::CONTENT_INSET))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                this.on_wheel(event, cx);
            }))
            .child(tab_switch(widgets_active, cx))
            .child(div().w(px(notch_w)))
            .child(
                div()
                    .id("settings-btn")
                    .size(px(theme::HIT_MIN))
                    .rounded_full()
                    .bg(theme::FILL)
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(theme::FILL_SECONDARY))
                    .active(|s| s.opacity(0.85))
                    .cursor(CursorStyle::PointingHand)
                    .child(lucide("settings", 14.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_settings(cx);
                        }),
                    ),
            )
    }

    fn render_widgets(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div()
            .id("widgets-row")
            .flex()
            .flex_row()
            .items_start()
            .gap_3()
            .h_full()
            .overflow_x_scroll()
            .on_scroll_wheel(cx.listener(|_, _: &ScrollWheelEvent, _, cx| {
                cx.stop_propagation();
            }));

        if self.settings.show_media && self.has_media() {
            row = row.child(media_card(&self.now_playing, cx));
        }
        if self.settings.show_agents {
            row = row.child(agents_card(&self.agents, self.pixel_t, cx));
        }
        if self.settings.show_observe {
            row = row.child(observe_card(
                &self.observe,
                &self.settings,
                self.observe_hover.as_ref(),
                cx,
            ));
        }
        if self.settings.show_calendar {
            row = row.child(calendar_card(&self.events, cx));
        }
        if self.settings.show_reminders {
            row = row.child(reminders_card(&self.reminders, cx));
        }
        row = row.child(timer_card(&self.timers, cx));
        row = row.child(notes_card(&self.notes, cx));
        row = row.child(speed_card(
            self.speed_mbps,
            self.speed_progress,
            self.speed_running,
            cx,
        ));
        row
    }
}

fn tab_switch(widgets_active: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .relative()
        .flex()
        .bg(theme::FILL)
        .rounded_full()
        .p(px(2.))
        .w(px(96.))
        .h(px(theme::HIT_MIN))
        .child(tab_segment(
            "tab-widgets",
            "layout-grid",
            widgets_active,
            cx,
            Tab::Widgets,
        ))
        .child(tab_segment(
            "tab-files",
            "files",
            !widgets_active,
            cx,
            Tab::Files,
        ))
}

fn tab_segment(
    id: &'static str,
    icon: &'static str,
    selected: bool,
    cx: &mut Context<Island>,
    tab: Tab,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(selected, |d| d.bg(theme::FILL_SECONDARY))
        .hover(|s| {
            if selected {
                s
            } else {
                s.bg(theme::FILL_TERTIARY)
            }
        })
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            icon,
            14.0,
            if selected {
                theme::LABEL
            } else {
                theme::SECONDARY_LABEL
            },
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.tab = tab;
                cx.notify();
            }),
        )
}
