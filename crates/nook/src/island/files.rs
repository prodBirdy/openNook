//! Expanded files tab: drop zone, grid, and tiles.

use super::ui::{label, text_btn};
use super::{Island, Tab};
use crate::icons::{lucide, lucide_color};
use crate::theme;
use gpui::{
    div, img, prelude::*, px, AnyElement, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ScrollWheelEvent, SharedString,
};
use nook_core::files::FileTrayItem;
use nook_core::share::{self, DeviceInfo, ShareKind, SharePhase};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// File chips plus AirDrop / Local Send. Tiles are a compact horizontal row.
const FILES_BORDER: f32 = 2.0;
const FILES_GAP: f32 = 10.0;
const FILES_MIN_TILE: f32 = 64.0;
const FILE_CARD_W: f32 = 104.0;
const FILE_CARD_RADIUS: f32 = 12.0;
const FILE_THUMB: f32 = 42.0;
const FILE_THUMB_RADIUS: f32 = 10.0;
const FILE_THUMB_ICON: f32 = 20.0;
const FILE_CARD_PAD_Y: f32 = 12.0;
const FILE_CARD_PAD_X: f32 = 8.0;
const FILE_CARD_GAP: f32 = 8.0;
const TRAY_PREVIEW: f32 = FILE_THUMB;
const TRAY_ZONE_RADIUS: f32 = 14.0;
const TRAY_ACTIONS_W: f32 = 164.0;
const TRAY_ACTION_RADIUS: f32 = 12.0;
const TRAY_ACTION_ICON: f32 = 20.0;
const TRAY_ROW_GAP: f32 = 16.0;
const DROP_ICON: f32 = 26.0;
const DROP_GAP: f32 = 8.0;
/// Mockup `file-plus` is missing from the pack; `files` is the document glyph.
const DROP_GLYPH: &str = "files";
/// Deck.key tile in the Pencil frame (`#FF9F0A`).
const THUMB_KEY: u32 = 0xFF9F0A;
const THUMB_IMAGE: u32 = 0xBF5AF2;
/// Same face as compact lucide glyphs.
const COMPACT_PREVIEW: f32 = theme::COMPACT_FACE;
const COMPACT_PREVIEW_RADIUS: f32 = 5.0;
const COMPACT_STACK_MAX: usize = 3;
const COMPACT_STACK_DX: f32 = 4.0;
const COMPACT_STACK_DY: f32 = 3.0;
const FILES_NAME: f32 = theme::SUBHEADLINE.size;
const RELEASE_HINT: &str = "Release to add";
const DROP_TITLE: &str = "Drop files to keep them here";
const DROP_HINT: &str = "Drag them out again whenever you need them";
const FILES_CAPTION_GAP: f32 = 2.0;
const FILES_CAPTION_PT: f32 = 8.0;
/// Nook row (128) minus the expanded bottom pad (20).
const FILE_CARD_H: f32 = theme::NOOK_BODY - theme::EXPANDED_PAD;

/// Content width the grid tracks actually lay out in: expanded island minus
/// the widgets/files pane inset, the dashed drop-zone border, and the grid pad.
#[allow(dead_code)]
pub(crate) fn file_grid_inner(island_w: f32) -> f32 {
    (island_w - theme::EXPANDED_PAD * 2.0 - FILES_BORDER * 2.0 - FILES_GAP * 2.0)
        .max(FILES_MIN_TILE)
}

#[allow(dead_code)]
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
    FILE_CARD_H.max(TRAY_PREVIEW + file_caption_height())
}

/// One tray row: file chips plus the AirDrop / Local Send column.
pub(crate) fn files_pane_min_height(_island_w: f32) -> f32 {
    file_tile_height(TRAY_PREVIEW)
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
        .border_color(theme::tertiary_label())
        .shadow_sm()
        .bg(theme::FILL_TERTIARY)
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
            d.child(lucide_color("files", 14.0, theme::tertiary_label()))
        })
}

