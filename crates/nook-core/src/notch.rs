use crate::models::NotchInfo;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

#[cfg(target_os = "macos")]
use objc2::{Encode, Encoding};

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[cfg(target_os = "macos")]
unsafe impl Encode for CGSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[cfg(target_os = "macos")]
unsafe impl Encode for CGPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

#[cfg(target_os = "macos")]
unsafe impl Encode for CGRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NSEdgeInsets {
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
}

#[cfg(target_os = "macos")]
unsafe impl Encode for NSEdgeInsets {
    const ENCODING: Encoding = Encoding::Struct(
        "NSEdgeInsets",
        &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING],
    );
}

fn calculate_dynamic_notch_width(screen_width: f64) -> f64 {
    (screen_width * 0.1).clamp(200.0, 260.0)
}

static SCREEN: OnceLock<RwLock<(f64, f64, f64, f64)>> = OnceLock::new();
static SCREEN_GEN: AtomicU64 = AtomicU64::new(1);

fn screen_store() -> &'static RwLock<(f64, f64, f64, f64)> {
    SCREEN.get_or_init(|| RwLock::new(read_screen_info()))
}

/// Screen + notch metrics. Heights are in logical points.
/// Cached until [`invalidate_screen_cache`].
pub fn get_screen_info() -> (f64, f64, f64, f64) {
    if let Ok(guard) = screen_store().read() {
        return *guard;
    }
    read_screen_info()
}

/// Re-read NSScreen / Win32 metrics. Call on display reconfiguration.
pub fn invalidate_screen_cache() {
    let fresh = read_screen_info();
    if let Ok(mut guard) = screen_store().write() {
        *guard = fresh;
    }
    SCREEN_GEN.fetch_add(1, Ordering::Relaxed);
}

pub fn screen_generation() -> u64 {
    SCREEN_GEN.load(Ordering::Relaxed)
}

fn read_screen_info() -> (f64, f64, f64, f64) {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::*;

        unsafe {
            let main_screen: *mut AnyObject = msg_send![class!(NSScreen), mainScreen];
            if main_screen.is_null() {
                return (1440.0, 900.0, 38.0, 220.0);
            }

            let frame: CGRect = msg_send![main_screen, frame];
            let screen_width = frame.size.width;
            let screen_height = frame.size.height;
            let insets: NSEdgeInsets = msg_send![main_screen, safeAreaInsets];
            let safe_area_top = insets.top;

            // The gap between the menu-bar regions on either side of the camera.
            let left: CGRect = msg_send![main_screen, auxiliaryTopLeftArea];
            let right: CGRect = msg_send![main_screen, auxiliaryTopRightArea];
            let measured_width = right.origin.x - (left.origin.x + left.size.width);

            let has_hardware_notch = safe_area_top > 0.0 && measured_width > 80.0;

            let notch_height = if has_hardware_notch {
                safe_area_top.max(32.0)
            } else {
                0.0
            };

            let notch_width = if has_hardware_notch {
                measured_width.clamp(160.0, 280.0)
            } else {
                calculate_dynamic_notch_width(screen_width)
            };

            (screen_width, screen_height, notch_height, notch_width)
        }
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        unsafe {
            let width = GetSystemMetrics(SM_CXSCREEN) as f64;
            let height = GetSystemMetrics(SM_CYSCREEN) as f64;
            let notch_width = calculate_dynamic_notch_width(width);
            (width, height, 0.0, notch_width)
        }
    }

    #[cfg(target_os = "linux")]
    {
        let notch_width = calculate_dynamic_notch_width(1920.0);
        (1920.0, 1080.0, 0.0, notch_width)
    }
}

pub fn get_notch_info() -> NotchInfo {
    let (screen_width, screen_height, notch_height, notch_width) = get_screen_info();
    NotchInfo {
        has_notch: notch_height > 0.0,
        notch_height,
        notch_width,
        screen_width,
        screen_height,
        visible_height: screen_height - notch_height,
    }
}

static OVERLAY_H: AtomicU64 = AtomicU64::new(0);

