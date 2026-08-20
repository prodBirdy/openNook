use crate::models::NotchInfo;
use crate::settings::get_window_settings;

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

/// Screen + notch metrics. Heights are in logical points.
pub fn get_screen_info() -> (f64, f64, f64, f64) {
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

/// Overlay window size: full display width, tall enough for the expanded island.
pub fn overlay_window_size() -> (f64, f64) {
    let (screen_width, _h, notch_height, _) = get_screen_info();
    let settings = get_window_settings();
    let height = (notch_height + 260.0 + settings.extra_height.min(40.0)).max(280.0);
    (screen_width, height)
}

pub fn overlay_window_origin() -> (f64, f64) {
    (0.0, 0.0)
}
