//! Island silhouette: flat top, concave wings, rounded bottom.

use crate::theme;
use gpui::{canvas, point, prelude::*, px, PathBuilder};

pub(super) const WING: f32 = 6.0;

/// Whether to outline the mouse hit regions. Off unless `NOOK_DEBUG_HITBOX=1`.
pub(super) fn hitbox_debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| match std::env::var("NOOK_DEBUG_HITBOX") {
        Ok(v) => matches!(v.trim(), "1" | "true" | "on" | "yes"),
        Err(_) => false,
    })
}

/// One filled silhouette: flat top, concave 6px wings, rounded bottom.
/// GPUI's per-corner radius was turning the compact island into a capsule.
pub(super) fn island_chrome(
    body_w: f32,
    body_h: f32,
    wing: f32,
    color: gpui::Rgba,
) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            let g = wing;
            let w = body_w;
            let h = body_h;
            let r = if h > 80.0 {
                theme::EXPANDED_RADIUS
            } else {
                theme::COMPACT_RADIUS
            }
            .min(h * 0.5);
            let k = 0.552_284_75;
            let p = |x: f32, y: f32| point(px(ox + x), px(oy + y));
            let cubic = |path: &mut PathBuilder, to: (f32, f32), c1: (f32, f32), c2: (f32, f32)| {
                path.cubic_bezier_to(p(to.0, to.1), p(c1.0, c1.1), p(c2.0, c2.1));
            };

            let mut path = PathBuilder::fill();
            path.move_to(p(0.0, 0.0));
            path.line_to(p(g + w + g, 0.0));
            if g > 0.5 {
                let kk = k * g;
                cubic(
                    &mut path,
                    (g + w, g),
                    (g + w + g - kk, 0.0),
                    (g + w, g - kk),
                );
            }
            path.line_to(p(g + w, h - r));
            let rk = k * r;
            cubic(
                &mut path,
                (g + w - r, h),
                (g + w, h - r + rk),
                (g + w - r + rk, h),
            );
            path.line_to(p(g + r, h));
            cubic(&mut path, (g, h - r), (g + r - rk, h), (g, h - r + rk));
            path.line_to(p(g, g.max(0.0)));
            if g > 0.5 {
                let kk = k * g;
                cubic(&mut path, (0.0, 0.0), (g, g - kk), (kk, 0.0));
            }
            path.close();
            match path.build() {
                Ok(built) => window.paint_path(built, color),
                Err(err) => log::warn!("island path: {err}"),
            }
        },
    )
    .w(px(body_w + wing * 2.0))
    .h(px(body_h))
}
