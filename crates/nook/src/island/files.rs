//! Expanded files tab: drop zone, grid, and tiles.

use super::ui::{label, text_btn};
use super::{Island, Tab};
use crate::icons::{lucide, lucide_color};
use crate::theme;
use gpui::{
    div, img, prelude::*, px, rgb, rgba, AnyElement, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ScrollWheelEvent, SharedString,
};
use nook_core::files::FileTrayItem;
use std::path::PathBuf;

/// React FileTray: `grid-cols-[repeat(auto-fill,minmax(100px,1fr))]` with 12px gaps.
const FILES_BORDER: f32 = 2.0;
const FILES_GAP: f32 = 12.0;
const FILES_MIN_TILE: f32 = 100.0;
const TILE_RADIUS: f32 = 12.0;
/// Same face as the compact album chip.
const COMPACT_PREVIEW: f32 = theme::COMPACT_FACE;
const COMPACT_PREVIEW_RADIUS: f32 = 5.0;
const COMPACT_STACK_MAX: usize = 3;
const COMPACT_STACK_DX: f32 = 4.0;
const COMPACT_STACK_DY: f32 = 3.0;
const FILES_NAME: f32 = 12.0;
const FILES_SIZE: f32 = 10.0;
const FILES_CAPTION_GAP: f32 = 2.0;
const FILES_CAPTION_PT: f32 = 8.0;
/// `text_btn` row under the grid (`pb_2`).
const FILES_CLEAR_PB: f32 = 8.0;

/// Content width the grid tracks actually lay out in: expanded island minus
/// the widgets/files pane inset, the dashed drop-zone border, and the grid pad.
pub(crate) fn file_grid_inner(island_w: f32) -> f32 {
    (island_w - theme::EXPANDED_PAD * 2.0 - FILES_BORDER * 2.0 - FILES_GAP * 2.0)
        .max(FILES_MIN_TILE)
}

pub(crate) fn file_grid_metrics(island_w: f32) -> (u16, f32) {
    let inner = file_grid_inner(island_w);
    let cols = ((inner + FILES_GAP) / (FILES_MIN_TILE + FILES_GAP))
        .floor()
        .max(1.0);
    let tile = ((inner - (cols - 1.0) * FILES_GAP) / cols).max(1.0);
    (cols as u16, tile)
}

fn file_caption_height() -> f32 {
    FILES_CAPTION_PT + FILES_NAME + FILES_CAPTION_GAP + FILES_SIZE + theme::CONTENT_INSET
}

pub(crate) fn file_tile_height(tile_w: f32) -> f32 {
    tile_w + file_caption_height()
}

/// Drop-zone chrome around one row of tiles: dashed border, grid pad, Clear All.
pub(crate) fn files_pane_min_height(island_w: f32) -> f32 {
    let (_, tile) = file_grid_metrics(island_w);
    FILES_BORDER * 2.0 + FILES_GAP * 2.0 + file_tile_height(tile) + theme::HIT_MIN + FILES_CLEAR_PB
}

/// Newest files first, then reversed so the oldest of that set paints at the
/// back of the compact stack.
fn compact_stack_items(files: &[FileTrayItem]) -> Vec<&FileTrayItem> {
    let mut items: Vec<&FileTrayItem> = files.iter().rev().take(COMPACT_STACK_MAX).collect();
    items.reverse();
    items
}

fn compact_stack_card(file: &FileTrayItem, size: f32, x: f32, y: f32) -> impl IntoElement {
    let is_img = file.mime_type.starts_with("image");
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .size(px(size))
        .rounded(px(COMPACT_PREVIEW_RADIUS))
        .border_1()
        .border_color(rgba(0xFFFFFF4D))
        .shadow_sm()
        .bg(rgba(0xffffff14))
        .flex()
        .items_center()
        .justify_center()
        .when(is_img, |d| {
            d.child(
                img(PathBuf::from(&file.path))
                    .object_fit(ObjectFit::Fill)
                    .size(px(size))
                    .rounded(px(COMPACT_PREVIEW_RADIUS)),
            )
        })
        .when(!is_img, |d| {
            d.child(lucide_color("files", 14.0, theme::TERTIARY_LABEL))
        })
}

