//! On-island widget customize mode — Droppy / Control Center style.
//!
//! Active widgets sit in dashed frames with a red − on the corner. Tap a dock
//! chip to add/remove; drag is secondary for place/reorder. Cancel / Done
//! commit or revert.

use super::Island;
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    div, prelude::*, px, AnyElement, App, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, ScrollHandle, ScrollWheelEvent, SharedString, Window,
};
use nook_core::settings::WidgetModule;
use std::cell::RefCell;
use std::time::{Duration, Instant};

const DOCK_ICON: f32 = 44.0;
const DOCK_COL: f32 = 56.0;
const DOCK_GAP: f32 = 10.0;
const DOCK_LABEL_H: f32 = 14.0;

thread_local! {
    static PICKER_SCROLL: RefCell<ScrollHandle> = RefCell::new(ScrollHandle::default());
}

fn picker_scroll() -> ScrollHandle {
    PICKER_SCROLL.with_borrow(|h| h.clone())
}

/// Where a customize drag started.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NookDragKind {
    /// Dock icon → place / replace on a slot.
    Place,
    /// Pane → pane reorder.
    Reorder,
}

/// Drag payload for customize mode.
#[derive(Clone, Copy)]
pub(super) struct NookWidgetDrag {
    pub module: WidgetModule,
    pub kind: NookDragKind,
}

impl gpui::Render for NookWidgetDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match self.kind {
            // Droppy: lifted app icon with a soft glow — no chrome card.
            NookDragKind::Place => div()
                .size(px(DOCK_ICON + 8.0))
                .rounded(px(theme::INNER_RADIUS + 2.0))
                .bg(theme::GROUPED_BG)
                .border_1()
                .border_color(theme::SEPARATOR)
                .shadow_lg()
                .flex()
                .items_center()
                .justify_center()
                .child(lucide_color(module_icon(self.module), 24.0, theme::LABEL)),
            NookDragKind::Reorder => div()
                .h(px(56.))
                .min_w(px(100.))
                .px(px(14.))
                .rounded(px(theme::CONTROL_RADIUS + 6.0))
                .bg(theme::WINDOW_BG)
                .border_1()
                .border_color(theme::SEPARATOR)
                .shadow_lg()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(lucide_color(module_icon(self.module), 18.0, theme::LABEL))
                .child(
                    div()
                        .text_size(px(theme::BODY.size))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::LABEL)
                        .child(module_short(self.module)),
                ),
        }
    }
}

pub(super) fn module_icon(module: WidgetModule) -> &'static str {
    match module {
        WidgetModule::Calendar => "calendar",
        WidgetModule::Music => "music",
        WidgetModule::Files => "files",
        WidgetModule::Notes => "notebook",
        WidgetModule::Observe => "activity",
        WidgetModule::Timers => "clock",
        WidgetModule::Reminders => "list-checks",
        WidgetModule::Speed => "gauge",
        WidgetModule::Agents => "bot",
        WidgetModule::Mirror => "webcam",
        WidgetModule::Battery => "battery",
        WidgetModule::Messages => "message-circle",
        WidgetModule::Obsidian => "book",
        WidgetModule::Weather => "cloud-sun",
        WidgetModule::Vpn => "shield",
        WidgetModule::HighAlert => "sun",
        WidgetModule::SysStats => "activity",
        WidgetModule::Recorder => "mic",
        WidgetModule::Meeting => "video",
        WidgetModule::Notifications => "bell",
    }
}

pub(super) fn module_short(module: WidgetModule) -> &'static str {
    match module {
        WidgetModule::Calendar => "Calendar",
        WidgetModule::Music => "Media",
        WidgetModule::Files => "Files",
        WidgetModule::Notes => "Notes",
        WidgetModule::Observe => "Observe",
        WidgetModule::Timers => "Timers",
        WidgetModule::Reminders => "Reminders",
        WidgetModule::Speed => "Speed",
        WidgetModule::Agents => "Agents",
        WidgetModule::Mirror => "Mirror",
        WidgetModule::Battery => "Battery",
        WidgetModule::Messages => "Messages",
        WidgetModule::Obsidian => "Obsidian",
        WidgetModule::Weather => "Weather",
        WidgetModule::Vpn => "VPN",
        WidgetModule::HighAlert => "Alert",
        WidgetModule::SysStats => "Stats",
        WidgetModule::Recorder => "Voice",
        WidgetModule::Meeting => "Meetings",
        WidgetModule::Notifications => "Notify",
    }
}

