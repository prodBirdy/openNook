//! Rectangle-style window snapping via the public Accessibility API.
//!
//! Geometry (halves / quarters, AppKit→AX flip, drag-edge zones) is pure and
//! unit-tested. The AX write path runs only on an explicit snap — hotkey,
//! settings card, or a future drag release — never from the island tick.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Tile of [`SnapRect`] produced by a snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SnapKind {
    LeftHalf = 0,
    RightHalf = 1,
    TopHalf = 2,
    BottomHalf = 3,
    TopLeft = 4,
    TopRight = 5,
    BottomLeft = 6,
    BottomRight = 7,
    Maximize = 8,
}

impl SnapKind {
    pub const ALL: [Self; 9] = [
        Self::LeftHalf,
        Self::RightHalf,
        Self::TopHalf,
        Self::BottomHalf,
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
        Self::Maximize,
    ];

    pub fn from_u8(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| *kind as u8 == value)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::LeftHalf => "Left half",
            Self::RightHalf => "Right half",
            Self::TopHalf => "Top half",
            Self::BottomHalf => "Bottom half",
            Self::TopLeft => "Top left",
            Self::TopRight => "Top right",
            Self::BottomLeft => "Bottom left",
            Self::BottomRight => "Bottom right",
            Self::Maximize => "Maximize",
        }
    }
}

/// Axis-aligned rect. AX / CG space is top-left origin; AppKit `visibleFrame`
/// is bottom-left — convert with [`ns_visible_to_ax`] before tiling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl SnapRect {
    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapError {
    Disabled,
    NotTrusted,
    Unsupported,
    NoWindow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibilityStatus {
    Granted,
    Denied,
    Unsupported,
}

impl AccessibilityStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Granted => "Granted",
            Self::Denied => "Not granted",
            Self::Unsupported => "macOS only",
        }
    }
}

/// Convert an AppKit `visibleFrame` (origin bottom-left of the primary
/// display, y up) into Accessibility / CG space (origin top-left of the
/// primary display, y down).
pub fn ns_visible_to_ax(visible_ns: SnapRect, primary_height: f64) -> SnapRect {
    SnapRect {
        x: visible_ns.x,
        y: primary_height - visible_ns.y - visible_ns.h,
        w: visible_ns.w,
        h: visible_ns.h,
    }
}

/// Tile `kind` inside an AX-space visible frame. Odd leftover pixels go to
/// the right / bottom so the two halves abut with no gap.
pub fn snap_rect(kind: SnapKind, visible: SnapRect) -> SnapRect {
    let (x, half_w, right_x) = split_axis(visible.x, visible.w);
    let (y, half_h, bottom_y) = split_axis(visible.y, visible.h);
    let right_w = visible.w - half_w;
    let bottom_h = visible.h - half_h;
    match kind {
        SnapKind::LeftHalf => SnapRect {
            x,
            y: visible.y,
            w: half_w,
            h: visible.h,
        },
        SnapKind::RightHalf => SnapRect {
            x: right_x,
            y: visible.y,
            w: right_w,
            h: visible.h,
        },
        SnapKind::TopHalf => SnapRect {
            x: visible.x,
            y,
            w: visible.w,
            h: half_h,
        },
        SnapKind::BottomHalf => SnapRect {
            x: visible.x,
            y: bottom_y,
            w: visible.w,
            h: bottom_h,
        },
        SnapKind::TopLeft => SnapRect {
            x,
            y,
            w: half_w,
            h: half_h,
        },
        SnapKind::TopRight => SnapRect {
            x: right_x,
            y,
            w: right_w,
            h: half_h,
        },
        SnapKind::BottomLeft => SnapRect {
            x,
            y: bottom_y,
            w: half_w,
            h: bottom_h,
        },
        SnapKind::BottomRight => SnapRect {
            x: right_x,
            y: bottom_y,
            w: right_w,
            h: bottom_h,
        },
        SnapKind::Maximize => visible,
    }
}

fn split_axis(origin: f64, length: f64) -> (f64, f64, f64) {
    let first = (length * 0.5).floor();
    (origin, first, origin + first)
}

