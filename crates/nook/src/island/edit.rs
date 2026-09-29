//! On-island widget customize mode — Control Center / Notification Center style.
//!
//! Live reflow on drag, a gallery of unused widgets, − to remove, S/M/L on
//! each pane. Cancel / Done commit or revert.

use super::Island;
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    div, prelude::*, px, AnyElement, App, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, ScrollHandle, ScrollWheelEvent, SharedString, Window,
};
use nook_core::settings::{WidgetModule, WidgetSize};
use std::cell::RefCell;

const GALLERY_FACE_H: f32 = 40.0;
const GALLERY_CELL_W: f32 = 9.0;
const GALLERY_GAP: f32 = 10.0;
const GALLERY_RADIUS: f32 = 10.0;
const SIZE_CAPSULE_H: f32 = 22.0;
pub(super) const EDIT_ROW_GAP: f32 = 12.0;
const EDIT_FRAME_RADIUS: f32 = theme::CONTROL_RADIUS + 10.0;

thread_local! {
    static PICKER_SCROLL: RefCell<ScrollHandle> = RefCell::new(ScrollHandle::default());
}

fn picker_scroll() -> ScrollHandle {
    PICKER_SCROLL.with_borrow(|h| h.clone())
}

/// Drag payload for customize mode.
#[derive(Clone, Copy)]
pub(super) struct NookWidgetDrag {
    pub module: WidgetModule,
    pub width: f32,
}

impl gpui::Render for NookWidgetDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.width.max(1.0)))
            .h(px(56.))
            .px(px(14.))
            .rounded(px(theme::CONTROL_RADIUS + 6.0))
            .bg(theme::WINDOW_BG)
            .border_1()
            .border_color(theme::SEPARATOR)
            .shadow_lg()
            .opacity(0.9)
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
            )
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

/// Insertion slot among panes whose widths are `widths`, laid out from `left`
/// with `gap` between them. Picks the first pane whose midpoint is to the
/// right of `x`, or `widths.len()` if the pointer is past every midpoint.
pub(super) fn insert_index(widths: &[f32], left: f32, gap: f32, x: f32) -> usize {
    let mut edge = left;
    for (i, &w) in widths.iter().enumerate() {
        let mid = edge + w * 0.5;
        if x < mid {
            return i;
        }
        edge += w + gap;
    }
    widths.len()
}

impl Island {
    pub(super) fn render_widget_edit_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cell_w = self.nook_cell_width();
        let modules: Vec<WidgetModule> = self
            .settings
            .ordered_widgets()
            .into_iter()
            .filter(|m| {
                m.occupies_nook_cells()
                    && m.is_available()
                    && self.settings.widget_visible(*m)
                    && !self.settings.is_enabled(*m)
            })
            .collect();

