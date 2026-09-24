//! Overlay window paint: chrome, motion-blur stack, compact vs expanded dispatch.

use super::chrome::{hitbox_debug, island_chrome, notch_wing, COMPACT_WING};
use super::files::drop_veil;
use super::{CompactMode, Island};
use crate::motion;
use crate::platform;
use crate::theme;
use gpui::{
    div, point, prelude::*, px, rgba, AnyElement, App, Bounds, Context, CursorStyle, ExternalPaths,
    FontFallbacks, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ScrollWheelEvent, Window, WindowBackgroundAppearance, WindowBounds,
    WindowDecorations, WindowKind, WindowOptions,
};
use nook_core::notch;
use std::any::Any;

impl gpui::Render for Island {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Real widget previews stay on screen in customize mode; park marquees
        // so overflowing titles do not keep requesting frames under edit chrome.
        super::marquee::set_animate(!self.widget_edit && !self.reduce_motion);
        if self.expanded {
            if let Some(focus) = &self.focus {
                if window.focused(cx).is_none() {
                    window.focus(focus);
                }
            }
        }
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

        if !self.suppressed {
            if self.springs_off_target() {
                self.springs_moving = true;
            }
            if self.springs_moving {
                self.arm_frame_driver(window, cx);
            }
        }

        let mode = self.mode();
        let expanded = self.expanded;
        let hovered = self.hovered;
        let notch_w = self.notch_width.max(1.0);
        let dropping = self.file_drag && (self.hovered || self.expanded);
        // HIG › Materials: Liquid Glass must yield to Reduce Transparency.
        // Compact stays black. The material fades in as the sheet opens; the
        // slider sets how far the black cap reaches once it is open.
        let want_glass = platform::island_glass_setting_on() && !self.suppressed;
        let mut wing = COMPACT_WING;

        if want_glass {
            wing = 0.00
        }
        let chrome_w = tw.max(1.0) + wing * 2.0;
        let chrome_h = th.max(1.0);

        let chrome_left = (body_left - wing).max(0.0);
        // Compact hover grows a chin under the pill. Pin the corner to the
        // parked compact half-height so the top clip and the side wings stay
        // put — tracking live `chrome_h` was rounding the top on every hover.
        let compact_h = self.notch_height.max(theme::NOTCH_MIN_H) + theme::COMPACT_HEIGHT_OVERFLOW;
        let radius = if expanded {
            theme::EXPANDED_RADIUS.min(chrome_h * 0.5)
        } else {
            // Compact glass corners must match chrome.rs (COMPACT_RADIUS when h <= 80,
            // including the hover chin at compact_h + 11).
            theme::COMPACT_RADIUS.min(compact_h * 0.5)
        };

