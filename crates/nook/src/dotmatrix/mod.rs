//! GPUI port of the 5×5 [Dot Matrix](https://dotmatrix.zzzzshawn.cloud/) loaders.
//! The island uses Prism Sweep (`dotm-circular-4`) and Gate Shift (`dotm-circular-7`)
//! at size 16, dotSize 2, speed 1.2, with bloom.
//!
//! Animation math follows the upstream CSS keyframes and the Swift
//! [matrix-swift](https://github.com/mana-am/matrix-swift) TimelineView port.
//! Embedded use in an application is permitted by the upstream license;
//! this is not a republished component library.

mod circular;
mod engine;

use engine::{bloom_level, circular_mask, Ctx, N};
use gpui::{canvas, fill, point, prelude::*, px, Bounds, IntoElement, Pixels, Rgba, Window};

/// Matches `<DotmCircular4 size={16} dotSize={2} speed={1.2} bloom />`.
pub const SIZE: f32 = 16.0;
pub const DOT_SIZE: f32 = 2.0;
pub const SPEED: f32 = 1.2;
/// Upstream keeps `dotSize` at an eighth of `size`; `element` re-derives the dot
/// from its `size` argument so the two call sites can diverge without the gaps
/// stretching to fill a grid of fixed-width dots.
const DOT_RATIO: f32 = DOT_SIZE / SIZE;
pub const COMPACT_SIZE: f32 = SIZE;
pub const WIDGET_SIZE: f32 = SIZE;

/// Prism Sweep + Gate Shift.
const POOL: [Kind; 2] = [Kind::Circular(4), Kind::Circular(7)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Circular(u8),
}

/// Deterministic pick: the same seed always lands on the same loader.
pub fn pick(seed: u32) -> Kind {
    POOL[(seed as usize) % POOL.len()]
}

pub fn cell_opacity(kind: Kind, row: i32, col: i32, now: f32, working: bool) -> f32 {
    if !circular_mask(row, col) {
        return 0.0;
    }
    let ctx = Ctx { row, col };
    match kind {
        Kind::Circular(n) => circular::opacity(n, &ctx, now, working),
    }
    .clamp(0.0, 1.0)
}

/// `gap = max(1, floor((size - dotSize * 5) / 4))`, span follows the dots.
pub fn layout(size: f32, dot: f32) -> (f32, f32, f32) {
    let gap = ((size - dot * N as f32) / (N as f32 - 1.0))
        .floor()
        .max(1.0);
    let span = dot * N as f32 + gap * (N as f32 - 1.0);
    (dot, gap, span)
}

pub fn element(kind: Kind, now: f32, working: bool, size: f32) -> impl IntoElement {
    let (dot, gap, span) = layout(size, (size * DOT_RATIO).round().max(1.0));
    let now = now * SPEED;
    // Resolved per frame, not cached, so changing the accent in System Settings
    // recolors the loader without a restart.
    let tint = crate::theme::accent();
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            paint_grid(window, bounds, kind, now, working, dot, gap, tint);
        },
    )
    .w(px(span))
    .h(px(span))
}

fn paint_grid(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    kind: Kind,
    now: f32,
    working: bool,
    dot: f32,
    gap: f32,
    tint: Rgba,
) {
    let ox: f32 = bounds.origin.x.into();
    let oy: f32 = bounds.origin.y.into();
    let bw: f32 = bounds.size.width.into();
    let bh: f32 = bounds.size.height.into();
    let span = dot * N as f32 + gap * (N as f32 - 1.0);
    let ox = ox + (bw - span).max(0.0) * 0.5;
    let oy = oy + (bh - span).max(0.0) * 0.5;
    let mut cells = Vec::with_capacity(21);
    for row in 0..N {
        for col in 0..N {
            if !circular_mask(row, col) {
                continue;
            }
            let a = cell_opacity(kind, row, col, now, working);
            if a < 0.01 {
                continue;
            }
            let x = ox + col as f32 * (dot + gap);
            let y = oy + row as f32 * (dot + gap);
            cells.push((x, y, a, bloom_level(a)));
        }
    }
    for &(x, y, _, level) in &cells {
        if level <= 0.0 {
            continue;
        }
        paint_glow(window, x, y, dot, level, tint);
    }
    let radius = px(dot * 0.5);
    for &(x, y, a, _) in &cells {
        window.paint_quad(
            fill(
                Bounds::from_corners(point(px(x), px(y)), point(px(x + dot), px(y + dot))),
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
    fn pick_is_circular_4_and_7() {
        assert_eq!(pick(0), Kind::Circular(4));
        assert_eq!(pick(1), Kind::Circular(7));
        assert_eq!(pick(2), Kind::Circular(4));
        assert_eq!(pick(0), pick(POOL.len() as u32));
    }

    #[test]
    fn working_differs_from_idle() {
        let idle = cell_opacity(Kind::Circular(4), 1, 4, 0.4, false);
        let work = cell_opacity(Kind::Circular(4), 1, 4, 0.4, true);
        assert!((idle - work).abs() > 0.01);
    }

    #[test]
    fn corners_are_off_for_every_loader() {
        for seed in 0..POOL.len() as u32 {
            let kind = pick(seed);
            for (row, col) in [(0, 0), (0, 4), (4, 0), (4, 4)] {
                assert_eq!(
                    cell_opacity(kind, row, col, 0.5, true),
                    0.0,
                    "{kind:?} corner ({row},{col})"
                );
            }
            assert!(cell_opacity(kind, 2, 2, 0.5, true) >= 0.0);
        }
    }

    #[test]
    fn layout_matches_size_32_dot_4() {
        let (dot, gap, span) = layout(32.0, 4.0);
        assert_eq!(dot, 4.0);
        assert_eq!(gap, 3.0);
        assert_eq!(span, 32.0);
    }

    #[test]
    fn dot_scales_with_size() {
        assert_eq!((SIZE * DOT_RATIO).round(), DOT_SIZE);
        assert_eq!((32.0 * DOT_RATIO).round(), 4.0);
        // size 16 floors the gap to 1, so the grid paints a 14px span.
        let (dot, gap, span) = layout(SIZE, (SIZE * DOT_RATIO).round().max(1.0));
        assert_eq!((dot, gap, span), (2.0, 1.0, 14.0));
    }

    #[test]
    fn every_loader_stays_in_unit_interval() {
        for seed in 0..POOL.len() as u32 {
            let kind = pick(seed);
            for working in [false, true] {
                for t in [0.0, 0.37, 1.1, 3.3] {
                    for row in 0..5 {
                        for col in 0..5 {
                            let a = cell_opacity(kind, row, col, t, working);
                            assert!(
                                a.is_finite() && (0.0..=1.0).contains(&a),
                                "{kind:?} ({row},{col}) t={t} working={working} -> {a}"
                            );
                        }
                    }
                }
            }
        }
    }
}