        let body = if modules.is_empty() {
            div()
                .id("widget-edit-picker")
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(theme::FOOTNOTE.size))
                        .line_height(px(theme::FOOTNOTE.leading))
                        .text_color(theme::secondary_label())
                        .child("All widgets are in use"),
                )
                .into_any_element()
        } else {
            let n = modules.len();
            let mut cards_w = 0.0;
            for (i, module) in modules.iter().enumerate() {
                cards_w += (self.settings.cells_for(*module) as f32 * GALLERY_CELL_W).max(36.0);
                if i + 1 < n {
                    cards_w += GALLERY_GAP;
                }
            }
            let mut cards = div()
                .id("widget-edit-picker-row")
                .flex()
                .flex_row()
                .items_start()
                .gap(px(GALLERY_GAP))
                .h_full()
                .w(px(cards_w))
                .flex_shrink_0();
            for module in modules {
                cards = cards.child(self.gallery_card(module, cell_w, cx));
            }
            let scroll = picker_scroll();
            let mut scroller = div()
                .id("widget-edit-picker")
                .track_scroll(&scroll)
                .flex_1()
                .min_w(px(0.))
                .h_full()
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
                .child(cards);
            scroller.style().restrict_scroll_to_axis = Some(false);
            scroller.into_any_element()
        };

        div()
            .w_full()
            .h(px(theme::WIDGET_EDIT_PICKER_H))
            .flex_shrink_0()
            .px(px(theme::NOOK_INSET))
            .pb(px(10.))
            .pt(px(4.))
            .flex()
            .items_center()
            .child(body)
    }

    fn gallery_card(
        &self,
        module: WidgetModule,
        cell_w: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let short = self.settings.cells_short_for(module);
        let no_space = short > 0;
        let cells = self.settings.cells_for(module);
        let face_w = (cells as f32 * GALLERY_CELL_W).max(36.0);
        let row_w = cells as f32 * cell_w;
        let drag = NookWidgetDrag {
            module,
            width: row_w,
        };
        div()
            .id(SharedString::from(format!("pick-{}", module as u8)))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .opacity(if no_space {
                theme::DISABLED_OPACITY
            } else {
                1.0
            })
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if this.settings.cells_short_for(module) > 0 {
                    nook_core::haptics::trigger(None);
                    cx.notify();
                    return;
                }
                let mut ok = false;
                nook_core::settings::tweak_app_settings(|s| {
                    ok = s.insert_widget_at(module, usize::MAX);
                });
                if ok {
                    this.settings = nook_core::settings::get_app_settings();
                    this.force_content_transition();
                    nook_core::haptics::trigger(None);
                    cx.notify();
                }
            }))
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| *drag))
            .child(
                div()
                    .id(SharedString::from(format!("pick-face-{}", module as u8)))
                    .relative()
                    .w(px(face_w))
                    .h(px(GALLERY_FACE_H))
                    .rounded(px(GALLERY_RADIUS))
                    .bg(theme::FILL)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(lucide_color(module_icon(module), 16.0, theme::LABEL))
                    .when(!no_space, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(4.))
                                .left(px(4.))
                                .size(px(16.))
                                .rounded_full()
                                .bg(theme::accent())
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(lucide_color("plus", 10.0, theme::LABEL)),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(px(theme::FOOTNOTE.size))
                    .line_height(px(theme::FOOTNOTE.leading))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::secondary_label())
                    .whitespace_nowrap()
                    .child(module_short(module)),
            )
            .when(no_space, |d| {
                d.child(
                    div()
                        .text_size(px(theme::FOOTNOTE.size))
                        .line_height(px(theme::FOOTNOTE.leading))
                        .text_color(theme::secondary_label())
                        .child("No space"),
                )
            })
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

