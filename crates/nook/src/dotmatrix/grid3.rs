//! The 3×3 Dot Matrix loaders the coding agents spin with:
//! Drift TL (`dotm-3x3-3`), Core Echo (`dotm-3x3-6`) and Smiley Spin
//! (`dotm-3x3-16`), all at half `speed 1.2` with bloom; the dot and gap come from
//! the face the loader is painted into.
//!
//! `DotMatrix3Base` passes `opacityBase = 0.06` and leaves mid/peak at the CSS
//! defaults, so the class-driven curves run 0.06 → 0.32 → 1; Smiley Spin sets
//! opacity inline instead and so goes through `remapOpacityToTriplet` — hence
//! `remap` below. Upstream's static (reduced-motion) frames are unused: an idle
//! agent is painted flat grey by the caller.
//!
//! `now` arrives pre-scaled by `SPEED`, which is what the CSS `--dmx-speed`
//! (`1 / speed`) divisor does to every duration and delay.

use super::engine::*;

/// The three loaders, named after their upstream ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `dotm-3x3-3` — diagonal bands drifting top-left to bottom-right.
    DriftTl,
    /// `dotm-3x3-6` — Manhattan rings rippling out of the centre.
    CoreEcho,
    /// `dotm-3x3-16` — a pixel smiley turning a quarter at a time.
    SmileySpin,
}

/// `.dmx-root.dmx-matrix-3 { --dmx-cycle: 1500ms }`.
const CYCLE_MS: f32 = 1500.0;
/// `--dmx-opacity-base` for the 3×3 base; mid and peak stay at the CSS defaults.
const BASE: f32 = 0.06;
const MID: f32 = 0.32;
const PEAK: f32 = 1.0;

/// Only ever called for a working agent; idle faces are painted flat by the
/// caller instead of running the upstream static frame.
pub fn opacity(kind: Kind, ctx: &Ctx, now: f32) -> f32 {
    match kind {
        Kind::DriftTl => drift_tl(ctx, now),
        Kind::CoreEcho => core_echo(ctx, now),
        Kind::SmileySpin => smiley_spin(ctx, now),
    }
}

/// `@keyframes dmx-ripple-3` — a short pulse that rests at base for the rest of
/// the loop, which is what makes the bands read as separate strikes.
const RIPPLE: [(f32, f32); 8] = [
    (0.00, BASE),
    (0.03, 0.62 * MID + 0.38 * BASE),
    (0.06, 0.35 * PEAK + 0.65 * MID),
    (0.10, PEAK),
    (0.14, 0.35 * PEAK + 0.65 * MID),
    (0.17, 0.62 * MID + 0.38 * BASE),
    (0.20, BASE),
    (1.00, BASE),
];

/// `@keyframes dmx-center-origin-ripple`.
const CENTER_RIPPLE: [(f32, f32); 4] = [
    (0.00, 0.625 * BASE),
    (0.34, PEAK),
    (0.60, 0.5 * (BASE + MID)),
    (1.00, 0.625 * BASE),
];

/// Drift TL — `dotm-3x3-3`. Diagonal bands strike from the top-left corner
/// toward the bottom-right, one `--dmx-path` step apart.
fn drift_tl(ctx: &Ctx, now: f32) -> f32 {
    let path = (ctx.row + ctx.col) as f32 / ((N - 1) * 2) as f32;
    let dur = 0.68 * CYCLE_MS;
    let delay = path * 0.19 * CYCLE_MS;
    track(&RIPPLE, phase_with_delay(now, dur / 1000.0, delay / 1000.0))
}

/// Core Echo — `dotm-3x3-6`. Manhattan rings ripple out of the centre cell.
fn core_echo(ctx: &Ctx, now: f32) -> f32 {
    let ring = ((ctx.row - CENTER).abs() + (ctx.col - CENTER).abs()).clamp(0, 2) as f32;
    let dur = 0.82 * CYCLE_MS;
    let delay = ring * 0.11 * CYCLE_MS;
    track(
        &CENTER_RIPPLE,
        phase_with_delay(now, dur / 1000.0, delay / 1000.0),
    )
}

/// Smiley Spin — `dotm-3x3-16`. Eyes on the top row, mouth below centre,
/// row-major 0/1.
const SMILEY: [u8; 9] = [1, 0, 1, 0, 0, 0, 0, 1, 0];
const SPIN_STEP_MS: f32 = 180.0;
const SPIN_STEPS: i32 = 4;
const SPIN_BASE: f32 = 0.09;
const SPIN_PEAK: f32 = 0.88;

/// The glyph is stepped through four quarter turns and cross-faded, so the
/// opacity is set per frame rather than by a keyframe track.
fn smiley_spin(ctx: &Ctx, now: f32) -> f32 {
    let cycle = SPIN_STEP_MS * SPIN_STEPS as f32 / 1000.0;
    let scaled = wrap01(now, cycle) / cycle * SPIN_STEPS as f32;
    let turns = scaled.floor();
    let t = ease_in_out(scaled - turns);
    let turns = turns as i32;
    let weight = lerp(
        lit(turns, ctx.row, ctx.col),
        lit(turns + 1, ctx.row, ctx.col),
        t,
    );
    remap(SPIN_BASE + weight * (SPIN_PEAK - SPIN_BASE))
}

