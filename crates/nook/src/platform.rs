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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(target_os = "macos")]
use std::sync::Once;
#[cfg(target_os = "macos")]
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

#[cfg(target_os = "macos")]
static INSTALL: Once = Once::new();
#[cfg(target_os = "macos")]
static LOGGED_PIN: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static OPEN_SETTINGS: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static PIN_NEEDED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static STATUS_ITEM: std::sync::atomic::AtomicPtr<objc2::runtime::AnyObject> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Call once before `open_island`. Safe to call again.
pub fn install() {
    #[cfg(target_os = "macos")]
    {
        install_macos();
        install_mouse_monitors();
    }
}

/// Ask the pin backstop loop to restyle/pin island windows. Notification
/// handlers also pin immediately on the main thread.
pub fn request_pin() {
    #[cfg(target_os = "macos")]
    {
        PIN_NEEDED.store(true, Ordering::Release);
        let wait = pin_wait();
        if let Ok(mut ready) = wait.ready.lock() {
            *ready = true;
            wait.cv.notify_one();
        }
    }
}

pub fn take_pin_needed() -> bool {
    #[cfg(target_os = "macos")]
    {
        return PIN_NEEDED.swap(false, Ordering::AcqRel);
    }
    #[cfg(not(target_os = "macos"))]
    false
}

#[cfg(target_os = "macos")]
struct PinWait {
    ready: Mutex<bool>,
    cv: Condvar,
}

#[cfg(target_os = "macos")]
fn pin_wait() -> &'static PinWait {
    static WAIT: OnceLock<PinWait> = OnceLock::new();
    WAIT.get_or_init(|| PinWait {
        ready: Mutex::new(false),
        cv: Condvar::new(),
    })
}

/// Block until [`request_pin`] or `timeout`. Must run off the main thread.
pub fn wait_pin_needed(timeout: Duration) -> bool {
    #[cfg(target_os = "macos")]
    {
        if PIN_NEEDED.load(Ordering::Acquire) {
            return true;
        }
        let wait = pin_wait();
        let Ok(mut ready) = wait.ready.lock() else {
            std::thread::sleep(timeout);
            return PIN_NEEDED.load(Ordering::Acquire);
        };
        if *ready {
            *ready = false;
            return true;
        }
        let (mut ready, _) = wait
            .cv
            .wait_timeout(ready, timeout)
            .unwrap_or_else(|e| e.into_inner());
        let signaled = *ready;
        *ready = false;
        signaled || PIN_NEEDED.load(Ordering::Acquire)
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::thread::sleep(timeout);
        false
    }
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

        let types =
            CString::new("{CGRect={CGPoint=dd}{CGSize=dd}}@:{CGRect={CGPoint=dd}{CGSize=dd}}@")
                .unwrap();
        let sel = sel!(constrainFrameRect:toScreen:);
        let imp: Imp = std::mem::transmute(
            constrain_frame as extern "C" fn(*mut AnyObject, Sel, CGRect, *mut AnyObject) -> CGRect,
        );
        let cls_ptr = gpui_panel as *const AnyClass as *mut AnyClass;
        if !class_addMethod(cls_ptr, sel, imp, types.as_ptr()).as_bool() {
            class_replaceMethod(cls_ptr, sel, imp, types.as_ptr());
        }
        log::info!("notch constrainFrameRect installed on GPUIPanel");
        install_app_target();
    });
}

