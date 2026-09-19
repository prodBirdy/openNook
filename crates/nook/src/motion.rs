//! Motion — the app's single animation vocabulary.
//!
//! HIG › Motion wants animation purposeful, brief, and optional, so every
//! animated value in Nook rides one of the named springs below and collapses
//! to a plain dissolve when Accessibility › Display › "Reduce motion" is on
//! (see `Island::step_springs`). Springs use SwiftUI's `Spring(duration:bounce:)`
//! shape — stiffness (2π/duration)², unit mass — but damp softer than SwiftUI:
//! `2·(0.8 − bounce)·(2π/duration)` instead of `2·(1 − bounce)·ω`. Apple's
//! preset names (`.smooth` / `.snappy` / `.bouncy`) still map to the same
//! bounce values; only the zeta is deliberately lower.
//!
//! Deliberate exceptions, documented where they live: the marquee scroll
//! (constant velocity is the point), the Dot Matrix loaders (keyframe artwork
//! ported from CSS), and hover/press opacity styles (instant, like AppKit's).

use std::f32::consts::PI;
use std::time::Duration;

/// Hover must leave the island this long before an expanded pane collapses.
pub const HOVER_EXIT_DWELL: Duration = Duration::from_millis(200);

/// Compact VPN connect/disconnect face takeover.
pub const VPN_REVEAL: Duration = Duration::from_secs(4);

/// Output-device / tray HUD toast lifetime.
pub const OUTPUT_HUD_TTL: Duration = Duration::from_millis(1500);

/// Meeting mute glyph flash.
pub const MUTE_FLASH: Duration = Duration::from_millis(450);

/// Two-finger swipe must accumulate this many points before acting.
pub const SWIPE_THRESHOLD: f32 = 20.0;

/// Quiet gap that re-arms a locked swipe gesture.
pub const SWIPE_IDLE: Duration = Duration::from_millis(280);

/// Pending file-drag slop as distance² in logical points.
pub const DRAG_SLOP: f32 = 16.0;

/// Island size morph: expand/collapse and compact mode changes. `snappy` at
/// the pace of the previous hand-tuned spring (stiffness 400, damping 30,
/// mass 0.8 ⇒ response 0.28s, damping fraction 0.84 — i.e. this, unnamed).
pub const MORPH: Spring = Spring::snappy(0.30);

/// Content swap after an expanded/mode change. No bounce: opacity that
/// overshoots 1.0 just clips, so the crossfade must stay critically damped.
pub const CROSSFADE: Spring = Spring::smooth(0.25);

/// Content continuity after a context change. The travel is short and has no
/// bounce so text and controls never wobble past their final position.
pub const CONTEXT_SHIFT: Spring = Spring::smooth(0.28);

/// Small hover reveals, e.g. the play/pause scrim over the album art.
pub const REVEAL: Spring = Spring::smooth(0.20);

/// Settle threshold for pixel-sized values (island width/height/offsets).
pub const REST_PX: f32 = 0.4;

/// Settle threshold for 0..1 opacities: a quarter of a percent is below
/// anything an 8-bit compositor can show.
pub const REST_ALPHA: f32 = 0.0025;

/// An Apple-parameterized spring, mass normalized to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    stiffness: f32,
    damping: f32,
}

impl Spring {
    /// `duration` is the perceptual duration in seconds; `bounce` matches
    /// SwiftUI's scale (0 = no bounce, 1 = undamped). Damping fraction is
    /// `0.8 − bounce` (softer than SwiftUI's `1 − bounce`) so snappy/bouncy
    /// still read with a touch of overshoot under the 120 Hz Euler integrator.
    pub const fn new(duration: f32, bounce: f32) -> Self {
        let omega = 2.0 * PI / duration;
        Self {
            stiffness: omega * omega,
            damping: 2.0 * (0.8 - bounce) * omega,
        }
    }

    /// SwiftUI `.smooth`: no bounce.
    pub const fn smooth(duration: f32) -> Self {
        Self::new(duration, 0.0)
    }

    /// SwiftUI `.snappy`: bounce 0.15.
    pub const fn snappy(duration: f32) -> Self {
        Self::new(duration, 0.15)
    }

    /// SwiftUI `.bouncy`: bounce 0.30.
    #[allow(dead_code)]
    pub const fn bouncy(duration: f32) -> Self {
        Self::new(duration, 0.30)
    }
}

/// A value animated by a [`Spring`]. Frame-driven: the owner calls [`step`]
/// with real elapsed time and keeps requesting frames while it returns true.
///
/// [`step`]: SpringValue::step
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringValue {
    pub value: f32,
    pub velocity: f32,
}