/// Edge / corner zone the cursor sits in, or `None` when it is in the open
/// middle. Corners win over sides. `inset` is the zone depth in the same
/// units as `visible` (points). Used by drag-to-edge; hotkeys ignore it.
pub fn edge_zone(x: f64, y: f64, visible: SnapRect, inset: f64) -> Option<SnapKind> {
    if inset <= 0.0 || !visible.contains(x, y) {
        return None;
    }
    let left = x <= visible.x + inset;
    let right = x >= visible.x + visible.w - inset;
    let top = y <= visible.y + inset;
    let bottom = y >= visible.y + visible.h - inset;
    match (left, right, top, bottom) {
        (true, false, true, false) => Some(SnapKind::TopLeft),
        (false, true, true, false) => Some(SnapKind::TopRight),
        (true, false, false, true) => Some(SnapKind::BottomLeft),
        (false, true, false, true) => Some(SnapKind::BottomRight),
        (true, false, false, false) => Some(SnapKind::LeftHalf),
        (false, true, false, false) => Some(SnapKind::RightHalf),
        (false, false, true, false) => Some(SnapKind::TopHalf),
        (false, false, false, true) => Some(SnapKind::BottomHalf),
        _ => None,
    }
}

const FLASH_MS: u64 = 1400;
static FLASH_KIND: AtomicU8 = AtomicU8::new(0);
static FLASH_AT_MS: AtomicU64 = AtomicU64::new(0);

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Record a just-applied snap so the compact face can flash a label.
pub fn note_flash(kind: SnapKind) {
    FLASH_KIND.store(kind as u8 + 1, Ordering::Relaxed);
    FLASH_AT_MS.store(unix_ms(), Ordering::Relaxed);
}

/// True while a snap-confirmation flash should stay on screen. Cheap atomic;
/// the island tick uses this as a dirty hint, not as a reason to talk to AX.
pub fn flash_is_live() -> bool {
    let at = FLASH_AT_MS.load(Ordering::Relaxed);
    at != 0 && unix_ms().saturating_sub(at) < FLASH_MS
}

pub fn flash_label() -> Option<&'static str> {
    if !flash_is_live() {
        return None;
    }
    let raw = FLASH_KIND.load(Ordering::Relaxed);
    SnapKind::from_u8(raw.saturating_sub(1)).map(SnapKind::label)
}

pub fn accessibility_status() -> AccessibilityStatus {
    #[cfg(target_os = "macos")]
    {
        if is_trusted() {
            AccessibilityStatus::Granted
        } else {
            AccessibilityStatus::Denied
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        AccessibilityStatus::Unsupported
    }
}

pub fn is_trusted() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        AXIsProcessTrusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Show the system Accessibility prompt when the process is not trusted.
/// Returns the post-prompt trust flag (still false until the user toggles
/// the grant and the app is relaunched on some macOS versions).
pub fn prompt_trust() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        prompt_trust_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    {
        let _ = open::that(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        );
    }
}

/// Snap the frontmost app's focused window. No-op when the feature is off,
/// Accessibility is denied, or there is no movable window.
pub fn snap_frontmost(kind: SnapKind) -> Result<SnapRect, SnapError> {
    if !crate::settings::get_app_settings().window_snap_enabled {
        return Err(SnapError::Disabled);
    }
    #[cfg(target_os = "macos")]
    {
        if !is_trusted() {
            return Err(SnapError::NotTrusted);
        }
        let rect = unsafe { snap_frontmost_macos(kind) }?;
        note_flash(kind);
        Ok(rect)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = kind;
        Err(SnapError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> *mut std::ffi::c_void;
    fn AXUIElementCopyAttributeValue(
        element: *mut std::ffi::c_void,
        attribute: *const std::ffi::c_void,
        value: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: *mut std::ffi::c_void,
        attribute: *const std::ffi::c_void,
        value: *const std::ffi::c_void,
    ) -> i32;
    fn AXValueCreate(the_type: u32, value_ptr: *const std::ffi::c_void) -> *mut std::ffi::c_void;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const std::ffi::c_void);
    fn CFBooleanGetValue(boolean: *const std::ffi::c_void) -> bool;
    static kCFBooleanTrue: *const std::ffi::c_void;
    static kCFBooleanFalse: *const std::ffi::c_void;
}

#[cfg(target_os = "macos")]
const AX_VALUE_CGPOINT: u32 = 1;
#[cfg(target_os = "macos")]
const AX_VALUE_CGSIZE: u32 = 2;

#[cfg(target_os = "macos")]
unsafe fn prompt_trust_macos() -> bool {
    use objc2::runtime::AnyObject;
    use objc2::*;

    let key: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"AXTrustedCheckOptionPrompt".as_ptr()
    ];
    let yes: *mut AnyObject = msg_send![class!(NSNumber), numberWithBool: true];
    if key.is_null() || yes.is_null() {
        return AXIsProcessTrusted();
    }
    let options: *mut AnyObject = msg_send![class!(NSDictionary), dictionaryWithObject: yes, forKey: key];
    if options.is_null() {
        return AXIsProcessTrusted();
    }
    AXIsProcessTrustedWithOptions(options as *const std::ffi::c_void)
}