/// Style + pin every GPUI island panel. Does not touch GPUI `Window`.
#[cfg(target_os = "macos")]
pub fn apply_island_chrome() {
    install_macos();
    unsafe {
        set_accessory_policy();
        for_each_island_window(|ns_win| {
            style_island_window(ns_win);
            pin_ns_window(ns_win);
        });
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply_island_chrome() {}

/// Kept for the settings window path that still has a GPUI `Window`.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
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
    let _: () =
        msg_send![ns_win, setCollectionBehavior: (1u64 | (1u64 << 4) | (1u64 << 8) | (1u64 << 6))];
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
            // display:false — a forced synchronous draw here lands as a frame
            // hitch when the strip resizes around an island animation; GPUI
            // repaints on its own resize delegate a frame later.
            let _: () = msg_send![ns_win, setFrame: rect, display: false];
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
            let glass = glass_cap().lock().unwrap_or_else(|e| e.into_inner()).view;
            let subviews: *mut AnyObject = msg_send![content, subviews];
            if !subviews.is_null() {
                let n: usize = msg_send![subviews, count];
                for i in 0..n {
                    let v: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
                    if !glass.is_null() && v == glass {
                        continue;
                    }
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
        if island_glass_setting_on() {
            let last = glass_cap().lock().unwrap_or_else(|e| e.into_inner()).last;
            if let Some(spec) = last {
                let _ = apply_island_glass(spec);
            }
        } else {
            hide_island_glass();
        }
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

/// Re-register Finder drop types on the overlay. `setStyleMask` and
/// `ignoresMouseEvents` both drop the list, so a drag that starts while we
/// were click-through would otherwise never get `draggingEntered`.
pub fn register_current_file_drops() {
    #[cfg(target_os = "macos")]
    unsafe {
        for_each_island_window(|w| register_file_drops(w));
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

#[allow(dead_code)]
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

/// Hand activation back to whatever app sits underneath before a drag-out.
/// The island is a nonactivating panel, but the app can still be the active
/// one (Settings brings it forward, and it stays forward after that window
/// closes). While we hold activation the destination app never comes forward
/// for the drop, so the file lands only when the target happens to accept a
/// background drag — hence "sometimes". `setHidesOnDeactivate: false` in
/// `style_island_window` keeps the island painted through this.
pub fn resign_focus() {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::{AnyClass, AnyObject};
        use objc2::*;
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let active: bool = msg_send![ns_app, isActive];
        if !active {
            return;
        }
        // Never yank focus out of Settings (a Normal window, not a GPUIPanel)
        // while the user is typing in it.
        let key_win: *mut AnyObject = msg_send![ns_app, keyWindow];
        if !key_win.is_null() {
            if let Some(panel_cls) = AnyClass::get(c"GPUIPanel") {
                let is_panel: bool = msg_send![key_win, isKindOfClass: panel_cls];
                if !is_panel {
                    return;
                }
            }
        }
        let _: () = msg_send![ns_app, deactivate];
        log::debug!("resigned focus for drag-out");
    }
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

/// Start a real OS drag of `path` so Finder / other apps accept the file.
/// GPUI 0.2.2 only has in-app `on_drag`; this uses AppKit `NSDraggingSource`
/// the same way the Tauri `drag` plugin (`beginDraggingSession`) does.
pub fn start_file_drag(path: &str, window: Option<&Window>) {
    #[cfg(target_os = "macos")]
    unsafe {
        if !std::path::Path::new(path).exists() {
            log::warn!("drag-out skipped; missing {path}");
            return;
        }
        if let Some(ns_win) = window.and_then(ns_window) {
            begin_file_drag(ns_win, path);
            return;
        }
        for_each_island_window(|ns_win| {
            if window_title_is(ns_win, "openNook-island") {
                begin_file_drag(ns_win, path);
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, window);
        log::warn!("file drag-out is macOS-only (AppKit NSDraggingSource)");
    }
}

#[cfg(target_os = "macos")]
unsafe fn window_title_is(ns_win: *mut objc2::runtime::AnyObject, want: &str) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::*;
    let title: *mut AnyObject = msg_send![ns_win, title];
    if title.is_null() {
        return false;
    }
    let cstr: *const i8 = msg_send![title, UTF8String];
    if cstr.is_null() {
        return false;
    }
    std::ffi::CStr::from_ptr(cstr).to_string_lossy() == want
}

#[cfg(target_os = "macos")]
fn drag_source_class() -> &'static objc2::runtime::AnyClass {
    use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, ClassBuilder, NSObject, Sel};
    use objc2::{sel, ClassType};
    use std::sync::OnceLock;

    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        if let Some(existing) = AnyClass::get(c"NookFileDragSource") {
            return existing;
        }
        let mut builder = ClassBuilder::new(c"NookFileDragSource", NSObject::class())
            .expect("NookFileDragSource");
        if let Some(proto) = AnyProtocol::get(c"NSDraggingSource") {
            builder.add_protocol(proto);
        }

        // NSDraggingContextOutsideApplication = 0, WithinApplication = 1.
        // Finder is outside. Offer Copy|Move|Generic so the destination can
        // pick; returning None for 0 was rejecting every Finder drop.
        extern "C-unwind" fn source_mask(
            _this: &NSObject,
            _cmd: Sel,
            _session: *mut AnyObject,
            _context: usize,
        ) -> usize {
            1 | 4 | 16 // NSDragOperationCopy | Generic | Move
        }
        unsafe {
            builder.add_method(
                sel!(draggingSession:sourceOperationMaskForDraggingContext:),
                source_mask as extern "C-unwind" fn(_, _, _, _) -> _,
            );
        }

        extern "C-unwind" fn session_ended(
            _this: &NSObject,
            _cmd: Sel,
            _session: *mut AnyObject,
            _point: nook_core::notch::CGPoint,
            operation: usize,
        ) {
            let dropped = operation != 0;
            log::info!("drag-out ended dropped={dropped} op={operation}");
            nook_core::files::finish_outbound_drag(dropped);
        }
        unsafe {
            builder.add_method(
                sel!(draggingSession:endedAtPoint:operation:),
                session_ended as extern "C-unwind" fn(_, _, _, _, _),
            );
        }

        builder.register()
    })
}

#[cfg(target_os = "macos")]
unsafe fn begin_file_drag(ns_win: *mut objc2::runtime::AnyObject, path: &str) {
    use nook_core::notch::{CGPoint, CGRect, CGSize};
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::CString;

    let Ok(c_path) = CString::new(path) else {
        return;
    };
    let path_ns: *mut AnyObject =
        msg_send![class!(NSString), stringWithUTF8String: c_path.as_ptr()];
    if path_ns.is_null() {
        return;
    }

    let is_dir = std::path::Path::new(path).is_dir();
    let url: *mut AnyObject =
        msg_send![class!(NSURL), fileURLWithPath: path_ns, isDirectory: is_dir];
    if url.is_null() {
        log::error!("drag-out: NSURL failed for {path}");
        return;
    }

    let item: *mut AnyObject = msg_send![class!(NSDraggingItem), alloc];
    let item: *mut AnyObject = msg_send![item, initWithPasteboardWriter: url];
    if item.is_null() {
        log::error!("drag-out: NSDraggingItem alloc failed");
        return;
    }

    let loc: CGPoint = msg_send![ns_win, mouseLocationOutsideOfEventStream];
    let icon_size = 32.0;
    let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
    let icon: *mut AnyObject = if workspace.is_null() {
        std::ptr::null_mut()
    } else {
        msg_send![workspace, iconForFile: path_ns]
    };
    if !icon.is_null() {
        let size = CGSize {
            width: icon_size,
            height: icon_size,
        };
        let _: () = msg_send![icon, setSize: size];
    }
    let frame = CGRect {
        origin: CGPoint {
            x: loc.x - icon_size / 2.0,
            y: loc.y - icon_size / 2.0,
        },
        size: CGSize {
            width: icon_size,
            height: icon_size,
        },
    };
    let _: () = msg_send![item, setDraggingFrame: frame, contents: icon];

    let items: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: item];
    let content: *mut AnyObject = msg_send![ns_win, contentView];
    if content.is_null() {
        log::error!("drag-out: contentView missing");
        return;
    }

    // AppKit wants a mouse-dragged event. Synthesize one, matching drag-rs /
    // tauri-plugin-drag — the same call the React tray uses.
    const LEFT_MOUSE_DRAGGED: u64 = 6;
    let window_number: i64 = msg_send![ns_win, windowNumber];
    let event: *mut AnyObject = msg_send![
        class!(NSEvent),
        mouseEventWithType: LEFT_MOUSE_DRAGGED,
        location: loc,
        modifierFlags: 0_u64,
        timestamp: 0.0_f64,
        windowNumber: window_number,
        context: std::ptr::null_mut::<AnyObject>(),
        eventNumber: 0_i64,
        clickCount: 1_i64,
        pressure: 1.0_f32
    ];
    if event.is_null() {
        log::error!("drag-out: failed to synthesize NSEvent");
        return;
    }

    let cls = drag_source_class();
    let source: *mut AnyObject = msg_send![cls, alloc];
    let source: *mut AnyObject = msg_send![source, init];
    if source.is_null() {
        log::error!("drag-out: drag source alloc failed");
        return;
    }

    nook_core::files::begin_outbound_drag(path);
    let session: *mut AnyObject = msg_send![
        content,
        beginDraggingSessionWithItems: items,
        event: event,
        source: source
    ];
    if session.is_null() {
        nook_core::files::finish_outbound_drag(false);
        log::error!("drag-out: beginDraggingSession returned nil for {path}");
        return;
    }
    log::info!("drag-out started {path}");
    // Only after the session exists — deactivating first would pull the
    // synthesized event's window out from under beginDraggingSession.
    resign_focus();
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

/// True once if the status item asked to open Settings.
pub fn take_open_settings() -> bool {
    #[cfg(target_os = "macos")]
    {
        OPEN_SETTINGS.swap(false, Ordering::SeqCst)
    }
    #[cfg(not(target_os = "macos"))]
    false
}

/// Menu-bar extra with Settings and Quit. Accessory apps have no Dock/menu otherwise.
pub fn install_status_item() {
    #[cfg(target_os = "macos")]
    unsafe {
        install_macos();
        use objc2::runtime::AnyObject;
        use objc2::*;

        if !STATUS_ITEM.load(Ordering::Relaxed).is_null() {
            return;
        }

        let bar: *mut AnyObject = msg_send![class!(NSStatusBar), systemStatusBar];
        // NSVariableStatusItemLength = -1
        let item: *mut AnyObject = msg_send![bar, statusItemWithLength: -1.0_f64];
        if item.is_null() {
            log::error!("failed to create NSStatusItem");
            return;
        }
        let _: *mut AnyObject = msg_send![item, retain];
        STATUS_ITEM.store(item, Ordering::Relaxed);

        let title: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"Nook".as_ptr()];
        let button: *mut AnyObject = msg_send![item, button];
        if !button.is_null() {
            let _: () = msg_send![button, setTitle: title];
        } else {
            let _: () = msg_send![item, setTitle: title];
        }

        let menu: *mut AnyObject = msg_send![class!(NSMenu), new];
        let settings_title: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"Settings".as_ptr()];
        let settings_item: *mut AnyObject = msg_send![class!(NSMenuItem), alloc];
        let empty: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c"".as_ptr()];
        let settings_item: *mut AnyObject = msg_send![
            settings_item,
            initWithTitle: settings_title,
            action: sel!(openSettings:),
            keyEquivalent: empty
        ];
        let target_cls = objc2::runtime::AnyClass::get(c"NookAppTarget");
        let target: *mut AnyObject = if let Some(cls) = target_cls {
            msg_send![cls, new]
        } else {
            std::ptr::null_mut()
        };
        if !target.is_null() {
            let _: () = msg_send![settings_item, setTarget: target];
        }
        let _: () = msg_send![menu, addItem: settings_item];

        let sep: *mut AnyObject = msg_send![class!(NSMenuItem), separatorItem];
        let _: () = msg_send![menu, addItem: sep];

        let quit_title: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"Quit openNook".as_ptr()];
        let quit_item: *mut AnyObject = msg_send![class!(NSMenuItem), alloc];
        let q: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c"q".as_ptr()];
        let quit_item: *mut AnyObject = msg_send![
            quit_item,
            initWithTitle: quit_title,
            action: sel!(terminate:),
            keyEquivalent: q
        ];
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![quit_item, setTarget: ns_app];
        let _: () = msg_send![menu, addItem: quit_item];

        let _: () = msg_send![item, setMenu: menu];
        log::info!("status item installed");
    }
}

