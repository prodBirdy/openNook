//! GPUI port of the 3×3 [Dot Matrix](https://dotmatrix.zzzzshawn.cloud/)
//! loaders the coding agents spin with: Drift TL (`dotm-3x3-3`), Core Echo
//! (`dotm-3x3-6`) and Smiley Spin (`dotm-3x3-16`), each at half the upstream
//! `speed 1.2` with bloom, one per agent by pid. Only a working agent
//! animates: an idle one holds a flat grey cluster.
//!
//! Animation math follows the upstream CSS keyframes and the Swift
//! [matrix-swift](https://github.com/mana-am/matrix-swift) TimelineView port.
//! Embedded use in an application is permitted by the upstream license;
//! this is not a republished component library.

mod engine;
mod grid3;

use engine::{bloom_level, Ctx, N};
use gpui::{canvas, fill, point, prelude::*, px, Bounds, IntoElement, Pixels, Rgba, Window};

pub use grid3::Kind;

/// Half of the `speed={1.2}` in
/// `<Dotm3x3_N size={32} dotSize={4} speed={1.2} bloom />` — the upstream
/// cadence reads as frantic in the notch, so the port runs it at half rate.
pub const SPEED: f32 = 0.6;
/// Upstream pins `dotSize` 4 in a 16px box (1px gap). Compact and the widget
/// row both use that cluster; scaling it to the 26px notch face made 7px dots
/// that filled the compact island.
const DOT_RATIO: f32 = 4.0 / 16.0;
const GAP_RATIO: f32 = 1.0 / 4.0;
/// Idle dots sit below the 0.6 bloom threshold, so the flat cluster never glows.
const IDLE_ALPHA: f32 = 0.45;
/// The idle cluster drops the accent for the secondary-label grey. Only the
/// RGB is used — `IDLE_ALPHA` sets the alpha — and only a working agent is
/// tinted with the accent.
const IDLE_TINT: Rgba = crate::theme::SECONDARY_LABEL;
/// Upstream `size={16}` / `dotSize={4}` cluster (14px span), not the 26px face.
pub const COMPACT_SIZE: f32 = 16.0;
pub const WIDGET_SIZE: f32 = 16.0;

const POOL: [Kind; 3] = [Kind::DriftTl, Kind::CoreEcho, Kind::SmileySpin];

/// Deterministic pick: the same seed always lands on the same loader.
pub fn pick(seed: u32) -> Kind {
    POOL[(seed as usize) % POOL.len()]
}

/// `pattern="full"`, so every in-bounds cell of the grid is lit. An idle agent
/// holds every cell at `IDLE_ALPHA` — no animation, no bright cell, no bloom —
/// so a working agent is the only loader that moves or glows.
pub fn cell_opacity(kind: Kind, row: i32, col: i32, now: f32, working: bool) -> f32 {
    if !(0..N).contains(&row) || !(0..N).contains(&col) {
        return 0.0;
    }
    if !working {
        return IDLE_ALPHA;
    }
    grid3::opacity(kind, &Ctx { row, col }, now).clamp(0.0, 1.0)
}

/// Geometry of a loader on one face: dot diameter, gap, and total span.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DotLayout {
    pub dot: f32,
    pub gap: f32,
    pub span: f32,
}

/// [`DotLayout`] for a face of `size`.
pub fn layout(size: f32) -> DotLayout {
    let dot = (size * DOT_RATIO).round().max(1.0);
    let gap = (dot * GAP_RATIO).round().max(1.0);
    DotLayout {
        dot,
        gap,
        span: dot * N as f32 + gap * (N as f32 - 1.0),
    }
}

pub fn element(kind: Kind, now: f32, working: bool, size: f32) -> impl IntoElement {
    let lay = layout(size);
    let now = now * SPEED;
    // Resolved per frame, not cached, so changing the accent in System Settings
    // recolors the loader without a restart.
    let tint = if working {
        crate::theme::accent()
    } else {
        IDLE_TINT
    };
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            paint_grid(window, bounds, kind, now, working, lay, tint);
        },
    )
    .size(px(size))
    .flex_shrink_0()
}

