//! Shared 5×5 math for the circular Dot Matrix loaders.

pub const N: i32 = 5;
pub const CENTER: i32 = 2;

pub struct Ctx {
    pub row: i32,
    pub col: i32,
}

#[allow(dead_code)]
pub fn idx(row: i32, col: i32) -> usize {
    (row * N + col) as usize
}

pub fn hypot(row: i32, col: i32) -> f32 {
    let x = (col - CENTER) as f32;
    let y = (row - CENTER) as f32;
    (x * x + y * y).sqrt()
}

pub fn circular_mask(row: i32, col: i32) -> bool {
    !matches!((row, col), (0, 0) | (0, 4) | (4, 0) | (4, 4))
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

pub fn cycle_phase(now: f32, cycle_ms: f32, active: bool) -> f32 {
    if !active {
        return 0.0;
    }
    phase_with_delay(now, cycle_ms / 1000.0, 0.0)
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
        assert!((cycle_phase(0.0, 1500.0, false) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn circular_mask_drops_corners() {
        assert!(!circular_mask(0, 0));
        assert!(circular_mask(0, 2));
        assert!(circular_mask(2, 2));
        assert_eq!(idx(2, 2), 12);
        let _ = idx(0, 0);
        assert_eq!(bloom_level(0.5), 0.0);
        assert!((bloom_level(0.8) - 0.5).abs() < 1e-5);
        assert!((bloom_level(1.0) - 1.0).abs() < 1e-5);
    }
}