#[cfg(target_os = "macos")]
fn install_app_target() {
    use objc2::ffi::{class_addMethod, objc_allocateClassPair, objc_registerClassPair};
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{class, sel};
    use std::ffi::CString;

    unsafe {
        if AnyClass::get(c"NookAppTarget").is_some() {
            observe_screen_changes();
            return;
        }

        extern "C" fn open_settings(_this: *mut AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
            OPEN_SETTINGS.store(true, Ordering::SeqCst);
        }
        extern "C" fn screen_changed(_this: *mut AnyObject, _cmd: Sel, _note: *mut AnyObject) {
            nook_core::notch::invalidate_screen_cache();
            request_pin();
            pin_island_windows();
        }
        extern "C" fn space_changed(_this: *mut AnyObject, _cmd: Sel, _note: *mut AnyObject) {
            nook_core::notch::invalidate_screen_cache();
            request_pin();
            pin_island_windows();
        }
        extern "C" fn app_activated(_this: *mut AnyObject, _cmd: Sel, _note: *mut AnyObject) {
            nook_core::occupancy::invalidate();
        }
        extern "C" fn accessibility_changed(
            _this: *mut AnyObject,
            _cmd: Sel,
            _note: *mut AnyObject,
        ) {
            invalidate_accessibility_flags();
        }
        extern "C" fn colors_changed(_this: *mut AnyObject, _cmd: Sel, _note: *mut AnyObject) {
            invalidate_accent_color();
        }

        let super_cls = class!(NSObject) as *const AnyClass as *mut AnyClass;
        let cls = objc_allocateClassPair(super_cls, c"NookAppTarget".as_ptr(), 0);
        if cls.is_null() {
            log::error!("could not allocate NookAppTarget");
            return;
        }
        let types = CString::new("v@:@").unwrap();
        let imp_settings: Imp = std::mem::transmute(
            open_settings as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
        let imp_screen: Imp = std::mem::transmute(
            screen_changed as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
        let imp_space: Imp = std::mem::transmute(
            space_changed as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
        let imp_app: Imp = std::mem::transmute(
            app_activated as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
        let imp_a11y: Imp = std::mem::transmute(
            accessibility_changed as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
        let imp_colors: Imp = std::mem::transmute(
            colors_changed as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
        let _ = class_addMethod(cls, sel!(openSettings:), imp_settings, types.as_ptr());
        let _ = class_addMethod(cls, sel!(screenChanged:), imp_screen, types.as_ptr());
        let _ = class_addMethod(cls, sel!(spaceChanged:), imp_space, types.as_ptr());
        let _ = class_addMethod(cls, sel!(appActivated:), imp_app, types.as_ptr());
        let _ = class_addMethod(cls, sel!(accessibilityChanged:), imp_a11y, types.as_ptr());
        let _ = class_addMethod(cls, sel!(colorsChanged:), imp_colors, types.as_ptr());
        objc_registerClassPair(cls);
        observe_screen_changes();
    }
}

#[cfg(target_os = "macos")]
unsafe fn observe_screen_changes() {
    use objc2::runtime::AnyObject;
    use objc2::*;

    let Some(cls) = objc2::runtime::AnyClass::get(c"NookAppTarget") else {
        return;
    };
    let target: *mut AnyObject = msg_send![cls, new];
    if target.is_null() {
        return;
    }
    let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
    let name: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"NSApplicationDidChangeScreenParametersNotification".as_ptr()
    ];
    let _: () = msg_send![
        center,
        addObserver: target,
        selector: sel!(screenChanged:),
        name: name,
        object: std::ptr::null_mut::<AnyObject>()
    ];

    let colors_name: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"NSSystemColorsDidChangeNotification".as_ptr()
    ];
    let _: () = msg_send![
        center,
        addObserver: target,
        selector: sel!(colorsChanged:),
        name: colors_name,
        object: std::ptr::null_mut::<AnyObject>()
    ];
    let appearance_name: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"NSApplicationDidChangeEffectiveAppearanceNotification".as_ptr()
    ];
    let _: () = msg_send![
        center,
        addObserver: target,
        selector: sel!(colorsChanged:),
        name: appearance_name,
        object: std::ptr::null_mut::<AnyObject>()
    ];

    let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
    if workspace.is_null() {
        return;
    }
    let wsnc: *mut AnyObject = msg_send![workspace, notificationCenter];
    if wsnc.is_null() {
        return;
    }
    let space_name: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"NSWorkspaceActiveSpaceDidChangeNotification".as_ptr()
    ];
    let _: () = msg_send![
        wsnc,
        addObserver: target,
        selector: sel!(spaceChanged:),
        name: space_name,
        object: std::ptr::null_mut::<AnyObject>()
    ];
    let app_name: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"NSWorkspaceDidActivateApplicationNotification".as_ptr()
    ];
    let _: () = msg_send![
        wsnc,
        addObserver: target,
        selector: sel!(appActivated:),
        name: app_name,
        object: std::ptr::null_mut::<AnyObject>()
    ];
    let a11y_name: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification"
            .as_ptr()
    ];
    let _: () = msg_send![
        wsnc,
        addObserver: target,
        selector: sel!(accessibilityChanged:),
        name: a11y_name,
        object: std::ptr::null_mut::<AnyObject>()
    ];
}

/// NSEvent global + local monitors for mouse moved / left-drag / left-up.
/// Main thread only. Handlers write the same atomics as the 250 ms backstop
/// poll in `nook_core::mouse`. Local handler must return the event.
pub fn install_mouse_monitors() {
    #[cfg(target_os = "macos")]
    unsafe {
        use block2::RcBlock;
        use objc2::runtime::AnyObject;
        use objc2::*;

        static INSTALLED: Once = Once::new();
        INSTALLED.call_once(|| {
            // MouseMoved | LeftMouseDragged | LeftMouseUp
            const MASK: u64 = (1 << 5) | (1 << 6) | (1 << 2);

            let global = RcBlock::new(move |_event: *mut AnyObject| {
                nook_core::mouse::sample_now();
            });
            let g: *mut AnyObject = msg_send![
                class!(NSEvent),
                addGlobalMonitorForEventsMatchingMask: MASK,
                handler: &*global
            ];
            std::mem::forget(global);
            let _ = g;

            let local = RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
                nook_core::mouse::sample_now();
                event
            });
            let l: *mut AnyObject = msg_send![
                class!(NSEvent),
                addLocalMonitorForEventsMatchingMask: MASK,
                handler: &*local
            ];
            std::mem::forget(local);
            let _ = l;
        });
    }
}

/// Hand files to the system AirDrop picker (`NSSharingServiceNameSendViaAirDrop`).
pub fn share_via_airdrop(paths: &[std::path::PathBuf]) {
    #[cfg(target_os = "macos")]
    unsafe {
        share_via_airdrop_macos(paths);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = paths;
        log::warn!("AirDrop is macOS-only");
    }
}

#[cfg(target_os = "macos")]
unsafe fn share_via_airdrop_macos(paths: &[std::path::PathBuf]) {
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::CString;

    if paths.is_empty() {
        return;
    }
    set_accessory(false);
    activate_app();
    let name: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"com.apple.share.AirDrop.send".as_ptr()
    ];
    let service: *mut AnyObject = msg_send![class!(NSSharingService), sharingServiceNamed: name];
    if service.is_null() {
        log::warn!("AirDrop sharing service missing");
        return;
    }
    let items: *mut AnyObject = msg_send![class!(NSMutableArray), array];
    for path in paths {
        let Ok(cstr) = CString::new(path.to_string_lossy().as_bytes()) else {
            continue;
        };
        let ns: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: cstr.as_ptr()];
        let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: ns];
        if !url.is_null() {
            let _: () = msg_send![items, addObject: url];
        }
    }
    let can: bool = msg_send![service, canPerformWithItems: items];
    if !can {
        log::warn!("AirDrop cannot send these items");
        return;
    }
    let _: () = msg_send![service, performWithItems: items];
}

/// Start a live FaceTime-camera stream for the Mirror circle.
pub fn start_mirror() -> bool {
    #[cfg(target_os = "macos")]
    {
        start_mirror_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        log::warn!("Mirror camera is macOS-only");
        false
    }
}

pub fn stop_mirror() {
    #[cfg(target_os = "macos")]
    stop_mirror_macos();
}

/// Pixel edge of the square BGRA frame `mirror_frame` returns.
/// 2× `theme::MIRROR_FACE` so the circle stays sharp on retina.
pub const MIRROR_SIZE: u32 = 224;

/// Latest square BGRA8 frame from the Mirror camera, if `gen` is newer than `seen`.
pub fn mirror_frame(seen: u64) -> Option<(u64, Vec<u8>)> {
    #[cfg(target_os = "macos")]
    {
        let cap = MIRROR
            .get_or_init(default_mirror)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if cap.running && cap.gen != seen {
            if let Some(bgra) = cap.bgra.as_ref() {
                return Some((cap.gen, bgra.clone()));
            }
        }
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = seen;
        None
    }
}

/// PNG of the main display's current desktop wallpaper.
///
/// AppKit performs the source decode, so this also works for macOS wallpaper
/// formats (such as HEIC) that GPUI's cross-platform image decoder may not
/// understand directly.
pub fn desktop_wallpaper_png() -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    unsafe {
        desktop_wallpaper_png_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
unsafe fn desktop_wallpaper_png_macos() -> Option<Vec<u8>> {
    desktop_wallpaper_window_png_macos().or_else(|| desktop_wallpaper_url_png_macos())
}

/// Modern macOS wallpapers can be rendered by an extension (dynamic, video, or
/// shader based). In that case `desktopImageURLForScreen:` returns the generic
/// `DefaultDesktop.heic`, not what the user sees. The Dock owns one wallpaper
/// window per display, so capture its already-rendered frame instead.
#[cfg(target_os = "macos")]
unsafe fn desktop_wallpaper_window_png_macos() -> Option<Vec<u8>> {
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2::*;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *mut AnyObject;
        fn CGWindowListCreateImage(
            screen_bounds: CGRect,
            list_option: u32,
            window_id: u32,
            image_option: u32,
        ) -> CGImageRef;
        fn CGImageRelease(image: CGImageRef);
    }

    const OPTION_ALL: u32 = 0;
    const OPTION_INCLUDING_WINDOW: u32 = 1 << 3;
    const IMAGE_BOUNDS_IGNORE_FRAMING: u32 = 1 << 0;
    const IMAGE_BEST_RESOLUTION: u32 = 1 << 3;

    autoreleasepool(|_| {
        let windows = CGWindowListCopyWindowInfo(OPTION_ALL, 0);
        if windows.is_null() {
            return None;
        }
        let owner_key: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: c"kCGWindowOwnerName".as_ptr()
        ];
        let name_key: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: c"kCGWindowName".as_ptr()
        ];
        let number_key: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: c"kCGWindowNumber".as_ptr()
        ];
        let dock: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"Dock".as_ptr()];
        let wallpaper: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: c"Wallpaper".as_ptr()
        ];

        let count: usize = msg_send![windows, count];
        let mut wallpaper_window = None;
        for index in 0..count {
            let info: *mut AnyObject = msg_send![windows, objectAtIndex: index];
            let owner: *mut AnyObject = msg_send![info, objectForKey: owner_key];
            let name: *mut AnyObject = msg_send![info, objectForKey: name_key];
            if owner.is_null() || name.is_null() {
                continue;
            }
            let is_dock: bool = msg_send![owner, isEqualToString: dock];
            let is_wallpaper: bool = msg_send![name, hasPrefix: wallpaper];
            if is_dock && is_wallpaper {
                let number: *mut AnyObject = msg_send![info, objectForKey: number_key];
                if !number.is_null() {
                    wallpaper_window = Some(msg_send![number, unsignedIntValue]);
                    break;
                }
            }
        }
        let _: () = msg_send![windows, release];

        let window_id = wallpaper_window?;
        let image = CGWindowListCreateImage(
            CGRect {
                origin: CGPoint {
                    x: f64::INFINITY,
                    y: f64::INFINITY,
                },
                size: CGSize {
                    width: 0.0,
                    height: 0.0,
                },
            },
            OPTION_INCLUDING_WINDOW,
            window_id,
            IMAGE_BOUNDS_IGNORE_FRAMING | IMAGE_BEST_RESOLUTION,
        );
        if image.0.is_null() {
            return None;
        }
        let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), alloc];
        let rep: *mut AnyObject = msg_send![rep, initWithCGImage: image];
        CGImageRelease(image);
        if rep.is_null() {
            return None;
        }
        let png = bitmap_rep_png(rep);
        let _: () = msg_send![rep, release];
        png
    })
}

