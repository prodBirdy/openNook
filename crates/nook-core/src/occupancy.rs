//! Whether the frontmost window is filling the display the island lives on.
//!
//! Used by the "Hide when an app fills the display" setting. Full-screen spaces
//! and zoomed windows both count: either one covers the notch and would fight
//! the overlay.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Refresh at most this often — `CGWindowListCopyWindowInfo` is not cheap
/// enough for the 20 ms mouse poll.
const CACHE_MS: u64 = 250;

static CACHE: AtomicBool = AtomicBool::new(false);
static LAST_MS: AtomicU64 = AtomicU64::new(0);

/// Axis-aligned rect in CG-style coordinates (origin top-left of the main
/// display). Pure geometry; the macOS sampler maps AppKit / window-list rects
/// into this space before calling [`window_fills_display`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl ScreenRect {
    pub fn covers(self, other: Self, tol: f64) -> bool {
        (self.x - other.x).abs() <= tol
            && (self.y - other.y).abs() <= tol
            && (self.w - other.w).abs() <= tol
            && (self.h - other.h).abs() <= tol
    }
}

/// True when `window` matches the full display or the visible frame (menu bar
/// and Dock subtracted) within a few points — native full screen and the green
/// button zoom both land here.
pub fn window_fills_display(window: ScreenRect, screen: ScreenRect, visible: ScreenRect) -> bool {
    const TOL: f64 = 24.0;
    window.covers(screen, TOL) || window.covers(visible, TOL)
}

/// Cached sample of [`query_frontmost_fills`]. Safe to call from the island
/// poll loop.
pub fn frontmost_fills_display() -> bool {
    let now = unix_ms();
    let last = LAST_MS.load(Ordering::Relaxed);
    if now.saturating_sub(last) < CACHE_MS {
        return CACHE.load(Ordering::Relaxed);
    }
    LAST_MS.store(now, Ordering::Relaxed);
    let fills = query_frontmost_fills();
    CACHE.store(fills, Ordering::Relaxed);
    fills
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn query_frontmost_fills() -> bool {
    #[cfg(target_os = "macos")]
    {
        query_frontmost_fills_macos()
    }
    #[cfg(target_os = "windows")]
    {
        query_frontmost_fills_windows()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
fn query_frontmost_fills_macos() -> bool {
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2::*;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *mut AnyObject;
    }

    // kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements
    const OPTIONS: u32 = 1 | (1 << 4);

    autoreleasepool(|_| unsafe {
        let Some((screen, visible)) = main_screen_rects() else {
            return false;
        };
        let info: *mut AnyObject = CGWindowListCopyWindowInfo(OPTIONS, 0);
        if info.is_null() {
            return false;
        }
        let our_pid = std::process::id() as i64;
        let count: usize = msg_send![info, count];
        let mut fills = false;
        for i in 0..count {
            let dict: *mut AnyObject = msg_send![info, objectAtIndex: i];
            if dict.is_null() {
                continue;
            }
            let pid = dict_i64(dict, c"kCGWindowOwnerPID").unwrap_or(0);
            if pid == 0 || pid == our_pid {
                continue;
            }
            let layer = dict_i64(dict, c"kCGWindowLayer").unwrap_or(-1);
            if layer != 0 {
                continue;
            }
            let alpha = dict_f64(dict, c"kCGWindowAlpha").unwrap_or(1.0);
            if alpha < 0.9 {
                continue;
            }
            let Some(bounds) = window_bounds(dict) else {
                continue;
            };
            if bounds.w < 64.0 || bounds.h < 64.0 {
                continue;
            }
            fills = window_fills_display(bounds, screen, visible);
            break;
        }
        let _: () = msg_send![info, release];
        fills
    })
}

#[cfg(target_os = "macos")]
unsafe fn main_screen_rects() -> Option<(ScreenRect, ScreenRect)> {
    use crate::notch::CGRect;
    use objc2::runtime::AnyObject;
    use objc2::*;

    let main_screen: *mut AnyObject = msg_send![class!(NSScreen), mainScreen];
    if main_screen.is_null() {
        return None;
    }
    let frame: CGRect = msg_send![main_screen, frame];
    let visible: CGRect = msg_send![main_screen, visibleFrame];
    let screen_h = frame.size.height;
    let screen = ScreenRect {
        x: 0.0,
        y: 0.0,
        w: frame.size.width,
        h: frame.size.height,
    };
    let visible = ScreenRect {
        x: visible.origin.x,
        y: screen_h - visible.origin.y - visible.size.height,
        w: visible.size.width,
        h: visible.size.height,
    };
    Some((screen, visible))
}

#[cfg(target_os = "macos")]
unsafe fn window_bounds(dict: *mut objc2::runtime::AnyObject) -> Option<ScreenRect> {
    use objc2::runtime::AnyObject;
    use objc2::*;

    let key: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c"kCGWindowBounds".as_ptr()];
    if key.is_null() {
        return None;
    }
    let bounds: *mut AnyObject = msg_send![dict, objectForKey: key];
    if bounds.is_null() {
        return None;
    }
    Some(ScreenRect {
        x: dict_f64(bounds, c"X")?,
        y: dict_f64(bounds, c"Y")?,
        w: dict_f64(bounds, c"Width")?,
        h: dict_f64(bounds, c"Height")?,
    })
}

#[cfg(target_os = "macos")]
unsafe fn dict_f64(dict: *mut objc2::runtime::AnyObject, key: &std::ffi::CStr) -> Option<f64> {
    use objc2::runtime::AnyObject;
    use objc2::*;
    let nskey: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: key.as_ptr()];
    if nskey.is_null() {
        return None;
    }
    let val: *mut AnyObject = msg_send![dict, objectForKey: nskey];
    if val.is_null() {
        return None;
    }
    let n: f64 = msg_send![val, doubleValue];
    Some(n)
}