/// Compact Live Activity face: one thumbnail at [`theme::COMPACT_FACE`], or a
/// small fanned stack when the tray holds more than one file.
pub(super) fn compact_left(files: &[FileTrayItem]) -> AnyElement {
    let items = compact_stack_items(files);
    if items.is_empty() {
        return div()
            .size(px(COMPACT_PREVIEW))
            .flex_shrink_0()
            .rounded(px(COMPACT_PREVIEW_RADIUS))
            .bg(theme::FILL_TERTIARY)
            .flex()
            .items_center()
            .justify_center()
            .child(lucide_color("files", 14.0, theme::tertiary_label()))
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
        .bg(theme::SCRIM)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1()
        .child(lucide("plus", theme::COMPACT_FACE))
        .child(label(RELEASE_HINT, theme::BODY, true))
}

fn tray_action(
    id: &'static str,
    title: &'static str,
    icon: &'static str,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &mut Context<Island>) + 'static,
    on_drop: impl Fn(&mut Island, &gpui::ExternalPaths, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .h_full()
        .rounded(px(TRAY_ACTION_RADIUS))
        .bg(theme::FILL)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .py(px(10.))
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.92))
        .can_drop(|drag: &dyn std::any::Any, _, _| {
            drag.downcast_ref::<gpui::ExternalPaths>().is_some()
        })
        .drag_over::<gpui::ExternalPaths>(|s, _, _, _| s.border_2().border_color(theme::accent()))
        .on_drop(
            cx.listener(move |this, paths: &gpui::ExternalPaths, _, cx| {
                cx.stop_propagation();
                on_drop(this, paths, cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, cx);
            }),
        )
        .child(lucide_color(icon, TRAY_ACTION_ICON, theme::ACCENT))
        .child(
            div()
                .text_size(px(theme::FOOTNOTE.size))
                .line_height(px(theme::FOOTNOTE.leading))
                .font_weight(FontWeight::NORMAL)
                .text_color(theme::LABEL)
                .child(title),
        )
}

fn airdrop_target(cx: &mut Context<Island>) -> impl IntoElement {
    tray_action(
        "airdrop-target",
        "AirDrop",
        "airdrop",
        cx,
        |this, cx| {
            let paths: Vec<PathBuf> = this.files.iter().map(|f| PathBuf::from(&f.path)).collect();
            if paths.is_empty() {
                return;
            }
            nook_core::haptics::trigger(None);
            crate::platform::share_via_airdrop(&paths);
            cx.notify();
        },
        |this, paths, cx| this.airdrop_paths(paths, cx),
    )
}

fn localsend_target(cx: &mut Context<Island>) -> impl IntoElement {
    tray_action(
        "localsend-target",
        "Local Send",
        "share",
        cx,
        |this, cx| {
            let paths: Vec<PathBuf> = this.files.iter().map(|f| PathBuf::from(&f.path)).collect();
            this.start_localsend(paths, cx);
        },
        |this, paths, cx| this.localsend_paths(paths, cx),
    )
}

fn format_file_size(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = bytes.max(0) as f64;
    if n < KB {
        format!("{} B", bytes.max(0))
    } else if n < MB {
        let kb = n / KB;
        if kb < 10.0 {
            format!("{kb:.1} KB")
        } else {
            format!("{:.0} KB", kb)
        }
    } else if n < GB {
        let mb = n / MB;
        if mb < 10.0 {
            format!("{mb:.1} MB")
        } else {
            format!("{:.0} MB", mb)
        }
    } else {
        format!("{:.1} GB", n / GB)
    }
}

