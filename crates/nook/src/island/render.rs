//! Overlay window paint: chrome, motion-blur stack, compact vs expanded dispatch.

use super::chrome::{hitbox_debug, island_chrome, WING};
use super::files::drop_veil;
use super::{CompactMode, Island};
use crate::platform;
use crate::theme;
use gpui::{
    div, point, prelude::*, px, rgba, AnyElement, App, Bounds, Context, CursorStyle,
    ExternalPaths, FontFallbacks, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ScrollWheelEvent, Window, WindowBackgroundAppearance, WindowBounds,
    WindowDecorations, WindowKind, WindowOptions,
};
use nook_core::notch;
use std::any::Any;

impl gpui::Render for Island {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_geometry(cx);
        if self.suppressed {
            nook_core::mouse::update_ui_bounds(0.0, -100.0, 0.0, 0.0);
            platform::sync_island_glass(None);
        }
        // Click-through is driven by the mouse poll loop against the painted
        // bounds, not from here — paint only publishes where those bounds are.
        let (tw, th) = (self.anim_w.value, self.anim_h.value);
        let attached = self.settings.island_attached(self.screen_height);
        let (body_left, body_top) = self.settings.island_origin(
            self.screen_width,
            self.screen_height,
            tw.max(1.0),
            th.max(1.0),
        );
        if !self.suppressed {
            nook_core::mouse::update_ui_bounds(
                body_left as f64,
                body_top as f64,
                tw as f64,
                th as f64,
            );
        }
        self.sync_overlay_strip(body_top, th.max(1.0), cx);

        let mode = self.mode();
        let expanded = self.expanded;
        let hovered = self.hovered;
        let notch_w = self.notch_width.max(1.0);
        let dropping = self.file_drag && (self.hovered || self.expanded);
        // Idle collapsed is a 1px wrap around the camera; the 6px ears would
        // stick out into the menu bar. They come back on hover, Live Activity,
        // and expand — those silhouettes are already wider than the housing.
        let show_wings = attached && th > 4.0 && (hovered || expanded || mode != CompactMode::Idle);

        let wing = if show_wings { WING } else { 0.0 };
        let chrome_w = tw.max(1.0) + wing * 2.0;
        let chrome_h = th.max(1.0);
        let chrome_left = (body_left - wing).max(0.0);
        let radius = if chrome_h > 80.0 {
            theme::EXPANDED_RADIUS
        } else {
            theme::COMPACT_RADIUS
        }
        .min(chrome_h * 0.5);
        // HIG › Materials: Liquid Glass must yield to Reduce Transparency.
        // Native glass sits behind Metal; a transparent fill lets it show.
        let want_glass = platform::island_glass_setting_on() && !self.suppressed;
        let tint = self.settings.island_color.map(|rgb| {
            let c = theme::rgba_from_u32(rgb, 1.0);
            (c.r, c.g, c.b)
        });
        let native_glass = if want_glass {
            let ok = platform::sync_island_glass(Some(platform::IslandGlass {
                x: chrome_left as f64,
                y: body_top as f64,
                w: chrome_w as f64,
                h: chrome_h as f64,
                radius: radius as f64,
                wing: wing as f64,
                tint,
            }));
            // If this tick failed to talk to AppKit, keep the live underlay
            // rather than painting the 82% black fallback over it.
            ok || platform::island_glass_attached()
        } else {
            platform::sync_island_glass(None)
        };
        let island_bg = if native_glass {
            rgba(0x00000000)
        } else if want_glass {
            theme::island_fill_glass(self.settings.island_color)
        } else {
            theme::island_fill(self.settings.island_color)
        };
        let agent_border = self
            .agents
            .iter()
            .any(|agent| agent.status.is_working())
            .then(theme::accent);
        let debug_hitbox = hitbox_debug();
        let content_radius = if expanded {
            theme::EXPANDED_RADIUS
        } else {
            theme::COMPACT_RADIUS
        };

