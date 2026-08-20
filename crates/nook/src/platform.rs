//! Native window chrome GPUI does not expose: status-item level, accessory
//! activation policy, click-through, and pinning the overlay to the physical
//! top of the main display (into the camera notch).
//!
//! GPUI always creates even `titlebar: None` windows with `NSTitledWindowMask`.
//! AppKit then clamps them to `visibleFrame` (below the menu bar) via
//! `constrainFrameRect:toScreen:`. The original Tauri client was borderless
//! from the start, so it never hit that clamp. We:
//!   1. Install an identity `constrainFrameRect:` on `GPUIPanel` *before* the
//!      island window is created, so GPUI's own `setFrameTopLeftPoint` sticks.
//!   2. After creation, flip the panel to borderless and pin using `NSScreen.frame`
//!      (not `visibleFrame`), without touching GPUI's `Window` (that RefCell-panics).

use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[cfg(target_os = "macos")]
use nook_core::notch::{CGPoint, CGRect, CGSize};
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::Once;

#[cfg(target_os = "macos")]
static INSTALL: Once = Once::new();
#[cfg(target_os = "macos")]
static LOGGED_PIN: AtomicBool = AtomicBool::new(false);

/// Call once before `open_island`. Safe to call again.
pub fn install() {
    #[cfg(target_os = "macos")]
    install_macos();
}

#[cfg(target_os = "macos")]
fn install_macos() {
    INSTALL.call_once(|| unsafe {
        use objc2::ffi::{class_addMethod, class_replaceMethod};
        use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
        use objc2::sel;
        use std::ffi::CString;

        let Some(gpui_panel) = AnyClass::get(c"GPUIPanel") else {
            log::error!("GPUIPanel class missing — notch pin will fail");
            return;
        };

        // Must be `extern "C"` (not C-unwind): AppKit calls this from C, and a
        // Rust panic here aborts the process via didFinishLaunching.
        extern "C" fn constrain_frame(
            _this: *mut AnyObject,
            _cmd: Sel,
            frame: CGRect,
            _screen: *mut AnyObject,
        ) -> CGRect {
            frame
        }

        let types = CString::new(
            "{CGRect={CGPoint=dd}{CGSize=dd}}@:{CGRect={CGPoint=dd}{CGSize=dd}}@",
        )
        .unwrap();
        let sel = sel!(constrainFrameRect:toScreen:);
        let imp: Imp = std::mem::transmute(
            constrain_frame
                as extern "C" fn(*mut AnyObject, Sel, CGRect, *mut AnyObject) -> CGRect,
        );
        let cls_ptr = gpui_panel as *const AnyClass as *mut AnyClass;
        if !class_addMethod(cls_ptr, sel, imp, types.as_ptr()).as_bool() {
            class_replaceMethod(cls_ptr, sel, imp, types.as_ptr());
        }
        log::info!("notch constrainFrameRect installed on GPUIPanel");
    });
}