impl Island {
    pub(super) fn render_widget_edit_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let modules: Vec<WidgetModule> = self
            .settings
            .ordered_widgets()
            .into_iter()
            .filter(|m| {
                m.occupies_nook_cells() && m.is_available() && self.settings.widget_visible(*m)
            })
            .collect();
        let n = modules.len();
        let chips_w = if n == 0 {
            0.0
        } else {
            n as f32 * DOCK_COL + (n.saturating_sub(1) as f32) * DOCK_GAP
        };
        let row_h = DOCK_ICON + 4.0 + DOCK_LABEL_H;
        let show_budget_hint = self
            .widget_edit_budget_hint_at
            .is_some_and(|t| t.elapsed() < Duration::from_secs(2));

        let mut chips = div()
            .id("widget-edit-picker-row")
            .flex()
            .flex_row()
            .items_start()
            .gap(px(DOCK_GAP))
            .h(px(row_h))
            .w(px(chips_w))
            .flex_shrink_0();

        for module in modules {
            let on = self.settings.is_enabled(module);
            let can = on || self.settings.can_enable(module);
            chips = chips.child(self.picker_chip(module, on, can, cx));
        }

        let scroll = picker_scroll();
        let mut scroller = div()
            .id("widget-edit-picker")
            .track_scroll(&scroll)
            .flex_1()
            .min_w(px(0.))
            .h(px(row_h))
            .overflow_x_scroll()
            .overflow_y_hidden()
            .on_scroll_wheel({
                let scroll = scroll.clone();
                move |event: &ScrollWheelEvent, window: &mut Window, cx: &mut App| {
                    let delta = event.delta.pixel_delta(window.line_height());
                    if scroll.max_offset().width > px(0.5)
                        && (delta.x.abs() > px(0.5) || delta.y.abs() > px(0.5))
                    {
                        cx.stop_propagation();
                    }
                }
            })
            .child(chips);
        scroller.style().restrict_scroll_to_axis = Some(false);

        div()
            .w_full()
            .h(px(theme::WIDGET_EDIT_PICKER_H))
            .flex_shrink_0()
            .px(px(theme::NOOK_INSET))
            .pb(px(if show_budget_hint { 8.0 } else { 14.0 }))
            .pt(px(4.))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(4.))
            .child(scroller)
            .when(show_budget_hint, |d| {
                d.child(
                    div()
                        .w_full()
                        .text_size(px(theme::FOOTNOTE.size))
                        .line_height(px(theme::FOOTNOTE.leading))
                        .text_color(theme::tertiary_label())
                        .child("No room — remove a widget first"),
                )
            })
    }

    fn picker_chip(
        &self,
        module: WidgetModule,
        on: bool,
        can: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let must_replace = !on && !can;
        let drag = NookWidgetDrag {
            module,
            kind: NookDragKind::Place,
        };
        let accent_fill = theme::with_alpha(theme::accent(), 0.18);
        let mut chip = div()
            .id(SharedString::from(format!("pick-{}", module as u8)))
            .w(px(DOCK_COL))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .opacity(if must_replace {
                theme::DISABLED_OPACITY
            } else {
                1.0
            })
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if this.settings.is_enabled(module) {
                    nook_core::settings::tweak_app_settings(|s| {
                        let _ = s.set_enabled(module, false);
                    });
                    this.settings = nook_core::settings::get_app_settings();
                    this.widget_edit_budget_hint_at = None;
                    this.force_content_transition();
                    nook_core::haptics::trigger(None);
                    cx.notify();
                } else if this.settings.can_enable(module) {
                    nook_core::settings::tweak_app_settings(|s| {
                        let _ = s.set_enabled(module, true);
                    });
                    this.settings = nook_core::settings::get_app_settings();
                    this.widget_edit_budget_hint_at = None;
                    this.force_content_transition();
                    nook_core::haptics::trigger(None);
                    cx.notify();
                } else {
                    this.widget_edit_budget_hint_at = Some(Instant::now());
                    nook_core::haptics::trigger(None);
                    cx.notify();
                }
            }));
        // Drag remains a secondary place affordance when the chip fits.
        if !must_replace {
            chip = chip.on_drag(drag, |drag, _, _, cx| cx.new(|_| *drag));
        }
        let accent_hover = theme::with_alpha(theme::accent(), 0.28);
        chip.child(
            div()
                .id(SharedString::from(format!("pick-icon-{}", module as u8)))
                .relative()
                .size(px(DOCK_ICON))
                .rounded(px(theme::INNER_RADIUS))
                .bg(if on { accent_fill } else { theme::FILL })
                .border_1()
                .border_color(if on {
                    theme::accent()
                } else {
                    theme::SEPARATOR
                })
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(accent_hover).border_color(theme::accent()))
                .active(|s| s.opacity(0.85))
                .child(lucide_color(module_icon(module), 20.0, theme::LABEL))
                .when(on, |d| {
                    d.child(
                        div()
                            .absolute()
                            .bottom(px(-3.))
                            .right(px(-3.))
                            .size(px(15.))
                            .rounded_full()
                            .bg(theme::accent())
                            .border_2()
                            .border_color(theme::WINDOW_BG)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(lucide_color("check", 9.0, theme::LABEL)),
                    )
                }),
        )
        .child(
            div()
                .w_full()
                .h(px(DOCK_LABEL_H))
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .child(
                    div()
                        .w_full()
                        .text_size(px(10.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::secondary_label())
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(module_short(module)),
                ),
        )
    }
}

