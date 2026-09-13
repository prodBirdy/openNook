//! Expanded island: Nook (media / calendar / mirror) vs Tray (files).

use super::edit::{edit_chrome, empty_edit_slot};
use super::media::nook_media_pane;
use super::ui::{empty_state, pill_btn};
use super::{CompactMode, Island, Tab};
use crate::icons::lucide_color;
use crate::theme;
use crate::widgets::{
    agents_card, battery_card, calendar_card, high_alert_card, meeting_card, messages_card,
    notes_card, notifications_card, observe_card, obsidian_card, recorder_card, reminders_card,
    speed_card, sysstats_card, terminal_card, timer_card, vpn_card, weather_card,
};
use gpui::{
    div, img, prelude::*, px, AnyElement, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, ObjectFit, RenderImage, ScrollWheelEvent,
};
use nook_core::settings::{AppSettings, WidgetModule};

/// Browse rows at or above this cell sum stretch to fill the island width.
/// Below it, panes stay proportional and centered (single-row layout).
const ROW_STRETCH_MIN_CELLS: u8 = AppSettings::TOTAL_CELLS - 2;

impl Island {
    /// Effective Nook cell width for the current expanded island width.
    /// Shared by browse and customize so partial rows do not stretch or clip.
    pub(super) fn nook_cell_width(&self) -> f32 {
        let avail = self.expanded_width() - 2.0 * theme::NOOK_INSET;
        // Allowance for ~6 pane dividers on a wide single row.
        (avail - 5.0 * theme::NOOK_DIVIDER) / AppSettings::TOTAL_CELLS as f32
    }

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
                div().flex_1().w_full().overflow_hidden().child(match tab {
                    Tab::Widgets => self.render_nook(cx).into_any_element(),
                    Tab::Files => div()
                        .size_full()
                        .px(px(theme::EXPANDED_PAD))
                        .pb(px(theme::EXPANDED_PAD))
                        .child(self.render_files(cx))
                        .into_any_element(),
                    Tab::Terminal => terminal_card(self, cx).into_any_element(),
                }),
            )
            .into_any_element()
    }

    fn render_topbar(&self, notch_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        if self.widget_edit {
            return div()
                .w_full()
                .flex_shrink_0()
                .h(px(self.notch_height.max(theme::NOTCH_MIN_H)))
                .flex()
                .items_center()
                .justify_between()
                .px(px(theme::NOOK_INSET))
                .child(
                    div()
                        .text_size(px(theme::SUBHEADLINE.size))
                        .line_height(px(theme::SUBHEADLINE.leading))
                        .font_weight(theme::SUBHEADLINE.weight)
                        .text_color(theme::secondary_label())
                        .child("Customize"),
                )
                .child(div().w(px(notch_w)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(super::edit::edit_action_btn(
                            "widget-edit-cancel",
                            "Cancel",
                            theme::FILL,
                            theme::LABEL,
                            cx,
                            |this, cx| this.cancel_widget_edit(cx),
                        ))
                        .child(super::edit::edit_action_btn(
                            "widget-edit-done",
                            "Done",
                            theme::accent(),
                            theme::LABEL,
                            cx,
                            |this, cx| this.finish_widget_edit(cx),
                        )),
                )
                .into_any_element();
        }

        div()
            .w_full()
            .flex_shrink_0()
            .h(px(self.notch_height.max(theme::NOTCH_MIN_H)))
            .flex()
            .items_center()
            .justify_between()
            .px(px(theme::NOOK_INSET))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                this.on_wheel(event, cx);
            }))
            .child(tab_switch(self, cx))
            .child(div().w(px(notch_w)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .id("customize-widgets-btn")
                            .size(px(theme::HIT_MIN))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.opacity(0.85))
                            .active(|s| s.opacity(0.7))
                            .cursor(CursorStyle::PointingHand)
                            .child(lucide_color("layout-grid", 16.0, theme::secondary_label()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.begin_widget_edit(cx);
                                }),
                            ),
                    )
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
                            .child(lucide_color("settings", 16.0, theme::secondary_label()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.open_settings(cx);
                                }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_nook(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.widget_edit && self.has_incoming_message() && self.mode() == CompactMode::Messages
        {
            return div()
                .id("nook-row")
                .size_full()
                .px(px(theme::NOOK_INSET))
                .pb(px(theme::NOOK_INSET))
                .child(messages_card(self, cx))
                .into_any_element();
        }
        if !self.widget_edit && self.settings.show_recorder && self.mode() == CompactMode::Recording
        {
            return div()
                .id("nook-row")
                .size_full()
                .px(px(theme::NOOK_INSET))
                .pb(px(theme::NOOK_INSET))
                .child(recorder_card(self, cx))
                .into_any_element();
        }
        let reminders_qa = if self.settings.quick_add && self.settings.show_reminders {
            Some(self.ensure_reminders_quick_add(cx))
        } else {
            None
        };
        let editing = self.widget_edit;
        let mut panes: Vec<(WidgetModule, u8, AnyElement)> = Vec::new();
        let push = |panes: &mut Vec<(WidgetModule, u8, AnyElement)>,
                    module: WidgetModule,
                    cells: u8,
                    child: AnyElement| { panes.push((module, cells, child)) };

        for (module, cells) in self.visible_nook_items() {
            match module {
                WidgetModule::Music => push(
                    &mut panes,
                    module,
                    cells,
                    nook_media_pane(self, cx).into_any_element(),
                ),
                WidgetModule::Calendar => push(
                    &mut panes,
                    module,
                    cells,
                    calendar_card(&self.events, self.calendar_day, cx).into_any_element(),
                ),
                WidgetModule::Mirror => push(
                    &mut panes,
                    module,
                    cells,
                    mirror_pane(self, cx).into_any_element(),
                ),
                WidgetModule::Agents => push(
                    &mut panes,
                    module,
                    cells,
                    agents_card(
                        &self.agents,
                        self.pixel_t,
                        theme::island_fill(self.settings.island_color),
                        self.size_morphing(),
                        cx,
                    )
                    .into_any_element(),
                ),
                WidgetModule::Meeting => push(
                    &mut panes,
                    module,
                    cells,
                    meeting_card(&self.meeting, cx).into_any_element(),
                ),
                WidgetModule::Observe => push(
                    &mut panes,
                    module,
                    cells,
                    observe_card(
                        &self.observe,
                        &self.settings,
                        self.observe_hover.as_ref(),
                        cx,
                    )
                    .into_any_element(),
                ),
                WidgetModule::Reminders => push(
                    &mut panes,
                    module,
                    cells,
                    reminders_card(&self.reminders, reminders_qa.clone(), cx).into_any_element(),
                ),
                WidgetModule::Timers => push(
                    &mut panes,
                    module,
                    cells,
                    timer_card(self, cx).into_any_element(),
                ),
                WidgetModule::Notes => push(
                    &mut panes,
                    module,
                    cells,
                    notes_card(self, cx).into_any_element(),
                ),
                WidgetModule::Obsidian => push(
                    &mut panes,
                    module,
                    cells,
                    obsidian_card(self, cx).into_any_element(),
                ),
                WidgetModule::Speed => push(
                    &mut panes,
                    module,
                    cells,
                    speed_card(self.speed_mbps, self.speed_progress, self.speed_running, cx)
                        .into_any_element(),
                ),
                WidgetModule::Battery => push(
                    &mut panes,
                    module,
                    cells,
                    battery_card(self, cx).into_any_element(),
                ),
                WidgetModule::Messages => push(
                    &mut panes,
                    module,
                    cells,
                    messages_card(self, cx).into_any_element(),
                ),
                WidgetModule::Weather => push(
                    &mut panes,
                    module,
                    cells,
                    weather_card(self, cx).into_any_element(),
                ),
                WidgetModule::Vpn => push(
                    &mut panes,
                    module,
                    cells,
                    vpn_card(&self.vpn).into_any_element(),
                ),
                WidgetModule::HighAlert => push(
                    &mut panes,
                    module,
                    cells,
                    high_alert_card(self, cx).into_any_element(),
                ),
                WidgetModule::SysStats => push(
                    &mut panes,
                    module,
                    cells,
                    sysstats_card(self, cx).into_any_element(),
                ),
                WidgetModule::Recorder => push(
                    &mut panes,
                    module,
                    cells,
                    recorder_card(self, cx).into_any_element(),
                ),
                WidgetModule::Notifications => push(
                    &mut panes,
                    module,
                    cells,
                    notifications_card(&self.notifications, cx).into_any_element(),
                ),
                WidgetModule::Files => {}
            }
        }

        let queue_extra = self.queue_extra_width();
        let cell_w = self.nook_cell_width();
        let mut packed = pack_pane_rows(panes, AppSettings::TOTAL_CELLS);
        packed.truncate(AppSettings::MAX_ROWS);
        // Remainder from the last packed visible row (not settings.remaining_cells,
        // which includes widgets the lockup paths may hide).
        let (_, last_remaining) = self.nook_row_count_for_render();
        let remaining = if editing { last_remaining } else { 0 };
        let last_row_full = !packed.is_empty() && remaining == 0;
        let append_new_row = editing && last_row_full && packed.len() < AppSettings::MAX_ROWS;
        // Customize always needs a drop row when nothing is enabled yet.
        if editing && packed.is_empty() {
            packed.push(Vec::new());
        }
        if append_new_row {
            packed.push(Vec::new());
        }

        let mut grid = div()
            .id("nook-row")
            .flex()
            .flex_col()
            .w_full()
            .flex_1()
            .min_h(px(0.))
            .px(px(theme::NOOK_INSET))
            .pt(px(0.))
            .pb(px(theme::NOOK_INSET))
            .when(!editing, |d| d.overflow_hidden())
            .when(!editing, |d| {
                d.on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                    this.on_wheel(event, cx);
                }))
            });

        if packed.is_empty() {
            grid = grid.child(
                empty_state(
                    "No widgets enabled",
                    pill_btn("Customize", cx, |this, _, cx| this.begin_widget_edit(cx)),
                )
                .into_any_element(),
            );
        } else {
            let row_count = packed.len();
            for (ri, row_panes) in packed.into_iter().enumerate() {
                if ri > 0 {
                    grid = grid.child(
                        div()
                            .h(px(1.))
                            .w_full()
                            .my(px(theme::CONTENT_INSET))
                            .bg(if editing {
                                theme::with_alpha(theme::SEPARATOR, 0.0)
                            } else {
                                theme::SEPARATOR
                            })
                            .flex_shrink_0(),
                    );
                }
                let is_last = ri + 1 == row_count;
                let row_was_empty = row_panes.is_empty();
                let row_cells = row_panes
                    .iter()
                    .map(|(_, cells, _)| *cells)
                    .fold(0u8, |sum, cells| sum.saturating_add(cells));
                let row_stretches = row_cells >= ROW_STRETCH_MIN_CELLS;
                let mut row = div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .h(px(theme::NOOK_BODY))
                    .flex_shrink_0()
                    .when(editing || row_stretches, |d| d.justify_start())
                    .when(!editing && !row_stretches, |d| d.justify_center())
                    .when(editing, |d| d.gap(px(12.)));

                for (i, (module, cells, child)) in row_panes.into_iter().enumerate() {
                    if !editing && i > 0 {
                        row = row.child(pane_divider());
                    }
                    let child = if editing {
                        edit_chrome(module, child, cx)
                    } else {
                        child
                    };
                    if editing {
                        let width = cells as f32 * cell_w;
                        row = row.child(cell_pane(width, child));
                    } else if row_stretches {
                        let basis = if module == WidgetModule::Music {
                            queue_extra
                        } else {
                            0.0
                        };
                        let mut pane = div()
                            .h_full()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .flex_basis(px(basis))
                            .child(child);
                        pane.style().flex_grow = Some(cells as f32);
                        pane.style().flex_shrink = Some(1.0);
                        row = row.child(pane);
                    } else {
                        let width = cells as f32 * cell_w
                            + if module == WidgetModule::Music {
                                queue_extra
                            } else {
                                0.0
                            };
                        row = row.child(
                            div()
                                .w(px(width))
                                .h_full()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .flex_shrink_0()
                                .child(child),
                        );
                    }
                }

                // Leftover budget → dashed drop slot spanning the free cells;
                // a fresh empty row (full last row under MAX_ROWS) flex-fills.
                if editing && is_last && (remaining > 0 || row_was_empty) {
                    let slot_w = if row_was_empty {
                        0.0
                    } else {
                        remaining as f32 * cell_w
                    };
                    row = row.child(empty_edit_slot(slot_w, cx));
                }

                grid = grid.child(row);
            }
        }

        if editing {
            div()
                .id("nook-edit")
                .flex()
                .flex_col()
                .size_full()
                .overflow_hidden()
                .child(grid)
                .child(self.render_widget_edit_picker(cx))
                .into_any_element()
        } else {
            grid.into_any_element()
        }
    }
}