#[cfg(target_os = "macos")]
unsafe fn desktop_wallpaper_url_png_macos() -> Option<Vec<u8>> {
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2::*;

    autoreleasepool(|_| {
        let screen: *mut AnyObject = msg_send![class!(NSScreen), mainScreen];
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if screen.is_null() || workspace.is_null() {
            return None;
        }

        let url: *mut AnyObject = msg_send![workspace, desktopImageURLForScreen: screen];
        if url.is_null() {
            return None;
        }
        let image: *mut AnyObject = msg_send![class!(NSImage), alloc];
        let image: *mut AnyObject = msg_send![image, initWithContentsOfURL: url];
        if image.is_null() {
            return None;
        }
        let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
        let _: () = msg_send![image, release];
        if tiff.is_null() {
            return None;
        }
        let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
        bitmap_rep_png(rep)
    })
}

#[cfg(target_os = "macos")]
unsafe fn bitmap_rep_png(rep: *mut objc2::runtime::AnyObject) -> Option<Vec<u8>> {
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::c_void;

    const PNG_TYPE: usize = 4; // NSBitmapImageFileTypePNG
    const MAX_PNG: usize = 64 * 1024 * 1024;

    if rep.is_null() {
        return None;
    }
    let props: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
    let png: *mut AnyObject = msg_send![rep, representationUsingType: PNG_TYPE, properties: props];
    if png.is_null() {
        return None;
    }
    let len: usize = msg_send![png, length];
    if len == 0 || len > MAX_PNG {
        log::warn!("desktop wallpaper: PNG length {len}");
        return None;
    }
    let mut bytes = vec![0u8; len];
    let dst: *mut c_void = bytes.as_mut_ptr().cast();
    let _: () = msg_send![png, getBytes: dst, length: len];
    Some(bytes)
}

/// PNG of the running media app's icon, from its bundle via NSWorkspace.
pub fn app_icon_png(bundle_id: Option<&str>, app_name: Option<&str>) -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    unsafe {
        app_icon_png_macos(bundle_id, app_name)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (bundle_id, app_name);
        None
    }
}

#[cfg(target_os = "macos")]
fn known_bundle_id(app_name: &str) -> Option<&'static str> {
    Some(match app_name {
        "Spotify" => "com.spotify.client",
        "Music" | "iTunes" => "com.apple.Music",
        "Safari" => "com.apple.Safari",
        "Chrome" | "Google Chrome" => "com.google.Chrome",
        "Brave" | "Brave Browser" => "com.brave.Browser",
        "Edge" | "Microsoft Edge" => "com.microsoft.edgemac",
        "Firefox" => "org.mozilla.firefox",
        "TV" => "com.apple.TV",
        "Podcasts" => "com.apple.podcasts",
        "IINA" => "com.colliderli.iina",
        "VLC" => "org.videolan.vlc",
        "TIDAL" => "com.tidal.desktop",
        "Arc" => "company.thebrowser.Browser",
        _ => return None,
    })
}

/// `CGImageRef` encodes as `^{CGImage=}`; a raw `*const c_void` is `^v`
/// and panics objc2's encoding check.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(transparent)]
struct CGImageRef(*const std::ffi::c_void);

#[cfg(target_os = "macos")]
unsafe impl objc2::Encode for CGImageRef {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Pointer(&objc2::Encoding::Struct("CGImage", &[]));
}

#[cfg(target_os = "macos")]
#[repr(transparent)]
struct NSRectPtr(*mut nook_core::notch::CGRect);

#[cfg(target_os = "macos")]
unsafe impl objc2::Encode for NSRectPtr {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Pointer(&<nook_core::notch::CGRect as objc2::Encode>::ENCODING);
}

#[cfg(target_os = "macos")]
unsafe fn app_icon_png_macos(bundle_id: Option<&str>, app_name: Option<&str>) -> Option<Vec<u8>> {
    use nook_core::notch::{CGPoint, CGRect, CGSize};
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::{c_void, CString};

    // App icons are multi-rep ICNS. `TIFFRepresentation` dumps every size
    // (tens of MB) and our 2 MB cap discarded them, so the media badge
    // always fell back to a generic glyph.
    const ICON_PT: f64 = 128.0;
    const PNG_TYPE: usize = 4; // NSBitmapImageFileTypePNG
    const MAX_PNG: usize = 512 * 1024;

    autoreleasepool(|_| {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return None;
        }

        let mut path: *mut AnyObject = std::ptr::null_mut();
        let bundle = bundle_id
            .filter(|s| !s.is_empty())
            .or_else(|| app_name.and_then(known_bundle_id));
        if let Some(id) = bundle {
            if let Ok(cstr) = CString::new(id) {
                let ns: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: cstr.as_ptr()];
                let url: *mut AnyObject = msg_send![ws, URLForApplicationWithBundleIdentifier: ns];
                if !url.is_null() {
                    path = msg_send![url, path];
                }
            }
        }
        if path.is_null() {
            if let Some(name) = app_name.filter(|s| !s.is_empty()) {
                if let Ok(cstr) = CString::new(name) {
                    let ns: *mut AnyObject =
                        msg_send![class!(NSString), stringWithUTF8String: cstr.as_ptr()];
                    path = msg_send![ws, fullPathForApplication: ns];
                }
            }
        }
        if path.is_null() {
            return None;
        }

        let icon: *mut AnyObject = msg_send![ws, iconForFile: path];
        if icon.is_null() {
            log::warn!("app icon: iconForFile returned nil");
            return None;
        }
        let size = CGSize {
            width: ICON_PT,
            height: ICON_PT,
        };
        let _: () = msg_send![icon, setSize: size];
        let mut rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size,
        };
        let null: *mut AnyObject = std::ptr::null_mut();
        let cg: CGImageRef = msg_send![
            icon,
            CGImageForProposedRect: NSRectPtr(&mut rect),
            context: null,
            hints: null
        ];
        if cg.0.is_null() {
            log::warn!("app icon: CGImageForProposedRect nil");
            return None;
        }
        let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), alloc];
        let rep: *mut AnyObject = msg_send![rep, initWithCGImage: cg];
        if rep.is_null() {
            log::warn!("app icon: NSBitmapImageRep initWithCGImage nil");
            return None;
        }
        let props: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        let png: *mut AnyObject =
            msg_send![rep, representationUsingType: PNG_TYPE, properties: props];
        let _: () = msg_send![rep, release];
        if png.is_null() {
            log::warn!("app icon: PNG representation nil");
            return None;
        }
        let len: usize = msg_send![png, length];
        if len == 0 || len > MAX_PNG {
            log::warn!("app icon: PNG length {len}");
            return None;
        }
        let mut bytes = vec![0u8; len];
        let dst: *mut c_void = bytes.as_mut_ptr().cast();
        let _: () = msg_send![png, getBytes: dst, length: len];
        log::debug!("app icon loaded {} bytes", bytes.len());
        Some(bytes)
    })
}

/// Directory picker for the Obsidian vault. Blocks on the main thread via
/// `NSOpenPanel.runModal`. `None` when cancelled or off macOS.
pub fn choose_directory() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        choose_directory_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn choose_directory_macos() -> Option<std::path::PathBuf> {
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::CStr;
    use std::path::PathBuf;

    unsafe {
        let panel: *mut AnyObject = msg_send![class!(NSOpenPanel), openPanel];
        if panel.is_null() {
            return None;
        }
        let _: () = msg_send![panel, setCanChooseFiles: false];
        let _: () = msg_send![panel, setCanChooseDirectories: true];
        let _: () = msg_send![panel, setAllowsMultipleSelection: false];
        let _: () = msg_send![panel, setCanCreateDirectories: true];
        let message: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"Choose Obsidian Vault".as_ptr()];
        let _: () = msg_send![panel, setMessage: message];
        let response: i64 = msg_send![panel, runModal];
        // NSModalResponseOK == 1
        if response != 1 {
            return None;
        }
        let url: *mut AnyObject = msg_send![panel, URL];
        if url.is_null() {
            return None;
        }
        let path_ns: *mut AnyObject = msg_send![url, path];
        if path_ns.is_null() {
            return None;
        }
        let cstr: *const i8 = msg_send![path_ns, UTF8String];
        if cstr.is_null() {
            return None;
        }
        Some(PathBuf::from(CStr::from_ptr(cstr).to_string_lossy().into_owned()))
    }
}