pub(super) fn edit_action_btn(
    id: &'static str,
    caption: &'static str,
    fill: gpui::Rgba,
    text_color: gpui::Rgba,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    // Slightly lighter fill on hover (raise alpha for translucent fills,
    // mix toward white for solid accent).
    let hover_fill = if fill.a < 0.99 {
        gpui::Rgba {
            r: fill.r,
            g: fill.g,
            b: fill.b,
            a: (fill.a + 0.08).min(1.0),
        }
    } else {
        gpui::Rgba {
            r: (fill.r + (1.0 - fill.r) * 0.18).min(1.0),
            g: (fill.g + (1.0 - fill.g) * 0.18).min(1.0),
            b: (fill.b + (1.0 - fill.b) * 0.18).min(1.0),
            a: fill.a,
        }
    };
    div()
        .id(id)
        .h(px(theme::HIT_MIN))
        .px(px(14.))
        .rounded(px(theme::ROW_RADIUS))
        .bg(fill)
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .cursor(CursorStyle::PointingHand)
        .hover(move |s| s.bg(hover_fill))
        .active(|s| s.opacity(0.85))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, cx);
            }),
        )
        .child(
            div()
                .text_size(px(theme::CALLOUT.size))
                .line_height(px(theme::CALLOUT.leading))
                .font_weight(theme::CALLOUT.emphasized)
                .text_color(text_color)
                .child(caption),
        )
}

