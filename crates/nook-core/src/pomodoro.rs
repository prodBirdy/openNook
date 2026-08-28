//! Pomodoro phase machine. Wall-clock deadlines live on the island timer;
//! this module is pure so phase advance is unit-tested without GPUI.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PomodoroPhase {
    Work,
    ShortBreak,
    LongBreak,
}

impl PomodoroPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Work => "Work",
            Self::ShortBreak => "Break",
            Self::LongBreak => "Long break",
        }
    }

    pub fn is_work(self) -> bool {
        matches!(self, Self::Work)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PomodoroSpec {
    pub phase: PomodoroPhase,
    /// 1-based index of the current (or just-finished) work interval.
    pub cycle: u8,
    pub work_secs: u32,
    pub break_secs: u32,
    pub long_break_secs: u32,
    pub cycles_per_long: u8,
    pub auto_advance: bool,
}

impl PomodoroSpec {
    pub fn new(
        work_secs: u32,
        break_secs: u32,
        long_break_secs: u32,
        cycles_per_long: u8,
        auto_advance: bool,
    ) -> Self {
        Self {
            phase: PomodoroPhase::Work,
            cycle: 1,
            work_secs: work_secs.max(1),
            break_secs: break_secs.max(1),
            long_break_secs: long_break_secs.max(1),
            cycles_per_long: cycles_per_long.max(1),
            auto_advance,
        }
    }

    pub fn duration_secs(self) -> u32 {
        match self.phase {
            PomodoroPhase::Work => self.work_secs,
            PomodoroPhase::ShortBreak => self.break_secs,
            PomodoroPhase::LongBreak => self.long_break_secs,
        }
    }

    pub fn label(self) -> &'static str {
        self.phase.label()
    }

    /// Advance to the next phase. Work N → short break, or long break when
    /// `cycle` is a multiple of `cycles_per_long`. Any break → next work.
    pub fn advance(self) -> Self {
        match self.phase {
            PomodoroPhase::Work => {
                let long = self.cycles_per_long > 0 && self.cycle % self.cycles_per_long == 0;
                Self {
                    phase: if long {
                        PomodoroPhase::LongBreak
                    } else {
                        PomodoroPhase::ShortBreak
                    },
                    ..self
                }
            }
            PomodoroPhase::ShortBreak => Self {
                phase: PomodoroPhase::Work,
                cycle: self.cycle.saturating_add(1),
                ..self
            },
            PomodoroPhase::LongBreak => Self {
                phase: PomodoroPhase::Work,
                cycle: 1,
                ..self
            },
        }
    }

    /// Dots filled in the featured timer: the current work cycle, including
    /// the break that follows it.
    pub fn filled_cycles(self) -> u8 {
        self.cycle.min(self.cycles_per_long)
    }
}

/// Seconds left until `deadline`. Zero when the deadline is missing or past.
pub fn remaining_until(deadline: Option<std::time::SystemTime>, now: std::time::SystemTime) -> u32 {
    let Some(deadline) = deadline else {
        return 0;
    };
    deadline
        .duration_since(now)
        .map(|d| d.as_secs().min(u32::MAX as u64) as u32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn spec(cycle: u8, phase: PomodoroPhase) -> PomodoroSpec {
        PomodoroSpec {
            phase,
            cycle,
            work_secs: 1500,
            break_secs: 300,
            long_break_secs: 900,
            cycles_per_long: 4,
            auto_advance: true,
        }
    }

    #[test]
    fn work_advances_to_short_break() {
        let next = spec(1, PomodoroPhase::Work).advance();
        assert_eq!(next.phase, PomodoroPhase::ShortBreak);
        assert_eq!(next.cycle, 1);
        assert_eq!(next.duration_secs(), 300);
        assert_eq!(next.filled_cycles(), 1);
    }

    #[test]
    fn fourth_work_advances_to_long_break() {
        let next = spec(4, PomodoroPhase::Work).advance();
        assert_eq!(next.phase, PomodoroPhase::LongBreak);
        assert_eq!(next.cycle, 4);
        assert_eq!(next.duration_secs(), 900);
        assert_eq!(next.filled_cycles(), 4);
    }

    #[test]
    fn short_break_starts_next_work() {
        let next = spec(2, PomodoroPhase::ShortBreak).advance();
        assert_eq!(next.phase, PomodoroPhase::Work);
        assert_eq!(next.cycle, 3);
        assert_eq!(next.duration_secs(), 1500);
    }

    #[test]
    fn long_break_restarts_the_set() {
        let next = spec(4, PomodoroPhase::LongBreak).advance();
        assert_eq!(next.phase, PomodoroPhase::Work);
        assert_eq!(next.cycle, 1);
        assert_eq!(next.duration_secs(), 1500);
    }

    #[test]
    fn remaining_until_is_zero_when_past() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        assert_eq!(remaining_until(None, now), 0);
        assert_eq!(
            remaining_until(Some(now - Duration::from_secs(5)), now),
            0
        );
        assert_eq!(
            remaining_until(Some(now + Duration::from_secs(90)), now),
            90
        );
    }
}