/// Open the system Calendar app. No-op off macOS.
pub fn open_calendar() {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-a")
            .arg("Calendar")
            .spawn();
    }
}

#[cfg(target_os = "macos")]
static ACCENT_VALID: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static ACCENT_R: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[cfg(target_os = "macos")]
static ACCENT_G: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[cfg(target_os = "macos")]
static ACCENT_B: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(target_os = "macos")]
fn invalidate_accent_color() {
    ACCENT_VALID.store(false, Ordering::Relaxed);
}

/// System Settings → Appearance → *Accent color* (`NSColor.controlAccentColor`),
/// as sRGB components in 0..=1. `None` when there is no AppKit to ask or the
/// color cannot be converted to an RGB space — callers fall back to systemBlue.
///
/// Cached until `NSSystemColorsDidChangeNotification` so idle frames do not
/// walk CoreUI catalog colors. Must be called on the main thread for the
/// first resolve.
pub fn accent_color() -> Option<(f32, f32, f32)> {
    #[cfg(target_os = "macos")]
    {
        if ACCENT_VALID.load(Ordering::Relaxed) {
            return Some((
                f32::from_bits(ACCENT_R.load(Ordering::Relaxed)),
                f32::from_bits(ACCENT_G.load(Ordering::Relaxed)),
                f32::from_bits(ACCENT_B.load(Ordering::Relaxed)),
            ));
        }
        let color = accent_color_macos()?;
        ACCENT_R.store(color.0.to_bits(), Ordering::Relaxed);
        ACCENT_G.store(color.1.to_bits(), Ordering::Relaxed);
        ACCENT_B.store(color.2.to_bits(), Ordering::Relaxed);
        ACCENT_VALID.store(true, Ordering::Relaxed);
        Some(color)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn accent_color_macos() -> Option<(f32, f32, f32)> {
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2::*;

    // `colorUsingColorSpace:` returns an autoreleased color; without a pool it
    // would pile up for as long as the frame's pool lives.
    autoreleasepool(|_| unsafe {
        let color: *mut AnyObject = msg_send![class!(NSColor), controlAccentColor];
        if color.is_null() {
            return None;
        }
        let space: *mut AnyObject = msg_send![class!(NSColorSpace), sRGBColorSpace];
        if space.is_null() {
            return None;
        }
        // `getRed:…` throws on a catalog/pattern color, so convert first and
        // bail if AppKit says the conversion is impossible.
        let srgb: *mut AnyObject = msg_send![color, colorUsingColorSpace: space];
        if srgb.is_null() {
            return None;
        }
        let (mut r, mut g, mut b, mut a) = (0f64, 0f64, 0f64, 0f64);
        let _: () = msg_send![
            srgb,
            getRed: &mut r as *mut f64,
            green: &mut g as *mut f64,
            blue: &mut b as *mut f64,
            alpha: &mut a as *mut f64
        ];
        Some((r as f32, g as f32, b as f32))
    })
}

#[cfg(target_os = "macos")]
static REDUCE_TRANSPARENCY_CACHE: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "macos")]
static REDUCE_MOTION_CACHE: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
fn invalidate_accessibility_flags() {
    REDUCE_TRANSPARENCY_CACHE.store(0, Ordering::Relaxed);
    REDUCE_MOTION_CACHE.store(0, Ordering::Relaxed);
}

/// 1s-TTL cache for an AppKit accessibility flag. Both flags below are read
/// from the 20ms island tick; two ObjC round-trips per tick is pointless for
/// settings a human toggles at most a few times a day. The workspace
/// accessibility-display observer also invalidates these so a toggle applies
/// on the next tick. Layout: bit0 = value, bit1 = valid, upper bits = last-read
/// unix ms.
#[cfg(target_os = "macos")]
fn cached_accessibility_flag(cache: &AtomicU64, read: unsafe fn() -> bool) -> bool {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let packed = cache.load(Ordering::Relaxed);
    if packed & 0b10 != 0 && now_ms.saturating_sub(packed >> 2) < 1000 {
        return packed & 1 != 0;
    }
    let value = unsafe { read() };
    cache.store((now_ms << 2) | 0b10 | value as u64, Ordering::Relaxed);
    value
}

/// Accessibility › Display › "Reduce transparency". HIG › Materials requires
/// materials to respond to it, so the island drops its translucency when it is
/// on. `false` when there is no AppKit to ask. Cached for 1s.
pub fn reduce_transparency() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe fn read() -> bool {
            use objc2::runtime::AnyObject;
            use objc2::*;
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return false;
            }
            msg_send![workspace, accessibilityDisplayShouldReduceTransparency]
        }
        cached_accessibility_flag(&REDUCE_TRANSPARENCY_CACHE, read)
    }
    #[cfg(not(target_os = "macos"))]
    false
}

/// Accessibility › Display › "Reduce motion". HIG › Motion requires motion to
/// be optional, so the island parks its size spring and drops the motion blur
/// when it is on, keeping only the crossfade (Apple's dissolve substitute).
/// `false` when there is no AppKit to ask. Cached for 1s.
pub fn reduce_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe fn read() -> bool {
            use objc2::runtime::AnyObject;
            use objc2::*;
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return false;
            }
            msg_send![workspace, accessibilityDisplayShouldReduceMotion]
        }
        cached_accessibility_flag(&REDUCE_MOTION_CACHE, read)
    }
    #[cfg(not(target_os = "macos"))]
    false
}

/// Accessibility (TCC) for window snap. Cached by the AX call itself; Settings
/// reads this when the pane is open, never from the island tick.
pub fn accessibility_trusted() -> bool {
    nook_core::window_snap::is_trusted()
}

/// Show the system Accessibility prompt. Call from a user gesture.
pub fn prompt_accessibility() -> bool {
    nook_core::window_snap::prompt_trust()
#[cfg(target_os = "macos")]
unsafe fn workspace_note_bundle_id(note: *mut objc2::runtime::AnyObject) -> Option<String> {
    use objc2::runtime::AnyObject;
    use objc2::*;
    if note.is_null() {
        return None;
    }
    let info: *mut AnyObject = msg_send![note, userInfo];
    if info.is_null() {
        return None;
    }
    let key: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"NSWorkspaceApplicationKey".as_ptr()
    ];
    let app: *mut AnyObject = msg_send![info, objectForKey: key];
    if app.is_null() {
        return None;
    }
    let ident: *mut AnyObject = msg_send![app, bundleIdentifier];
    if ident.is_null() {
        return None;
    }
    let utf8: *const std::ffi::c_char = msg_send![ident, UTF8String];
    if utf8.is_null() {
        return None;
    }
    Some(
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned(),
    )
}

/// Subscribe to Spotify's and Music's public playback-change broadcasts.
///
/// Both players post on the distributed notification center on every
/// play/pause/track change (no TCC involved). With these installed the
/// now-playing poll can idle at seconds-scale cadence and still react
/// instantly — each poll spawns an osascript/perl child, so cadence is the
/// battery cost that matters. Safari/YouTube posts nothing and rides the slow
/// poll. Blocks only flip an atomic, so the posting thread is fine. Observer
/// tokens are intentionally leaked — they live for the process.
pub fn install_media_observers() {
    #[cfg(target_os = "macos")]
    unsafe {
        use block2::RcBlock;
        use objc2::runtime::AnyObject;
        use objc2::*;

        static INSTALLED: Once = Once::new();
        INSTALLED.call_once(|| {
            let _ = nook_core::audio::prime_media_apps();

            let center: *mut AnyObject =
                msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
            if !center.is_null() {
                for name in [
                    c"com.spotify.client.PlaybackStateChanged",
                    c"com.apple.Music.playerInfo",
                    c"com.apple.iTunes.playerInfo",
                ] {
                    let ns_name: *mut AnyObject =
                        msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
                    let block = RcBlock::new(move |_note: *mut AnyObject| {
                        nook_core::audio::note_media_event();
                    });
                    let token: *mut AnyObject = msg_send![
                        center,
                        addObserverForName: ns_name,
                        object: std::ptr::null_mut::<AnyObject>(),
                        queue: std::ptr::null_mut::<AnyObject>(),
                        usingBlock: &*block
                    ];
                    std::mem::forget(block);
                    let _ = token;
                }
            }

            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return;
            }
            let wsnc: *mut AnyObject = msg_send![workspace, notificationCenter];
            if wsnc.is_null() {
                return;
            }
            for (name, running) in [
                (c"NSWorkspaceDidLaunchApplicationNotification", true),
                (c"NSWorkspaceDidTerminateApplicationNotification", false),
            ] {
                let ns_name: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
                let block = RcBlock::new(move |note: *mut AnyObject| {
                    if let Some(id) = workspace_note_bundle_id(note) {
                        nook_core::audio::note_media_app_running(&id, running);
                        if matches!(
                            id.as_str(),
                            "com.spotify.client" | "com.apple.Music" | "com.apple.Safari"
                        ) {
                            nook_core::audio::note_media_event();
                        }
                    }
                });
                let token: *mut AnyObject = msg_send![
                    wsnc,
                    addObserverForName: ns_name,
                    object: std::ptr::null_mut::<AnyObject>(),
                    queue: std::ptr::null_mut::<AnyObject>(),
                    usingBlock: &*block
                ];
                std::mem::forget(block);
                let _ = token;
            }
        });
    }
}

