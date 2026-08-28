//! Expanded island: Nook (media / calendar / mirror) vs Tray (files).

use super::media::nook_media_pane;
use super::{Island, Tab};
use crate::icons::lucide_color;
use crate::theme;
use crate::widgets::{
    agents_card, battery_card, calendar_card, notes_card, observe_card, reminders_card, speed_card,
    timer_card,
};
use gpui::{
    div, img, prelude::*, px, rgba, AnyElement, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, ObjectFit, RenderImage, ScrollWheelEvent,
};
use nook_core::settings::WidgetModule;

impl Island {
    pub(super) fn render_expanded(
        &mut self,
        notch_w: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tab = if self.tab == Tab::Files && !self.settings.show_files {
            Tab::Widgets
        } else {
            self.tab
        };
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
            .child(tab_switch(widgets_active, self.settings.show_files, cx))
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
        let mut kids: Vec<AnyElement> = Vec::new();
        let add = |kids: &mut Vec<AnyElement>, child: AnyElement| {
            if !kids.is_empty() {
                kids.push(pane_divider().into_any_element());
            }
            kids.push(child);
        };

        for module in self.settings.ordered_widgets() {
            match module {
                WidgetModule::Music if self.settings.show_media => add(
                    &mut kids,
                    cell_pane(
                        self.settings.cells_for(module),
                        nook_media_pane(&self.now_playing, cx),
                    ),
                ),
                WidgetModule::Calendar if self.settings.show_calendar => add(
                    &mut kids,
                    cell_pane(
                        self.settings.cells_for(module),
                        calendar_card(&self.events, self.calendar_day, cx),
                    ),
                ),
                WidgetModule::Mirror if self.settings.show_mirror => add(
                    &mut kids,
                    cell_pane(self.settings.cells_for(module), mirror_pane(self, cx)),
                ),
                WidgetModule::Agents if self.settings.show_agents => add(
                    &mut kids,
                    cell_pane(
                        self.settings.cells_for(module),
                        agents_card(&self.agents, self.pixel_t, cx),
                    ),
                ),
                WidgetModule::Observe if self.settings.show_observe => add(
                    &mut kids,
                    cell_pane(
                        self.settings.cells_for(module),
                        observe_card(
                            &self.observe,
                            &self.settings,
                            self.observe_hover.as_ref(),
                            cx,
                        ),
                    ),
                ),
                WidgetModule::Reminders if self.settings.show_reminders => add(
                    &mut kids,
                    cell_pane(
                        self.settings.cells_for(module),
                        reminders_card(&self.reminders, cx),
                    ),
                ),
                WidgetModule::Timers if self.settings.show_timers => add(
                    &mut kids,
                    cell_pane(
                        self.settings.cells_for(module),
                        timer_card(&self.timers, self.timer_composer, cx),
                    ),
                ),
                WidgetModule::Notes if self.settings.show_notes => add(
                    &mut kids,
                    cell_pane(self.settings.cells_for(module), notes_card(self, cx)),
                ),
                WidgetModule::Speed if self.settings.show_speed => add(
                    &mut kids,
                    cell_pane(
                        self.settings.cells_for(module),
                        speed_card(self.speed_mbps, self.speed_progress, self.speed_running, cx),
                    ),
                ),
                WidgetModule::Battery if self.settings.show_battery => add(
                    &mut kids,
                    cell_pane(self.settings.cells_for(module), battery_card(self, cx)),
                ),
                _ => {}
            }
        }

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
        for child in kids {
            row = row.child(child);
        }
        row
    }
}

fn cell_pane(cells: u8, child: impl IntoElement) -> AnyElement {
    div()
        .w(px(cells as f32 * theme::NOOK_CELL))
        .h_full()
        .flex_shrink_0()
        .overflow_hidden()
        .child(child)
        .into_any_element()
}

fn pane_divider() -> impl IntoElement {
    div()
        .w(px(1.))
        .h_full()
        .mx(px(12.))
        .bg(rgba(0xffffff1A))
        .flex_shrink_0()
}

fn tab_switch(
    widgets_active: bool,
    show_files: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let mut row = div().flex().items_center().gap(px(4.)).child(labeled_tab(
        "tab-nook",
        "map-pin",
        "Nook",
        widgets_active || !show_files,
        cx,
        Tab::Widgets,
    ));
    if show_files {
        row = row.child(labeled_tab(
            "tab-tray",
            "files",
            "Tray",
            !widgets_active,
            cx,
            Tab::Files,
        ));
    }
    row
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
        .hover(|s| if selected { s } else { s.bg(rgba(0xffffff0D)) })
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

fn mirror_pane(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let live = island.mirror_on;
    let frame = island.mirror_frame.clone();
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
                .size(px(theme::MIRROR_FACE))
                .rounded_full()
                .bg(rgba(0xffffff14))
                .cursor(CursorStyle::PointingHand)
                .hover(|s| if live { s } else { s.bg(rgba(0xffffff1F)) })
                .active(|s| s.opacity(0.9))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.toggle_mirror(cx);
                    }),
                )
                .when(live, |d| d.child(mirror_frame_el(frame)))
                .when(!live, |d| {
                    d.flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(8.))
                        .child(lucide_color("webcam", 28.0, theme::SECONDARY_LABEL))
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(15.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::SECONDARY_LABEL)
                                .child("Mirror"),
                        )
                }),
        )
}

fn mirror_frame_el(frame: Option<std::sync::Arc<RenderImage>>) -> AnyElement {
    match frame {
        Some(image) => {
            // Fill, not Cover: Cover paints a larger quad so the corner
            // radii miss the visible box and the frame sticks out square.
            // RenderImage (not Image): JPEG assets flash a 200ms loading
            // placeholder on every camera tick, which looks like a reinit.
            img(image)
                .id("mirror-video")
                .size(px(theme::MIRROR_FACE))
                .rounded_full()
                .object_fit(ObjectFit::Fill)
                .into_any_element()
        }
        None => div()
            .size(px(theme::MIRROR_FACE))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .child(lucide_color("webcam", 28.0, theme::LABEL))
            .into_any_element(),
    }
}