/// Compact Live Activity face: one 26pt thumbnail, or a small fanned stack
/// when the tray holds more than one file.
pub(super) fn compact_left(files: &[FileTrayItem]) -> AnyElement {
    let items = compact_stack_items(files);
    if items.is_empty() {
        return div()
            .size(px(COMPACT_PREVIEW))
            .flex_shrink_0()
            .rounded(px(COMPACT_PREVIEW_RADIUS))
            .bg(rgba(0xffffff14))
            .flex()
            .items_center()
            .justify_center()
            .child(lucide_color("files", 14.0, theme::TERTIARY_LABEL))
            .into_any_element();
    }
    let n = items.len();
    let slack_x = (n.saturating_sub(1) as f32) * COMPACT_STACK_DX;
    let slack_y = (n.saturating_sub(1) as f32) * COMPACT_STACK_DY;
    let mut stack = div()
        .relative()
        .w(px(COMPACT_PREVIEW + slack_x))
        .h(px(COMPACT_PREVIEW + slack_y))
        .flex_shrink_0();
    for (i, file) in items.iter().enumerate() {
        let from_front = (n - 1 - i) as f32;
        stack = stack.child(compact_stack_card(
            file,
            COMPACT_PREVIEW,
            from_front * COMPACT_STACK_DX,
            from_front * COMPACT_STACK_DY,
        ));
    }
    stack.into_any_element()
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
    let show_img = file.mime_type.starts_with("image");

    // Do not overflow_hidden this card: grid items with Hidden overflow get
    // min-size 0 and auto rows collapse, so the tile paints at 0 height.
    div()
        .id(SharedString::from(format!("file-{}", file.path)))
        .relative()
        .w(px(tile_w))
        .flex()
        .flex_col()
        .flex_shrink_0()
        .rounded(px(TILE_RADIUS))
        .bg(rgba(0xffffff14))
        .hover(|s| s.bg(rgba(0xFFFFFF1F)))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.arm_file_drag(path.clone());
            }),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
            if event.dragging() {
                cx.stop_propagation();
                if this.poll_pending_file_drag(Some(window)) {
                    cx.notify();
                }
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| {
                cx.stop_propagation();
                if this.finish_file_press() {
                    cx.notify();
                }
            }),
        )
        .child(
            div()
                .w(px(tile_w))
                .h(px(tile_w))
                .flex_shrink_0()
                // Same radius as the card's bottom corners. Do not
                // overflow_hidden: GPUI's overflow clip is a rectangle, so it
                // would square the top that the card rounds at the bottom.
                .rounded_t(px(TILE_RADIUS))
                .bg(rgba(0xFFFFFF0D))
                .flex()
                .items_center()
                .justify_center()
                .when(show_img, |d| {
                    d.child(
                        img(PathBuf::from(img_path))
                            .object_fit(ObjectFit::Fill)
                            .size(px(tile_w))
                            .rounded_t(px(TILE_RADIUS)),
                    )
                })
                .when(!show_img, |d| {
                    d.child(lucide_color("files", 28.0, theme::TERTIARY_LABEL))
                }),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(FILES_CAPTION_GAP))
                .px(px(theme::CONTENT_INSET))
                .pt(px(FILES_CAPTION_PT))
                .pb(px(theme::CONTENT_INSET))
                .child(
                    div()
                        .w_full()
                        .text_size(px(FILES_NAME))
                        .line_height(px(FILES_NAME))
                        .text_color(rgba(0xFFFFFFE6))
                        .font_weight(FontWeight::NORMAL)
                        .truncate()
                        .child(name),
                )
                .when(size > 0, |d| {
                    d.child(
                        div()
                            .text_size(px(FILES_SIZE))
                            .line_height(px(FILES_SIZE))
                            .text_color(rgba(0xffffff80))
                            .child(nook_core::files::format_size(size)),
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
            let mut grid = div().grid().grid_cols(cols).gap(px(FILES_GAP)).w_full();
            for file in &self.files {
                grid = grid.child(file_card(file, tile_w, cx));
            }
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(
                    div()
                        .id("files-list")
                        .flex_1()
                        .min_h(px(0.))
                        .w_full()
                        .p(px(FILES_GAP))
                        .overflow_y_scroll()
                        .on_scroll_wheel(cx.listener(|_, _: &ScrollWheelEvent, _, cx| {
                            cx.stop_propagation();
                        }))
                        .child(grid),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .flex_shrink_0()
                        .px_3()
                        .pb(px(FILES_CLEAR_PB))
                        .child(text_btn("Clear All", cx, |this, _, cx| {
                            this.clear_files(cx);
                        })),
                )
                .into_any_element()
        };

        // React FileTray: bg-white/5 rounded-[16px] border-2 border-dashed border-white/10
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(px(16.))
            .bg(rgba(0xFFFFFF0D))
            .border_2()
            .border_dashed()
            .border_color(rgba(0xFFFFFF1A))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_fit_dropzone_inner() {
        for w in [300.0, 400.0, 548.0, 600.0, 1280.0] {
            let inner = file_grid_inner(w);
            let (cols, tile) = file_grid_metrics(w);
            let used = cols as f32 * tile + (cols.saturating_sub(1) as f32) * FILES_GAP;
            assert!(
                used <= inner + 0.05,
                "w={w} cols={cols} tile={tile} used={used} inner={inner}"
            );
            assert!(tile + 0.05 >= FILES_MIN_TILE || cols == 1);
        }
    }

    #[test]
    fn narrow_card_does_not_force_five_columns() {
        let (cols, _) = file_grid_metrics(300.0);
        assert!(
            cols < 5,
            "a 300pt-wide island cannot fit five 100pt tiles, got {cols}"
        );
    }

    fn tray_item(path: &str, mime: &str) -> FileTrayItem {
        FileTrayItem {
            name: path.into(),
            size: 1,
            path: path.into(),
            mime_type: mime.into(),
            last_modified: 0,
        }
    }

    #[test]
    fn compact_stack_keeps_newest_on_top() {
        let files = [
            tray_item("/tmp/a.png", "image"),
            tray_item("/tmp/notes.pdf", "pdf"),
            tray_item("/tmp/b.jpg", "image"),
            tray_item("/tmp/c.png", "image"),
        ];
        let stack = compact_stack_items(&files);
        let paths: Vec<&str> = stack.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["/tmp/notes.pdf", "/tmp/b.jpg", "/tmp/c.png"]);
        let one = [tray_item("/tmp/a.png", "image")];
        assert_eq!(compact_stack_items(&one).len(), 1);
        assert!(compact_stack_items(&[]).is_empty());
    }

    #[test]
    fn files_pane_is_at_least_one_tile_tall() {
        for w in [300.0, 400.0, 548.0, 600.0, 1280.0] {
            let (_, tile) = file_grid_metrics(w);
            let pane = files_pane_min_height(w);
            let chrome = FILES_BORDER * 2.0 + FILES_GAP * 2.0 + theme::HIT_MIN + FILES_CLEAR_PB;
            assert!(
                pane + 0.05 >= chrome + file_tile_height(tile),
                "w={w} pane={pane} tile_h={} chrome={chrome}",
                file_tile_height(tile)
            );
            assert!(file_tile_height(tile) > tile);
        }
    }
}