/// Room below the island's lowest edge while a drag or reposition is active:
/// the 80pt Finder drag-capture pad plus slack. Every backing buffer scales
/// with this, so the pad is only paid for while something needs it.
const OVERLAY_MARGIN_CAPTURE: f64 = 100.0;
/// Quiet margin: spring overshoot plus the 7px motion-blur smear.
const OVERLAY_MARGIN: f64 = 24.0;
/// Height granularity. Coarse on purpose so a settling spring lands in one
/// step instead of resizing the NSWindow frame by frame.
const OVERLAY_STEP: f64 = 50.0;
/// Floor covering the pre-paint notch fallback hit region.
const OVERLAY_MIN: f64 = 100.0;

/// Publish how far down the overlay strip must reach. Called by the island on
/// paint with a [`quantized_overlay_height`] value. Returns the previously
/// published height (0.0 if never set) so the caller can react to changes.
pub fn set_overlay_height(height: f64) -> f64 {
    f64::from_bits(OVERLAY_H.swap(height.to_bits(), Ordering::Relaxed))
}

/// The last height published via [`set_overlay_height`], 0.0 if never set.
pub fn published_overlay_height() -> f64 {
    f64::from_bits(OVERLAY_H.load(Ordering::Relaxed))
}

/// Strip height that covers an island whose lowest edge sits at
/// `island_bottom`, padded, quantized to [`OVERLAY_STEP`], and clamped to the
/// display. `capture` widens the pad to the Finder drag-capture region while a
/// drag or reposition is in flight.
pub fn quantized_overlay_height(island_bottom: f64, screen_height: f64, capture: bool) -> f64 {
    let margin = if capture {
        OVERLAY_MARGIN_CAPTURE
    } else {
        OVERLAY_MARGIN
    };
    (((island_bottom + margin) / OVERLAY_STEP).ceil() * OVERLAY_STEP)
        .max(OVERLAY_MIN)
        .min(screen_height)
}

/// Overlay window size: a full-width strip along the top of the display, just
/// tall enough to hold the island.
///
/// The strip used to be the whole display, but Metal retains several
/// screen-sized backing buffers for the window — hundreds of MB at Retina
/// resolution for a mostly-empty transparent canvas. Full width and the fixed
/// top-left anchor are kept so window coordinates stay equal to screen
/// coordinates everywhere the strip covers: `crate::mouse` hit-testing, drag
/// capture, and the glass underlay all carry over unchanged. The height is
/// whatever the island last published via [`set_overlay_height`]; if the user
/// drags the island toward the bottom edge the strip simply grows with it.
pub fn overlay_window_size() -> (f64, f64) {
    let (screen_width, screen_height, _, _) = get_screen_info();
    let published = f64::from_bits(OVERLAY_H.load(Ordering::Relaxed));
    let height = if published > 0.0 {
        published
    } else {
        OVERLAY_MIN
    };
    (
        screen_width,
        height.clamp(OVERLAY_MIN.min(screen_height), screen_height),
    )
}

pub fn overlay_window_origin() -> (f64, f64) {
    (0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_strip_quantizes_and_clamps() {
        // Idle island attached to the notch; hover fits in the same step.
        assert_eq!(quantized_overlay_height(44.0, 982.0, false), 100.0);
        assert_eq!(quantized_overlay_height(56.0, 982.0, false), 100.0);
        // Expanded widgets pane.
        assert_eq!(quantized_overlay_height(182.0, 982.0, false), 250.0);
        // Same step while a spring settles nearby — no per-frame resizes.
        assert_eq!(
            quantized_overlay_height(205.0, 982.0, false),
            quantized_overlay_height(215.0, 982.0, false)
        );
        // A live Finder drag buys the 80pt capture pad below the island.
        assert_eq!(quantized_overlay_height(44.0, 982.0, true), 150.0);
        // Dragged to the bottom edge: never taller than the display.
        assert_eq!(quantized_overlay_height(950.0, 982.0, true), 982.0);
    }

    #[test]
    fn overlay_window_is_a_full_width_strip() {
        let (screen_w, screen_h, _, _) = get_screen_info();
        set_overlay_height(300.0);
        let (w, h) = overlay_window_size();
        assert_eq!(w, screen_w);
        assert_eq!(h, 300.0f64.min(screen_h));
        set_overlay_height(1e9);
        assert_eq!(overlay_window_size().1, screen_h);
        set_overlay_height(0.0);
        assert_eq!(overlay_window_size().1, OVERLAY_MIN.min(screen_h));
    }
}
