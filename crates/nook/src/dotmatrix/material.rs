//! Global LED material: one cell grain and one look for every face.
//!
//! Agent logo masks are authored at [`LedMaterial::resolution`]. Larger faces
//! (speed dial) keep the same cell size in points by using a denser grid —
//! [`LedMaterial::grid_for`] — so beads match the agent-row icons.

use super::engine::bloom_level;
use super::{alpha, paint_glow};
use crate::theme;
use gpui::{canvas, fill, point, prelude::*, px, Bounds, IntoElement, Pixels, Rgba, Window};

/// Shared LED look. Tune [`LedMaterial::resolution`] (mask grid) here; cell
/// size in points follows from the agent-row face [`theme::HIT_MIN`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedMaterial {
    /// Cells per side for authored masks (agent logos).
    pub resolution: i32,
}

/// App-wide LED material.
pub const LED: LedMaterial = LedMaterial { resolution: 16 };

pub const BODY_IDLE: f32 = 0.55;
pub const BODY_WORK: f32 = 0.72;
/// Skip cells the mask barely covers so silhouettes stay sharp.
pub const MASK_FLOOR: f32 = 0.25;
/// Mild bottom fade so faces sit in the island instead of floating.
pub const FADE_BOTTOM: f32 = 0.18;
pub const SHIMMER_PERIOD: f32 = 1.35;
pub const SHIMMER_BAND: f32 = 0.28;

impl LedMaterial {
    /// Grid for agent masks — one cell per authored pixel.
    pub fn grid(self) -> i32 {
        self.resolution.max(1)
    }

    /// LED cell size in points. Matches agent-row icons (`HIT_MIN` ÷ resolution).
    #[allow(dead_code)]
    pub fn cell_px(self) -> f32 {
        theme::HIT_MIN / self.grid() as f32
    }

    /// Grid for an arbitrary face so each cell stays [`Self::cell_px`] wide.
    #[allow(dead_code)]
    pub fn grid_for(self, face: f32) -> i32 {
        (face / self.cell_px()).round().max(1.0) as i32
    }

    pub fn cell(self, face: f32, grid: i32) -> f32 {
        face / grid.max(1) as f32
    }
}

pub fn fade_row(row: i32, grid: i32) -> f32 {
    if grid <= 1 {
        return 1.0;
    }
    1.0 - (row as f32 / (grid - 1) as f32) * FADE_BOTTOM
}

/// Smooth diagonal highlight, 0 outside the band and 1 at its crest.
pub fn shimmer(row: i32, col: i32, grid: i32, now: f32) -> f32 {
    if grid <= 1 {
        return 0.0;
    }
    let path = (row + col) as f32 / ((grid - 1) * 2) as f32;
    let t = (now / SHIMMER_PERIOD).rem_euclid(1.0);
    let mut d = (path - t).abs();
    if d > 0.5 {
        d = 1.0 - d;
    }
    if d >= SHIMMER_BAND {
        return 0.0;
    }
    let x = 1.0 - d / SHIMMER_BAND;
    x * x * (3.0 - 2.0 * x)
}

/// Solid LED cells only — no mini-glyphs inside cells.
pub fn paint_grid(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    size: f32,
    material: LedMaterial,
    grid: i32,
    tint: Rgba,
    now: f32,
    working: bool,
    lite: bool,
    occ_at: impl Fn(i32, i32) -> f32,
) {
    let grid = grid.max(1);
    let cell = material.cell(size, grid);
    let ox: f32 = bounds.origin.x.into();
    let oy: f32 = bounds.origin.y.into();
    let bw: f32 = bounds.size.width.into();
    let bh: f32 = bounds.size.height.into();
    let ox = ox + (bw - size).max(0.0) * 0.5;
    let oy = oy + (bh - size).max(0.0) * 0.5;

    let mut cells = Vec::with_capacity((grid * grid) as usize);
    let body = if working { BODY_WORK } else { BODY_IDLE };
    for row in 0..grid {
        let fade = fade_row(row, grid);
        for col in 0..grid {
            let occ = occ_at(col, row);
            if occ < MASK_FLOOR {
                continue;
            }
            let body_a = if working {
                let sheen = if lite {
                    0.0
                } else {
                    shimmer(row, col, grid, now)
                };
                (occ * fade * (BODY_WORK + (1.0 - BODY_WORK) * sheen)).clamp(0.0, 1.0)
            } else {
                (occ * fade * body).clamp(0.0, 1.0)
            };
            let x = ox + col as f32 * cell;
            let y = oy + row as f32 * cell;
            cells.push((x, y, body_a, bloom_level(body_a)));
        }
    }

    if !lite {
        for &(x, y, _, level) in &cells {
            if level <= 0.0 {
                continue;
            }
            paint_glow(window, x, y, cell * 0.85, level, tint);
        }
    }
    paint_cell_quads(window, &cells, cell, tint);
}

