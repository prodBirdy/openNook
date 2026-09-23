//! Island silhouette: flat top, concave wings, rounded bottom.

use gpui::{canvas, point, prelude::*, px, Background, PathBuilder};

pub(super) const COMPACT_WING: f32 = 14.0;

/// Whether to outline the mouse hit regions. Off unless `NOOK_DEBUG_HITBOX=1`.
pub(super) fn hitbox_debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| match std::env::var("NOOK_DEBUG_HITBOX") {
        Ok(v) => matches!(v.trim(), "1" | "true" | "on" | "yes"),
        Err(_) => false,
    })
}

/// One filled silhouette, plus an optional hairline rim.
///
/// Notch-attached: flat top, concave wings, rounded bottom — GPUI's
/// per-corner radius was turning that into a capsule. Detached (moved off the
/// top edge): a rounded rect so the top corners are visible.
///
/// `rim` strokes the same outline half a point inside the edge — the Liquid
/// Glass edge highlight. Its paint is a top-to-bottom fade, so the flat top
/// against the screen edge stays dark and only the lower rim catches light.
pub(super) fn island_chrome(
    body_w: f32,
    body_h: f32,
    wing: f32,
    color: Background,
    attached: bool,
    radius: f32,
    rim: Option<Background>,
) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            let g = if attached { wing } else { 0.0 };
            let k = 0.552_284_8;
            let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
            let cubic = |path: &mut PathBuilder, to: (f32, f32), c1: (f32, f32), c2: (f32, f32)| {
                path.cubic_bezier_to(p(to.0, to.1), p(c1.0, c1.1), p(c2.0, c2.1));
            };

            // `inset` pulls the sides and bottom (and a detached top) in, so a
            // centred stroke lands inside the fill rather than on the desktop.
            let append_silhouette = |path: &mut PathBuilder, inset: f32| {
                let x0 = g + inset;
                let w = (body_w - 2.0 * inset).max(0.0);
                let h = (body_h - inset).max(0.0);
                let r = (radius - inset).max(0.0).min(h * 0.5).min(w * 0.5);
                let rk = k * r;
                if attached {
                    let top = 0.0;
                    path.move_to(p(0.0, top));
                    path.line_to(p(x0 + w + g, top));
                    if g > 0.5 {
                        let kk = k * g;
                        cubic(path, (x0 + w, g), (x0 + w + g - kk, top), (x0 + w, g - kk));
                    }
                    path.line_to(p(x0 + w, h - r));
                    cubic(path, (x0 + w - r, h), (x0 + w, h - r + rk), (x0 + w - r + rk, h));
                    path.line_to(p(x0 + r, h));
                    cubic(path, (x0, h - r), (x0 + r - rk, h), (x0, h - r + rk));
                    path.line_to(p(x0, g.max(0.0)));
                    if g > 0.5 {
                        let kk = k * g;
                        cubic(path, (x0 - g, top), (x0, g - kk), (x0 - g + kk, top));
                    }
                } else {
                    let y0 = inset;
                    let h = h - inset;
                    let r = r.min(h * 0.5);
                    let rk = k * r;
                    path.move_to(p(x0 + r, y0));
                    path.line_to(p(x0 + w - r, y0));
                    cubic(path, (x0 + w, y0 + r), (x0 + w - r + rk, y0), (x0 + w, y0 + r - rk));
                    path.line_to(p(x0 + w, y0 + h - r));
                    cubic(
                        path,
                        (x0 + w - r, y0 + h),
                        (x0 + w, y0 + h - r + rk),
                        (x0 + w - r + rk, y0 + h),
                    );
                    path.line_to(p(x0 + r, y0 + h));
                    cubic(path, (x0, y0 + h - r), (x0 + r - rk, y0 + h), (x0, y0 + h - r + rk));
                    path.line_to(p(x0, y0 + r));
                    cubic(path, (x0 + r, y0), (x0, y0 + r - rk), (x0 + r - rk, y0));
                }
                path.close();
            };

            // Always fill. On native glass `color` is a black veil; where it
            // clears, the system material behind Metal is the other end.
            let mut fill = PathBuilder::fill();
            append_silhouette(&mut fill, 0.0);
            match fill.build() {
                Ok(built) => window.paint_path(built, color),
                Err(err) => log::warn!("island path: {err}"),
            }
            if let Some(rim) = rim {
                let mut stroke = PathBuilder::stroke(px(RIM_WIDTH));
                append_silhouette(&mut stroke, RIM_WIDTH * 0.5);
                match stroke.build() {
                    Ok(built) => window.paint_path(built, rim),
                    Err(err) => log::warn!("island rim: {err}"),
                }
            }
        },
    )
    .w(px(body_w + if attached { wing * 2.0 } else { 0.0 }))
    .h(px(body_h))
}

/// Liquid Glass edge highlight width.
const RIM_WIDTH: f32 = 1.0;

/// One concave fillet beside the notch, drawn on its own (Liquid Glass keeps
/// the glass view a plain rect, so the wings it would have carried are
/// painted here). `left` is the wing west of the pill: filled along the top
/// and its right edge, `M0 0 l14 0 0 14 c0-7.73-6.27-14-14-14z`; the east
/// wing mirrors it. Purely visual — no hitbox, so click-through is unchanged.
pub(super) fn notch_wing(left: bool, size: f32, color: Background) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
            let kk = 0.552_284_8 * size;
            let mut path = PathBuilder::fill();
            path.move_to(p(0.0, 0.0));
            path.line_to(p(size, 0.0));
            if left {
                path.line_to(p(size, size));
                path.cubic_bezier_to(p(0.0, 0.0), p(size, size - kk), p(kk, 0.0));
            } else {
                path.cubic_bezier_to(p(0.0, size), p(size - kk, 0.0), p(0.0, size - kk));
            }
            path.close();
            match path.build() {
                Ok(built) => window.paint_path(built, color),
                Err(err) => log::warn!("island wing: {err}"),
            }
        },
    )
    .size(px(size))
}