        // The root is the overlay strip (full display width, sized by
        // `sync_overlay_strip`). Every layer inside is absolutely positioned,
        // so the island keeps its exact top-centre placement while the rest of
        // the strip stays available to paint into — an earlier flow layout
        // left a lit strip below the island in the leftover gap.
        // Nothing here decides input: click-through is driven by the mouse poll
        // loop against `update_ui_bounds` above, which still publishes only the
        // island's own rect.
        let root = div()
            .id("island-root")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgba(0x00000000))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if this.repositioning {
                    this.apply_reposition(event.position.x.into(), event.position.y.into());
                    cx.notify();
                }
                if this.poll_pending_file_drag(Some(window)) {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    let moved = this.finish_reposition();
                    let file = this.finish_file_press();
                    if moved || file {
                        cx.notify();
                    }
                }),
            )
            .font(gpui::Font {
                family: "SF Pro".into(),
                features: gpui::FontFeatures::default(),
                fallbacks: Some(FontFallbacks::from_fonts(vec![
                    "SF Compact".into(),
                    "SF Symbols".into(),
                    ".AppleSystemUIFont".into(),
                ])),
                weight: FontWeight::NORMAL,
                style: gpui::FontStyle::Normal,
            });
        let root = self.accept_file_drop(root, cx);
        root.when(!self.suppressed, |root| {
            root.child(
                div()
                    .absolute()
                    .top(px(body_top))
                    .left(px(chrome_left))
                    .w(px(chrome_w))
                    .h(px(chrome_h))
                    .child(
                        self.accept_file_drop(
                            div()
                                .id("island")
                                .relative()
                                .w(px(chrome_w))
                                .h(px(chrome_h))
                                .overflow_hidden()
                                .cursor(if self.repositioning {
                                    CursorStyle::ClosedHand
                                } else {
                                    CursorStyle::PointingHand
                                }),
                            cx,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                this.on_island_press(event, cx);
                            }),
                        )
                        .when(!expanded, |d| {
                            d.on_scroll_wheel(cx.listener(
                                |this, event: &ScrollWheelEvent, _, cx| {
                                    this.on_wheel(event, cx);
                                },
                            ))
                        })
                        .child(div().absolute().inset_0().child(island_chrome(
                            // Native glass draws no wings — NSGlassEffectView is a
                            // plain rounded rect spanning the full chrome width — so
                            // trace the accent border along that glass edge instead
                            // of the winged silhouette the painted fills use.
                            if native_glass { chrome_w } else { tw.max(1.0) },
                            th.max(1.0),
                            if native_glass { 0.0 } else { wing },
                            island_bg,
                            agent_border,
                            attached,
                        )))
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left(px(wing))
                                .w(px(tw.max(1.0)))
                                .h(px(th.max(1.0)))
                                .overflow_hidden()
                                .when(attached, |d| {
                                    d.rounded_bl(px(content_radius))
                                        .rounded_br(px(content_radius))
                                })
                                .when(!attached, |d| d.rounded(px(content_radius)))
                                .child(self.content_stack(expanded, mode, hovered, notch_w, cx))
                                .when(dropping && !expanded, |d| d.child(drop_veil()))
                                .when(!expanded, |d| d.child(self.mode_dots(cx))),
                        ),
                    ),
            )
        })
        .when(debug_hitbox, |d| d.child(self.hitbox_overlay()))
    }
}