/// Paint pre-positioned LED cells at a fixed [`LedMaterial::cell_px`] size.
/// Used by the speed ring (polar) so beads match agent grain without a
/// cartesian grid staircasing the circle.
#[allow(dead_code)]
pub fn paint_cells(
    window: &mut Window,
    cells: &[(f32, f32, f32)],
    cell: f32,
    tint: Rgba,
    lite: bool,
) {
    if !lite {
        for &(x, y, body_a) in cells {
            let level = bloom_level(body_a);
            if level <= 0.0 {
                continue;
            }
            paint_glow(window, x, y, cell * 0.85, level, tint);
        }
    }
    let quads: Vec<_> = cells
        .iter()
        .map(|&(x, y, body_a)| (x, y, body_a, 0.0))
        .collect();
    paint_cell_quads(window, &quads, cell, tint);
}

fn paint_cell_quads(window: &mut Window, cells: &[(f32, f32, f32, f32)], cell: f32, tint: Rgba) {
    let (gap, body_s, radius) = cell_metrics(cell);
    for &(x, y, body_a, _) in cells {
        let dx = x + gap * 0.5;
        let dy = y + gap * 0.5;
        window.paint_quad(
            fill(
                Bounds::from_corners(
                    point(px(dx), px(dy)),
                    point(px(dx + body_s), px(dy + body_s)),
                ),
                alpha(tint, body_a),
            )
            .corner_radii(radius),
        );
    }
}

fn cell_metrics(cell: f32) -> (f32, f32, gpui::Pixels) {
    let gap = if cell < 3.0 {
        (cell * 0.08).min(0.25)
    } else {
        (cell * 0.12).min(0.6)
    };
    // Don't floor body size — that made small cells look oversized vs logos.
    let body_s = (cell - gap).max(0.5);
    let radius = px((body_s * 0.22).min(0.9));
    (gap, body_s, radius)
}

/// Canvas wrapper. `grid` is usually [`LedMaterial::grid`] (agents) or
/// [`LedMaterial::grid_for`] (larger faces that must keep the same cell size).
pub fn element(
    size: f32,
    material: LedMaterial,
    grid: i32,
    tint: Rgba,
    now: f32,
    working: bool,
    lite: bool,
    occ: impl Fn(i32, i32) -> f32 + 'static,
) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            paint_grid(
                window, bounds, size, material, grid, tint, now, working, lite, &occ,
            );
        },
    )
    .size(px(size))
    .flex_shrink_0()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_resolution_matches_agent_masks() {
        assert_eq!(LED.resolution, 16);
        assert_eq!(LED.grid(), 16);
    }

    #[test]
    fn cell_px_matches_agent_row_icons() {
        assert!((LED.cell_px() - theme::HIT_MIN / 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn large_faces_keep_the_same_cell_size() {
        let dial = 100.0;
        let grid = LED.grid_for(dial);
        let cell = LED.cell(dial, grid);
        assert!(
            (cell - LED.cell_px()).abs() < 0.05,
            "cell={cell} want {}",
            LED.cell_px()
        );
        assert!(grid > LED.grid(), "dial needs a denser grid than the mask");
    }

    #[test]
    fn shimmer_sweeps() {
        let a = shimmer(0, 0, 16, 0.0);
        let b = shimmer(8, 8, 16, 0.0);
        assert!(a > 0.7, "crest starts top-left {a}");
        assert!(b < 0.2, "opposite corner is dark {b}");
    }
}