/// Dashed edit frame + red − on the corner (iOS jiggle / Droppy).
pub(super) fn edit_chrome(
    module: WidgetModule,
    child: AnyElement,
    cx: &mut Context<Island>,
) -> AnyElement {
    let drag = NookWidgetDrag {
        module,
        kind: NookDragKind::Reorder,
    };
    let accent_hi = theme::with_alpha(theme::accent(), 0.80);
    let accent_lo = theme::with_alpha(theme::accent(), 0.16);
    div()
        .id(SharedString::from(format!("edit-pane-{}", module as u8)))
        .relative()
        .size_full()
        .rounded(px(theme::CONTROL_RADIUS + 10.0))
        .border_1()
        .border_dashed()
        .border_color(theme::SEPARATOR)
        .child(
            div()
                .size_full()
                .rounded(px(theme::CONTROL_RADIUS + 9.0))
                .overflow_hidden()
                .child(child),
        )
        .child(
            div()
                .id(SharedString::from(format!("hit-{}", module as u8)))
                .absolute()
                .inset_0()
                .rounded(px(theme::CONTROL_RADIUS + 10.0))
                .occlude()
                .cursor(CursorStyle::OpenHand)
                .drag_over::<NookWidgetDrag>(move |style, drag, _, _| {
                    if drag.module == module {
                        style
                    } else {
                        style.bg(accent_lo).border_color(accent_hi)
                    }
                })
                .can_drop(move |value, _, _| {
                    value
                        .downcast_ref::<NookWidgetDrag>()
                        .is_some_and(|drag| drag.module != module)
                })
                .on_drop(cx.listener(move |this, drag: &NookWidgetDrag, _, cx| {
                    if drag.module == module {
                        return;
                    }
                    let mut ok = false;
                    nook_core::settings::tweak_app_settings(|s| {
                        ok = match drag.kind {
                            NookDragKind::Reorder => s.try_move_widget_to(drag.module, module),
                            NookDragKind::Place => s.place_widget_on(drag.module, module),
                        };
                    });
                    if ok {
                        this.settings = nook_core::settings::get_app_settings();
                        this.widget_edit_budget_hint_at = None;
                        this.force_content_transition();
                        nook_core::haptics::trigger(None);
                        cx.notify();
                    } else {
                        this.widget_edit_budget_hint_at = Some(Instant::now());
                        nook_core::haptics::trigger(None);
                        cx.notify();
                    }
                }))
                .on_drag(drag, |drag, _, _, cx| cx.new(|_| *drag)),
        )
        // − sits on the corner of the dashed frame, Droppy / springboard style.
        .child(
            div()
                .id(SharedString::from(format!("rm-{}", module as u8)))
                .absolute()
                .top(px(-7.))
                .right(px(-7.))
                .size(px(22.))
                .rounded_full()
                .bg(theme::DESTRUCTIVE)
                .shadow_sm()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.opacity(0.9))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        nook_core::settings::tweak_app_settings(|s| {
                            let _ = s.set_enabled(module, false);
                        });
                        this.settings = nook_core::settings::get_app_settings();
                        this.force_content_transition();
                        nook_core::haptics::trigger(None);
                        cx.notify();
                    }),
                )
                // `minus.svg` is not in the asset pack; lucide path still used for tint.
                .child(lucide_color("minus", 12.0, theme::LABEL)),
        )
        .into_any_element()
}

/// Dashed drop target spanning leftover (or full) row width.
/// `width_px <= 0` flex-fills the row.
pub(super) fn empty_edit_slot(width_px: f32, cx: &mut Context<Island>) -> AnyElement {
    let flex = width_px <= 0.0;
    let show_caption = flex || width_px > 160.0;
    let accent_hi = theme::with_alpha(theme::accent(), 0.80);
    let accent_lo = theme::with_alpha(theme::accent(), 0.16);
    let mute = theme::tertiary_label();
    div()
        .id("edit-empty-slot")
        .when(flex, |d| d.flex_1())
        .when(!flex, |d| d.w(px(width_px)).flex_shrink_0())
        .h_full()
        .min_w(px(if flex { 96.0 } else { width_px.min(96.0) }))
        .rounded(px(theme::CONTROL_RADIUS + 10.0))
        .border_1()
        .border_dashed()
        .border_color(theme::SEPARATOR)
        .occlude()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .drag_over::<NookWidgetDrag>(move |style, drag, _, _| {
            if drag.kind == NookDragKind::Place {
                style.bg(accent_lo).border_color(accent_hi)
            } else {
                style
            }
        })
        .can_drop(|value, _, _| {
            value
                .downcast_ref::<NookWidgetDrag>()
                .is_some_and(|drag| drag.kind == NookDragKind::Place)
        })
        .on_drop(cx.listener(|this, drag: &NookWidgetDrag, _, cx| {
            if drag.kind != NookDragKind::Place {
                return;
            }
            let mut ok = false;
            nook_core::settings::tweak_app_settings(|s| {
                ok = s.place_widget_append(drag.module);
            });
            if ok {
                this.settings = nook_core::settings::get_app_settings();
                this.force_content_transition();
                nook_core::haptics::trigger(None);
                cx.notify();
            }
        }))
        .child(lucide_color("plus", 20.0, mute))
        .when(show_caption, |d| {
            d.child(
                div()
                    .text_size(px(theme::FOOTNOTE.size))
                    .line_height(px(theme::FOOTNOTE.leading))
                    .font_weight(theme::FOOTNOTE.weight)
                    .text_color(mute)
                    .child("Tap or drag a widget to add it"),
            )
        })
        .into_any_element()
}
