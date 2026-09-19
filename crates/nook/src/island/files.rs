//! Expanded files tab: drop zone and tiles.

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

/// Dashed drop-zone chrome. Tiles are a compact horizontal row, not a grid.
const FILES_BORDER: f32 = 2.0;
const FILES_GAP: f32 = 16.0;
const FILES_MIN_TILE: f32 = 64.0;
const TILE_RADIUS: f32 = theme::CONTROL_RADIUS;
const TRAY_PREVIEW: f32 = 48.0;
const TRAY_PAD: f32 = 16.0;
const TRAY_ZONE_RADIUS: f32 = theme::ROW_RADIUS;
const AIRDROP_W: f32 = 132.0;
/// Same face as compact lucide glyphs.
const COMPACT_PREVIEW: f32 = theme::COMPACT_FACE;
const COMPACT_PREVIEW_RADIUS: f32 = 5.0;
const COMPACT_STACK_MAX: usize = 3;
const COMPACT_STACK_DX: f32 = 4.0;
const COMPACT_STACK_DY: f32 = 3.0;
const FILES_NAME: f32 = theme::CALLOUT.size;
const RELEASE_HINT: &str = "Release to add";
const FILES_CAPTION_GAP: f32 = 2.0;
const FILES_CAPTION_PT: f32 = 8.0;

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

const AIRDROP_BLUE: gpui::Rgba = gpui::Rgba {
    r: 0.08,
    g: 0.42,
    b: 0.86,
    a: 1.0,
};

fn drop_target(
    id: &'static str,
    title: &'static str,
    icon: &'static str,
    fill: gpui::Rgba,
    cx: &mut Context<Island>,
    on_drop: impl Fn(&mut Island, &gpui::ExternalPaths, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_shrink_0()
        .h_full()
        .w(px(AIRDROP_W))
        .rounded(px(TRAY_ZONE_RADIUS))
        .bg(fill)
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
        .drag_over::<gpui::ExternalPaths>(|s, _, _, _| s.border_2().border_color(theme::LABEL))
        .on_drop(
            cx.listener(move |this, paths: &gpui::ExternalPaths, _, cx| {
                cx.stop_propagation();
                on_drop(this, paths, cx);
            }),
        )
        .child(lucide_color(icon, 28.0, theme::LABEL))
        .child(
            div()
                .text_size(px(theme::TITLE_3.size))
                .line_height(px(theme::TITLE_3.leading))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child(title),
        )
}

const LOCALSEND_GREEN: gpui::Rgba = gpui::Rgba {
    r: 0.10,
    g: 0.62,
    b: 0.47,
    a: 1.0,
};
fn airdrop_target(cx: &mut Context<Island>) -> impl IntoElement {
    drop_target(
        "airdrop-target",
        "AirDrop",
        "airdrop",
        AIRDROP_BLUE,
        cx,
        |this, paths, cx| this.airdrop_paths(paths, cx),
    )
}

fn localsend_target(cx: &mut Context<Island>) -> impl IntoElement {
    drop_target(
        "localsend-target",
        "LocalSend",
        "share",
        LOCALSEND_GREEN,
        cx,
        |this, paths, cx| this.localsend_paths(paths, cx),
    )
}

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
    let show_img = file.mime_type.starts_with("image");
    let img_path = file.path.clone();
    let badge = if show_img {
        None
    } else {
        extension_badge(file)
    };
    div()
        .size(px(TRAY_PREVIEW))
        .flex_shrink_0()
        .rounded(px(TILE_RADIUS))
        .overflow_hidden()
        .bg(theme::FILL_TERTIARY)
        .flex()
        .items_center()
        .justify_center()
        .when(show_img, |d| {
            d.bg(theme::FILL_TERTIARY).child(
                img(PathBuf::from(img_path))
                    .object_fit(ObjectFit::Fill)
                    .size(px(TRAY_PREVIEW))
                    .rounded(px(TILE_RADIUS)),
            )
        })
        .when_some(badge.clone(), |d, badge| {
            d.flex_col().gap(px(2.)).child(
                div()
                    .text_size(px(theme::FOOTNOTE.size))
                    .line_height(px(theme::FOOTNOTE.leading))
                    .font_weight(FontWeight::SEMIBOLD)
                    .bg(theme::SECONDARY_LABEL)
                    .text_color(theme::LABEL)
                    .child(badge),
            )
        })
        .when(!show_img && badge.is_none(), |d| {
            d.bg(theme::FILL_TERTIARY)
                .child(lucide_color("files", 22.0, theme::tertiary_label()))
        })
}

fn file_card(file: &FileTrayItem, cx: &mut Context<Island>) -> impl IntoElement {
    let path = file.path.clone();
    let path_send = path.clone();
    let path_rm = path.clone();
    let name = file.name.clone();

    div()
        .id(SharedString::from(format!("file-{}", file.path)))
        .group("file-card")
        .relative()
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
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                this.on_wheel(event, cx);
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
                        theme::COMPACT_FACE,
                        if hot {
                            theme::LABEL
                        } else {
                            theme::tertiary_label()
                        },
                    ))
                    .child(label(
                        if hot { RELEASE_HINT } else { "Drop files here" },
                        theme::CALLOUT,
                        true,
                    )),
            );
        }

        let picking = self.share.shows_picker();
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
                theme::SEPARATOR
            } else {
                theme::tertiary_label()
            })
            .child(row)
            .when(picking, |d| d.child(self.localsend_picker(cx)));

        let undo_live = self
            .last_cleared_files
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(5));
        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(6.))
            .when(!self.files.is_empty(), |d| {
                d.child(text_btn("Clear", cx, |this, _, cx| this.clear_files(cx)))
            })
            .when(undo_live, |d| {
                d.child(text_btn("Undo", cx, |this, _, cx| {
                    this.undo_clear_files(cx)
                }))
            });

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap(px(6.))
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .gap(px(12.))
                    .child(zone)
                    .when(hot, |row| {
                        let mut row = row.child(airdrop_target(cx));
                        if share::localsend::app_installed() {
                            row = row.child(localsend_target(cx));
                        }
                        row
                    }),
            )
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
