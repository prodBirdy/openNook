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
#[cfg(target_os = "macos")]
static OPEN_SETTINGS: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static STATUS_ITEM: std::sync::atomic::AtomicPtr<objc2::runtime::AnyObject> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

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
            pin_island_windows();
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
        let _ = class_addMethod(cls, sel!(openSettings:), imp_settings, types.as_ptr());
        let _ = class_addMethod(cls, sel!(screenChanged:), imp_screen, types.as_ptr());
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
}

/// Open the system camera preview (Photo Booth) for the Mirror control.
pub fn open_mirror() {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-a")
            .arg("Photo Booth")
            .spawn();
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

/// System Settings → Appearance → *Accent color* (`NSColor.controlAccentColor`),
/// as sRGB components in 0..=1. `None` when there is no AppKit to ask or the
/// color cannot be converted to an RGB space — callers fall back to systemBlue.
///
/// Read live rather than cached so switching the accent in System Settings
/// shows up on the next frame. Must be called on the main thread.
pub fn accent_color() -> Option<(f32, f32, f32)> {
    #[cfg(target_os = "macos")]
    {
        accent_color_macos()
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

/// Accessibility › Display › "Reduce transparency". HIG › Materials requires
/// materials to respond to it, so the island drops its translucency when it is
/// on. `false` when there is no AppKit to ask.
pub fn reduce_transparency() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::*;
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return false;
        }
        msg_send![workspace, accessibilityDisplayShouldReduceTransparency]
    }
    #[cfg(not(target_os = "macos"))]
    false
}
