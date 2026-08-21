//! Expanded island: Nook (media / calendar / mirror) vs Tray (files).

use super::media::nook_media_pane;
use super::{Island, Tab};
use crate::icons::lucide_color;
use crate::theme;
use crate::widgets::{
    agents_card, calendar_card, notes_card, observe_card, reminders_card, speed_card, timer_card,
};
use gpui::{
    div, prelude::*, px, rgba, AnyElement, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, ScrollWheelEvent,
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
                    .overflow_hidden()
                    .child(if tab == Tab::Widgets {
                        self.render_nook(cx).into_any_element()
                    } else {
                        div()
                            .size_full()
                            .px(px(theme::EXPANDED_PAD))
                            .pb(px(theme::EXPANDED_PAD))
                            .child(self.render_files(cx))
                            .into_any_element()
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
            .px(px(theme::NOOK_INSET))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                this.on_wheel(event, cx);
            }))
            .child(tab_switch(widgets_active, cx))
            .child(div().w(px(notch_w)))
            .child(
                div()
                    .id("settings-btn")
                    .size(px(theme::HIT_MIN))
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.opacity(0.85))
                    .active(|s| s.opacity(0.7))
                    .cursor(CursorStyle::PointingHand)
                    .child(lucide_color("settings", 16.0, theme::SECONDARY_LABEL))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_settings(cx);
                        }),
                    ),
            )
    }

    fn render_nook(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div()
            .id("nook-row")
            .flex()
            .flex_row()
            .h_full()
            .px(px(theme::NOOK_INSET))
            .pb(px(theme::NOOK_INSET))
            .overflow_x_scroll()
            .on_scroll_wheel(cx.listener(|_, _: &ScrollWheelEvent, _, cx| {
                cx.stop_propagation();
            }));

        let mut first = true;
        let mut push = |row: gpui::Div, child: AnyElement| {
            let row = if first {
                first = false;
                row
            } else {
                row.child(pane_divider())
            };
            row.child(child)
        };

        if self.settings.show_media {
            row = push(row, nook_media_pane(&self.now_playing, cx).into_any_element());
        }
        if self.settings.show_calendar {
            row = push(
                row,
                calendar_card(&self.events, self.calendar_day, cx).into_any_element(),
            );
        }
        row = push(row, mirror_pane(cx).into_any_element());

        if self.settings.show_agents {
            row = push(row, agents_card(&self.agents, self.pixel_t, cx).into_any_element());
        }
        if self.settings.show_observe {
            row = push(
                row,
                observe_card(
                    &self.observe,
                    &self.settings,
                    self.observe_hover.as_ref(),
                    cx,
                )
                .into_any_element(),
            );
        }
        if self.settings.show_reminders {
            row = push(row, reminders_card(&self.reminders, cx).into_any_element());
        }
        row = push(
            row,
            timer_card(&self.timers, self.timer_composer, cx).into_any_element(),
        );
        row = push(row, notes_card(self, cx).into_any_element());
        row = push(
            row,
            speed_card(
                self.speed_mbps,
                self.speed_progress,
                self.speed_running,
                cx,
            )
            .into_any_element(),
        );
        row
    }
}

fn pane_divider() -> impl IntoElement {
    div()
        .w(px(1.))
        .h_full()
        .mx(px(12.))
        .bg(rgba(0xffffff1A))
        .flex_shrink_0()
}

fn tab_switch(widgets_active: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .child(labeled_tab(
            "tab-nook",
            "map-pin",
            "Nook",
            widgets_active,
            cx,
            Tab::Widgets,
        ))
        .child(labeled_tab(
            "tab-tray",
            "files",
            "Tray",
            !widgets_active,
            cx,
            Tab::Files,
        ))
}

fn labeled_tab(
    id: &'static str,
    icon: &'static str,
    title: &'static str,
    selected: bool,
    cx: &mut Context<Island>,
    tab: Tab,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded_full()
        .when(selected, |d| d.bg(rgba(0xffffff18)))
        .hover(|s| {
            if selected {
                s
            } else {
                s.bg(rgba(0xffffff0D))
            }
        })
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(lucide_color(
            icon,
            13.0,
            if selected {
                theme::LABEL
            } else {
                theme::SECONDARY_LABEL
            },
        ))
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if selected {
                    theme::LABEL
                } else {
                    theme::SECONDARY_LABEL
                })
                .child(title),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.tab = tab;
                cx.notify();
            }),
        )
}

fn mirror_pane(cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("mirror-pane")
        .flex_shrink_0()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .px(px(8.))
        .child(
            div()
                .id("mirror-btn")
                .size(px(88.))
                .rounded_full()
                .bg(rgba(0xffffff14))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(rgba(0xffffff1F)))
                .active(|s| s.opacity(0.9))
                .child(lucide_color("webcam", 22.0, theme::SECONDARY_LABEL))
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::SECONDARY_LABEL)
                        .child("Mirror"),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _: &MouseDownEvent, _, _| {
                        crate::platform::open_mirror();
                    }),
                ),
        )
}
