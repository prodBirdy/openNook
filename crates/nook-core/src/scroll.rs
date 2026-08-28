//! LiquidMouse: smooth scrolling for discrete wheel mice.
//!
//! Trackpads and Magic Mouse already emit continuous pixel events
//! (`kCGScrollWheelEventIsContinuous != 0`) and pass through untouched.
//! Wheel mice emit line ticks; those are swallowed and replayed as decaying
//! pixel deltas with trackpad-style phase / momentum-phase sequencing.
//!
//! Per-individual-device settings need private sender IDs and are not
//! shipped — mouse-vs-trackpad is the public, robust split.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

/// Bundle ids that already own the scroll path. Detected so Settings can warn.
pub const CONFLICT_BUNDLES: &[&str] = &[
    "com.caldis.Mos",
    "com.lujjjh.LinearMouse",
    "com.macmousefix.MacMouseFix",
    "com.macmousefix.Mac-Mouse-Fix",
    "com.pilotmoon.scroll-reverser",
    "com.logitech.LogiOptions",
    "com.logitech.manager.daemon",
    "com.logitech.lh21",
];

/// Suggested exclusions: games, VMs, remotes that want raw wheel ticks.
pub const DEFAULT_EXCLUSIONS: &[&str] = &[
    "com.apple.ScreenSharing",
    "com.utmapp.UTM",
    "org.virtualbox.app.VirtualBox",
    "com.vmware.fusion",
    "com.parallels.desktop.console",
    "com.valvesoftware.steam",
];

const PIXEL_PER_LINE: f64 = 36.0;
const VELOCITY_EPS: f64 = 8.0;
const FRAME_DT: f64 = 1.0 / 120.0;
const SYNTHETIC_TAG: i64 = 0x4F4E4B15;

/// How a wheel event should be handled. Pure; no AppKit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelKind {
    /// Trackpad / Magic Mouse / already-continuous. Leave it alone.
    PassThrough,
    /// Discrete mouse in an excluded (or conflicting) app.
    Excluded,
    /// Discrete mouse we own.
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollEmitPhase {
    Began,
    Changed,
    MomentumBegin,
    MomentumContinue,
    MomentumEnd,
}

impl ScrollEmitPhase {
    pub fn scroll_phase(self) -> i64 {
        match self {
            Self::Began => 1,
            Self::Changed => 2,
            Self::MomentumBegin | Self::MomentumContinue | Self::MomentumEnd => 0,
        }
    }