/// Greedy first-fit pack matching `nook_core::settings::pack_rows`, for panes.
fn pack_pane_rows(
    panes: Vec<(WidgetModule, u8, AnyElement)>,
    cap: u8,
) -> Vec<Vec<(WidgetModule, u8, AnyElement)>> {
    let mut rows: Vec<Vec<(WidgetModule, u8, AnyElement)>> = Vec::new();
    let mut used: u8 = 0;
    for (module, raw, child) in panes {
        let cells = raw.min(cap);
        if rows.is_empty() || used.saturating_add(cells) > cap {
            rows.push(Vec::new());
            used = 0;
        }
        used = used.saturating_add(cells);
        rows.last_mut().unwrap().push((module, cells, child));
    }
    rows
}

fn cell_pane(width: f32, child: impl IntoElement) -> AnyElement {
    div()
        .w(px(width))
        .h_full()
        .min_w(px(0.))
        .flex_shrink_0()
        // Edit − badges hang outside the dashed frame; don't clip them.
        .child(child)
        .into_any_element()
}

fn pane_divider() -> impl IntoElement {
    div()
        .w(px(1.))
        .h_full()
        .mx(px(theme::CONTENT_INSET))
        .bg(theme::SEPARATOR)
        .flex_shrink_0()
}

fn tab_switch(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let current = island.tab;
    let mut row = div().flex().items_center().gap(px(4.)).child(labeled_tab(
        "tab-nook",
        "map-pin",
        "Nook",
        current == Tab::Widgets,
        cx,
        Tab::Widgets,
    ));
    if island.settings.show_files {
        row = row.child(labeled_tab(
            "tab-tray",
            "files",
            "Tray",
            current == Tab::Files,
            cx,
            Tab::Files,
        ));
    }
    if island.settings.terminal_enabled {
        row = row.child(labeled_tab(
            "tab-term",
            "terminal",
            "Term",
            current == Tab::Terminal,
            cx,
            Tab::Terminal,
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
        .h(px(theme::HIT_MIN))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded_full()
        .when(selected, |d| d.bg(theme::FILL))
        .hover(|s| {
            if selected {
                s.bg(theme::FILL_SECONDARY)
            } else {
                s.bg(theme::FILL_TERTIARY)
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
                theme::secondary_label()
            },
        ))
        .child(
            div()
                .text_size(px(theme::CALLOUT.size))
                .line_height(px(theme::CALLOUT.leading))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if selected {
                    theme::LABEL
                } else {
                    theme::secondary_label()
                })
                .child(title),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.tab = tab;
                this.arm_content_transition();
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
                .bg(theme::FILL_TERTIARY)
                .cursor(CursorStyle::PointingHand)
                .hover(|s| if live { s } else { s.bg(theme::FILL) })
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
                        .child(lucide_color("webcam", 28.0, theme::secondary_label()))
                        .child(
                            div()
                                .text_size(px(theme::CALLOUT.size))
                                .line_height(px(theme::CALLOUT.leading))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::secondary_label())
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