/// Re-assert OSDUIHelper suppression after sleep — launchd can respawn it.
pub fn install_osd_wake_observer() {
    #[cfg(target_os = "macos")]
    unsafe {
        use block2::RcBlock;
        use objc2::runtime::AnyObject;
        use objc2::*;

        static INSTALLED: Once = Once::new();
        INSTALLED.call_once(|| {
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return;
            }
            let center: *mut AnyObject = msg_send![workspace, notificationCenter];
            if center.is_null() {
                return;
            }
            let name: *mut AnyObject = msg_send![
                class!(NSString),
                stringWithUTF8String: c"NSWorkspaceDidWakeNotification".as_ptr()
            ];
            let block = RcBlock::new(move |_note: *mut AnyObject| {
                nook_core::osd::apply_from_settings();
            });
            let token: *mut AnyObject = msg_send![
                center,
                addObserverForName: name,
                object: std::ptr::null_mut::<AnyObject>(),
                queue: std::ptr::null_mut::<AnyObject>(),
                usingBlock: &*block
            ];
            std::mem::forget(block);
            let _ = token;
        });
    }
}

/// GPUI-space island rect (origin top-left) for the native glass underlay.
#[derive(Clone, Copy, Debug)]
pub struct IslandGlass {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub radius: f64,
    pub wing: f64,
    /// Optional stained-glass tint (`NSGlassEffectView.tintColor`). `None`
    /// leaves the system default.
    pub tint: Option<(f32, f32, f32)>,
}

impl IslandGlass {
    fn same_shape(self, other: Self) -> bool {
        (self.w - other.w).abs() < 0.5
            && (self.h - other.h).abs() < 0.5
            && (self.y - other.y).abs() < 0.5
            && (self.wing - other.wing).abs() < 0.5
            && (self.radius - other.radius).abs() < 0.5
    }
}

/// Convert a GPUI top-left rect into AppKit view coordinates (origin bottom-left).
pub fn cocoa_rect_from_gpui(x: f64, y: f64, w: f64, h: f64, view_h: f64) -> (f64, f64, f64, f64) {
    (x, view_h - y - h, w, h)
}

/// Glass underlay height. Attached to the top edge: island plus corner radius
/// so the top rounding is clipped at the screen and the visible top stays
/// flat. Detached: the island's own height, so all four corners show.
pub fn glass_underlay_height(island_h: f64, radius: f64, attached: bool) -> f64 {
    if attached {
        island_h + radius.max(0.0)
    } else {
        island_h
    }
}

fn glass_extra(spec: IslandGlass) -> f64 {
    if spec.y < 1.0 {
        spec.radius.max(0.0)
    } else {
        0.0
    }
}

/// Place, update, or hide the island's native glass underlay.
///
/// Returns `true` when a system material view is showing, so GPUI must paint
/// the island fill fully transparent. macOS 26+ uses `NSGlassEffectView`
/// (regular style — the island is text-heavy). Older systems fall back to
/// `NSVisualEffectView` HUD material.
///
/// Hard gate: Settings → Liquid Glass island must be on, and Reduce
/// Transparency must be off. Pin/restyle cannot resurrect the view otherwise.
///
/// The glass is a sibling *behind* GPUI's content view, sized to the island —
/// wrapping the Metal view as `contentView` would glass the whole overlay.
pub fn sync_island_glass(spec: Option<IslandGlass>) -> bool {
    #[cfg(target_os = "macos")]
    {
        sync_island_glass_macos(spec)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = spec;
        false
    }
}

/// Settings toggle plus Reduce Transparency. Native glass is illegal otherwise.
pub fn island_glass_setting_on() -> bool {
    nook_core::settings::get_app_settings().liquid_glass_mode && !reduce_transparency()
}

/// True while the AppKit underlay is in the window, even if this tick's
/// `sync_island_glass` failed to talk to the panel.
pub fn island_glass_attached() -> bool {
    #[cfg(target_os = "macos")]
    {
        !glass_cap()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .view
            .is_null()
    }
    #[cfg(not(target_os = "macos"))]
    false
}

#[cfg(target_os = "macos")]
struct GlassCap {
    view: *mut objc2::runtime::AnyObject,
    last: Option<IslandGlass>,
}

#[cfg(target_os = "macos")]
unsafe impl Send for GlassCap {}

#[cfg(target_os = "macos")]
fn glass_cap() -> &'static std::sync::Mutex<GlassCap> {
    static GLASS: std::sync::OnceLock<std::sync::Mutex<GlassCap>> = std::sync::OnceLock::new();
    GLASS.get_or_init(|| {
        std::sync::Mutex::new(GlassCap {
            view: std::ptr::null_mut(),
            last: None,
        })
    })
}

#[cfg(target_os = "macos")]
fn sync_island_glass_macos(spec: Option<IslandGlass>) -> bool {
    if !island_glass_setting_on() {
        hide_island_glass();
        return false;
    }
    let Some(spec) = spec else {
        hide_island_glass();
        return false;
    };
    if spec.w < 2.0 || spec.h < 2.0 {
        hide_island_glass();
        return false;
    }
    unsafe { apply_island_glass(spec) }
}

#[cfg(target_os = "macos")]
fn hide_island_glass() {
    use objc2::*;
    let mut cap = glass_cap().lock().unwrap_or_else(|e| e.into_inner());
    cap.last = None;
    if cap.view.is_null() {
        return;
    }
    let view = cap.view;
    cap.view = std::ptr::null_mut();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let _: () = msg_send![view, removeFromSuperview];
        let _: () = msg_send![view, release];
    }));
}

#[cfg(target_os = "macos")]
unsafe fn apply_island_glass(spec: IslandGlass) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::*;

    if !island_glass_setting_on() {
        hide_island_glass();
        return false;
    }

    let mut ns_win: *mut AnyObject = std::ptr::null_mut();
    for_each_island_window(|w| {
        if ns_win.is_null() {
            ns_win = w;
        }
    });
    if ns_win.is_null() {
        return false;
    }

    let content: *mut AnyObject = msg_send![ns_win, contentView];
    if content.is_null() {
        return false;
    }
    let content_frame: CGRect = msg_send![content, frame];
    let rect = glass_underlay_rect(content_frame, spec);

    let mut cap = glass_cap().lock().unwrap_or_else(|e| e.into_inner());
    if cap.view.is_null() {
        cap.view = match create_glass_view(rect, spec) {
            Some(v) => v,
            None => return false,
        };
        attach_glass_behind_content(ns_win, content, cap.view);
    } else {
        let hosted: *mut AnyObject = msg_send![cap.view, window];
        if hosted != ns_win {
            attach_glass_behind_content(ns_win, content, cap.view);
            pin_dark_appearance(cap.view);
        }
        let current: CGRect = msg_send![cap.view, frame];
        let moved = (current.origin.x - rect.origin.x).abs() > 0.4
            || (current.origin.y - rect.origin.y).abs() > 0.4
            || (current.size.width - rect.size.width).abs() > 0.4
            || (current.size.height - rect.size.height).abs() > 0.4;
        if moved {
            let _: () = msg_send![cap.view, setFrame: rect];
        }
        // Replacing the mask/layer every tick makes Regular glass flash
        // between the inactive (dark) and specular (white) recipes.
        let reshape = cap.last.map(|old| !old.same_shape(spec)).unwrap_or(true);
        let retint = cap.last.map(|old| old.tint != spec.tint).unwrap_or(true);
        if reshape {
            configure_glass_shape(cap.view, spec);
        }
        if reshape || retint {
            apply_glass_tint(cap.view, spec.tint);
        }
    }
    cap.last = Some(spec);
    let hidden: bool = msg_send![cap.view, isHidden];
    if hidden {
        let _: () = msg_send![cap.view, setHidden: false];
    }
    true
}

