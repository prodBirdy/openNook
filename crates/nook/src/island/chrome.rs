//! Island silhouette: flat top, concave wings, rounded bottom.

use gpui::{canvas, point, prelude::*, px, PathBuilder, Rgba};

pub(super) const COMPACT_WING: f32 = 14.0;
/// Extra canvas around the silhouette so the brand glow is not clipped.
pub(super) const GLOW_PAD: f32 = 14.0;

/// Whether to outline the mouse hit regions. Off unless `NOOK_DEBUG_HITBOX=1`.
pub(super) fn hitbox_debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| match std::env::var("NOOK_DEBUG_HITBOX") {
        Ok(v) => matches!(v.trim(), "1" | "true" | "on" | "yes"),
        Err(_) => false,
    })
}

/// One filled silhouette.
///
/// Notch-attached: flat top, concave wings, rounded bottom — GPUI's
/// per-corner radius was turning that into a capsule. Detached (moved off the
/// top edge): a rounded rect so the top corners are visible.
pub(super) fn island_chrome(
    body_w: f32,
    body_h: f32,
    wing: f32,
    color: gpui::Rgba,
    border_color: Option<gpui::Rgba>,
    glow: f32,
    attached: bool,
    radius: f32,
    // Soft multi-band radiation. Off while the size spring is moving so the
    // morph is not redrawing six stroked paths every frame.
    soft_glow: bool,
    // Native glass already rims the curve; a 1px core on that edge doubles it.
    glass_rim: bool,
) -> impl IntoElement {
    let glow = glow.clamp(0.0, 1.0);
    let pad = if border_color.is_some() && glow > 0.02 {
        GLOW_PAD
    } else {
        0.0
    };
    let top_pad = if attached { 0.0 } else { pad };
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            let ox = ox + pad;
            let oy = oy + top_pad;
            let g = if attached { wing } else { 0.0 };
            let w = body_w;
            let h = body_h;
            let r = radius.min(h * 0.5).min(w * 0.5);
            let k = 0.552_284_8;
            let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
            let cubic = |path: &mut PathBuilder, to: (f32, f32), c1: (f32, f32), c2: (f32, f32)| {
                path.cubic_bezier_to(p(to.0, to.1), p(c1.0, c1.1), p(c2.0, c2.1));
            };

            let append_silhouette = |path: &mut PathBuilder| {
                if attached {
                    path.move_to(p(0.0, 0.0));
                    path.line_to(p(g + w + g, 0.0));
                    if g > 0.5 {
                        let kk = k * g;
                        cubic(path, (g + w, g), (g + w + g - kk, 0.0), (g + w, g - kk));
                    }
                    path.line_to(p(g + w, h - r));
                    let rk = k * r;
                    cubic(
                        path,
                        (g + w - r, h),
                        (g + w, h - r + rk),
                        (g + w - r + rk, h),
                    );
                    path.line_to(p(g + r, h));
                    cubic(path, (g, h - r), (g + r - rk, h), (g, h - r + rk));
                    path.line_to(p(g, g.max(0.0)));
                    if g > 0.5 {
                        let kk = k * g;
                        cubic(path, (0.0, 0.0), (g, g - kk), (kk, 0.0));
                    }
                } else {
                    let rk = k * r;
                    path.move_to(p(r, 0.0));
                    path.line_to(p(w - r, 0.0));
                    cubic(path, (w, r), (w - r + rk, 0.0), (w, r - rk));
                    path.line_to(p(w, h - r));
                    cubic(path, (w - r, h), (w, h - r + rk), (w - r + rk, h));
                    path.line_to(p(r, h));
                    cubic(path, (0.0, h - r), (r - rk, h), (0.0, h - r + rk));
                    path.line_to(p(0.0, r));
                    cubic(path, (r, 0.0), (0.0, r - rk), (r - rk, 0.0));
                }
                path.close();
            };

            if !glass_rim {
                let mut fill = PathBuilder::fill();
                append_silhouette(&mut fill);
                match fill.build() {
                    Ok(built) => window.paint_path(built, color),
                    Err(err) => log::warn!("island path: {err}"),
                }
            }

            if let Some(border_color) = border_color {
                let stroke_edge = |path: &mut PathBuilder| {
                    if attached {
                        // The screen edge is the attached island's top edge, so leave
                        // that edge open instead of drawing an accent line across it.
                        path.move_to(p(g + w + g, 0.0));
                        if g > 0.5 {
                            let kk = k * g;
                            cubic(path, (g + w, g), (g + w + g - kk, 0.0), (g + w, g - kk));
                        }
                        path.line_to(p(g + w, h - r));
                        let rk = k * r;
                        cubic(
                            path,
                            (g + w - r, h),
                            (g + w, h - r + rk),
                            (g + w - r + rk, h),
                        );
                        path.line_to(p(g + r, h));
                        cubic(path, (g, h - r), (g + r - rk, h), (g, h - r + rk));
                        path.line_to(p(g, g.max(0.0)));
                        if g > 0.5 {
                            let kk = k * g;
                            cubic(path, (0.0, 0.0), (g, g - kk), (kk, 0.0));
                        }
                    } else {
                        append_silhouette(path);
                    }
                };
                // Soft radiation, wide to tight, then a crisp 1px core.
                // On native glass skip the core — NSGlassEffectView already
                // sheens that curve, and a second 1px line is a doubled border
                // (loudest on the compact agent face, where the glow pulses).
                let bands: &[(f32, f32)] = if soft_glow && glow > 0.02 {
                    if glass_rim {
                        &[(16.0, 0.05), (11.0, 0.08), (7.0, 0.12), (4.0, 0.20)]
                    } else {
                        &[
                            (16.0, 0.05),
                            (11.0, 0.08),
                            (7.0, 0.12),
                            (4.0, 0.20),
                            (2.2, 0.38),
                            (1.15, 0.92),
                        ]
                    }
                } else if glass_rim {
                    &[(4.0, 0.35)]
                } else {
                    &[(1.15, 1.0)]
                };
                for &(width, a) in bands {
                    let mut border = PathBuilder::stroke(px(width));
                    stroke_edge(&mut border);
                    match border.build() {
                        Ok(built) => window.paint_path(
                            built,
                            with_alpha(
                                border_color,
                                a * if soft_glow && glow > 0.02 { glow } else { 1.0 },
                            ),
                        ),
                        Err(err) => log::warn!("island border path: {err}"),
                    }
                }
            }
        },
    )
    .w(px(body_w
        + if attached { wing * 2.0 } else { 0.0 }
        + pad * 2.0))
    .h(px(body_h + pad + top_pad))
}

fn with_alpha(color: Rgba, a: f32) -> Rgba {
    Rgba {
        a: (color.a * a).clamp(0.0, 1.0),
        ..color
    }
}