#[cfg(target_os = "macos")]
unsafe fn dict_i64(dict: *mut objc2::runtime::AnyObject, key: &std::ffi::CStr) -> Option<i64> {
    use objc2::runtime::AnyObject;
    use objc2::*;
    let nskey: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: key.as_ptr()];
    if nskey.is_null() {
        return None;
    }
    let val: *mut AnyObject = msg_send![dict, objectForKey: nskey];
    if val.is_null() {
        return None;
    }
    let n: i64 = msg_send![val, longLongValue];
    Some(n)
}

#[cfg(target_os = "windows")]
fn query_frontmost_fills_windows() -> bool {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, IsZoomed,
    };

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0 == 0 {
            return false;
        }
        if IsZoomed(hwnd).as_bool() {
            return true;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
        let (sw, sh, _, _) = crate::notch::get_screen_info();
        window_fills_display(
            ScreenRect {
                x: rect.left as f64,
                y: rect.top as f64,
                w: (rect.right - rect.left) as f64,
                h: (rect.bottom - rect.top) as f64,
            },
            ScreenRect {
                x: 0.0,
                y: 0.0,
                w: sw,
                h: sh,
            },
            ScreenRect {
                x: 0.0,
                y: 0.0,
                w: sw,
                h: sh,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_matches_the_display() {
        let screen = ScreenRect {
            x: 0.0,
            y: 0.0,
            w: 1512.0,
            h: 982.0,
        };
        let visible = ScreenRect {
            x: 0.0,
            y: 38.0,
            w: 1512.0,
            h: 894.0,
        };
        assert!(window_fills_display(screen, screen, visible));
        assert!(window_fills_display(visible, screen, visible));
        assert!(window_fills_display(
            ScreenRect {
                x: 2.0,
                y: 1.0,
                w: 1510.0,
                h: 980.0,
            },
            screen,
            visible
        ));
        assert!(!window_fills_display(
            ScreenRect {
                x: 80.0,
                y: 80.0,
                w: 900.0,
                h: 600.0,
            },
            screen,
            visible
        ));
    }
}