fn file_kind(file: &FileTrayItem) -> (&'static str, gpui::Rgba) {
    let mime = file.mime_type.to_ascii_lowercase();
    let name = file.name.to_ascii_lowercase();
    let ext = std::path::Path::new(&name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if mime.starts_with("image")
        || matches!(
            ext,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "tif" | "tiff"
        )
    {
        ("files", theme::rgba_from_u32(THUMB_IMAGE, 1.0))
    } else if ext == "pdf" || mime.contains("pdf") {
        ("files", theme::DESTRUCTIVE)
    } else if matches!(ext, "key" | "ppt" | "pptx" | "odp") || mime.contains("presentation") {
        ("monitor", theme::rgba_from_u32(THUMB_KEY, 1.0))
    } else if matches!(
        ext,
        "md" | "rs" | "ts" | "js" | "py" | "go" | "toml" | "json" | "html" | "css" | "sh"
    ) || mime.contains("text")
    {
        ("notebook", theme::SUCCESS)
    } else {
        ("files", theme::FILL_SECONDARY)
    }
}

#[allow(dead_code)]
fn extension_badge(file: &FileTrayItem) -> Option<String> {
    let name = file.name.to_ascii_lowercase();
    let ext = std::path::Path::new(&name)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_uppercase();
    if ext.is_empty() {
        return None;
    }
    let mut chars = ext.chars();
    let head: String = chars.by_ref().take(4).collect();
    Some(head)
}

fn file_preview(file: &FileTrayItem) -> impl IntoElement {
    let (glyph, fill) = file_kind(file);
    div()
        .size(px(FILE_THUMB))
        .flex_shrink_0()
        .rounded(px(FILE_THUMB_RADIUS))
        .overflow_hidden()
        .bg(fill)
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color(glyph, FILE_THUMB_ICON, theme::LABEL))
}

fn file_card(file: &FileTrayItem, cx: &mut Context<Island>) -> impl IntoElement {
    let path = file.path.clone();
    let path_send = path.clone();
    let path_rm = path.clone();
    let name = file.name.clone();

    let size = format_file_size(file.size);
    div()
        .id(SharedString::from(format!("file-{}", file.path)))
        .group("file-card")
        .relative()
        .w(px(FILE_CARD_W))
        .h_full()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .gap(px(FILE_CARD_GAP))
        .px(px(FILE_CARD_PAD_X))
        .py(px(FILE_CARD_PAD_Y))
        .rounded(px(FILE_CARD_RADIUS))
        .bg(theme::FILL_TERTIARY)
        .border_1()
        .border_color(theme::FILL_TERTIARY)
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
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let paths = vec![PathBuf::from(path_send.clone())];
                if share::localsend::app_installed() {
                    this.start_localsend(paths, cx);
                } else {
                    nook_core::haptics::trigger(None);
                    crate::platform::share_via_airdrop(&paths);
                    cx.notify();
                }
            }),
        )
        .child(file_preview(file))
        .child(
            div()
                .id(SharedString::from(format!("rm-{}", name)))
                .absolute()
                .top(px(-4.))
                .right(px(-4.))
                .size(px(theme::HIT_MIN))
                .rounded_full()
                .bg(theme::SCRIM)
                .flex()
                .items_center()
                .justify_center()
                .opacity(0.)
                .group_hover("file-card", |s| s.opacity(1.0))
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.remove_file(&path_rm, cx);
                    }),
                )
                .child(lucide_color("x", 12.0, theme::LABEL)),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(FILES_CAPTION_GAP))
                .child(
                    div()
                        .w_full()
                        .text_size(px(FILES_NAME))
                        .line_height(px(theme::SUBHEADLINE.leading))
                        .text_color(theme::LABEL)
                        .font_weight(FontWeight::NORMAL)
                        .text_center()
                        .truncate()
                        .child(name),
                )
                .child(
                    div()
                        .text_size(px(theme::FOOTNOTE.size))
                        .line_height(px(theme::FOOTNOTE.leading))
                        .text_color(theme::secondary_label())
                        .child(size),
                ),
        )
}