    pub fn momentum_phase(self) -> i64 {
        match self {
            Self::Began | Self::Changed => 0,
            Self::MomentumBegin => 1,
            Self::MomentumContinue => 2,
            Self::MomentumEnd => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GesturePhase {
    Idle,
    Active,
    Coasting,
}

/// Exponential-decay interpolator. Driven from the tap (ingest) and a
/// 120 Hz animator thread (step) that parks the moment velocity dies.
#[derive(Clone, Debug)]
pub struct ScrollGesture {
    vx: f64,
    vy: f64,
    phase: GesturePhase,
    emitted: bool,
}

impl Default for ScrollGesture {
    fn default() -> Self {
        Self {
            vx: 0.0,
            vy: 0.0,
            phase: GesturePhase::Idle,
            emitted: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollFrame {
    pub dx: f64,
    pub dy: f64,
    pub phase: ScrollEmitPhase,
}

impl ScrollGesture {
    pub fn is_idle(&self) -> bool {
        matches!(self.phase, GesturePhase::Idle)
    }

    /// Feed a discrete line delta. `speed` is the settings multiplier.
    pub fn ingest(&mut self, dx_lines: f64, dy_lines: f64, speed: f64, reverse: bool) {
        let sign = if reverse { -1.0 } else { 1.0 };
        let gain = PIXEL_PER_LINE * speed.max(0.05) * sign;
        self.vx += dx_lines * gain * 8.0;
        self.vy += dy_lines * gain * 8.0;
        self.phase = GesturePhase::Active;
    }

    /// Advance by `dt` seconds. `duration` is the decay time constant.
    /// Returns `None` once the gesture has fully stopped (including the
    /// terminal MomentumEnd frame).
    pub fn step(&mut self, dt: f64, duration: f64) -> Option<ScrollFrame> {
        if matches!(self.phase, GesturePhase::Idle) {
            return None;
        }
        let tau = duration.clamp(0.05, 2.0);
        let decay = (-dt / tau).exp();
        let dx = self.vx * dt;
        let dy = self.vy * dt;
        self.vx *= decay;
        self.vy *= decay;

        let still = self.vx.hypot(self.vy) >= VELOCITY_EPS;
        let phase = match self.phase {
            GesturePhase::Idle => return None,
            GesturePhase::Active if !self.emitted => {
                self.emitted = true;
                ScrollEmitPhase::Began
            }
            GesturePhase::Active => ScrollEmitPhase::Changed,
            GesturePhase::Coasting if !still => ScrollEmitPhase::MomentumEnd,
            GesturePhase::Coasting if self.emitted => ScrollEmitPhase::MomentumContinue,
            GesturePhase::Coasting => ScrollEmitPhase::MomentumBegin,
        };

        if matches!(self.phase, GesturePhase::Active) && !still {
            self.phase = GesturePhase::Coasting;
            self.emitted = false;
        } else if matches!(phase, ScrollEmitPhase::MomentumEnd) {
            *self = Self::default();
        } else if matches!(self.phase, GesturePhase::Coasting) && !self.emitted {
            self.emitted = true;
        }

        Some(ScrollFrame { dx, dy, phase })
    }

    /// Mark the physical wheel as quiet so the next steps emit momentum.
    pub fn release(&mut self) {
        if matches!(self.phase, GesturePhase::Active) {
            self.phase = GesturePhase::Coasting;
            self.emitted = false;
        }
    }
}

/// Classify a scroll event. Trackpad (`is_continuous`) always passes.
pub fn classify_wheel(is_continuous: bool, excluded: bool) -> WheelKind {
    if is_continuous {
        WheelKind::PassThrough
    } else if excluded {
        WheelKind::Excluded
    } else {
        WheelKind::Smooth
    }
}

pub fn is_excluded(bundle_id: &str, extra: &[String]) -> bool {
    let id = bundle_id.trim();
    if id.is_empty() {
        return false;
    }
    extra.iter().any(|item| item == id) || DEFAULT_EXCLUSIONS.iter().any(|item| *item == id)
}

/// Shift+wheel becomes horizontal (macOS convention).
pub fn apply_shift(dx: f64, dy: f64, shift: bool) -> (f64, f64) {
    if shift && dy.abs() > dx.abs() {
        (dy, 0.0)
    } else {
        (dx, dy)
    }
}

pub fn conflict_labels(running: &[String]) -> Vec<String> {
    running
        .iter()
        .filter(|id| CONFLICT_BUNDLES.contains(&id.as_str()))
        .cloned()
        .collect()
}

// --- live animator (macOS posts; other platforms just step the model) ---

struct Engine {
    gesture: Mutex<ScrollGesture>,
    wake: Condvar,
    running: AtomicBool,
    last_ingest_ms: AtomicU64,
}

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| Engine {
        gesture: Mutex::new(ScrollGesture::default()),
        wake: Condvar::new(),
        running: AtomicBool::new(false),
        last_ingest_ms: AtomicU64::new(0),
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Called from the scroll tap. Starts the animator on the first tick.
pub fn ingest_wheel(dx_lines: f64, dy_lines: f64, shift: bool) {
    let settings = crate::settings::get_app_settings();
    if !settings.smooth_scroll_enabled && !settings.reverse_mouse_scroll {
        return;
    }
    let (dx, dy) = apply_shift(dx_lines, dy_lines, shift);
    let mut guard = engine().gesture.lock().unwrap_or_else(|e| e.into_inner());
    guard.ingest(dx, dy, settings.scroll_speed as f64, settings.reverse_mouse_scroll);
    engine().last_ingest_ms.store(now_ms(), Ordering::Relaxed);
    engine().wake.notify_one();
    drop(guard);
    ensure_animator();
}

pub fn note_wheel_quiet() {
    if let Ok(mut guard) = engine().gesture.lock() {
        guard.release();
    }
}

fn ensure_animator() {
    if engine()
        .running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    thread::Builder::new()
        .name("nook-scroll".into())
        .spawn(animator_loop)
        .expect("scroll animator");
}

fn animator_loop() {
    let engine = engine();
    loop {
        let settings = crate::settings::get_app_settings();
        let duration = settings.scroll_duration as f64;
        let frame = {
            let mut guard = engine.gesture.lock().unwrap_or_else(|e| e.into_inner());
            if guard.is_idle() {
                let (next, timeout) = engine
                    .wake
                    .wait_timeout(guard, Duration::from_millis(250))
                    .unwrap_or_else(|e| e.into_inner());
                guard = next;
                if guard.is_idle() {
                    if timeout.timed_out() {
                        engine.running.store(false, Ordering::SeqCst);
                        if engine
                            .gesture
                            .lock()
                            .map(|g| g.is_idle())
                            .unwrap_or(true)
                        {
                            return;
                        }
                        engine.running.store(true, Ordering::SeqCst);
                    }
                    continue;
                }
            }
            let quiet = now_ms().saturating_sub(engine.last_ingest_ms.load(Ordering::Relaxed)) > 40;
            if quiet {
                guard.release();
            }
            guard.step(FRAME_DT, duration)
        };
        if let Some(frame) = frame {
            post_pixel_frame(frame);
        }
        thread::sleep(Duration::from_secs_f64(FRAME_DT));
    }
}

fn post_pixel_frame(frame: ScrollFrame) {
    #[cfg(target_os = "macos")]
    unsafe {
        post_pixel_frame_macos(frame);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = frame;
    }
}

#[cfg(target_os = "macos")]
unsafe fn post_pixel_frame_macos(frame: ScrollFrame) {
    use super::eventtap::ffi::{
        CGEventCreateScrollWheelEvent2, CGEventPost, CGEventSetDoubleValueField,
        CGEventSetIntegerValueField, CFRelease, kCGEventSourceUserData,
        kCGHIDEventTap, kCGScrollEventUnitPixel, kCGScrollWheelEventIsContinuous,
        kCGScrollWheelEventMomentumPhase, kCGScrollWheelEventPointDeltaAxis1,
        kCGScrollWheelEventPointDeltaAxis2, kCGScrollWheelEventScrollPhase,
    };

    let dy = frame.dy.round() as i32;
    let dx = frame.dx.round() as i32;
    if dy == 0 && dx == 0 && !matches!(frame.phase, ScrollEmitPhase::MomentumEnd) {
        return;
    }
    let event = CGEventCreateScrollWheelEvent2(
        std::ptr::null_mut(),
        kCGScrollEventUnitPixel,
        2,
        dy,
        dx,
        0,
    );
    if event.is_null() {
        return;
    }
    CGEventSetIntegerValueField(event, kCGScrollWheelEventIsContinuous, 1);
    CGEventSetIntegerValueField(event, kCGScrollWheelEventScrollPhase, frame.phase.scroll_phase());
    CGEventSetIntegerValueField(
        event,
        kCGScrollWheelEventMomentumPhase,
        frame.phase.momentum_phase(),
    );
    CGEventSetDoubleValueField(event, kCGScrollWheelEventPointDeltaAxis1, frame.dy);
    CGEventSetDoubleValueField(event, kCGScrollWheelEventPointDeltaAxis2, frame.dx);
    CGEventSetIntegerValueField(event, kCGEventSourceUserData, SYNTHETIC_TAG);
    CGEventPost(kCGHIDEventTap, event);
    CFRelease(event);
}

pub fn synthetic_tag() -> i64 {
    SYNTHETIC_TAG
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trackpad_always_passes() {
        assert_eq!(classify_wheel(true, false), WheelKind::PassThrough);
        assert_eq!(classify_wheel(true, true), WheelKind::PassThrough);
        assert_eq!(classify_wheel(false, false), WheelKind::Smooth);
        assert_eq!(classify_wheel(false, true), WheelKind::Excluded);
    }

    #[test]
    fn shift_swaps_vertical_to_horizontal() {
        assert_eq!(apply_shift(0.0, 3.0, true), (3.0, 0.0));
        assert_eq!(apply_shift(2.0, 0.0, true), (2.0, 0.0));
        assert_eq!(apply_shift(0.0, 3.0, false), (0.0, 3.0));
    }

    #[test]
    fn exclusion_matches_defaults_and_extras() {
        assert!(is_excluded("com.utmapp.UTM", &[]));
        assert!(is_excluded("games.mine", &["games.mine".into()]));
        assert!(!is_excluded("com.apple.Safari", &[]));
        assert!(!is_excluded("", &["x".into()]));
    }

    #[test]
    fn velocity_decays_and_stops() {
        let mut g = ScrollGesture::default();
        assert!(g.is_idle());
        g.ingest(0.0, 1.0, 1.0, false);
        assert!(!g.is_idle());

        let first = g.step(FRAME_DT, 0.35).expect("began");
        assert_eq!(first.phase, ScrollEmitPhase::Began);
        assert!(first.dy > 0.0);

        let mut saw_changed = false;
        for _ in 0..8 {
            if let Some(frame) = g.step(FRAME_DT, 0.35) {
                if frame.phase == ScrollEmitPhase::Changed {
                    saw_changed = true;
                }
            }
        }
        assert!(saw_changed, "active frames should report Changed");

        g.release();
        let mut saw_end = false;
        for _ in 0..200 {
            match g.step(FRAME_DT, 0.2) {
                Some(frame) if frame.phase == ScrollEmitPhase::MomentumEnd => {
                    saw_end = true;
                    break;
                }
                Some(_) => {}
                None => break,
            }
        }
        assert!(saw_end);
        assert!(g.is_idle());
        assert!(g.step(FRAME_DT, 0.35).is_none());
    }

    #[test]
    fn reverse_negates_ingest() {
        let mut fwd = ScrollGesture::default();
        let mut rev = ScrollGesture::default();
        fwd.ingest(0.0, 1.0, 1.0, false);
        rev.ingest(0.0, 1.0, 1.0, true);
        let a = fwd.step(FRAME_DT, 0.35).unwrap();
        let b = rev.step(FRAME_DT, 0.35).unwrap();
        assert!(a.dy > 0.0);
        assert!(b.dy < 0.0);
        assert!((a.dy + b.dy).abs() < 1e-9);
    }

    #[test]
    fn faster_speed_makes_larger_pixels() {
        let mut slow = ScrollGesture::default();
        let mut fast = ScrollGesture::default();
        slow.ingest(0.0, 1.0, 0.5, false);
        fast.ingest(0.0, 1.0, 2.0, false);
        let a = slow.step(FRAME_DT, 0.35).unwrap();
        let b = fast.step(FRAME_DT, 0.35).unwrap();
        assert!(b.dy > a.dy * 3.0);
    }

    #[test]
    fn conflict_filter() {
        let running = vec!["com.apple.Safari".into(), "com.caldis.Mos".into()];
        assert_eq!(conflict_labels(&running), vec!["com.caldis.Mos".to_string()]);
    }

    #[test]
    fn emit_phases_match_coregraphics_constants() {
        assert_eq!(ScrollEmitPhase::Began.scroll_phase(), 1);
        assert_eq!(ScrollEmitPhase::Changed.scroll_phase(), 2);
        assert_eq!(ScrollEmitPhase::MomentumBegin.momentum_phase(), 1);
        assert_eq!(ScrollEmitPhase::MomentumContinue.momentum_phase(), 2);
        assert_eq!(ScrollEmitPhase::MomentumEnd.momentum_phase(), 3);
    }
}