/// Style + pin every GPUI island panel. Does not touch GPUI `Window`.
#[cfg(target_os = "macos")]
pub fn apply_island_chrome() {
    #[cfg(target_os = "macos")]
    {
        install_macos();
        unsafe {
            set_accessory_policy();
            for_each_island_window(|ns_win| {
                style_island_window(ns_win);
                pin_ns_window(ns_win);
            });
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply_island_chrome() {}

/// Kept for the settings window path that still has a GPUI `Window`.
#[cfg(target_os = "macos")]
pub fn apply_island_chrome_on(window: &Window) {
    install_macos();
    unsafe {
        set_accessory_policy();
        if let Some(ns_win) = ns_window(window) {
            style_island_window(ns_win);
            pin_ns_window(ns_win);
        } else {
            apply_island_chrome();
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply_island_chrome_on(_window: &Window) {}

#[cfg(target_os = "macos")]
unsafe fn set_accessory_policy() {
    use objc2::runtime::AnyObject;
    use objc2::*;
    let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
    let _: bool = msg_send![ns_app, setActivationPolicy: 1_i64];
}

#[cfg(target_os = "macos")]
unsafe fn style_island_window(ns_win: *mut objc2::runtime::AnyObject) {
    use objc2::runtime::AnyObject;
    use objc2::*;

    // Do not object_setClass — GPUI looks up a windowState ivar on GPUIPanel.
    // constrainFrameRect is already installed on GPUIPanel itself.

    // Borderless | NonactivatingPanel | FullSizeContentView
    // (titled mask is why AppKit inset us below the menu bar)
    const BORDERLESS: u64 = 0;
    const NONACTIVATING_PANEL: u64 = 1 << 7;
    const FULL_SIZE_CONTENT: u64 = 1 << 15;
    let mask = BORDERLESS | NONACTIVATING_PANEL | FULL_SIZE_CONTENT;
    let _: () = msg_send![ns_win, setStyleMask: mask];
    let _: () = msg_send![ns_win, setLevel: 25_i64];
    // CanJoinAllSpaces | Stationary | FullScreenAuxiliary | IgnoresCycle
    let _: () = msg_send![ns_win, setCollectionBehavior: (1u64 | (1u64 << 4) | (1u64 << 8) | (1u64 << 6))];
    let _: () = msg_send![ns_win, setHasShadow: false];
    let _: () = msg_send![ns_win, setOpaque: false];
    let _: () = msg_send![ns_win, setMovable: false];
    let _: () = msg_send![ns_win, setHidesOnDeactivate: false];
    // NSWindowAnimationBehaviorNone — don't animate our pin
    let _: () = msg_send![ns_win, setAnimationBehavior: 2_i64];
    let title: *mut AnyObject =
        msg_send![class!(NSString), stringWithUTF8String: c"openNook-island".as_ptr()];
    let _: () = msg_send![ns_win, setTitle: title];
    let clear: *mut AnyObject = msg_send![class!(NSColor), clearColor];
    let _: () = msg_send![ns_win, setBackgroundColor: clear];
    // setStyleMask drops GPUI's registerForDraggedTypes; without it Finder
    // never delivers draggingEntered to GPUIPanel.
    register_file_drops(ns_win);
}

/// GPUI registers NSFilenamesPboardType on the window at creation. Restyle
/// (borderless / nonactivating) clears that list, so the island stops being
/// a drop target until we put it back.
#[cfg(target_os = "macos")]
unsafe fn register_file_drops(ns_win: *mut objc2::runtime::AnyObject) {
    use objc2::runtime::AnyObject;
    use objc2::*;

    // Same type GPUI's draggingEntered reads via propertyListForType.
    let filenames: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"NSFilenamesPboardType".as_ptr()
    ];
    let types: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: filenames];
    let _: () = msg_send![ns_win, registerForDraggedTypes: types];
}

#[cfg(target_os = "macos")]
fn pin_ns_window(ns_win: *mut objc2::runtime::AnyObject) {
    use objc2::runtime::AnyObject;
    use objc2::*;

    unsafe {
        let screen: *mut AnyObject = {
            let s: *mut AnyObject = msg_send![ns_win, screen];
            if s.is_null() {
                msg_send![class!(NSScreen), mainScreen]
            } else {
                s
            }
        };
        if screen.is_null() {
            return;
        }
        let frame: CGRect = msg_send![screen, frame];
        let visible: CGRect = msg_send![screen, visibleFrame];
        let (_, height) = nook_core::notch::overlay_window_size();
        // Cocoa origin is bottom-left. TOP of the window = TOP of NSScreen.frame
        // (the hardware notch), not visibleFrame (below the menu bar).
        let rect = CGRect {
            origin: CGPoint {
                x: frame.origin.x,
                y: frame.origin.y + frame.size.height - height,
            },
            size: CGSize {
                width: frame.size.width,
                height,
            },
        };

        let current: CGRect = msg_send![ns_win, frame];
        let moved = (current.origin.x - rect.origin.x).abs() > 0.5
            || (current.origin.y - rect.origin.y).abs() > 0.5
            || (current.size.width - rect.size.width).abs() > 0.5
            || (current.size.height - rect.size.height).abs() > 0.5;

        if moved {
            let _: () = msg_send![ns_win, setFrame: rect, display: true];
            let top_left = CGPoint {
                x: frame.origin.x,
                y: frame.origin.y + frame.size.height,
            };
            let _: () = msg_send![ns_win, setFrameTopLeftPoint: top_left];
            let _: () = msg_send![ns_win, setLevel: 25_i64];
        }

        // After dropping the titled mask, keep the content view flush with the
        // window so GPUI doesn't paint below a leftover titlebar inset.
        let content: *mut AnyObject = msg_send![ns_win, contentView];
        if !content.is_null() {
            let local = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: rect.size,
            };
            let _: () = msg_send![content, setFrame: local];
            let subviews: *mut AnyObject = msg_send![content, subviews];
            if !subviews.is_null() {
                let n: usize = msg_send![subviews, count];
                for i in 0..n {
                    let v: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
                    let _: () = msg_send![v, setFrame: local];
                }
            }
        }

        if !LOGGED_PIN.swap(true, Ordering::Relaxed) {
            let after: CGRect = msg_send![ns_win, frame];
            let cv: CGRect = if content.is_null() {
                CGRect {
                    origin: CGPoint { x: 0.0, y: 0.0 },
                    size: CGSize {
                        width: 0.0,
                        height: 0.0,
                    },
                }
            } else {
                msg_send![content, frame]
            };
            let mask: u64 = msg_send![ns_win, styleMask];
            let level: i64 = msg_send![ns_win, level];
            let name_ns: *mut AnyObject = msg_send![ns_win, className];
            let name_c: *const i8 = msg_send![name_ns, UTF8String];
            let name = if name_c.is_null() {
                std::borrow::Cow::Borrowed("?")
            } else {
                std::ffi::CStr::from_ptr(name_c).to_string_lossy()
            };
            log::info!(
                "pin {name} level={level} mask={mask:#x} screen=({:.0},{:.0} {:.0}×{:.0}) visible_top={:.0} notch_gap={:.0} before=({:.1},{:.1} {:.0}×{:.0}) after=({:.1},{:.1} {:.0}×{:.0}) content=({:.1},{:.1} {:.0}×{:.0}) window_top={:.1}",
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height,
                visible.origin.y + visible.size.height,
                frame.origin.y + frame.size.height - (visible.origin.y + visible.size.height),
                current.origin.x,
                current.origin.y,
                current.size.width,
                current.size.height,
                after.origin.x,
                after.origin.y,
                after.size.width,
                after.size.height,
                cv.origin.x,
                cv.origin.y,
                cv.size.width,
                cv.size.height,
                after.origin.y + after.size.height,
            );
        }
    }
}

/// Re-pin without touching GPUI's Window (safe from timers).
pub fn pin_island_windows() {
    #[cfg(target_os = "macos")]
    unsafe {
        install_macos();
        for_each_island_window(|w| {
            use objc2::*;
            let mask: u64 = msg_send![w, styleMask];
            let level: i64 = msg_send![w, level];
            // NSTitledWindowMask = 1. GPUI re-applies it; restyle when that happens.
            if (mask & 1) != 0 || level != 25 {
                style_island_window(w);
            }
            pin_ns_window(w);
        });
    }
}

#[cfg(target_os = "macos")]
unsafe fn for_each_island_window(mut f: impl FnMut(*mut objc2::runtime::AnyObject)) {
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::*;

    let Some(panel_cls) = AnyClass::get(c"GPUIPanel") else {
        return;
    };
    let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
    let windows: *mut AnyObject = msg_send![ns_app, windows];
    if windows.is_null() {
        return;
    }
    let count: usize = msg_send![windows, count];
    for i in 0..count {
        let w: *mut AnyObject = msg_send![windows, objectAtIndex: i];
        let is_panel: bool = msg_send![w, isKindOfClass: panel_cls];
        if is_panel {
            f(w);
        }
    }
}

pub fn set_click_through_current(ignore: bool) {
    #[cfg(target_os = "macos")]
    unsafe {
        for_each_island_window(|w| {
            use objc2::*;
            let _: () = msg_send![w, setIgnoresMouseEvents: ignore];
            if !ignore {
                register_file_drops(w);
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = ignore;
}

pub fn set_click_through(window: &Window, ignore: bool) {
    #[cfg(target_os = "macos")]
    unsafe {
        if let Some(ns_win) = ns_window(window) {
            use objc2::*;
            let _: () = msg_send![ns_win, setIgnoresMouseEvents: ignore];
            if !ignore {
                register_file_drops(ns_win);
            }
        } else {
            set_click_through_current(ignore);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, ignore);
}

pub fn activate_app() {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::*;
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
    }
}

pub fn set_accessory(accessory: bool) {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::*;
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let policy: i64 = if accessory { 1 } else { 0 };
        let _: bool = msg_send![ns_app, setActivationPolicy: policy];
    }
}

#[cfg(target_os = "macos")]
fn ns_window(window: &Window) -> Option<*mut objc2::runtime::AnyObject> {
    use objc2::runtime::AnyObject;
    use objc2::*;
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::AppKit(appkit) => unsafe {
            let ns_view = appkit.ns_view.as_ptr() as *mut AnyObject;
            let ns_win: *mut AnyObject = msg_send![ns_view, window];
            if ns_win.is_null() {
                None
            } else {
                Some(ns_win)
            }
        },
        _ => None,
    }
}
