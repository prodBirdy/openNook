//! Global mouse polling for hover-to-expand, independent of GPUI hit testing.
//!
//! Click-through is owned by the GPUI window (NSWindow ignoresMouseEvents).
//! This module only reports enter/exit against the island's UI bounds.

use crate::notch::get_screen_info;
use crate::settings::get_app_settings;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;

#[derive(Debug, Clone, Copy)]
pub struct UiBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

static UI_BOUNDS: std::sync::OnceLock<RwLock<Option<UiBounds>>> = std::sync::OnceLock::new();
static MOUSE_X: AtomicU64 = AtomicU64::new(0);
static MOUSE_Y: AtomicU64 = AtomicU64::new(0);
static DRAG_ACTIVE: AtomicBool = AtomicBool::new(false);
static POLL_STARTED: AtomicBool = AtomicBool::new(false);

fn bounds_store() -> &'static RwLock<Option<UiBounds>> {
    UI_BOUNDS.get_or_init(|| RwLock::new(None))
}

pub fn update_ui_bounds(x: f64, y: f64, width: f64, height: f64) {
    if let Ok(mut guard) = bounds_store().write() {
        *guard = Some(UiBounds {
            x,
            y,
            width,
            height,
        });
    }
}

pub fn current_mouse_logical() -> (f64, f64) {
    (
        f64::from_bits(MOUSE_X.load(Ordering::Relaxed)),
        f64::from_bits(MOUSE_Y.load(Ordering::Relaxed)),
    )
}

pub fn drag_active() -> bool {
    DRAG_ACTIVE.load(Ordering::Relaxed)
}

/// Hit-test the cursor against the island activation area.
///
/// Drag capture may extend beyond this area, but hovering and expansion must not.
pub fn hit_test(mouse_x: f64, mouse_y: f64) -> bool {
    contains(hit_region(0.0), mouse_x, mouse_y)
}

/// Hit-test the wider region used only to let the overlay receive Finder drags.
pub fn hit_test_drag_capture(mouse_x: f64, mouse_y: f64) -> bool {
    contains(hit_region(drag_capture_padding()), mouse_x, mouse_y)
}

/// Margin used only to let the overlay receive an inbound Finder drag before
/// the cursor reaches the painted island.
pub fn drag_capture_padding() -> f64 {
    if drag_active() || file_drag_needs_capture() {
        80.0
    } else {
        0.0
    }
}

fn file_drag_needs_capture() -> bool {
    // Tests drive padding through `DRAG_ACTIVE` only. On macOS the real
    // drag pasteboard plus a leftover left-button can otherwise pin padding
    // at 80 for the whole libtest process.
    #[cfg(test)]
    {
        false
    }
    #[cfg(not(test))]
    {
        crate::files::file_drag_active()
    }
}

/// Strict test against the painted island — no hover tolerance, no drag
/// widening. The overlay NSWindow spans the full screen width and is far taller
/// than the island, so this is what decides whether a click belongs to us or
/// falls through to whatever is underneath.
pub fn hit_test_exact(mouse_x: f64, mouse_y: f64) -> bool {
    hit_test(mouse_x, mouse_y)
}

/// What [`hit_test_drag_capture`] accepts, as a rect. For the debug overlay, so
/// what gets drawn cannot drift from what gets tested.
pub fn drag_capture_bounds() -> UiBounds {
    rect(hit_region(drag_capture_padding()))
}

/// What [`hit_test_exact`] accepts, as a rect.
pub fn exact_bounds() -> UiBounds {
    rect(hit_region(0.0))
}

/// Whether the cursor is within approach distance of the island. The UI tick
/// loop uses this to stay at its fast cadence while the pointer could reach
/// the island within one slow tick, and to idle down otherwise. Plain math on
/// the published bounds — no AppKit calls.
pub fn hit_test_near(mouse_x: f64, mouse_y: f64) -> bool {
    const NEAR: f64 = 96.0;
    let b = exact_bounds();
    mouse_x >= b.x - NEAR
        && mouse_x <= b.x + b.width + NEAR
        && mouse_y >= b.y - NEAR
        && mouse_y <= b.y + b.height.max(MIN_GRAB_HEIGHT) + NEAR
}