impl Island {
    fn accept_file_drop<E>(&mut self, el: E, cx: &mut Context<Self>) -> E
    where
        E: InteractiveElement,
    {
        el.can_drop(|drag: &dyn Any, _, _| drag.downcast_ref::<ExternalPaths>().is_some())
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                log::info!("drop {} path(s)", paths.paths().len());
                this.ingest_paths(paths, cx);
            }))
    }

    /// Outlines the rects `nook_core::mouse` tests the cursor against, straight
    /// from the module that owns them so the drawing cannot drift from the
    /// testing: solid red for `hit_test_exact` (what decides click-through, i.e.
    /// whether a click is ours or the app's underneath) and faint red for
    /// `hit_test` (what arms hover). They coincide unless a Finder drag has
    /// widened the hover, in which case only the faint one grows.
    ///
    /// Screen x/y map straight to root x/y because the overlay strip spans the
    /// full display width from the top-left corner and the root is
    /// `size_full`. Carries no id or listeners, so it
    /// inserts no hitbox of its own and can't change what it is measuring.
    fn hitbox_overlay(&self) -> AnyElement {
        const EXACT: u32 = 0xff3b30ff;
        const DRAG_CAPTURE: u32 = 0xff3b3066;

        let outline = |bounds: nook_core::mouse::UiBounds, color: u32| {
            div()
                .absolute()
                .top(px(bounds.y as f32))
                .left(px(bounds.x as f32))
                .w(px(bounds.width as f32))
                .h(px(bounds.height as f32))
                .border_1()
                .border_color(rgba(color))
        };
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(outline(
                nook_core::mouse::drag_capture_bounds(),
                DRAG_CAPTURE,
            ))
            .child(outline(nook_core::mouse::exact_bounds(), EXACT))
            .into_any_element()
    }

    /// The animating content. GPUI has no blur filter, so while the spring is
    /// running we build one out of the compositor: the content is drawn three
    /// times as a 1-2-1 kernel offset along the direction of travel, which
    /// smears the edges instead of stepping them frame by frame. The taps fade
    /// out with the spring, so at rest this is the single crisp layer it was
    /// before — no cost once the island is parked.
    fn content_stack(
        &mut self,
        expanded: bool,
        mode: CompactMode,
        hovered: bool,
        notch_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let alpha = self.content_fade.value.clamp(0.0, 1.0);
        let content_x = self.content_x.value;
        let content_y = self.content_y.value;
        // A Finder drag needs the root's drop hitbox reachable, and the crisp
        // layer below blocks the mouse to keep the taps inert — so no smear
        // while something is being dragged onto us.
        let taps = if self.file_drag {
            None
        } else {
            self.blur_offset()
        };
        let mut stack = div().relative().size_full().overflow_hidden();

        if let Some((dx, dy)) = taps {
            // Clipping is left to the parent: masking each tap at its own
            // shifted frame would draw a seam inside the island.
            let tap_alpha = (0.25 * self.blur * alpha).clamp(0.0, 1.0);
            for (id, (ox, oy)) in [("blur-tap-lead", (dx, dy)), ("blur-tap-trail", (-dx, -dy))] {
                let ghost = self.content(expanded, mode, hovered, notch_w, cx);
                stack = stack.child(
                    div()
                        .id(id)
                        .absolute()
                        .top(px(content_y + oy))
                        .left(px(content_x + ox))
                        .w_full()
                        .h_full()
                        .opacity(tap_alpha)
                        .child(ghost),
                );
            }
        }

        let crisp = self.content(expanded, mode, hovered, notch_w, cx);
        // Losing half the centre weight to the taps is what makes the smear
        // read as a blur rather than as a doubled image.
        let centre_alpha = if taps.is_some() {
            alpha * (1.0 - 0.5 * self.blur)
        } else {
            alpha
        };
        stack
            .child(
                div()
                    .id("island-content")
                    .absolute()
                    .top(px(content_y))
                    .left(px(content_x))
                    .w_full()
                    .h_full()
                    .overflow_hidden()
                    .opacity(centre_alpha.clamp(0.0, 1.0))
                    .when(taps.is_some(), |d| {
                        // The taps carry copies of the real listeners. Block the
                        // mouse (not the wheel — that drives the swipes) so a
                        // click can only ever land once, and take over the
                        // expand toggle the blocked root would have handled.
                        d.block_mouse_except_scroll().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                this.on_island_press(event, cx);
                            }),
                        )
                    })
                    .child(crisp),
            )
            .into_any_element()
    }

    fn content(
        &mut self,
        expanded: bool,
        mode: CompactMode,
        hovered: bool,
        notch_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if expanded {
            self.render_expanded(notch_w, cx).into_any_element()
        } else {
            self.render_compact(mode, hovered, notch_w, cx)
                .into_any_element()
        }
    }

    /// Keep the overlay strip just tall enough for the island.
    ///
    /// The window is a full-width strip pinned to the top of the display, not
    /// a screen-sized canvas — Metal retains several backing buffers for the
    /// window, and at Retina resolution a full-screen transparent window costs
    /// hundreds of MB of GPU memory for pixels that are never visible. The
    /// strip covers the larger of the painted island and its animation target,
    /// so the window grows *before* an expand starts rather than chasing it,
    /// and the quantization inside `quantized_overlay_height` keeps a settling
    /// spring from resizing the NSWindow frame by frame. Screen coordinates
    /// stay equal to window coordinates everywhere the strip covers, so
    /// hit-testing, repositioning, drops, and the glass underlay carry over.
    fn sync_overlay_strip(&mut self, body_top: f32, body_h: f32, cx: &mut Context<Island>) {
        let (tw, th) = self.target_size();
        let (_, target_top) = self.settings.island_origin(
            self.screen_width,
            self.screen_height,
            tw.max(1.0),
            th.max(1.0),
        );
        let mut bottom = (body_top + body_h).max(target_top + th.max(1.0));
        // Pre-arm on approach: reserve the expanded footprint while the
        // cursor is merely near the parked island, before any animation can
        // start. An NSWindow resize mid-animation shows one stretched frame
        // (CoreAnimation scales the stale drawable), but over a resting
        // collapsed island it is invisible — the sliver is black on the notch.
        if (self.cursor_near || self.hovered) && !self.expanded {
            bottom = bottom.max(self.expanded_bottom());
        }
        // The 80pt Finder drag-capture pad below the island is only paid for
        // while a drag or reposition can actually use it; the strip regrows on
        // the next paint after the poll loop arms `file_drag`.
        let capture = self.file_drag || self.repositioning || self.pending_file_drag.is_some();
        let needed =
            notch::quantized_overlay_height(bottom as f64, self.screen_height as f64, capture);
        let published = notch::published_overlay_height();
        // Growing is urgent — content would clip. Shrinking is cosmetic: hold
        // it until the springs are parked and nothing is dragging, so the
        // resize never runs inside the collapse animation.
        if needed < published {
            let settled = (self.anim_w.value - tw).abs() < 0.5
                && (self.anim_h.value - th).abs() < 0.5
                && self.anim_w.velocity.abs() < 1.0
                && self.anim_h.velocity.abs() < 1.0;
            if !settled || capture {
                return;
            }
        }
        if notch::set_overlay_height(needed) != needed {
            Self::spawn_strip_resize(cx);
        }
    }

    /// Apply a published strip height to the NSWindow from a foreground task.
    ///
    /// Never resize from inside `render`: `Window::resize` fires
    /// `windowDidResize` synchronously, which re-enters GPUI's window state
    /// while render still borrows it — the resize is dropped with a "RefCell
    /// already borrowed" error and the viewport never grows, clipping the
    /// expanded island. The pin pass sets the frame through AppKit once the
    /// current update has fully unwound (the same footing as `spawn_pin`),
    /// and GPUI picks the new size up through its own resize delegate.
    fn spawn_strip_resize(cx: &mut Context<Island>) {
        cx.spawn(async move |_, _| {
            platform::pin_island_windows();
        })
        .detach();
    }

    pub(super) fn sync_geometry(&mut self, cx: &mut Context<Island>) {
        let gen = notch::screen_generation();
        if gen == self.screen_gen {
            return;
        }
        self.screen_gen = gen;
        let info = notch::get_notch_info();
        self.notch_width = info.notch_width as f32;
        self.notch_height = if info.has_notch {
            info.notch_height as f32
        } else {
            32.0
        };
        self.screen_width = info.screen_width as f32;
        self.screen_height = info.screen_height as f32;
        Self::spawn_strip_resize(cx);
    }
}

pub fn open_island(cx: &mut App) {
    let (w, h) = notch::overlay_window_size();
    let bounds = Bounds::from_corners(point(px(0.), px(0.)), point(px(w as f32), px(h as f32)));

    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            window_decorations: Some(WindowDecorations::Client),
            app_id: Some("com.jonasvogel.opennook-gpui".into()),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| Island::new(window, cx)),
    )
    .unwrap_or_else(|err| {
        log::error!("failed to open island window: {err}");
        panic!("open island window: {err}");
    });
}