fn paint_grid(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    kind: Kind,
    now: f32,
    working: bool,
    lay: DotLayout,
    tint: Rgba,
) {
    let ox: f32 = bounds.origin.x.into();
    let oy: f32 = bounds.origin.y.into();
    let bw: f32 = bounds.size.width.into();
    let bh: f32 = bounds.size.height.into();
    let ox = ox + (bw - lay.span).max(0.0) * 0.5;
    let oy = oy + (bh - lay.span).max(0.0) * 0.5;
    let mut cells = Vec::with_capacity((N * N) as usize);
    for row in 0..N {
        for col in 0..N {
            let a = cell_opacity(kind, row, col, now, working);
            if a < 0.01 {
                continue;
            }
            let x = ox + col as f32 * (lay.dot + lay.gap);
            let y = oy + row as f32 * (lay.dot + lay.gap);
            cells.push((x, y, a, bloom_level(a)));
        }
    }
    for &(x, y, _, level) in &cells {
        if level <= 0.0 {
            continue;
        }
        paint_glow(window, x, y, lay.dot, level, tint);
    }
    let radius = px(lay.dot * 0.5);
    for &(x, y, a, _) in &cells {
        window.paint_quad(
            fill(
                Bounds::from_corners(point(px(x), px(y)), point(px(x + lay.dot), px(y + lay.dot))),
                alpha(tint, a),
            )
            .corner_radii(radius),
        );
    }
}

/// The animation drives opacity only; hue and saturation stay the accent's.
fn alpha(tint: Rgba, a: f32) -> Rgba {
    Rgba {
        a: a.clamp(0.0, 1.0),
        ..tint
    }
}

fn paint_glow(window: &mut Window, x: f32, y: f32, dot: f32, level: f32, tint: Rgba) {
    // CSS: drop-shadow radii `dot * 0.75 * level` and `dot * 1.35 * level`.
    let bands = [
        (dot * 1.35 * level, 0.22 * level),
        (dot * 0.75 * level, 0.45 * level),
    ];
    let cx = x + dot * 0.5;
    let cy = y + dot * 0.5;
    for (blur, a) in bands {
        if a <= 0.0 {
            continue;
        }
        let r = dot * 0.5 + blur;
        window.paint_quad(
            fill(
                Bounds::from_corners(point(px(cx - r), px(cy - r)), point(px(cx + r), px(cy + r))),
                alpha(tint, a),
            )
            .corner_radii(px(r)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_cycles_the_trio() {
        assert_eq!(pick(0), Kind::DriftTl);
        assert_eq!(pick(1), Kind::CoreEcho);
        assert_eq!(pick(2), Kind::SmileySpin);
        assert_eq!(pick(0), pick(POOL.len() as u32));
    }

    fn frame(kind: Kind, now: f32, working: bool) -> Vec<f32> {
        (0..N)
            .flat_map(|row| (0..N).map(move |col| (row, col)))
            .map(|(row, col)| cell_opacity(kind, row, col, now, working))
            .collect()
    }

    #[test]
    fn idle_is_flat_and_never_blooms() {
        for kind in POOL {
            for t in [0.0, 0.4, 2.7] {
                let idle = frame(kind, t, false);
                assert!(idle.iter().all(|&a| a == IDLE_ALPHA), "{kind:?} {idle:?}");
                assert_eq!(engine::bloom_level(IDLE_ALPHA), 0.0);
            }
            let work = frame(kind, 0.4, true);
            assert!(
                work.iter().any(|&a| (a - IDLE_ALPHA).abs() > 0.01),
                "{kind:?} working {work:?}"
            );
        }
    }

    #[test]
    fn working_animates_over_time() {
        for kind in POOL {
            let a = frame(kind, 0.1, true);
            let b = frame(kind, 0.35, true);
            assert!(
                a.iter().zip(&b).any(|(x, y)| (x - y).abs() > 0.01),
                "{kind:?} {a:?} {b:?}"
            );
        }
    }

    #[test]
    fn out_of_bounds_cells_are_off() {
        for kind in POOL {
            assert_eq!(cell_opacity(kind, N, 0, 0.5, true), 0.0);
            assert_eq!(cell_opacity(kind, 0, -1, 0.5, true), 0.0);
        }
    }

    #[test]
    fn layout_keeps_the_upstream_cluster() {
        // Compact and the widget row share the upstream `dotSize 4` / `gap 1`.
        assert_eq!(
            layout(COMPACT_SIZE),
            DotLayout {
                dot: 4.0,
                gap: 1.0,
                span: 14.0
            }
        );
        assert_eq!(
            layout(WIDGET_SIZE),
            DotLayout {
                dot: 4.0,
                gap: 1.0,
                span: 14.0
            }
        );
        // Never collapses to a zero gap on tiny faces.
        assert_eq!(
            layout(2.0),
            DotLayout {
                dot: 1.0,
                gap: 1.0,
                span: 5.0
            }
        );
    }

    #[test]
    fn every_loader_stays_in_unit_interval() {
        for kind in POOL {
            for working in [false, true] {
                for t in [0.0, 0.37, 1.1, 3.3] {
                    for (i, a) in frame(kind, t, working).iter().enumerate() {
                        assert!(
                            a.is_finite() && (0.0..=1.0).contains(a),
                            "{kind:?} cell {i} t={t} working={working} -> {a}"
                        );
                    }
                }
            }
        }
    }
}
