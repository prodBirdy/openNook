//! Circular 5×5 loaders used on the island:
//! Prism Sweep (`dotm-circular-4`) and Gate Shift (`dotm-circular-7`).

use super::engine::*;

const SRC_BASE: f32 = 0.08;

pub fn opacity(n: u8, ctx: &Ctx, now: f32, working: bool) -> f32 {
    if !circular_mask(ctx.row, ctx.col) {
        return 0.0;
    }
    if n == 7 {
        c7(ctx, now, working)
    } else {
        c4(ctx, now, working)
    }
}

fn xy(ctx: &Ctx) -> (f32, f32) {
    ((ctx.col - 2) as f32, (ctx.row - 2) as f32)
}

/// Prism Sweep — `dotm-circular-4`.
fn c4(ctx: &Ctx, now: f32, working: bool) -> f32 {
    let (x, y) = xy(ctx);
    let radius = hypot(ctx.row, ctx.col);
    let theta = cycle_phase(now, 1800.0, working) * std::f32::consts::TAU;
    let (sx, sy) = (theta.cos(), theta.sin());
    let proj = x * sx + y * sy;
    let perp = (x * sy - y * sx).abs();
    if radius < 0.5 {
        return 0.62;
    }
    if proj > 0.3 && perp < 0.55 {
        return 0.96;
    }
    if proj > 0.0 && perp < 1.15 {
        return 0.36;
    }
    if radius > 1.6 && radius < 2.3 {
        return 0.22;
    }
    SRC_BASE
}

/// Gate Shift — `dotm-circular-7`.
fn c7(ctx: &Ctx, now: f32, working: bool) -> f32 {
    let (x, y) = xy(ctx);
    let t = cycle_phase(now, 1600.0, working) * std::f32::consts::TAU;
    let ring = hypot(ctx.row, ctx.col);
    let angle = y.atan2(x);
    let petal = 0.5 + 0.5 * (5.0 * angle - t * 1.7).cos();
    let ring_w = 0.5 + 0.5 * (ring * 3.3 - t * 1.2).cos();
    let chord = 0.5 + 0.5 * ((x + y) * 1.6 + t * 1.35).cos();
    let blend = 0.68 * petal.powf(2.2) + 0.22 * ring_w + 0.1 * chord;
    SRC_BASE + (0.92 - SRC_BASE) * blend
}