#[cfg(target_os = "macos")]
unsafe fn snap_frontmost_macos(kind: SnapKind) -> Result<SnapRect, SnapError> {
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2::*;

    autoreleasepool(|_| {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return Err(SnapError::NoWindow);
        }
        let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return Err(SnapError::NoWindow);
        }
        let pid: i32 = msg_send![app, processIdentifier];
        if pid <= 0 || pid == std::process::id() as i32 {
            return Err(SnapError::NoWindow);
        }

        let Some(visible) = visible_frame_under_cursor() else {
            return Err(SnapError::NoWindow);
        };
        let target = snap_rect(kind, visible);

        let app_el = AXUIElementCreateApplication(pid);
        if app_el.is_null() {
            return Err(SnapError::NoWindow);
        }

        let enhanced_attr = ns_attr(c"AXEnhancedUserInterface");
        let was_enhanced = ax_bool(app_el, enhanced_attr).unwrap_or(false);
        if was_enhanced {
            ax_set_bool(app_el, enhanced_attr, false);
        }

        let focused_attr = ns_attr(c"AXFocusedWindow");
        let mut window: *mut std::ffi::c_void = std::ptr::null_mut();
        let err = AXUIElementCopyAttributeValue(app_el, focused_attr, &mut window);
        if err != 0 || window.is_null() {
            if was_enhanced {
                ax_set_bool(app_el, enhanced_attr, true);
            }
            CFRelease(app_el);
            return Err(SnapError::NoWindow);
        }

        // Size → position → size. Some apps clamp the first size write until
        // the origin is inside the destination display.
        let _ = ax_set_size(window, target.w, target.h);
        let _ = ax_set_point(window, target.x, target.y);
        let applied = ax_set_size(window, target.w, target.h);

        if was_enhanced {
            ax_set_bool(app_el, enhanced_attr, true);
        }
        CFRelease(window);
        CFRelease(app_el);

        if applied {
            Ok(target)
        } else {
            Err(SnapError::NoWindow)
        }
    })
}

#[cfg(target_os = "macos")]
unsafe fn visible_frame_under_cursor() -> Option<SnapRect> {
    use crate::notch::{CGPoint, CGRect};
    use objc2::runtime::AnyObject;
    use objc2::*;

    let mouse: CGPoint = msg_send![class!(NSEvent), mouseLocation];
    let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
    if screens.is_null() {
        return None;
    }
    let count: usize = msg_send![screens, count];
    if count == 0 {
        return None;
    }
    let primary: *mut AnyObject = msg_send![screens, objectAtIndex: 0usize];
    if primary.is_null() {
        return None;
    }
    let primary_frame: CGRect = msg_send![primary, frame];
    let primary_h = primary_frame.size.height;

    let mut chosen: *mut AnyObject = std::ptr::null_mut();
    for i in 0..count {
        let screen: *mut AnyObject = msg_send![screens, objectAtIndex: i];
        if screen.is_null() {
            continue;
        }
        let frame: CGRect = msg_send![screen, frame];
        if mouse.x >= frame.origin.x
            && mouse.x < frame.origin.x + frame.size.width
            && mouse.y >= frame.origin.y
            && mouse.y < frame.origin.y + frame.size.height
        {
            chosen = screen;
            break;
        }
    }
    if chosen.is_null() {
        chosen = msg_send![class!(NSScreen), mainScreen];
    }
    if chosen.is_null() {
        return None;
    }
    let visible: CGRect = msg_send![chosen, visibleFrame];
    Some(ns_visible_to_ax(
        SnapRect {
            x: visible.origin.x,
            y: visible.origin.y,
            w: visible.size.width,
            h: visible.size.height,
        },
        primary_h,
    ))
}