/// `rotate3x3` read backwards: one clockwise turn puts the cell at
/// `(row, col)` where `(2 - col, row)` used to be.
fn lit(turns: i32, row: i32, col: i32) -> f32 {
    let (mut row, mut col) = (row, col);
    for _ in 0..turns.rem_euclid(SPIN_STEPS) {
        (row, col) = (N - 1 - col, row);
    }
    SMILEY[(row * N + col) as usize] as f32
}

/// `remapOpacityToTriplet` for the static frames: the source triplet
/// (0.08, 0.34, 0.94) mapped onto the 3×3 base's (0.06, 0.34, 0.94), so only
/// the dimmest band is pulled down.
fn remap(raw: f32) -> f32 {
    const SRC: [f32; 3] = [0.08, 0.34, 0.94];
    const DST: [f32; 3] = [0.06, 0.34, 0.94];
    let raw = raw.clamp(0.0, 1.0);
    let (lo, hi, from, to) = if raw <= SRC[0] {
        (0.0, SRC[0], 0.0, DST[0])
    } else if raw <= SRC[1] {
        (SRC[0], SRC[1], DST[0], DST[1])
    } else if raw <= SRC[2] {
        (SRC[1], SRC[2], DST[1], DST[2])
    } else {
        (SRC[2], 1.0, DST[2], 1.0)
    };
    let span = hi - lo;
    if span <= f32::EPSILON {
        return to;
    }
    lerp(from, to, (raw - lo) / span).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(kind: Kind, row: i32, col: i32, now: f32) -> f32 {
        opacity(kind, &Ctx { row, col }, now)
    }

    #[test]
    fn drift_bands_strike_top_left_first() {
        // The band delay is `path * 0.19 * cycle`, so the corner peaks while the
        // opposite corner is still resting.
        let peak = 0.10 * 0.68 * CYCLE_MS / 1000.0;
        assert!((cell(Kind::DriftTl, 0, 0, peak) - PEAK).abs() < 1e-4);
        assert!(cell(Kind::DriftTl, 2, 2, peak) < 0.2);
        let late = peak + 0.19 * CYCLE_MS / 1000.0;
        assert!((cell(Kind::DriftTl, 2, 2, late) - PEAK).abs() < 1e-4);
    }

    #[test]
    fn core_echo_ripples_out_of_the_centre() {
        let peak = 0.34 * 0.82 * CYCLE_MS / 1000.0;
        assert!((cell(Kind::CoreEcho, 1, 1, peak) - PEAK).abs() < 1e-4);
        // Ring 1 trails the centre by `0.11 * cycle` and ring 2 by twice that.
        assert!(cell(Kind::CoreEcho, 0, 1, peak) < PEAK);
        assert!(cell(Kind::CoreEcho, 0, 0, peak) < cell(Kind::CoreEcho, 0, 1, peak));
    }

    #[test]
    fn smiley_rotates_a_quarter_turn_per_step() {
        // Eyes at (0,0) and (0,2), mouth at (2,1).
        assert_eq!(lit(0, 0, 0), 1.0);
        assert_eq!(lit(0, 2, 1), 1.0);
        assert_eq!(lit(0, 1, 1), 0.0);
        // One clockwise turn walks the top-left eye to the top-right.
        assert_eq!(lit(1, 0, 2), 1.0);
        assert_eq!(lit(1, 1, 0), 1.0);
        // Four turns are the identity, and negative turns wrap.
        for row in 0..N {
            for col in 0..N {
                assert_eq!(lit(4, row, col), lit(0, row, col));
                assert_eq!(lit(-1, row, col), lit(3, row, col));
            }
        }
    }

    #[test]
    fn smiley_crossfades_between_steps() {
        let step = SPIN_STEP_MS / 1000.0;
        let lit_dot = cell(Kind::SmileySpin, 0, 0, 0.0);
        assert!((lit_dot - remap(SPIN_PEAK)).abs() < 1e-4, "{lit_dot}");
        // Mid-step the departing eye sits between the two frames.
        let mid = cell(Kind::SmileySpin, 0, 0, step * 0.5);
        assert!(mid > remap(SPIN_BASE) && mid < lit_dot, "{mid}");
        // The centre never lights up in any rotation.
        assert!(cell(Kind::SmileySpin, 1, 1, step * 0.5) < 0.1);
    }

    #[test]
    fn remap_only_pulls_down_the_dimmest_band() {
        assert_eq!(remap(0.0), 0.0);
        assert!((remap(0.08) - 0.06).abs() < 1e-6);
        assert!((remap(0.34) - 0.34).abs() < 1e-6);
        assert!((remap(0.94) - 0.94).abs() < 1e-6);
        assert_eq!(remap(1.0), 1.0);
        assert_eq!(remap(2.0), 1.0);
        assert!(remap(0.2) < 0.2);
    }
}
