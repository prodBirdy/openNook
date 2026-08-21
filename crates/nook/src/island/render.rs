//! Overlay window paint: chrome, motion-blur stack, compact vs expanded dispatch.

use super::chrome::{hitbox_debug, island_chrome, WING};
use super::files::drop_veil;
use super::{CompactMode, Island};
use crate::platform;
use crate::theme;
use gpui::{
    div, point, prelude::*, px, rgba, size, AnyElement, App, Bounds, Context, CursorStyle,
    ExternalPaths, FontFallbacks, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ScrollWheelEvent, Window, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions,
};
use nook_core::notch;
use std::any::Any;

impl gpui::Render for Island {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_geometry(window);
        // Click-through is driven by the mouse poll loop against the painted
        // bounds, not from here — paint only publishes where those bounds are.
        let (tw, th) = (self.anim_w, self.anim_h);
        nook_core::mouse::update_ui_bounds(
            ((self.screen_width - tw) / 2.0) as f64,
            0.0,
            tw as f64,
            th as f64,
        );

        // HIG > Materials: materials must "respond to system settings such as
        // reduced transparency", and translucency here is opt-in, so the system
        // setting overrides it.
        let island_bg =
            if self.settings.liquid_glass_mode && !crate::platform::reduce_transparency() {
                theme::ISLAND_GLASS
            } else {
                theme::ISLAND
            };
        let mode = self.mode();
        let expanded = self.expanded;
        let hovered = self.hovered;
        let notch_w = self.notch_width.max(180.0);
        let dropping = self.file_drag && (self.hovered || self.expanded);
        let show_wings = th > 4.0
            && (!self.settings.non_notch_mode || mode != CompactMode::Idle || hovered || expanded);

        let wing = if show_wings { WING } else { 0.0 };
        let chrome_w = tw.max(1.0) + wing * 2.0;
        let chrome_h = th.max(1.0);
        let debug_hitbox = hitbox_debug();

        // The root is the whole display. Every layer inside is absolutely
        // positioned, so the island keeps its exact top-centre placement while
        // the rest of the screen stays available to paint into — an earlier
        // flow layout left a lit strip below the island in the leftover gap.
        // Nothing here decides input: click-through is driven by the mouse poll
        // loop against `update_ui_bounds` above, which still publishes only the
        // island's own rect.
        let root = div()
            .id("island-root")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgba(0x00000000))
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, window, cx| {
                if this.poll_pending_file_drag(Some(window)) {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.finish_file_press() {
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
        root.child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(chrome_h))
                .flex()
                .justify_center()
                .child(
                    self.accept_file_drop(
                        div()
                            .id("island")
                            .relative()
                            .w(px(chrome_w))
                            .h(px(chrome_h))
                            .overflow_hidden()
                            .cursor(CursorStyle::PointingHand),
                        cx,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.toggle_expanded(cx);
                        }),
                    )
                    .when(!expanded, |d| {
                        d.on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                            this.on_wheel(event, cx);
                        }))
                    })
                    .child(div().absolute().inset_0().child(island_chrome(
                        tw.max(1.0),
                        th.max(1.0),
                        wing,
                        island_bg,
                    )))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left(px(wing))
                            .w(px(tw.max(1.0)))
                            .h(px(th.max(1.0)))
                            .overflow_hidden()
                            .rounded_bl(px(if expanded {
                                theme::EXPANDED_RADIUS
                            } else {
                                theme::COMPACT_RADIUS
                            }))
                            .rounded_br(px(if expanded {
                                theme::EXPANDED_RADIUS
                            } else {
                                theme::COMPACT_RADIUS
                            }))
                            .child(self.content_stack(expanded, mode, hovered, notch_w, cx))
                            .when(dropping && !expanded, |d| d.child(drop_veil()))
                            .when(!expanded, |d| d.child(self.mode_dots(cx))),
                    ),
                ),
        )
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
    /// Screen x maps straight to root x because the overlay window spans the
    /// whole display and the root is `w_full`. Carries no id or listeners, so it
    /// inserts no hitbox of its own and can't change what it is measuring.
    fn hitbox_overlay(&self) -> AnyElement {
        const EXACT: u32 = 0xff3b30ff;
        const HOVER: u32 = 0xff3b3066;

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
            .child(outline(nook_core::mouse::hover_bounds(), HOVER))
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
        let alpha = self.content_opacity.clamp(0.0, 1.0);
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
                        .top(px(oy))
                        .left(px(ox))
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
                    .top_0()
                    .left_0()
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
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.toggle_expanded(cx);
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

    pub(super) fn sync_geometry(&mut self, window: &mut Window) {
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
        let (w, h) = notch::overlay_window_size();
        window.resize(size(px(w as f32), px(h as f32)));
        platform::pin_island_windows();
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
            window_decorations: None,
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