/// Solid edit frame, − inside the pane, optional S/M/L capsule.
pub(super) fn edit_chrome(
    module: WidgetModule,
    child: AnyElement,
    pane_width: f32,
    selected: WidgetSize,
    size_opts: &[(WidgetSize, bool)],
    cx: &mut Context<Island>,
) -> AnyElement {
    let drag = NookWidgetDrag {
        module,
        width: pane_width,
    };
    let mut frame = div()
        .id(SharedString::from(format!("edit-pane-{}", module as u8)))
        .relative()
        .size_full()
        .rounded(px(EDIT_FRAME_RADIUS))
        .border_1()
        .border_color(theme::SEPARATOR)
        .child(
            div()
                .size_full()
                .rounded(px(EDIT_FRAME_RADIUS - 1.0))
                .overflow_hidden()
                .child(child),
        )
        .child(
            div()
                .id(SharedString::from(format!("hit-{}", module as u8)))
                .absolute()
                .inset_0()
                .rounded(px(EDIT_FRAME_RADIUS))
                .occlude()
                .cursor(CursorStyle::OpenHand)
                .on_drag(drag, |drag, _, _, cx| cx.new(|_| *drag)),
        )
        .child(
            div()
                .id(SharedString::from(format!("rm-{}", module as u8)))
                .absolute()
                .top(px(6.))
                .left(px(6.))
                .size(px(theme::HIT_MIN))
                .flex()
                .items_center()
                .justify_center()
                .occlude()
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
                .child(
                    div()
                        .size(px(20.))
                        .rounded_full()
                        .bg(theme::FILL_SECONDARY)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(lucide_color("minus", 12.0, theme::LABEL)),
                ),
        );
    if size_opts.len() > 1 {
        let mut capsule = div()
            .id(SharedString::from(format!("size-{}", module as u8)))
            .h(px(SIZE_CAPSULE_H))
            .px(px(2.))
            .rounded(px(SIZE_CAPSULE_H / 2.0))
            .bg(theme::SCRIM)
            .flex()
            .flex_row()
            .items_center()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                }),
            );
        for &(size, fits) in size_opts {
            let on = size == selected;
            let mut seg = div()
                .id(SharedString::from(format!(
                    "size-{}-{}",
                    module as u8,
                    size.label()
                )))
                .h(px(SIZE_CAPSULE_H - 4.0))
                .px(px(8.))
                .rounded(px((SIZE_CAPSULE_H - 4.0) / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .when(on, |d| d.bg(theme::FILL))
                .opacity(if fits {
                    1.0
                } else {
                    theme::DISABLED_OPACITY
                })
                .child(
                    div()
                        .text_size(px(theme::FOOTNOTE.size))
                        .line_height(px(theme::FOOTNOTE.leading))
                        .font_weight(if on {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(theme::LABEL)
                        .child(size.label()),
                );
            if fits {
                seg = seg.cursor(CursorStyle::PointingHand).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        nook_core::settings::tweak_app_settings(|s| {
                            let _ = s.set_size(module, size);
                        });
                        this.settings = nook_core::settings::get_app_settings();
                        this.force_content_transition();
                        nook_core::haptics::trigger(None);
                        cx.notify();
                    }),
                );
            }
            capsule = capsule.child(seg);
        }
        frame = frame.child(
            div()
                .absolute()
                .bottom(px(6.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(capsule),
        );
    }
    frame.into_any_element()
}

/// Insertion preview at the live-reflow index.
pub(super) fn insert_placeholder(width: f32, fits: bool) -> AnyElement {
    div()
        .w(px(width.max(1.0)))
        .h_full()
        .flex_shrink_0()
        .rounded(px(EDIT_FRAME_RADIUS))
        .border_1()
        .border_color(if fits {
            theme::accent()
        } else {
            theme::SEPARATOR
        })
        .bg(if fits {
            theme::with_alpha(theme::accent(), 0.12)
        } else {
            theme::FILL_TERTIARY
        })
        .flex()
        .items_center()
        .justify_center()
        .when(!fits, |d| {
            d.child(
                div()
                    .text_size(px(theme::FOOTNOTE.size))
                    .line_height(px(theme::FOOTNOTE.leading))
                    .text_color(theme::secondary_label())
                    .child("Not enough space"),
            )
        })
        .into_any_element()
}

/// Quiet leftover space at the end of the customize row.
/// `width_px <= 0` flex-fills the row.
pub(super) fn empty_edit_slot(width_px: f32) -> AnyElement {
    let flex = width_px <= 0.0;
    let show_caption = flex || width_px > 160.0;
    div()
        .id("edit-empty-slot")
        .when(flex, |d| d.flex_1())
        .when(!flex, |d| d.w(px(width_px)).flex_shrink_0())
        .h_full()
        .min_w(px(if flex { 96.0 } else { width_px.min(96.0) }))
        .rounded(px(EDIT_FRAME_RADIUS))
        .bg(theme::FILL_TERTIARY)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .child(lucide_color("plus", 20.0, theme::tertiary_label()))
        .when(show_caption, |d| {
            d.child(
                div()
                    .text_size(px(theme::FOOTNOTE.size))
                    .line_height(px(theme::FOOTNOTE.leading))
                    .font_weight(theme::FOOTNOTE.weight)
                    .text_color(theme::secondary_label())
                    .child("Drag widgets here"),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_index_picks_the_first_midpoint_to_the_right() {
        let widths = [100.0, 80.0, 60.0];
        let gap = 12.0;
        let left = 20.0;
        // mids at 70, 172, 254
        assert_eq!(insert_index(&widths, left, gap, 0.0), 0);
        assert_eq!(insert_index(&widths, left, gap, 69.9), 0);
        assert_eq!(insert_index(&widths, left, gap, 70.0), 1);
        assert_eq!(insert_index(&widths, left, gap, 171.9), 1);
        assert_eq!(insert_index(&widths, left, gap, 172.0), 2);
        assert_eq!(insert_index(&widths, left, gap, 253.9), 2);
        assert_eq!(insert_index(&widths, left, gap, 254.0), 3);
        assert_eq!(insert_index(&[], left, gap, 50.0), 0);
    }
}
