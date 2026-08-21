//! Motion — the app's single animation vocabulary.
//!
//! HIG › Motion wants animation purposeful, brief, and optional, so every
//! animated value in Nook rides one of the named springs below and collapses
//! to a plain dissolve when Accessibility › Display › "Reduce motion" is on
//! (see `Island::step_springs`). Springs are parameterized exactly like
//! SwiftUI's `Spring(duration:bounce:)` — stiffness (2π/duration)², damping
//! 4π(1 − bounce)/duration, unit mass — so any value tuned here means the
//! same thing it would in SwiftUI, and Apple's presets carry over verbatim.
//!
//! Deliberate exceptions, documented where they live: the marquee scroll
//! (constant velocity is the point), the Dot Matrix loaders (keyframe artwork
//! ported from CSS), and hover/press opacity styles (instant, like AppKit's).

use std::f32::consts::PI;

/// Island size morph: expand/collapse and compact mode changes. `snappy` at
/// the pace of the previous hand-tuned spring (stiffness 400, damping 30,
/// mass 0.8 ⇒ response 0.28s, damping fraction 0.84 — i.e. this, unnamed).
pub const MORPH: Spring = Spring::snappy(0.30);

/// Content swap after an expanded/mode change. No bounce: opacity that
/// overshoots 1.0 just clips, so the crossfade must stay critically damped.
pub const CROSSFADE: Spring = Spring::smooth(0.25);

/// Small hover reveals, e.g. the play/pause scrim over the album art.
pub const REVEAL: Spring = Spring::smooth(0.20);

/// Settle threshold for pixel-sized values (island width/height).
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
    /// SwiftUI (0 = critically damped, 1 = undamped). The damping fraction is
    /// 1 − bounce.
    pub const fn new(duration: f32, bounce: f32) -> Self {
        let omega = 2.0 * PI / duration;
        Self {
            stiffness: omega * omega,
            damping: 2.0 * (1.0 - bounce) * omega,
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

    /// The worked example on developer.apple.com/documentation/swiftui/spring:
    /// Spring(duration: 0.5, bounce: 0.3) ⇒ mass 1, stiffness 157.9,
    /// damping 17.6.
    #[test]
    fn matches_swiftui_parameter_conversion() {
        let spring = Spring::new(0.5, 0.3);
        assert!(
            (spring.stiffness - 157.9).abs() < 0.05,
            "{}",
            spring.stiffness
        );
        assert!((spring.damping - 17.6).abs() < 0.05, "{}", spring.damping);
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

    #[test]
    fn smooth_never_overshoots() {
        let mut v = SpringValue::at(0.0);
        while v.step(CROSSFADE, 1.0, 1.0 / 60.0, REST_ALPHA) {
            assert!(
                v.value <= 1.0,
                "critically damped fade overshot: {}",
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
        assert!(peak < 816.0, "morph bounce too violent: {peak}");
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