impl Island {
    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hot = self.file_drag;
        let picking = self.share.shows_picker();
        let undo_live = self
            .last_cleared_files
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(5));

        if self.files.is_empty() {
            return div()
                .flex()
                .size_full()
                .child(self.tray_drop_zone(hot, picking, cx))
                .into_any_element();
        }

        let mut strip = div()
            .id("files-list")
            .flex()
            .flex_row()
            .flex_1()
            .items_center()
            .gap(px(FILES_GAP))
            .h_full()
            .min_w(px(0.))
            .overflow_x_scroll()
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                this.on_wheel(event, cx);
            }));
        for file in &self.files {
            strip = strip.child(file_card(file, cx));
        }

        let send = div()
            .id("tray-send")
            .flex()
            .flex_1()
            .w_full()
            .gap(px(8.))
            .child(airdrop_target(cx))
            .child(localsend_target(cx));

        let actions = div()
            .w(px(TRAY_ACTIONS_W))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(send)
            .child(
                div()
                    .id("tray-clear")
                    .w_full()
                    .py(px(6.))
                    .rounded(px(theme::CONTROL_RADIUS))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.opacity(0.85))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.clear_files(cx);
                        }),
                    )
                    .child(
                        div()
                            .text_size(px(theme::SUBHEADLINE.size))
                            .line_height(px(theme::SUBHEADLINE.leading))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(theme::secondary_label())
                            .child("Clear All"),
                    ),
            )
            .when(undo_live, |d| {
                d.child(text_btn("Undo", cx, |this, _, cx| {
                    this.undo_clear_files(cx)
                }))
            });

        div()
            .flex()
            .items_center()
            .size_full()
            .gap(px(TRAY_ROW_GAP))
            .child(strip)
            .child(
                div()
                    .w(px(1.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(theme::SEPARATOR),
            )
            .child(actions)
            .when(picking, |d| d.relative().child(self.localsend_picker(cx)))
            .into_any_element()
    }

    fn tray_drop_zone(&self, hot: bool, picking: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("tray-drop")
            .relative()
            .flex_1()
            .h_full()
            .rounded(px(TRAY_ZONE_RADIUS))
            .bg(theme::with_alpha(theme::ACCENT, 0x1A as f32 / 255.0))
            .border_2()
            .border_color(theme::ACCENT)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(DROP_GAP))
            .child(lucide_color(DROP_GLYPH, DROP_ICON, theme::ACCENT))
            .child(
                div()
                    .text_size(px(theme::BODY.size))
                    .line_height(px(theme::BODY.leading))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(theme::LABEL)
                    .child(if hot { RELEASE_HINT } else { DROP_TITLE }),
            )
            .child(label(DROP_HINT, theme::SUBHEADLINE, false))
            .when(picking, |d| d.child(self.localsend_picker(cx)))
    }

    fn localsend_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().flex().flex_col().gap(px(6.)).w_full();
        if self.share.phase == SharePhase::Failed {
            let msg: SharedString = self
                .share
                .error
                .clone()
                .unwrap_or_else(|| "Share failed".into())
                .into();
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(lucide_color(
                        "alert-triangle",
                        theme::GLYPH_SM,
                        theme::DESTRUCTIVE,
                    ))
                    .child(label(msg, theme::CALLOUT, false))
                    .child(text_btn("Dismiss", cx, |this, _, cx| this.cancel_share(cx))),
            );
        } else if self.share.phase == SharePhase::Discovering {
            list = list.child(label(
                "Looking for LocalSend devices…",
                theme::CALLOUT,
                true,
            ));
        } else if self.share.peers.is_empty() {
            list = list
                .child(label("No devices found", theme::BODY, true))
                .child(label(
                    "Open LocalSend on the other device, or allow Local Network for openNook.",
                    theme::CALLOUT,
                    false,
                ));
        } else {
            for peer in &self.share.peers {
                let peer = peer.clone();
                let caption = if peer.device_model.as_deref().unwrap_or("").is_empty() {
                    peer.alias.clone()
                } else {
                    format!(
                        "{} · {}",
                        peer.alias,
                        peer.device_model.as_deref().unwrap_or("")
                    )
                };
                list = list.child(
                    div()
                        .id(SharedString::from(format!("peer-{}", peer.fingerprint)))
                        .h(px(theme::HIT_MIN))
                        .px(px(10.))
                        .rounded(px(theme::CONTROL_RADIUS))
                        .bg(theme::FILL)
                        .flex()
                        .items_center()
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(theme::FILL_SECONDARY))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.send_to_peer(peer.clone(), cx);
                            }),
                        )
                        .child(label(caption, theme::CALLOUT, true)),
                );
            }
        }
        let title = "Send with LocalSend";
        div()
            .absolute()
            .inset_0()
            .bg(theme::SCRIM)
            .flex()
            .flex_col()
            .p(px(12.))
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(title, theme::BODY, true))
                    .child(text_btn("Cancel", cx, |this, _, cx| this.cancel_share(cx))),
            )
            .child(list)
    }

    #[allow(dead_code)]
    pub(super) fn file_layout(&self) -> (u16, f32) {
        file_grid_metrics(self.expanded_width())
    }

    pub(crate) fn clear_files(&mut self, cx: &mut Context<Self>) {
        if self.files.is_empty() {
            return;
        }
        // files.rs renders the Undo chip
        self.last_cleared_files = Some((std::mem::take(&mut self.files), Instant::now()));
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
        self.arm_content_transition();
        cx.notify();
    }

    pub(super) fn localsend_paths(&mut self, paths: &gpui::ExternalPaths, cx: &mut Context<Self>) {
        self.start_localsend(paths.paths().to_vec(), cx);
    }

    fn begin_share(&mut self, kind: ShareKind, paths: Vec<PathBuf>, cx: &mut Context<Self>) -> u64 {
        self.share.gen = self.share.gen.wrapping_add(1);
        self.share.kind = kind;
        self.share.paths = paths;
        self.share.peers.clear();
        self.share.progress = 0.0;
        self.share.error = None;
        self.share.hud = None;
        self.share.status.clear();
        self.expanded = true;
        self.tab = Tab::Files;
        self.preferred = Some(super::CompactMode::Share);
        nook_core::haptics::trigger(None);
        self.arm_content_transition();
        cx.notify();
        self.share.gen
    }

    pub(crate) fn cancel_share(&mut self, cx: &mut Context<Self>) {
        self.share.gen = self.share.gen.wrapping_add(1);
        self.share = share::ShareSession {
            gen: self.share.gen,
            ..share::ShareSession::default()
        };
        cx.notify();
    }

    pub(crate) fn start_localsend(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !share::localsend::app_installed() {
            return;
        }
        let paths: Vec<PathBuf> = paths.into_iter().filter(|path| path.is_file()).collect();
        if paths.is_empty() {
            return;
        }
        let gen = self.begin_share(ShareKind::LocalSend, paths, cx);
        self.share.phase = SharePhase::Discovering;
        self.share.status = "Looking for devices".into();
        let alias = self.settings.share.device_alias.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    nook_core::runtime().block_on(share::localsend::discover_peers(
                        &alias,
                        share::localsend::DISCOVER_WINDOW,
                    ))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.share.gen != gen {
                    return;
                }
                match result {
                    Ok(peers) => {
                        this.share.peers = peers;
                        this.share.phase = SharePhase::Picking;
                        this.share.status = if this.share.peers.is_empty() {
                            "No devices found".into()
                        } else {
                            format!("{} nearby", this.share.peers.len())
                        };
                    }
                    Err(err) => {
                        this.share.mark_failed(err);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn send_to_peer(&mut self, peer: DeviceInfo, cx: &mut Context<Self>) {
        if self.share.paths.is_empty() {
            return;
        }
        let gen = self.share.gen;
        self.share.phase = SharePhase::Transferring;
        self.share.status = format!("Sending to {}", peer.alias);
        self.share.progress = 0.0;
        let paths = self.share.paths.clone();
        let alias = self.settings.share.device_alias.clone();
        let pin = self.settings.share.localsend_pin.clone();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
        let progress_ui = progress.clone();
        let done_ui = done.clone();
        cx.spawn(async move |this, cx| {
            let pin = if pin.is_empty() { None } else { Some(pin) };
            {
                let progress = progress.clone();
                let done = done.clone();
                let slot = slot.clone();
                cx.background_executor()
                    .spawn(async move {
                        let outcome = nook_core::runtime().block_on(share::localsend::send_files(
                            &alias,
                            &peer,
                            &paths,
                            pin.as_deref(),
                            |sample| {
                                progress.store(
                                    (sample.fraction() * 1000.0) as u32,
                                    std::sync::atomic::Ordering::Relaxed,
                                );
                            },
                        ));
                        if let Ok(mut guard) = slot.lock() {
                            *guard = Some(outcome);
                        }
                        done.store(true, std::sync::atomic::Ordering::SeqCst);
                    })
                    .detach();
            }
            loop {
                let keep = this
                    .update(cx, |this, cx| {
                        if this.share.gen != gen {
                            return false;
                        }
                        this.share.progress =
                            progress_ui.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
                        cx.notify();
                        !done_ui.load(std::sync::atomic::Ordering::SeqCst)
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(80))
                    .await;
            }
            let outcome = slot
                .lock()
                .ok()
                .and_then(|mut guard| guard.take())
                .unwrap_or_else(|| Err("transfer cancelled".into()));
            this.update(cx, |this, cx| {
                if this.share.gen != gen {
                    return;
                }
                match outcome {
                    Ok(()) => {
                        this.share.phase = SharePhase::Done;
                        this.share.progress = 1.0;
                        this.share.status = "Sent".into();
                        this.share.hud = Some("Sent".into());
                    }
                    Err(err) => {
                        this.share.mark_failed(err);
                    }
                }
                cx.notify();
            })
            .ok();
            cx.background_executor()
                .timer(std::time::Duration::from_secs(2))
                .await;
            this.update(cx, |this, cx| {
                if this.share.gen != gen {
                    return;
                }
                if matches!(this.share.phase, SharePhase::Done) {
                    this.share = share::ShareSession {
                        gen: this.share.gen,
                        ..share::ShareSession::default()
                    };
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
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
            assert!(pane + 0.05 >= tile_h, "w={w} pane={pane} tile_h={tile_h}");
            assert!(tile_h > TRAY_PREVIEW);
            assert!((tile_h - FILE_CARD_H).abs() < 0.05 || tile_h >= FILE_CARD_H);
        }
    }

    #[test]
    fn tray_cards_match_mockup() {
        assert_eq!(FILE_CARD_W, 104.0);
        assert_eq!(FILE_THUMB, 42.0);
        assert_eq!(FILE_THUMB_RADIUS, 10.0);
        assert_eq!(FILE_CARD_RADIUS, 12.0);
        assert_eq!(TRAY_ACTIONS_W, 164.0);
        assert_eq!(TRAY_ZONE_RADIUS, 14.0);
        assert_eq!(TRAY_ROW_GAP, 16.0);
        assert_eq!(FILES_GAP, 10.0);
        assert_eq!(DROP_ICON, 26.0);
        assert_eq!(DROP_GLYPH, "files");
        assert_eq!(THUMB_KEY, 0xFF9F0A);
        assert_eq!(THUMB_IMAGE, 0xBF5AF2);
    }

    #[test]
    fn file_size_labels_follow_the_mockup() {
        assert_eq!(format_file_size(12 * 1024), "12 KB");
        assert_eq!(format_file_size((2.4 * 1024.0 * 1024.0) as i64), "2.4 MB");
        assert_eq!(format_file_size(18 * 1024 * 1024), "18 MB");
        assert_eq!(format_file_size(800), "800 B");
    }

    #[test]
    fn file_kind_tints_common_types() {
        let pdf = tray_item("/tmp/Report.pdf", "application/pdf");
        assert_eq!(file_kind(&pdf).0, "files");
        assert_eq!(file_kind(&pdf).1, theme::DESTRUCTIVE);
        let png = tray_item("/tmp/Hero.png", "image/png");
        assert_eq!(file_kind(&png).0, "files");
        assert_eq!(file_kind(&png).1, theme::rgba_from_u32(THUMB_IMAGE, 1.0));
        let key = tray_item("/tmp/Deck.key", "application/x-iwork-keynote");
        assert_eq!(file_kind(&key).0, "monitor");
        assert_eq!(file_kind(&key).1, theme::rgba_from_u32(THUMB_KEY, 1.0));
        let md = tray_item("/tmp/notes.md", "text/markdown");
        assert_eq!(file_kind(&md).0, "notebook");
        assert_eq!(file_kind(&md).1, theme::SUCCESS);
    }
}