fn rect((x0, x1, y0, y1): (f64, f64, f64, f64)) -> UiBounds {
    UiBounds {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// Without a notch the idle island collapses to a 1px line, which no cursor can
/// land on. The bottom edge alone is floored to this so the sliver stays
/// grabbable by shoving the pointer at the top of the screen; the sides stay
/// flush with the paint.
const MIN_GRAB_HEIGHT: f64 = 8.0;

/// The island rect in screen-logical, y-down coordinates, grown by `padding` on
/// every side. Falls back to the physical notch until the first paint reports
/// real bounds.
fn hit_region(padding: f64) -> (f64, f64, f64, f64) {
    if let Ok(guard) = bounds_store().try_read() {
        if let Some(bounds) = *guard {
            let y0 = bounds.y - padding;
            let y1 = (bounds.y + bounds.height).max(MIN_GRAB_HEIGHT) + padding;
            // Compact island is a short centred pill. A pad around that box
            // misses Finder drags that come in along the menu bar from the
            // sides. During a drag, take the whole top strip so click-through
            // lifts before the cursor reaches the painted island.
            if padding > 0.0 && bounds.y <= 2.0 {
                // Attached to the menu bar: a Finder drag along the top
                // strip has to lift click-through before it reaches the pill.
                // A moved island is hit through the padded box alone.
                let (screen_width, _, _, _) = get_screen_info();
                return (0.0, screen_width, y0.max(0.0), y1);
            }
            return (
                bounds.x - padding,
                bounds.x + bounds.width + padding,
                y0,
                y1,
            );
        }
    }

    let (screen_width, _, notch_height, notch_width) = get_screen_info();
    let settings = get_app_settings();
    let effective_notch_width = if settings.non_notch_mode {
        0.0
    } else {
        notch_width
    };
    let x_start = (screen_width - effective_notch_width) / 2.0;
    let y_end = if settings.non_notch_mode {
        1.0
    } else {
        notch_height.max(38.0)
    };

    (
        x_start - padding,
        x_start + effective_notch_width + padding,
        -padding,
        y_end + padding,
    )
}

fn contains((x0, x1, y0, y1): (f64, f64, f64, f64), x: f64, y: f64) -> bool {
    x >= x0 && x <= x1 && y >= y0 && y <= y1
}

/// Sample the cursor off the UI thread. Safe to call more than once.
pub fn start_polling() {
    if POLL_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    sample_now();
    let _ = std::thread::Builder::new()
        .name("nook-mouse".into())
        .spawn(|| loop {
            sample_now();
            let inside = {
                let (mx, my) = current_mouse_logical();
                hit_test(mx, my)
            };
            let ms = if inside || DRAG_ACTIVE.load(Ordering::Relaxed) {
                20
            } else {
                33
            };
            std::thread::sleep(std::time::Duration::from_millis(ms));
        });
}

fn sample_now() {
    let (x, y) = read_mouse_logical();
    MOUSE_X.store(x.to_bits(), Ordering::Relaxed);
    MOUSE_Y.store(y.to_bits(), Ordering::Relaxed);
    DRAG_ACTIVE.store(crate::files::file_drag_active(), Ordering::Relaxed);
}

fn read_mouse_logical() -> (f64, f64) {
    #[cfg(target_os = "macos")]
    {
        use crate::notch::CGPoint;
        use objc2::*;
        unsafe {
            let mouse_loc: CGPoint = msg_send![class!(NSEvent), mouseLocation];
            let (_, screen_height, _, _) = get_screen_info();
            (mouse_loc.x, screen_height - mouse_loc.y)
        }
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut point = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut point);
        }
        (point.x as f64, point.y as f64)
    }
    #[cfg(target_os = "linux")]
    {
        (0.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::{Mutex, MutexGuard};

    /// `UI_BOUNDS` / `DRAG_ACTIVE` are process-global, so tests that publish
    /// bounds must not run concurrently or they read each other's island.
    static BOUNDS: Mutex<()> = Mutex::new(());

    struct BoundsLock {
        _guard: MutexGuard<'static, ()>,
    }

    impl Drop for BoundsLock {
        fn drop(&mut self) {
            DRAG_ACTIVE.store(false, Ordering::Relaxed);
        }
    }

    fn lock() -> BoundsLock {
        let guard = BOUNDS.lock().unwrap_or_else(|e| e.into_inner());
        DRAG_ACTIVE.store(false, Ordering::Relaxed);
        BoundsLock { _guard: guard }
    }

    #[test]
    fn hover_region_is_the_painted_box() {
        let _guard = lock();
        update_ui_bounds(100.0, 0.0, 200.0, 34.0);
        assert_eq!(drag_capture_padding(), 0.0);
        assert!(hit_test(150.0, 10.0));
        // A pixel out on any side is out — no entry pad, no hysteresis.
        assert!(!hit_test(99.0, 10.0));
        assert!(!hit_test(301.0, 10.0));
        assert!(!hit_test(150.0, 35.0));

        let bounds = drag_capture_bounds();
        assert_eq!((bounds.x, bounds.width), (100.0, 200.0));
        assert_eq!((bounds.y, bounds.height), (0.0, 34.0));
    }

    #[test]
    fn hover_and_click_regions_agree() {
        let _guard = lock();
        update_ui_bounds(100.0, 0.0, 200.0, 34.0);
        // Hover used to reach further than clicks did; nothing arms from
        // outside the paint any more.
        for (x, y) in [(150.0, 10.0), (150.0, 39.0), (95.0, 10.0), (250.0, 33.0)] {
            assert_eq!(hit_test(x, y), hit_test_exact(x, y), "at ({x}, {y})");
        }
        assert!(hit_test_exact(150.0, 10.0));
        assert!(!hit_test_exact(150.0, 39.0));
        assert!(!hit_test_exact(95.0, 10.0));
    }

    #[test]
    fn sliver_island_keeps_a_grabbable_strip() {
        let _guard = lock();
        // What non-notch mode paints when idle: a 1px line.
        update_ui_bounds(100.0, 0.0, 200.0, 1.0);
        assert!(hit_test(150.0, 6.0));
        assert!(!hit_test(150.0, 12.0));
        // The floor is on the bottom edge only; the sides stay flush.
        assert!(!hit_test(99.0, 6.0));
    }

    #[test]
    fn uses_bounds_origin_not_recomputed_center() {
        let _guard = lock();
        update_ui_bounds(10.0, 0.0, 100.0, 34.0);
        assert!(hit_test(20.0, 10.0));
        assert!(!hit_test(200.0, 10.0));
    }

    #[test]
    fn finder_drag_only_hovers_the_painted_island() {
        let _guard = lock();
        update_ui_bounds(790.0, 0.0, 220.0, 38.0);
        super::DRAG_ACTIVE.store(true, Ordering::Relaxed);

        assert!(hit_test(800.0, 20.0), "the painted island activates");
        assert!(
            !hit_test(10.0, 10.0),
            "a file elsewhere on screen must not open the island"
        );
        assert!(
            hit_test_drag_capture(10.0, 10.0),
            "the overlay still captures the drag before it reaches the island"
        );
        let far_x = (get_screen_info().0 - 10.0).max(0.0);
        assert!(
            !hit_test(far_x, 20.0),
            "the drag capture strip must not count as hover"
        );
        assert!(hit_test_drag_capture(far_x, 20.0));
    }

    #[test]
    fn finder_drag_does_not_open_a_top_strip_when_the_island_moved() {
        let _guard = lock();
        update_ui_bounds(790.0, 200.0, 220.0, 38.0);
        super::DRAG_ACTIVE.store(true, Ordering::Relaxed);
        assert!(
            !hit_test_drag_capture(10.0, 10.0),
            "a moved island does not steal the menu bar"
        );
        assert!(
            hit_test_drag_capture(800.0, 210.0),
            "padded capture box around the island still works"
        );
        super::DRAG_ACTIVE.store(false, Ordering::Relaxed);
    }
}