#[cfg(target_os = "macos")]
unsafe fn create_glass_view(
    rect: CGRect,
    spec: IslandGlass,
) -> Option<*mut objc2::runtime::AnyObject> {
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::*;

    // No wrapper and no CALayer mask: masking the view clips
    // NSGlassEffectView's rim (sheen / refraction) off the bottom radius.
    let glass = if let Some(cls) = AnyClass::get(c"NSGlassEffectView") {
        let v: *mut AnyObject = msg_send![cls, alloc];
        let v: *mut AnyObject = msg_send![v, initWithFrame: rect];
        if v.is_null() {
            return None;
        }
        // Regular, not Clear: island chrome is text-heavy (HIG Materials).
        // Leave tintColor at the default — a 45% black tint was the "always
        // dark" look, and interactive lighting then flashed white on top.
        let _: () = msg_send![v, setStyle: 0_i64];
        let ix = sel!(setEffectIsInteractive:);
        let can_ix: bool = msg_send![v, respondsToSelector: ix];
        if can_ix {
            let no = Bool::from(false);
            let _: () = msg_send![v, setEffectIsInteractive: no];
        }
        // Nonactivating panel is never key; without this the material sits
        // in the inactive (dark) recipe until a hover lights it.
        let sub = sel!(setSubduedState:);
        let can_sub: bool = msg_send![v, respondsToSelector: sub];
        if can_sub {
            let _: () = msg_send![v, setSubduedState: 0_i64];
        }
        v
    } else {
        let v: *mut AnyObject = msg_send![class!(NSVisualEffectView), alloc];
        let v: *mut AnyObject = msg_send![v, initWithFrame: rect];
        if v.is_null() {
            return None;
        }
        // HUDWindow = 13, BehindWindow = 0, Active = 1.
        let _: () = msg_send![v, setMaterial: 13_i64];
        let _: () = msg_send![v, setBlendingMode: 0_i64];
        let _: () = msg_send![v, setState: 1_i64];
        v
    };

    let id_sel = sel!(setIdentifier:);
    let can_id: bool = msg_send![glass, respondsToSelector: id_sel];
    if can_id {
        let id = ns_string("nook-island-glass");
        if !id.is_null() {
            let _: () = msg_send![glass, setIdentifier: id];
        }
    }
    // Island chrome is always the dark Live Activity fill (white labels).
    // Without this, NSGlassEffectView inherits Light appearance and paints
    // the light glass recipe under those labels.
    pin_dark_appearance(glass);
    configure_glass_shape(glass, spec);
    apply_glass_tint(glass, spec.tint);
    Some(glass)
}

/// Pin the underlay to Dark Aqua so Regular glass keeps the dark luminosity
/// recipe in Light Mode. Choosing a Specific Appearance for Your macOS App:
/// set the view's `appearance` when a surface must not follow the system.
#[cfg(target_os = "macos")]
unsafe fn pin_dark_appearance(view: *mut objc2::runtime::AnyObject) {
    use objc2::runtime::AnyObject;
    use objc2::*;

    let name = ns_string("NSAppearanceNameDarkAqua");
    if name.is_null() {
        return;
    }
    let dark: *mut AnyObject = msg_send![class!(NSAppearance), appearanceNamed: name];
    if dark.is_null() {
        return;
    }
    let _: () = msg_send![view, setAppearance: dark];
}

#[cfg(target_os = "macos")]
unsafe fn apply_glass_tint(view: *mut objc2::runtime::AnyObject, tint: Option<(f32, f32, f32)>) {
    use objc2::runtime::AnyObject;
    use objc2::*;

    let sel = sel!(setTintColor:);
    let can: bool = msg_send![view, respondsToSelector: sel];
    if !can {
        return;
    }
    match tint {
        Some((r, g, b)) => {
            let color: *mut AnyObject = msg_send![
                class!(NSColor),
                colorWithSRGBRed: r as f64,
                green: g as f64,
                blue: b as f64,
                alpha: 0.45
            ];
            if !color.is_null() {
                let _: () = msg_send![view, setTintColor: color];
            }
        }
        None => {
            let nil: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![view, setTintColor: nil];
        }
    }
}

/// Grow the underlay upward by `radius`. Cocoa origin is the island bottom,
/// so extra height is clipped at the window's top — a flat, notch-attached
/// edge instead of all-corner rounding.
#[cfg(target_os = "macos")]
fn glass_underlay_rect(content_frame: CGRect, spec: IslandGlass) -> CGRect {
    let (gx, gy, gw, gh) =
        cocoa_rect_from_gpui(spec.x, spec.y, spec.w, spec.h, content_frame.size.height);
    CGRect {
        origin: CGPoint {
            x: content_frame.origin.x + gx,
            y: content_frame.origin.y + gy,
        },
        size: CGSize {
            width: gw,
            height: gh + glass_extra(spec),
        },
    }
}

#[cfg(target_os = "macos")]
unsafe fn set_inner_corner_radius(view: *mut objc2::runtime::AnyObject, radius: f64) {
    use objc2::runtime::AnyObject;
    use objc2::*;

    // Only NSGlassEffectView.cornerRadius — CALayer.cornerRadius / a path
    // mask clips the glass rim off the curve.
    let sel = sel!(setCornerRadius:);
    let can: bool = msg_send![view, respondsToSelector: sel];
    if can {
        let _: () = msg_send![view, setCornerRadius: radius];
        return;
    }
    let _: () = msg_send![view, setWantsLayer: true];
    let layer: *mut AnyObject = msg_send![view, layer];
    if !layer.is_null() {
        let _: () = msg_send![layer, setCornerRadius: radius];
    }
}

#[cfg(target_os = "macos")]
unsafe fn configure_glass_shape(view: *mut objc2::runtime::AnyObject, spec: IslandGlass) {
    set_inner_corner_radius(view, spec.radius);
}

#[cfg(target_os = "macos")]
unsafe fn attach_glass_behind_content(
    ns_win: *mut objc2::runtime::AnyObject,
    content: *mut objc2::runtime::AnyObject,
    glass: *mut objc2::runtime::AnyObject,
) {
    use objc2::runtime::AnyObject;
    use objc2::*;

    let _ = ns_win;
    let super_view: *mut AnyObject = msg_send![content, superview];
    let parent = if super_view.is_null() {
        content
    } else {
        super_view
    };
    let relative: *mut AnyObject = if super_view.is_null() {
        std::ptr::null_mut()
    } else {
        content
    };
    // NSWindowBelow = -1 so Metal (the content view) composites on top.
    let _: () = msg_send![
        parent,
        addSubview: glass,
        positioned: -1_i64,
        relativeTo: relative
    ];
}

#[cfg(target_os = "macos")]
struct MirrorCap {
    session: *mut objc2::runtime::AnyObject,
    sink: *mut objc2::runtime::AnyObject,
    running: bool,
    gen: u64,
    /// Square BGRA8, `MIRROR_SIZE` on a side.
    bgra: Option<Vec<u8>>,
    last_tick: std::time::Instant,
}

#[cfg(target_os = "macos")]
unsafe impl Send for MirrorCap {}

#[cfg(target_os = "macos")]
fn default_mirror() -> std::sync::Mutex<MirrorCap> {
    std::sync::Mutex::new(MirrorCap {
        session: std::ptr::null_mut(),
        sink: std::ptr::null_mut(),
        running: false,
        gen: 0,
        bgra: None,
        last_tick: std::time::Instant::now(),
    })
}

#[cfg(target_os = "macos")]
static MIRROR: std::sync::OnceLock<std::sync::Mutex<MirrorCap>> = std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
#[link(name = "AVFoundation", kind = "framework")]
#[link(name = "CoreMedia", kind = "framework")]
#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    fn CMSampleBufferGetImageBuffer(sample: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn CVPixelBufferLockBaseAddress(buf: *mut std::ffi::c_void, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(buf: *mut std::ffi::c_void, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddress(buf: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn CVPixelBufferGetBytesPerRow(buf: *mut std::ffi::c_void) -> usize;
    fn CVPixelBufferGetWidth(buf: *mut std::ffi::c_void) -> usize;
    fn CVPixelBufferGetHeight(buf: *mut std::ffi::c_void) -> usize;
    fn CVPixelBufferGetPixelFormatType(buf: *mut std::ffi::c_void) -> u32;
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut std::ffi::c_void;
}

#[cfg(target_os = "macos")]
fn ns_string(s: &str) -> *mut objc2::runtime::AnyObject {
    use objc2::*;
    let c = std::ffi::CString::new(s).unwrap_or_default();
    unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
}

#[cfg(target_os = "macos")]
fn av_class(name: &std::ffi::CStr) -> Option<&'static objc2::runtime::AnyClass> {
    load_avfoundation();
    objc2::runtime::AnyClass::get(name)
}

#[cfg(target_os = "macos")]
fn load_avfoundation() {
    use objc2::*;
    use std::sync::Once;
    static LOAD: Once = Once::new();
    LOAD.call_once(|| unsafe {
        let path = ns_string("/System/Library/Frameworks/AVFoundation.framework");
        if path.is_null() {
            return;
        }
        let bundle: *mut objc2::runtime::AnyObject =
            msg_send![class!(NSBundle), bundleWithPath: path];
        if !bundle.is_null() {
            let _: objc2::runtime::Bool = msg_send![bundle, load];
        }
    });
}