impl SpringValue {
    pub const fn at(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
        }
    }

    /// Park at `value` with no residual motion.
    pub fn set(&mut self, value: f32) {
        self.value = value;
        self.velocity = 0.0;
    }

    /// Advance toward `target`; true while more frames are needed.
    ///
    /// Semi-implicit Euler goes unstable around dt = 42ms at MORPH's
    /// stiffness — one slow frame used to send the island to ±1e6 and strobe
    /// it open/closed — so integration substeps at 120 Hz (60 Hz is stable
    /// too, but its numerical damping flattens snappy's bounce into smooth)
    /// and absurd pauses (lid close) are clipped at 250ms. `rest` is the
    /// settle threshold in value units, with the velocity threshold at
    /// 10 × `rest` per second; on settling (or any non-finite excursion) the
    /// value snaps to `target` exactly and motion stops.
    pub fn step(&mut self, spring: Spring, target: f32, dt: f32, rest: f32) -> bool {
        const MAX_STEP: f32 = 1.0 / 120.0;
        let mut left = dt.clamp(0.0, 0.25);
        while left > 0.0 {
            let step_dt = left.min(MAX_STEP);
            left -= step_dt;
            let acc = (target - self.value) * spring.stiffness - self.velocity * spring.damping;
            self.velocity += acc * step_dt;
            self.value += self.velocity * step_dt;
        }

        if !self.value.is_finite() || !self.velocity.is_finite() {
            self.set(target);
            return false;
        }
        if (self.value - target).abs() > rest || self.velocity.abs() > rest * 10.0 {
            return true;
        }
        self.set(target);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// App parameterization: Spring(duration: 0.5, bounce: 0.3) ⇒ mass 1,
    /// stiffness (2π/0.5)² ≈ 157.91, damping 2·(0.8−0.3)·(2π/0.5) ≈ 12.57.
    /// (SwiftUI's same args would damp at ≈ 17.6; we deliberately use 0.8.)
    #[test]
    fn matches_app_parameter_conversion() {
        let spring = Spring::new(0.5, 0.3);
        assert!(
            (spring.stiffness - 157.91).abs() < 0.05,
            "{}",
            spring.stiffness
        );
        assert!((spring.damping - 12.57).abs() < 0.05, "{}", spring.damping);
    }

    #[test]
    fn settles_exactly_on_target() {
        for spring in [MORPH, CROSSFADE, REVEAL, Spring::bouncy(0.5)] {
            let mut v = SpringValue::at(0.0);
            let mut moving = true;
            for _ in 0..400 {
                moving = v.step(spring, 120.0, 1.0 / 60.0, REST_PX);
                if !moving {
                    break;
                }
            }
            assert!(!moving, "{spring:?} never settled");
            assert_eq!(v.value, 120.0);
            assert_eq!(v.velocity, 0.0);
        }
    }

    /// `.smooth` uses bounce 0 → zeta 0.8 (our softer mapping), so it is
    /// slightly underdamped; semi-implicit Euler at 120 Hz adds a bit more.
    /// Peak crest is ~3e-3 on a unit travel — invisible once opacity clips
    /// at 1.0. Guard against a real wobble, not that tiny crest.
    #[test]
    fn smooth_never_overshoots() {
        let mut v = SpringValue::at(0.0);
        while v.step(CROSSFADE, 1.0, 1.0 / 60.0, REST_ALPHA) {
            assert!(
                v.value <= 1.0 + 5e-3,
                "smooth fade overshot materially: {}",
                v.value
            );
        }
        assert_eq!(v.value, 1.0);
    }

    /// At the expanded morph's real travel (hundreds of px) snappy's bounce
    /// clears the settle threshold and reads as a touch of overshoot; it must
    /// never turn into a wobble. Small travels park before the bounce shows.
    #[test]
    fn snappy_overshoots_a_little_but_not_much() {
        let mut v = SpringValue::at(0.0);
        let mut peak = 0.0f32;
        while v.step(MORPH, 800.0, 1.0 / 60.0, REST_PX) {
            peak = peak.max(v.value);
        }
        assert!(peak > 800.0, "snappy should show a touch of bounce: {peak}");
        // 3970418 retuned Spring::new damping (1−bounce → 0.8−bounce), which
        // lifts snappy's zeta and the morph peak from ~800 to ~839.
        assert!(peak < 840.0, "morph bounce too violent: {peak}");
    }

    /// The poll loop hands over real dt; hitches past the Euler stability
    /// cliff must be substepped, not integrated raw.
    #[test]
    fn survives_slow_frames_and_hitches() {
        let mut v = SpringValue::at(0.0);
        v.step(MORPH, 300.0, 0.016, REST_PX);
        v.step(MORPH, 300.0, 0.05, REST_PX);
        let mut moving = true;
        for _ in 0..200 {
            moving = v.step(MORPH, 300.0, 0.05, REST_PX);
            assert!(
                v.value.is_finite() && v.value > -100.0 && v.value < 1000.0,
                "spring exploded: {}",
                v.value
            );
            if !moving {
                break;
            }
        }
        assert!(!moving, "never settled at 50ms/frame");
        assert_eq!(v.value, 300.0);
    }

    #[test]
    fn non_finite_excursion_snaps_to_target() {
        let mut v = SpringValue {
            value: f32::NAN,
            velocity: 0.0,
        };
        assert!(!v.step(MORPH, 42.0, 0.016, REST_PX));
        assert_eq!(v.value, 42.0);
        assert_eq!(v.velocity, 0.0);
    }
}