        let tint = self.settings.island_color.map(|rgb| {
            let c = theme::rgba_from_u32(rgb, 1.0);
            (c.r, c.g, c.b)
        });
        let glass_ceiling = theme::compact_glass_ceiling(self.notch_height);
        let glass_amount = self.settings.glass_gradient();
        // Height opens the sheet. The slow swipe opens the veil across the
        // whole stretch, over material that is already present.
        let from_height = theme::glass_veil_reveal(chrome_h, glass_ceiling);
        let from_pull = theme::glass_pull_reveal(self.expand_pull, motion::EXPAND_PULL_MAX);
        let glass_open = from_height.max(from_pull);
        let glass_material = from_height.max(theme::glass_pull_material(
            self.expand_pull,
            motion::EXPAND_PULL_MAX,
        ));
        let native_glass = if want_glass {
            let ok = platform::sync_island_glass(Some(platform::IslandGlass {
                x: chrome_left as f64,
                y: body_top as f64,
                w: chrome_w as f64,
                h: chrome_h as f64,
                radius: radius as f64,
                wing: wing as f64,
                tint,
                opacity: glass_material as f64,
            }));
            // If this tick failed to talk to AppKit, keep the live underlay
            // rather than painting the 82% black fallback over it.
            ok || platform::island_glass_attached()
        } else {
            platform::sync_island_glass(None)
        };
        let island_bg = if native_glass {
            theme::island_glass_veil(chrome_h, glass_open, glass_amount)
        } else if want_glass {
            theme::island_glass_fallback_veil(
                self.settings.island_color,
                chrome_h,
                glass_open,
                glass_amount,
            )
        } else {
            theme::island_fill(self.settings.island_color).into()
        };
        // Glass keeps NSGlassEffectView a plain rect (wing 0), so the notch
        // fillets are painted beside it in the colour of the veil's top edge:
        // black on the compact cap, then whatever the veil holds at the top
        // as the sheet opens (the mockup keeps them on the expanded sheet).
        let glass_wings = if want_glass && attached {
            let (_, floor) = theme::glass_veil_curve(
                chrome_h,
                glass_amount,
                platform::increase_contrast(),
            );
            let (top_a, _) = theme::glass_veil_alphas(glass_open, glass_amount, floor);
            let cap = if native_glass {
                theme::ISLAND
            } else {
                theme::island_fill(self.settings.island_color)
            };
            Some(theme::with_alpha(cap, top_a * cap.a))
        } else {
            None
        };
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
                this.alt_held = event.modifiers.alt;
                if this.repositioning {
                    this.apply_reposition(event.position.x.into(), event.position.y.into());
                    cx.notify();
                }
                if this.scrubber_drag.is_some() {
                    this.update_scrubber_from_x(event.position.x.into());
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
                    let seek = this.finish_scrubber(cx);
                    this.end_hud_drag();
                    if moved || file || seek {
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
        root.when_some(glass_wings, |root, color| {
            root.child(
                div()
                    .absolute()
                    .top(px(body_top))
                    .left(px(chrome_left - COMPACT_WING))
                    .child(notch_wing(true, COMPACT_WING, color.into())),
            )
            .child(
                div()
                    .absolute()
                    .top(px(body_top))
                    .left(px(chrome_left + chrome_w))
                    .child(notch_wing(false, COMPACT_WING, color.into())),
            )
        })
        .when(!self.suppressed, |root| {
            root.child(
                div()
                    .absolute()
                    .top(px(body_top))
                    .left(px(chrome_left))
                    .w(px(chrome_w))
                    .h(px(chrome_h))
                    .child(div().absolute().inset_0().child(island_chrome(
                        // Native glass draws no wings — NSGlassEffectView is a
                        // plain rounded rect spanning the full chrome width.
                        if native_glass { chrome_w } else { tw.max(1.0) },
                        th.max(1.0),
                        if native_glass { 0.0 } else { wing },
                        island_bg,
                        attached,
                        content_radius,
                        want_glass.then(theme::island_glass_rim),
                    )))
                    .child(
                        self.accept_file_drop(
                            div()
                                .id("island")
                                .absolute()
                                .top(px(0.))
                                .left(px(0.))
                                .w(px(chrome_w))
                                .h(px(chrome_h))
                                .overflow_hidden()
                                .cursor(if self.repositioning {
                                    CursorStyle::ClosedHand
                                } else if self.alt_held {
                                    CursorStyle::OpenHand
                                } else {
                                    CursorStyle::PointingHand
                                })
                                .when_some(self.focus.as_ref(), |d, focus| {
                                    d.track_focus(focus).key_context("Island").on_key_down(
                                        cx.listener(|this, event: &KeyDownEvent, window, cx| {
                                            this.on_island_key(event, window, cx);
                                        }),
                                    )
                                }),
                            cx,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                this.on_island_press(event, cx);
                            }),
                        )
                        .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                            this.on_wheel(event, cx);
                        }))
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
                                .child(if expanded {
                                    // Body band under the pinned top bar. Clipping
                                    // here keeps the blur taps and the tab slide
                                    // from ghosting over the pills.
                                    let bar_h = self.expanded_topbar_height();
                                    div()
                                        .absolute()
                                        .top(px(bar_h))
                                        .left_0()
                                        .w_full()
                                        .h(px((th.max(1.0) - bar_h).max(0.0)))
                                        .overflow_hidden()
                                        .child(
                                            self.content_stack(expanded, mode, hovered, notch_w, cx),
                                        )
                                        .into_any_element()
                                } else {
                                    self.content_stack(expanded, mode, hovered, notch_w, cx)
                                })
                                .when(expanded, |d| d.child(self.pinned_topbar(notch_w, cx)))
                                .when(dropping && !expanded, |d| d.child(drop_veil()))
                                .when(
                                    !expanded
                                        && self.anim_h.value
                                            > self.notch_height.max(theme::NOTCH_MIN_H) + 0.5,
                                    |d| {
                                        // Opacity must wrap only the dots. Applying it to
                                        // this parent also faded `content_stack` — and the
                                        // 1px compact overflow alone was enough to leave
                                        // the face at ~9% opacity in normal compact mode.
                                        let base = self.notch_height.max(theme::NOTCH_MIN_H);
                                        let fade = ((self.anim_h.value - base)
                                            / theme::COMPACT_HOVER_CHIN)
                                            .clamp(0.0, 1.0);
                                        d.child(div().opacity(fade).child(self.mode_dots(cx)))
                                    },
                                ),
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
        // while something is being dragged onto us. Working-agent LED faces
        // also skip taps during lite morph/shift: 3× glowing matrices starved
        // Files↔Agents context springs.
        let taps = if self.file_drag || (self.size_morphing() && self.agent_is_working()) {
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

    /// The expanded tab bar, drawn once above the animating body so tab
    /// switches leave it perfectly still: no crossfade, no slide, no blur
    /// taps. Only an expand reveal or a customize swap moves it with the body.
    fn pinned_topbar(&mut self, notch_w: f32, cx: &mut Context<Self>) -> AnyElement {
        let (alpha, dy) = if self.topbar_follows_reveal {
            (self.content_fade.value.clamp(0.0, 1.0), self.content_y.value)
        } else {
            (1.0, 0.0)
        };
        div()
            .id("island-topbar")
            .absolute()
            .top(px(dy))
            .left_0()
            .w_full()
            .opacity(alpha)
            .child(self.render_topbar(notch_w, cx))
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
            self.render_expanded(cx).into_any_element()
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
        // Reserve the expanded footprint while expanded or on approach, so a
        // switch to a taller tab (or an expand) never resizes the NSWindow
        // mid-animation. CoreAnimation would scale the stale drawable for one
        // stretched frame; over a resting collapsed island that resize is
        // invisible — the sliver is black on the notch.
        if self.expanded || self.cursor_near || self.hovered {
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
            // Observe open grows the strip before flipping observe_expanded;
            // a paint in between would otherwise shrink it back.
            if !settled || capture || self.observe_open_pending {
                return;
            }
        }
        if notch::set_overlay_height(needed) != needed {
            Self::spawn_strip_resize(cx).detach();
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
    pub(super) fn spawn_strip_resize(cx: &mut Context<Island>) -> gpui::Task<()> {
        cx.spawn(async move |_, _| {
            platform::pin_island_windows();
        })
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
        Self::spawn_strip_resize(cx).detach();
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
