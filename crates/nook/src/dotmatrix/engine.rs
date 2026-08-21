//! Shared 3×3 math for the Dot Matrix loaders.

pub const N: i32 = 3;
pub const CENTER: i32 = 1;

pub struct Ctx {
    pub row: i32,
    pub col: i32,
}

pub fn wrap01(x: f32, m: f32) -> f32 {
    if m <= 0.0 {
        return 0.0;
    }
    let mut r = x % m;
    if r < 0.0 {
        r += m;
    }
    r
}

pub fn phase_with_delay(now: f32, cycle: f32, delay: f32) -> f32 {
    if cycle <= 0.0 {
        return 0.0;
    }
    wrap01(now - delay, cycle) / cycle
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `cubic-bezier(0.42, 0, 0.58, 1)`, the CSS `ease-in-out` the loaders animate
/// with. Smoothstep tracks it to within ~0.005 over the unit interval.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Samples a `@keyframes` opacity track. `stops` are `(offset, opacity)` pairs
/// in ascending offset order, eased between neighbours like the CSS timing
/// function; `phase` is the position in the loop, 0..1.
pub fn track(stops: &[(f32, f32)], phase: f32) -> f32 {
    let Some(&(_, first)) = stops.first() else {
        return 0.0;
    };
    let phase = phase.clamp(0.0, 1.0);
    let mut prev = (0.0, first);
    for &(offset, value) in stops {
        if phase <= offset {
            let span = offset - prev.0;
            if span <= 0.0 {
                return value;
            }
            return lerp(prev.1, value, ease_in_out((phase - prev.0) / span));
        }
        prev = (offset, value);
    }
    prev.1
}

/// Bloom starts at remapped opacity 0.6 (`DMX_BLOOM_OPACITY_MIN`).
pub fn bloom_level(opacity: f32) -> f32 {
    ((opacity - 0.6) / 0.4).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_wraps_positive() {
        let t = phase_with_delay(0.1, 1.5, 0.4);
        assert!(t > 0.7 && t < 0.9);
        assert_eq!(phase_with_delay(0.4, 0.0, 0.0), 0.0);
        assert_eq!(wrap01(0.4, 0.0), 0.0);
    }

    #[test]
    fn track_eases_between_stops() {
        let stops = [(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)];
        assert_eq!(track(&stops, 0.0), 0.0);
        assert_eq!(track(&stops, 0.5), 1.0);
        assert_eq!(track(&stops, 1.0), 0.0);
        // Segment midpoints stay linear; the easing bites inside a segment.
        assert!((track(&stops, 0.25) - 0.5).abs() < 1e-6);
        let eighth = track(&stops, 0.125);
        assert!(eighth > 0.0 && eighth < 0.25, "{eighth}");
        assert!((track(&stops, 0.125) - track(&stops, 0.875)).abs() < 1e-6);
        assert_eq!(track(&[], 0.4), 0.0);
        // Flat tails hold the last stop.
        assert_eq!(track(&[(0.0, 0.3), (0.2, 0.9)], 0.9), 0.9);
        assert_eq!(ease_in_out(-1.0), 0.0);
        assert_eq!(lerp(0.0, 2.0, 0.5), 1.0);
    }

    #[test]
    fn bloom_starts_at_six_tenths() {
        assert_eq!(bloom_level(0.5), 0.0);
        assert!((bloom_level(0.8) - 0.5).abs() < 1e-5);
        assert!((bloom_level(1.0) - 1.0).abs() < 1e-5);
    }
}
