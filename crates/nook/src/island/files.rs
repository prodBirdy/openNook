//! Expanded files tab: drop zone, grid, and tiles.

use super::ui::{label, text_btn};
use super::{Island, Tab};
use crate::icons::{lucide, lucide_color};
use crate::theme;
use gpui::{
    div, img, prelude::*, px, rgb, rgba, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, ObjectFit, ScrollWheelEvent, SharedString,
};
use nook_core::files::FileTrayItem;
use std::path::PathBuf;

/// React FileTray: `grid-cols-[repeat(auto-fill,minmax(100px,1fr))]` with 12px gaps.
pub(crate) fn file_grid_metrics(island_w: f32) -> (u16, f32) {
    let inner = (island_w - theme::CONTENT_INSET * 2.0 - 4.0 - 24.0).max(100.0);
    let cols = ((inner + 12.0) / 112.0).floor().max(1.0);
    let tile = (inner - (cols - 1.0) * 12.0) / cols;
    (cols as u16, tile)
}

pub(super) fn drop_veil() -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .bg(rgba(0x000000B3))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1()
        .child(lucide("plus", 18.0))
        .child(label("Release to Add", theme::BODY, true))
}

fn file_card(file: &FileTrayItem, tile_w: f32, cx: &mut Context<Island>) -> impl IntoElement {
    let path = file.path.clone();
    let path_rm = file.path.clone();
    let img_path = file.path.clone();
    let name = file.name.clone();
    let size = file.size;
    let thumb = (tile_w - 24.0).max(32.0);
    let show_img = file.mime_type.starts_with("image");

    div()
        .id(SharedString::from(format!("file-{}", file.path)))
        .relative()
        .w(px(tile_w))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .p(px(12.))
        .rounded(px(12.))
        .bg(rgba(0xffffff14))
        .hover(|s| s.bg(rgba(0xffffff1F)))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let _ = nook_core::files::open_file(path.clone());
            }),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(2.))
                .child(
                    div()
                        .w_full()
                        .text_size(px(12.))
                        .text_color(rgba(0xffffffE6))
                        .font_weight(FontWeight::NORMAL)
                        .text_center()
                        .truncate()
                        .child(name),
                )
                .when(size > 0, |d| {
                    d.child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgba(0xffffff80))
                            .child(nook_core::files::format_size(size)),
                    )
                }),
        )
        .child(
            div()
                .w(px(thumb))
                .h(px(thumb))
                .rounded(px(8.))
                .bg(rgba(0xffffff0D))
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_center()
                .when(show_img, |d| {
                    d.child(
                        img(PathBuf::from(img_path))
                            .object_fit(ObjectFit::ScaleDown)
                            .size_full()
                            .rounded(px(4.)),
                    )
                }),
        )
        .child(
            div()
                .id(SharedString::from(format!("file-rm-{}", file.path)))
                .absolute()
                .top(px(4.))
                .right(px(4.))
                .size(px(theme::HIT_MIN))
                .rounded_full()
                .bg(theme::FILL)
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(theme::FILL_SECONDARY))
                .active(|s| s.opacity(0.85))
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.remove_file(&path_rm, cx);
                    }),
                )
                .child(lucide_color("x", 12.0, theme::DESTRUCTIVE)),
        )
}

impl Island {
    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hot = self.file_drag;
        let body = if self.files.is_empty() {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .rounded(px(theme::INNER_RADIUS))
                .when(hot, |d| {
                    d.bg(theme::FILL).border_1().border_color(rgb(0xffffff))
                })
                .child(lucide_color(
                    if hot { "plus" } else { "upload-thin" },
                    48.0,
                    if hot {
                        theme::LABEL
                    } else {
                        theme::TERTIARY_LABEL
                    },
                ))
                .child(label(
                    if hot {
                        "Release to Add"
                    } else {
                        "Drop files onto the island"
                    },
                    theme::BODY,
                    true,
                ))
                .child(label(
                    "They stay here until you open or clear them.",
                    theme::SUBHEADLINE,
                    false,
                ))
                .into_any_element()
        } else {
            let (cols, tile_w) = self.file_layout();
            let pane_w = self.expanded_width() - theme::CONTENT_INSET * 2.0;
            let mut grid = div()
                .id("files-list")
                .grid()
                .grid_cols(cols)
                .gap(px(12.))
                .w(px(pane_w))
                .h_full()
                .flex_shrink_0()
                .p(px(12.))
                .overflow_y_scroll()
                .on_scroll_wheel(cx.listener(|_, _: &ScrollWheelEvent, _, cx| {
                    cx.stop_propagation();
                }));
            for file in &self.files {
                grid = grid.child(file_card(file, tile_w, cx));
            }
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(grid)
                .child(div().flex().justify_end().px_3().pb_2().child(text_btn(
                    "Clear All",
                    cx,
                    |this, _, cx| {
                        this.clear_files(cx);
                    },
                )))
                .into_any_element()
        };

        // React FileTray: bg-white/5 rounded-[16px] border-2 border-dashed border-white/10
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(px(theme::WIDGET_RADIUS))
            .bg(rgba(0xffffff0D))
            .border_2()
            .border_dashed()
            .border_color(rgba(0xffffff1A))
            .child(body)
            .when(self.file_drag, |d| d.child(drop_veil()))
    }

    pub(super) fn file_layout(&self) -> (u16, f32) {
        file_grid_metrics(self.expanded_width())
    }

    pub(crate) fn clear_files(&mut self, cx: &mut Context<Self>) {
        self.files.clear();
        let _ = nook_core::files::save_file_tray(self.files.clone());
        cx.notify();
    }

    pub(crate) fn remove_file(&mut self, path: &str, cx: &mut Context<Self>) {
        self.files.retain(|f| f.path != path);
        let _ = nook_core::files::save_file_tray(self.files.clone());
        cx.notify();
    }

    pub(crate) fn arm_dropzone(&mut self, cx: &mut Context<Self>) {
        self.expanded = true;
        self.tab = Tab::Files;
        self.preferred = Some(super::CompactMode::Files);
        nook_core::haptics::trigger(None);
        cx.notify();
    }
}
