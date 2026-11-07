//! Two-finger swipes on the island, read as 1.x read them. One physical swipe is one action. On
//! the closed island a swipe down stretches it first and only opens it past `PULL_COMMIT`
//! (letting go earlier springs it back); sideways cycles what it shows. On the open island,
//! sideways changes the page and up closes it.

use rune::GesturePhase;
use std::time::{Duration, Instant};

/// Points a swipe travels before a sideways change or a close.
const THRESHOLD: f32 = 20.0;
/// Down-swipe that lets the closed island open. Shorter travel only stretches it.
const PULL_COMMIT: f32 = 72.0;
/// Most the closed island grows while a down-swipe still resists, and the extra width then:
/// wider than tall, so it opens outward under a slow swipe.
const PULL_MAX: f32 = 18.0;
const PULL_WIDTH: f32 = 64.0;
/// A quiet gap that ends a swipe (a lift macOS never reported).
pub const IDLE: Duration = Duration::from_millis(280);

#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    /// The stretch changed; `tick` when it passed a third of the way to opening (a detent).
    Stretch { tick: bool },
    /// The stretch springs back.
    Release,
    Open,
    Close,
    /// Sideways: forward (right) or back.
    Cycle(bool),
}

#[derive(Default)]
pub struct Swipe {
    acc: (f32, f32),
    locked: bool,
    last: Option<Instant>,
    /// How far the closed island is stretched down, in points.
    pub pull: f32,
    detent: u8,
}

/// The island moves less than the fingers, then stops growing at the cap.
fn pull_height(travel: f32) -> f32 {
    if !travel.is_finite() || travel <= 0.0 {
        return 0.0;
    }
    let t = (travel / PULL_COMMIT).clamp(0.0, 1.0);
    (1.0 - (1.0 - t) * (1.0 - t)) * PULL_MAX
}

fn detent(travel: f32) -> u8 {
    let p = (travel / PULL_COMMIT).clamp(0.0, 1.0);
    if p >= 2.0 / 3.0 { 2 } else if p >= 1.0 / 3.0 { 1 } else { 0 }
}

impl Swipe {
    /// Extra width for the current stretch.
    pub fn pull_width(&self) -> f32 {
        (self.pull / PULL_MAX).clamp(0.0, 1.0) * PULL_WIDTH
    }

    fn release(&mut self) -> bool {
        self.detent = 0;
        std::mem::take(&mut self.pull) > 0.0
    }

    pub fn wheel(&mut self, dx: f32, dy: f32, phase: GesturePhase, expanded: bool, now: Instant) -> Action {
        if phase == GesturePhase::Began || self.last.is_none_or(|t| now.duration_since(t) >= IDLE) {
            self.locked = false;
            self.acc = (0.0, 0.0);
            self.detent = 0;
        }
        self.last = Some(now);
        if self.locked {
            return Action::None;
        }
        self.acc.0 += dx;
        self.acc.1 += dy;
        let (ax, ay) = self.acc;

        // Once stretching, stay in the stretch: a wobble sideways mustn't turn the same swipe
        // into a cycle.
        let vertical = self.pull > 0.0 || ay.abs() >= ax.abs();
        if !expanded && vertical && ay > 0.0 {
            if ay >= PULL_COMMIT {
                self.detent = 0;
                self.locked = true;
                self.acc = (0.0, 0.0);
                return Action::Open;
            }
            if phase == GesturePhase::Ended {
                self.acc = (0.0, 0.0);
                return if self.release() { Action::Release } else { Action::None };
            }
            let d = detent(ay);
            let tick = d > self.detent;
            self.detent = d;
            let next = pull_height(ay);
            if (next - self.pull).abs() <= 0.3 && !tick {
                return Action::None;
            }
            self.pull = next;
            return Action::Stretch { tick };
        }

        let released = self.release();
        let action = if ax.abs() > ay.abs() {
            if ax.abs() > THRESHOLD { Action::Cycle(ax > 0.0) } else { Action::None }
        } else if expanded && ay < -THRESHOLD {
            Action::Close
        } else {
            Action::None
        };
        if action != Action::None {
            self.locked = true;
            self.acc = (0.0, 0.0);
            return action;
        }
        if released { Action::Release } else { Action::None }
    }

    /// The swipe went quiet without a lift: let the stretch go.
    pub fn stale(&mut self, now: Instant) -> bool {
        if self.pull > 0.0 && self.last.is_some_and(|t| now.duration_since(t) >= IDLE) {
            self.acc = (0.0, 0.0);
            return self.release();
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(s: &mut Swipe, dx: f32, dy: f32, expanded: bool, at: Instant) -> Action {
        s.wheel(dx, dy, GesturePhase::Moved, expanded, at)
    }

    #[test]
    fn a_slow_pull_stretches_then_opens() {
        let (mut s, t) = (Swipe::default(), Instant::now());
        assert_eq!(s.wheel(0.0, 10.0, GesturePhase::Began, false, t), Action::Stretch { tick: false });
        assert!(s.pull > 0.0 && s.pull < PULL_MAX);
        assert_eq!(moved(&mut s, 0.0, 20.0, false, t), Action::Stretch { tick: true });
        assert_eq!(moved(&mut s, 0.0, 50.0, false, t), Action::Open);
        // The rest of the same swipe does nothing.
        assert_eq!(moved(&mut s, 0.0, 50.0, false, t), Action::None);
    }

    #[test]
    fn letting_go_early_springs_back() {
        let (mut s, t) = (Swipe::default(), Instant::now());
        moved(&mut s, 0.0, 30.0, false, t);
        assert_eq!(s.wheel(0.0, 0.0, GesturePhase::Ended, false, t), Action::Release);
        assert_eq!(s.pull, 0.0);
    }

    #[test]
    fn a_stretch_left_hanging_is_released() {
        let (mut s, t) = (Swipe::default(), Instant::now());
        moved(&mut s, 0.0, 30.0, false, t);
        assert!(!s.stale(t + Duration::from_millis(100)));
        assert!(s.stale(t + IDLE));
    }

    #[test]
    fn sideways_cycles_once_per_swipe_and_up_closes() {
        let (mut s, t) = (Swipe::default(), Instant::now());
        assert_eq!(moved(&mut s, 12.0, 0.0, true, t), Action::None);
        assert_eq!(moved(&mut s, 12.0, 0.0, true, t), Action::Cycle(true));
        assert_eq!(moved(&mut s, 40.0, 0.0, true, t), Action::None);
        let later = t + IDLE;
        assert_eq!(moved(&mut s, 0.0, -25.0, true, later), Action::Close);
    }
}