#[cfg(target_os = "macos")]
unsafe fn ns_attr(name: &std::ffi::CStr) -> *const std::ffi::c_void {
    use objc2::runtime::AnyObject;
    use objc2::*;
    let s: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
    s as *const std::ffi::c_void
}

#[cfg(target_os = "macos")]
unsafe fn ax_bool(element: *mut std::ffi::c_void, attr: *const std::ffi::c_void) -> Option<bool> {
    let mut value: *mut std::ffi::c_void = std::ptr::null_mut();
    if AXUIElementCopyAttributeValue(element, attr, &mut value) != 0 || value.is_null() {
        return None;
    }
    let flag = CFBooleanGetValue(value);
    CFRelease(value);
    Some(flag)
}

#[cfg(target_os = "macos")]
unsafe fn ax_set_bool(element: *mut std::ffi::c_void, attr: *const std::ffi::c_void, value: bool) {
    let cf = if value { kCFBooleanTrue } else { kCFBooleanFalse };
    let _ = AXUIElementSetAttributeValue(element, attr, cf);
}

#[cfg(target_os = "macos")]
unsafe fn ax_set_point(element: *mut std::ffi::c_void, x: f64, y: f64) -> bool {
    use crate::notch::CGPoint;
    let point = CGPoint { x, y };
    let value = AXValueCreate(AX_VALUE_CGPOINT, &point as *const CGPoint as *const std::ffi::c_void);
    if value.is_null() {
        return false;
    }
    let ok = AXUIElementSetAttributeValue(element, ns_attr(c"AXPosition"), value) == 0;
    CFRelease(value);
    ok
}