#[cfg(target_os = "macos")]
fn camera_sink_class() -> Option<&'static objc2::runtime::AnyClass> {
    use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, NSObject, Sel};
    use objc2::{sel, ClassType};
    static CLASS: std::sync::OnceLock<Option<&'static AnyClass>> = std::sync::OnceLock::new();
    *CLASS.get_or_init(|| {
        if let Some(existing) = AnyClass::get(c"NookCameraSink") {
            return Some(existing);
        }
        let mut builder = ClassBuilder::new(c"NookCameraSink", NSObject::class())?;
        extern "C" fn did_output(
            _this: &NSObject,
            _cmd: Sel,
            _output: *mut AnyObject,
            sample: *mut std::ffi::c_void,
            _conn: *mut AnyObject,
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                ingest_camera_sample(sample);
            }));
        }
        unsafe {
            builder.add_method(
                sel!(captureOutput:didOutputSampleBuffer:fromConnection:),
                did_output as extern "C" fn(_, _, _, _, _),
            );
        }
        Some(builder.register())
    })
}

#[cfg(target_os = "macos")]
unsafe fn ingest_camera_sample(sample: *mut std::ffi::c_void) {
    if sample.is_null() {
        return;
    }
    {
        let cap = MIRROR
            .get_or_init(default_mirror)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !cap.running || cap.last_tick.elapsed() < std::time::Duration::from_millis(80) {
            return;
        }
    }
    let buf = CMSampleBufferGetImageBuffer(sample);
    if buf.is_null() {
        return;
    }
    // 32BGRA. Planar YUV would make GetBaseAddress null or a 1-byte plane;
    // reading it as 4-byte pixels overruns and kills the process.
    const BGRA: u32 = 0x4247_5241;
    if CVPixelBufferGetPixelFormatType(buf) != BGRA {
        return;
    }
    if CVPixelBufferLockBaseAddress(buf, 0) != 0 {
        return;
    }
    let width = CVPixelBufferGetWidth(buf);
    let height = CVPixelBufferGetHeight(buf);
    let stride = CVPixelBufferGetBytesPerRow(buf);
    let base = CVPixelBufferGetBaseAddress(buf) as *const u8;
    if base.is_null() || width < 16 || height < 16 || stride < width.saturating_mul(4) {
        let _ = CVPixelBufferUnlockBaseAddress(buf, 0);
        return;
    }
    // Square centre-crop so the BGRA buffer is the same shape as the Mirror
    // circle. Landscape 16:9 would otherwise stick out of the rounded sprite.
    const DST: u32 = MIRROR_SIZE;
    let side = width.min(height);
    let x0 = (width - side) / 2;
    let y0 = (height - side) / 2;
    let mut rgba = vec![0u8; (DST * DST * 4) as usize];
    for y in 0..DST {
        let sy = y0 + (y as usize * side) / DST as usize;
        if sy >= height {
            break;
        }
        let row = unsafe { base.add(sy * stride) };
        for x in 0..DST {
            let local = (x as usize * side) / DST as usize;
            let sx = x0 + (side - 1 - local);
            if sx >= width || sx.saturating_mul(4).saturating_add(4) > stride {
                continue;
            }
            let px = unsafe { row.add(sx * 4) };
            let i = ((y * DST + x) * 4) as usize;
            // Keep BGRA — `RenderImage` wants that channel order.
            rgba[i] = unsafe { *px.add(0) };
            rgba[i + 1] = unsafe { *px.add(1) };
            rgba[i + 2] = unsafe { *px.add(2) };
            rgba[i + 3] = unsafe { *px.add(3) };
        }
    }
    let _ = CVPixelBufferUnlockBaseAddress(buf, 0);
    let mut cap = MIRROR
        .get_or_init(default_mirror)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if !cap.running {
        return;
    }
    cap.bgra = Some(rgba);
    cap.gen = cap.gen.wrapping_add(1);
    cap.last_tick = std::time::Instant::now();
}

#[cfg(target_os = "macos")]
fn start_mirror_macos() -> bool {
    {
        let cap = MIRROR
            .get_or_init(default_mirror)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if cap.running {
            return true;
        }
    }
    match std::panic::catch_unwind(start_capture_session) {
        Ok(ok) => ok,
        Err(_) => {
            log::error!("mirror: capture setup panicked");
            false
        }
    }
}

#[cfg(target_os = "macos")]
fn start_capture_session() -> bool {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::*;

    unsafe {
        let Some(dev_cls) = av_class(c"AVCaptureDevice") else {
            log::warn!("AVCaptureDevice missing — AVFoundation not loaded");
            return false;
        };
        let Some(in_cls) = av_class(c"AVCaptureDeviceInput") else {
            return false;
        };
        let Some(sess_cls) = av_class(c"AVCaptureSession") else {
            return false;
        };
        let Some(out_cls) = av_class(c"AVCaptureVideoDataOutput") else {
            return false;
        };
        let Some(sink_cls) = camera_sink_class() else {
            return false;
        };

        let vide = ns_string("vide");
        if vide.is_null() {
            return false;
        }
        let device: *mut AnyObject = msg_send![dev_cls, defaultDeviceWithMediaType: vide];
        if device.is_null() {
            log::warn!("no camera device");
            return false;
        }
        let mut err: *mut AnyObject = std::ptr::null_mut();
        let input: *mut AnyObject = msg_send![
            in_cls,
            deviceInputWithDevice: device,
            error: &mut err
        ];
        if input.is_null() {
            log::warn!("camera input failed");
            return false;
        }
        let session: *mut AnyObject = msg_send![sess_cls, new];
        if session.is_null() {
            return false;
        }
        let can_in: Bool = msg_send![session, canAddInput: input];
        if !can_in.as_bool() {
            log::warn!("camera cannot add input");
            let _: () = msg_send![session, release];
            return false;
        }
        let _: () = msg_send![session, addInput: input];

        let output: *mut AnyObject = msg_send![out_cls, new];
        if output.is_null() {
            let _: () = msg_send![session, release];
            return false;
        }
        let yes = Bool::from(true);
        let _: () = msg_send![output, setAlwaysDiscardsLateVideoFrames: yes];
        let key = ns_string("PixelFormatType");
        let num_cls = class!(NSNumber);
        let num: *mut AnyObject = msg_send![num_cls, numberWithUnsignedInt: 0x4247_5241_u32];
        let settings: *mut AnyObject =
            msg_send![class!(NSDictionary), dictionaryWithObject: num, forKey: key];
        let _: () = msg_send![output, setVideoSettings: settings];

        let sink: *mut AnyObject = msg_send![sink_cls, new];
        // dispatch_queue_t is an NSObject on Darwin; passing *mut c_void
        // panics objc2's encoding check (`^v` vs `@`).
        let queue = dispatch_get_global_queue(0, 0) as *mut AnyObject;
        if queue.is_null() || sink.is_null() {
            let _: () = msg_send![session, release];
            let _: () = msg_send![output, release];
            return false;
        }
        let _: () = msg_send![output, setSampleBufferDelegate: sink, queue: queue];

        let can_out: Bool = msg_send![session, canAddOutput: output];
        if !can_out.as_bool() {
            log::warn!("camera cannot add output");
            let _: () = msg_send![session, release];
            let _: () = msg_send![output, release];
            let _: () = msg_send![sink, release];
            return false;
        }
        let _: () = msg_send![session, addOutput: output];

        {
            let mut cap = MIRROR
                .get_or_init(default_mirror)
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            cap.session = session;
            cap.sink = sink;
            cap.running = true;
            cap.bgra = None;
        }

        let session_ptr = session as usize;
        std::thread::Builder::new()
            .name("nook-mirror".into())
            .spawn(move || {
                let session = session_ptr as *mut AnyObject;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _: () = msg_send![session, startRunning];
                }));
                if result.is_err() {
                    log::error!("mirror: startRunning panicked");
                    let mut cap = MIRROR
                        .get_or_init(default_mirror)
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    cap.running = false;
                } else {
                    log::info!("mirror camera running");
                }
            })
            .ok();
        true
    }
}

#[cfg(target_os = "macos")]
fn stop_mirror_macos() {
    use objc2::*;

    let (session, sink) = {
        let mut cap = MIRROR
            .get_or_init(default_mirror)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cap.running = false;
        cap.bgra = None;
        let session = cap.session;
        let sink = cap.sink;
        cap.session = std::ptr::null_mut();
        cap.sink = std::ptr::null_mut();
        (session, sink)
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        if !session.is_null() {
            let _: () = msg_send![session, stopRunning];
            let _: () = msg_send![session, release];
        }
        if !sink.is_null() {
            let _: () = msg_send![sink, release];
        }
    }));
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn media_app_icon_png_is_a_reasonable_png() {
        let png = app_icon_png(Some("com.spotify.client"), Some("Spotify"))
            .or_else(|| app_icon_png(Some("com.apple.finder"), Some("Finder")))
            .expect("NSWorkspace should yield an app icon PNG");
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            "app icon must be PNG, got {} bytes",
            png.len()
        );
        assert!(
            (64..=512 * 1024).contains(&png.len()),
            "unexpected app icon size {}",
            png.len()
        );
        let decoded = image::load_from_memory(&png).expect("png should decode");
        assert!(
            decoded.width() >= 32 && decoded.height() >= 32,
            "icon too small {}x{}",
            decoded.width(),
            decoded.height()
        );
        assert!(
            decoded.width() <= 512 && decoded.height() <= 512,
            "icon too large {}x{}",
            decoded.width(),
            decoded.height()
        );
    }
}
