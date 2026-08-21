//! Expanded files tab: drop zone, grid, and tiles.

use super::ui::label;
use super::{Island, Tab};
use crate::icons::{lucide, lucide_color};
use crate::theme;
use gpui::{
    div, img, prelude::*, px, rgb, rgba, AnyElement, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ScrollWheelEvent, SharedString,
};
use nook_core::files::FileTrayItem;
use std::path::PathBuf;

/// Dashed drop-zone chrome. Tiles are a compact horizontal row, not a grid.
const FILES_BORDER: f32 = 2.0;
const FILES_GAP: f32 = 16.0;
const FILES_MIN_TILE: f32 = 64.0;
const TILE_RADIUS: f32 = 8.0;
const TRAY_PREVIEW: f32 = 48.0;
const TRAY_PAD: f32 = 16.0;
const TRAY_ZONE_RADIUS: f32 = 22.0;
const AIRDROP_W: f32 = 156.0;
/// Same face as the compact album chip.
const COMPACT_PREVIEW: f32 = theme::COMPACT_FACE;
const COMPACT_PREVIEW_RADIUS: f32 = 5.0;
const COMPACT_STACK_MAX: usize = 3;
const COMPACT_STACK_DX: f32 = 4.0;
const COMPACT_STACK_DY: f32 = 3.0;
const FILES_NAME: f32 = 12.0;
const FILES_CAPTION_GAP: f32 = 2.0;
const FILES_CAPTION_PT: f32 = 8.0;

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
    FILES_CAPTION_PT + FILES_NAME * 2.0 + FILES_CAPTION_GAP
}

pub(crate) fn file_tile_height(_tile_w: f32) -> f32 {
    TRAY_PREVIEW + file_caption_height()
}

/// Drop-zone chrome around one row of compact tiles.
pub(crate) fn files_pane_min_height(_island_w: f32) -> f32 {
    FILES_BORDER * 2.0 + TRAY_PAD * 2.0 + file_tile_height(TRAY_PREVIEW)
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

const AIRDROP_BLUE: gpui::Rgba = gpui::Rgba {
    r: 0.08,
    g: 0.42,
    b: 0.86,
    a: 1.0,
};

fn airdrop_target(cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("airdrop-target")
        .flex_shrink_0()
        .h_full()
        .w(px(AIRDROP_W))
        .rounded(px(TRAY_ZONE_RADIUS))
        .bg(AIRDROP_BLUE)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.92))
        .can_drop(|drag: &dyn std::any::Any, _, _| {
            drag.downcast_ref::<gpui::ExternalPaths>().is_some()
        })
        .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
            cx.stop_propagation();
            this.airdrop_paths(paths, cx);
        }))
        .child(lucide_color("airdrop", 28.0, rgb(0xffffff)))
        .child(
            div()
                .text_size(px(15.))
                .line_height(px(18.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0xffffff))
                .child("AirDrop"),
        )
}

fn is_pdf(file: &FileTrayItem) -> bool {
    file.mime_type.to_ascii_lowercase().contains("pdf")
        || file.name.to_ascii_lowercase().ends_with(".pdf")
}

fn file_preview(file: &FileTrayItem) -> impl IntoElement {
    let show_img = file.mime_type.starts_with("image");
    let img_path = file.path.clone();
    div()
        .size(px(TRAY_PREVIEW))
        .flex_shrink_0()
        .rounded(px(TILE_RADIUS))
        .overflow_hidden()
        .bg(rgb(0xffffff))
        .flex()
        .items_center()
        .justify_center()
        .when(show_img, |d| {
            d.bg(rgba(0xffffff14)).child(
                img(PathBuf::from(img_path))
                    .object_fit(ObjectFit::Fill)
                    .size(px(TRAY_PREVIEW))
                    .rounded(px(TILE_RADIUS)),
            )
        })
        .when(is_pdf(file) && !show_img, |d| {
            d.flex_col().gap(px(2.)).child(
                div()
                    .text_size(px(9.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0xE23D3D))
                    .child("PDF"),
            )
        })
        .when(!show_img && !is_pdf(file), |d| {
            d.bg(rgba(0xffffff14))
                .child(lucide_color("files", 22.0, theme::TERTIARY_LABEL))
        })
}

fn file_card(file: &FileTrayItem, cx: &mut Context<Island>) -> impl IntoElement {
    let path = file.path.clone();
    let name = file.name.clone();

    div()
        .id(SharedString::from(format!("file-{}", file.path)))
        .w(px(FILES_MIN_TILE))
        .flex()
        .flex_col()
        .flex_shrink_0()
        .items_center()
        .gap(px(FILES_CAPTION_PT))
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
        .child(file_preview(file))
        .child(
            div()
                .w_full()
                .text_size(px(FILES_NAME))
                .line_height(px(FILES_NAME + 2.0))
                .text_color(theme::LABEL)
                .font_weight(FontWeight::MEDIUM)
                .truncate()
                .child(name),
        )
}

impl Island {
    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hot = self.file_drag;
        let mut row = div()
            .id("files-list")
            .flex()
            .flex_row()
            .items_start()
            .gap(px(FILES_GAP))
            .size_full()
            .p(px(TRAY_PAD))
            .overflow_x_scroll()
            .on_scroll_wheel(cx.listener(|_, _: &ScrollWheelEvent, _, cx| {
                cx.stop_propagation();
            }));
        for file in &self.files {
            row = row.child(file_card(file, cx));
        }
        if self.files.is_empty() {
            row = row.child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(lucide_color(
                        if hot { "plus" } else { "upload-thin" },
                        28.0,
                        if hot {
                            theme::LABEL
                        } else {
                            theme::TERTIARY_LABEL
                        },
                    ))
                    .child(label(
                        if hot {
                            "Release to add"
                        } else {
                            "Drop files here"
                        },
                        theme::CALLOUT,
                        true,
                    )),
            );
        }

        let zone = div()
            .relative()
            .flex_1()
            .h_full()
            .min_w(px(0.))
            .overflow_hidden()
            .rounded(px(TRAY_ZONE_RADIUS))
            .border_2()
            .border_dashed()
            .border_color(if hot {
                rgba(0xFFFFFF55)
            } else {
                rgba(0xFFFFFF2E)
            })
            .child(row);

        let mut pane = div().flex().size_full().gap(px(12.)).child(zone);
        if hot {
            pane = pane.child(airdrop_target(cx));
        }
        pane
    }

    pub(super) fn file_layout(&self) -> (u16, f32) {
        file_grid_metrics(self.expanded_width())
    }

    #[allow(dead_code)]
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
            let pane = files_pane_min_height(w);
            let tile_h = file_tile_height(TRAY_PREVIEW);
            let chrome = FILES_BORDER * 2.0 + TRAY_PAD * 2.0;
            assert!(
                pane + 0.05 >= chrome + tile_h,
                "w={w} pane={pane} tile_h={tile_h} chrome={chrome}"
            );
            assert!(tile_h > TRAY_PREVIEW);
        }
    }
}