#[cfg(target_os = "macos")]
unsafe fn ax_set_size(element: *mut std::ffi::c_void, w: f64, h: f64) -> bool {
    use crate::notch::CGSize;
    let size = CGSize {
        width: w,
        height: h,
    };
    let value = AXValueCreate(AX_VALUE_CGSIZE, &size as *const CGSize as *const std::ffi::c_void);
    if value.is_null() {
        return false;
    }
    let ok = AXUIElementSetAttributeValue(element, ns_attr(c"AXSize"), value) == 0;
    CFRelease(value);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible() -> SnapRect {
        SnapRect {
            x: 0.0,
            y: 38.0,
            w: 1512.0,
            h: 894.0,
        }
    }

    #[test]
    fn ns_visible_flips_to_ax_using_primary_height() {
        let ns = SnapRect {
            x: 0.0,
            y: 50.0,
            w: 1512.0,
            h: 894.0,
        };
        let ax = ns_visible_to_ax(ns, 982.0);
        assert_eq!(
            ax,
            SnapRect {
                x: 0.0,
                y: 38.0,
                w: 1512.0,
                h: 894.0,
            }
        );
    }

    #[test]
    fn ns_visible_on_a_right_display_keeps_x() {
        let ns = SnapRect {
            x: 1512.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        let ax = ns_visible_to_ax(ns, 982.0);
        assert_eq!(ax.x, 1512.0);
        assert!((ax.y - (982.0 - 1080.0)).abs() < f64::EPSILON);
        assert_eq!(ax.w, 1920.0);
        assert_eq!(ax.h, 1080.0);
    }

    #[test]
    fn halves_and_quarters_tile_the_visible_frame() {
        let v = visible();
        assert_eq!(
            snap_rect(SnapKind::LeftHalf, v),
            SnapRect {
                x: 0.0,
                y: 38.0,
                w: 756.0,
                h: 894.0,
            }
        );
        assert_eq!(
            snap_rect(SnapKind::RightHalf, v),
            SnapRect {
                x: 756.0,
                y: 38.0,
                w: 756.0,
                h: 894.0,
            }
        );
        assert_eq!(
            snap_rect(SnapKind::TopHalf, v),
            SnapRect {
                x: 0.0,
                y: 38.0,
                w: 1512.0,
                h: 447.0,
            }
        );
        assert_eq!(
            snap_rect(SnapKind::BottomHalf, v),
            SnapRect {
                x: 0.0,
                y: 485.0,
                w: 1512.0,
                h: 447.0,
            }
        );
        assert_eq!(
            snap_rect(SnapKind::TopLeft, v),
            SnapRect {
                x: 0.0,
                y: 38.0,
                w: 756.0,
                h: 447.0,
            }
        );
        assert_eq!(
            snap_rect(SnapKind::TopRight, v),
            SnapRect {
                x: 756.0,
                y: 38.0,
                w: 756.0,
                h: 447.0,
            }
        );
        assert_eq!(
            snap_rect(SnapKind::BottomLeft, v),
            SnapRect {
                x: 0.0,
                y: 485.0,
                w: 756.0,
                h: 447.0,
            }
        );
        assert_eq!(
            snap_rect(SnapKind::BottomRight, v),
            SnapRect {
                x: 756.0,
                y: 485.0,
                w: 756.0,
                h: 447.0,
            }
        );
        assert_eq!(snap_rect(SnapKind::Maximize, v), v);
    }

    #[test]
    fn odd_dimensions_give_the_remainder_to_the_far_side() {
        let v = SnapRect {
            x: 10.0,
            y: 20.0,
            w: 1513.0,
            h: 895.0,
        };
        let left = snap_rect(SnapKind::LeftHalf, v);
        let right = snap_rect(SnapKind::RightHalf, v);
        assert_eq!(left.w, 756.0);
        assert_eq!(right.w, 757.0);
        assert_eq!(left.x + left.w, right.x);
        assert_eq!(right.x + right.w, v.x + v.w);

        let top = snap_rect(SnapKind::TopHalf, v);
        let bottom = snap_rect(SnapKind::BottomHalf, v);
        assert_eq!(top.h, 447.0);
        assert_eq!(bottom.h, 448.0);
        assert_eq!(top.y + top.h, bottom.y);
        assert_eq!(bottom.y + bottom.h, v.y + v.h);
    }

    #[test]
    fn edge_zone_prefers_corners_then_sides() {
        let v = visible();
        assert_eq!(edge_zone(10.0, 400.0, v, 24.0), Some(SnapKind::LeftHalf));
        assert_eq!(edge_zone(1500.0, 400.0, v, 24.0), Some(SnapKind::RightHalf));
        assert_eq!(edge_zone(800.0, 40.0, v, 24.0), Some(SnapKind::TopHalf));
        assert_eq!(edge_zone(800.0, 920.0, v, 24.0), Some(SnapKind::BottomHalf));
        assert_eq!(edge_zone(10.0, 40.0, v, 24.0), Some(SnapKind::TopLeft));
        assert_eq!(edge_zone(1500.0, 40.0, v, 24.0), Some(SnapKind::TopRight));
        assert_eq!(edge_zone(10.0, 920.0, v, 24.0), Some(SnapKind::BottomLeft));
        assert_eq!(
            edge_zone(1500.0, 920.0, v, 24.0),
            Some(SnapKind::BottomRight)
        );
        assert_eq!(edge_zone(800.0, 400.0, v, 24.0), None);
        assert_eq!(edge_zone(-4.0, 400.0, v, 24.0), None);
        assert_eq!(edge_zone(800.0, 400.0, v, 0.0), None);
    }

    #[test]
    fn snap_kind_round_trips_and_labels() {
        for kind in SnapKind::ALL {
            assert_eq!(SnapKind::from_u8(kind as u8), Some(kind));
            assert!(!kind.label().is_empty());
        }
        assert_eq!(SnapKind::from_u8(99), None);
    }

    #[test]
    fn snap_is_disabled_by_default() {
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(accessibility_status(), AccessibilityStatus::Unsupported);
            assert!(!is_trusted());
        }
        assert_eq!(snap_frontmost(SnapKind::LeftHalf), Err(SnapError::Disabled));
    }
}
