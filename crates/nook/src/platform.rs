//! What the OS knows that nook-core doesn't cover: running apps and where an app lives.

use serde_json::{Value, json};
use std::path::PathBuf;

#[cfg(target_os = "macos")]
mod mac {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ffi::{CStr, CString};

    pub unsafe fn string(ns: *mut AnyObject) -> Option<String> {
        if ns.is_null() {
            return None;
        }
        let ptr: *const std::os::raw::c_char = unsafe { msg_send![ns, UTF8String] };
        (!ptr.is_null()).then(|| unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
    }

    pub unsafe fn ns_string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).unwrap_or_default();
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }
}

/// Regular (Dock) apps that are running: `{ bundleId, name, pid }`.
pub fn running_apps() -> Vec<Value> {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let mut out = Vec::new();
        let pool: *mut AnyObject = msg_send![class!(NSAutoreleasePool), new];
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let apps: *mut AnyObject = msg_send![ws, runningApplications];
        let count: usize = if apps.is_null() { 0 } else { msg_send![apps, count] };
        for i in 0..count {
            let app: *mut AnyObject = msg_send![apps, objectAtIndex: i];
            // 0 = regular, 1 = accessory (menu-bar apps like Hex), 2 = background-only.
            let policy: isize = msg_send![app, activationPolicy];
            if policy > 1 {
                continue;
            }
            let Some(bundle) = mac::string(msg_send![app, bundleIdentifier]) else { continue };
            let name = mac::string(msg_send![app, localizedName]).unwrap_or_default();
            let pid: i32 = msg_send![app, processIdentifier];
            out.push(json!({ "bundleId": bundle, "name": name, "pid": pid }));
        }
        let _: () = msg_send![pool, drain];
        out.sort_by(|a, b| a["bundleId"].as_str().cmp(&b["bundleId"].as_str()));
        out
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

/// Pointer input Rune doesn't deliver to an overlay panel: it routes only left presses and
/// movement there. Scrolls and right-clicks on the island's panel come through here.
pub enum IslandInput {
    /// Points, positive moving content right and down (the fingers' way with natural scrolling).
    /// `x`, `y`: the pointer, in window points from the panel's top-left.
    Scroll { dx: f32, dy: f32, phase: rune::GesturePhase, momentum: bool, x: f32, y: f32 },
    /// Window points from the panel's top-left.
    SecondaryClick { x: f32, y: f32 },
}

/// The island's panel becomes key only when asked (`request_keys`, for a focused text field),
/// never from a click: a press on a button in the island leaves the keyboard where it was.
pub fn keys_only_on_request(window: u32) {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let panel: *mut AnyObject = msg_send![app, windowWithWindowNumber: window as isize];
        if !panel.is_null() {
            let _: () = msg_send![panel, setBecomesKeyOnlyIfNeeded: true];
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// Gives the keyboard back to the app it came from. The panel never activated openNook, so
/// that app is still frontmost; once the panel resigns, its key window hears keys again.
pub fn give_up_keys(window: u32) {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let panel: *mut AnyObject = msg_send![app, windowWithWindowNumber: window as isize];
        if !panel.is_null() {
            let key: bool = msg_send![panel, isKeyWindow];
            if key {
                let _: () = msg_send![panel, resignKeyWindow];
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// Watches the app's own events for ones aimed at the window numbered `window`. Must run on
/// the main thread; the monitor lives as long as the app.
pub fn watch_island_input(window: u32, on: impl Fn(IslandInput) + 'static) {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        // NSEventMaskRightMouseDown | NSEventMaskScrollWheel.
        const MASK: u64 = (1 << 3) | (1 << 22);
        let handler = block2::RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
            if event.is_null() {
                return event;
            }
            let number: isize = msg_send![event, windowNumber];
            if number as u32 != window {
                return event;
            }
            let kind: usize = msg_send![event, type];
            #[repr(C)]
            #[derive(Clone, Copy)]
            struct Point { x: f64, y: f64 }
            #[repr(C)]
            #[derive(Clone, Copy)]
            struct Rect { origin: Point, size: Point }
            unsafe impl objc2::Encode for Point {
                const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
            }
            unsafe impl objc2::Encode for Rect {
                const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGRect", &[Point::ENCODING, Point::ENCODING]);
            }
            // Window points from the panel's top-left (AppKit's run from its bottom-left).
            let at = || -> (f32, f32) {
                let at: Point = msg_send![event, locationInWindow];
                let win: *mut AnyObject = msg_send![event, window];
                let frame: Rect = if win.is_null() { Rect { origin: Point { x: 0.0, y: 0.0 }, size: Point { x: 0.0, y: 0.0 } } } else { msg_send![win, frame] };
                (at.x as f32, (frame.size.y - at.y) as f32)
            };
            if kind == 22 {
                let precise: bool = msg_send![event, hasPreciseScrollingDeltas];
                // A notched wheel reports lines; a trackpad, points.
                let unit = if precise { 1.0 } else { 10.0 };
                let dx: f64 = msg_send![event, scrollingDeltaX];
                let dy: f64 = msg_send![event, scrollingDeltaY];
                let phase: usize = msg_send![event, phase];
                let momentum: usize = msg_send![event, momentumPhase];
                // NSEventPhase: began 1, changed 4, ended 8, cancelled 16.
                let phase = match phase {
                    1 => rune::GesturePhase::Began,
                    8 | 16 => rune::GesturePhase::Ended,
                    _ => rune::GesturePhase::Moved,
                };
                let (x, y) = at();
                on(IslandInput::Scroll { dx: dx as f32 * unit, dy: dy as f32 * unit, phase, momentum: momentum != 0, x, y });
                return std::ptr::null_mut();
            }
            if kind == 3 {
                let (x, y) = at();
                on(IslandInput::SecondaryClick { x, y });
                return std::ptr::null_mut();
            }
            event
        });
        let token: *mut AnyObject = msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: MASK, handler: &*handler];
        // The monitor holds the block; keep both for the process.
        let _: *mut AnyObject = msg_send![token, retain];
        std::mem::forget(handler);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, on);
}

/// System Settings › Accessibility › Display › Reduce transparency.
pub fn reduce_transparency() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let on: bool = msg_send![ws, accessibilityDisplayShouldReduceTransparency];
        on
    }
    #[cfg(not(target_os = "macos"))]
    false
}

/// An app's icon in its light (Aqua) variant, as straight-alpha RGBA, `px` square. The island is
/// black: a dark-mode icon (macOS's "dark" icon style) would vanish on it.
pub fn app_icon_light(bundle_id: &str, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Size { w: f64, h: f64 }
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Rect { x: f64, y: f64, w: f64, h: f64 }
        unsafe impl objc2::Encode for Size {
            const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
        }
        unsafe impl objc2::Encode for Rect {
            const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGRect", &[
                objc2::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]),
                objc2::Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]),
            ]);
        }
        let path = app_path(bundle_id)?;
        let pool: *mut AnyObject = msg_send![class!(NSAutoreleasePool), new];
        // The bundle's own .icns holds the standard (light) artwork; the workspace's icon follows
        // the person's icon style, dark included.
        let bundle: *mut AnyObject = msg_send![class!(NSBundle), bundleWithPath: mac::ns_string(&path.to_string_lossy())];
        let name: *mut AnyObject = if bundle.is_null() { std::ptr::null_mut() } else { msg_send![bundle, objectForInfoDictionaryKey: mac::ns_string("CFBundleIconFile")] };
        let file: Option<String> = mac::string(name).and_then(|n| {
            let n = n.trim_end_matches(".icns").to_owned();
            mac::string(msg_send![bundle, pathForResource: mac::ns_string(&n), ofType: mac::ns_string("icns")])
        });
        let mut icon: *mut AnyObject = std::ptr::null_mut();
        if let Some(file) = file {
            let alloc: *mut AnyObject = msg_send![class!(NSImage), alloc];
            icon = msg_send![alloc, initWithContentsOfFile: mac::ns_string(&file)];
            let _: *mut AnyObject = msg_send![icon, autorelease];
        }
        if icon.is_null() {
            let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            icon = msg_send![ws, iconForFile: mac::ns_string(&path.to_string_lossy())];
        }
        if icon.is_null() {
            let _: () = msg_send![pool, drain];
            return None;
        }
        let _: () = msg_send![icon, setSize: Size { w: px as f64, h: px as f64 }];
        // Premultiplied (CoreGraphics draws no other); straightened below, as Rune takes it.
        let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), alloc];
        let rep: *mut AnyObject = msg_send![rep,
            initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(),
            pixelsWide: px as isize, pixelsHigh: px as isize,
            bitsPerSample: 8isize, samplesPerPixel: 4isize,
            hasAlpha: true, isPlanar: false,
            colorSpaceName: mac::ns_string("NSDeviceRGBColorSpace"),
            bitmapFormat: 0usize,
            bytesPerRow: (px * 4) as isize, bitsPerPixel: 32isize];
        if rep.is_null() {
            let _: () = msg_send![pool, drain];
            return None;
        }
        let ctx: *mut AnyObject = msg_send![class!(NSGraphicsContext), graphicsContextWithBitmapImageRep: rep];
        let aqua: *mut AnyObject = msg_send![class!(NSAppearance), appearanceNamed: mac::ns_string("NSAppearanceNameAqua")];
        let previous: *mut AnyObject = msg_send![class!(NSAppearance), currentDrawingAppearance];
        let _: () = msg_send![class!(NSGraphicsContext), saveGraphicsState];
        let _: () = msg_send![class!(NSGraphicsContext), setCurrentContext: ctx];
        // The drawing appearance picks which of the icon's variants draws.
        let _: () = msg_send![class!(NSAppearance), setCurrentAppearance: aqua];
        let rect = Rect { x: 0.0, y: 0.0, w: px as f64, h: px as f64 };
        let zero = Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
        // NSCompositingOperationSourceOver = 2.
        let _: () = msg_send![icon, drawInRect: rect, fromRect: zero, operation: 2usize, fraction: 1.0f64];
        let _: () = msg_send![class!(NSAppearance), setCurrentAppearance: previous];
        let _: () = msg_send![class!(NSGraphicsContext), restoreGraphicsState];
        let data: *const u8 = msg_send![rep, bitmapData];
        let out = (!data.is_null()).then(|| {
            let mut rgba = std::slice::from_raw_parts(data, (px * px * 4) as usize).to_vec();
            for p in rgba.as_chunks_mut::<4>().0 {
                let a = p[3] as u32;
                if a > 0 && a < 255 {
                    for c in &mut p[..3] {
                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
            rgba
        });
        let _: () = msg_send![rep, release];
        let _: () = msg_send![pool, drain];
        out.map(|rgba| (px, px, rgba))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (bundle_id, px);
        None
    }
}

/// Where an app is installed, by bundle id (for its icon).
pub fn app_path(bundle_id: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let url: *mut AnyObject = msg_send![ws, URLForApplicationWithBundleIdentifier: mac::ns_string(bundle_id)];
        if url.is_null() {
            return None;
        }
        mac::string(msg_send![url, path]).map(PathBuf::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bundle_id;
        None
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn displays_are_listed() {
        let all = super::displays();
        for d in &all {
            eprintln!("{d:?} fullscreen={}", super::fullscreen_on(d));
        }
        assert!(!all.is_empty());
        assert_eq!(all.iter().filter(|d| d.main).count(), 1);
    }

    #[test]
    fn light_icons_have_pixels() {
        for id in ["com.apple.Music", "com.apple.weather"] {
            let (w, h, rgba) = super::app_icon_light(id, 64).expect(id);
            let opaque = rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
            let mean: u64 = rgba.chunks_exact(4).filter(|p| p[3] > 0).map(|p| (p[0] as u64 + p[1] as u64 + p[2] as u64) / 3).sum::<u64>() / opaque.max(1) as u64;
            eprintln!("{id}: {w}x{h} opaque={opaque} mean-lum={mean}");
            assert!(opaque > 1000, "{id} drew nothing");
        }
    }
}

/// A display, in AppKit's global points (origin at the primary display's bottom-left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// CGDirectDisplayID (`NSScreenNumber`), what Rune targets an overlay at.
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// The camera housing's size, when the display has one.
    pub notch: Option<(f64, f64)>,
    pub builtin: bool,
    /// Holds the menu bar's origin (the primary display).
    pub main: bool,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGDisplayIsBuiltin(display: u32) -> u32;
    fn CGMainDisplayID() -> u32;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *mut objc2::runtime::AnyObject;
    // Private (SkyLight, exported through CoreGraphics): the Space a display shows, and its kind.
    fn CGSMainConnectionID() -> i32;
    fn CGSManagedDisplayGetCurrentSpace(connection: i32, display_uuid: *const std::ffi::c_void) -> u64;
    fn CGSSpaceGetType(connection: i32, space: u64) -> i32;
}

#[cfg(target_os = "macos")]
#[link(name = "ColorSync", kind = "framework")]
unsafe extern "C" {
    fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> *const std::ffi::c_void;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFUUIDCreateString(alloc: *const std::ffi::c_void, uuid: *const std::ffi::c_void) -> *const std::ffi::c_void;
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// Whether `display` shows a full-screen Space (`CGSSpaceGetType` 4). Window bounds can't say:
/// on a notched display a full-screen app sits below the camera housing, exactly where a
/// zoomed window sits when the Dock is hidden.
#[cfg(target_os = "macos")]
fn fullscreen_space(display: u32) -> bool {
    const FULLSCREEN_SPACE: i32 = 4;
    unsafe {
        let uuid = CGDisplayCreateUUIDFromDisplayID(display);
        if uuid.is_null() {
            return false;
        }
        let name = CFUUIDCreateString(std::ptr::null(), uuid);
        CFRelease(uuid);
        if name.is_null() {
            return false;
        }
        let cid = CGSMainConnectionID();
        let space = CGSManagedDisplayGetCurrentSpace(cid, name);
        CFRelease(name);
        space != 0 && CGSSpaceGetType(cid, space) == FULLSCREEN_SPACE
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct NsRect { x: f64, y: f64, w: f64, h: f64 }
#[cfg(target_os = "macos")]
unsafe impl objc2::Encode for NsRect {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGRect", &[
        objc2::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]),
        objc2::Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]),
    ]);
}
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct NsInsets { top: f64, left: f64, bottom: f64, right: f64 }
#[cfg(target_os = "macos")]
unsafe impl objc2::Encode for NsInsets {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct("NSEdgeInsets", &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING]);
}

/// Every display, with its camera housing measured the way AppKit reports it: the top safe-area
/// inset and the gap between the menu bar's two auxiliary areas.
pub fn displays() -> Vec<Display> {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let pool: *mut AnyObject = msg_send![class!(NSAutoreleasePool), new];
        let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
        let count: usize = if screens.is_null() { 0 } else { msg_send![screens, count] };
        let main_id = CGMainDisplayID();
        let mut out = Vec::new();
        for i in 0..count {
            let screen: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            let frame: NsRect = msg_send![screen, frame];
            let desc: *mut AnyObject = msg_send![screen, deviceDescription];
            let number: *mut AnyObject = msg_send![desc, objectForKey: mac::ns_string("NSScreenNumber")];
            let id: u32 = if number.is_null() { 0 } else { msg_send![number, unsignedIntValue] };
            let responds: bool = msg_send![screen, respondsToSelector: objc2::sel!(safeAreaInsets)];
            let notch = if responds {
                let insets: NsInsets = msg_send![screen, safeAreaInsets];
                let left: NsRect = msg_send![screen, auxiliaryTopLeftArea];
                let right: NsRect = msg_send![screen, auxiliaryTopRightArea];
                let gap = right.x - (left.x + left.w);
                (insets.top > 0.0 && gap > 80.0).then(|| (gap.clamp(160.0, 280.0), insets.top.max(32.0)))
            } else {
                None
            };
            out.push(Display { id, x: frame.x, y: frame.y, w: frame.w, h: frame.h, notch, builtin: CGDisplayIsBuiltin(id) != 0, main: id == main_id });
        }
        let _: () = msg_send![pool, drain];
        out
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

/// The pointer, in AppKit's global points.
pub fn pointer() -> (f64, f64) {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::{class, msg_send};
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct P { x: f64, y: f64 }
        unsafe impl objc2::Encode for P {
            const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
        }
        let p: P = msg_send![class!(NSEvent), mouseLocation];
        (p.x, p.y)
    }
    #[cfg(not(target_os = "macos"))]
    (0.0, 0.0)
}

/// Whether `display` shows a full-screen Space, or another app's window covers all of it (a
/// borderless full-screen game or video). Only the current spaces' windows are on screen, so on
/// an ordinary desktop none does. (Not "the frontmost window decides": a full-screen app often puts its
/// toolbar in a window of its own in front.) A zoomed window doesn't count: it stays under the
/// menu bar, clear of the island.
pub fn fullscreen_on(display: &Display) -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        if fullscreen_space(display.id) {
            return true;
        }
        // Window bounds are in CoreGraphics' space: origin at the primary display's top-left.
        let primary_h = displays().iter().find(|d| d.main).map_or(display.h, |d| d.h);
        let top = primary_h - (display.y + display.h);
        let left = display.x;
        let pool: *mut AnyObject = msg_send![class!(NSAutoreleasePool), new];
        // kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements: front to back.
        let list = CGWindowListCopyWindowInfo(1 | (1 << 4), 0);
        let count: usize = if list.is_null() { 0 } else { msg_send![list, count] };
        let num = |dict: *mut AnyObject, key: &str| -> f64 {
            let v: *mut AnyObject = msg_send![dict, objectForKey: mac::ns_string(key)];
            if v.is_null() { 0.0 } else { msg_send![v, doubleValue] }
        };
        let ours = std::process::id() as f64;
        let mut fills = false;
        for i in 0..count {
            let dict: *mut AnyObject = msg_send![list, objectAtIndex: i];
            if num(dict, "kCGWindowLayer") != 0.0 || num(dict, "kCGWindowOwnerPID") == ours || num(dict, "kCGWindowAlpha") < 0.9 {
                continue;
            }
            let b: *mut AnyObject = msg_send![dict, objectForKey: mac::ns_string("kCGWindowBounds")];
            if b.is_null() {
                continue;
            }
            let (x, y, w, h) = (num(b, "X"), num(b, "Y"), num(b, "Width"), num(b, "Height"));
            if (x - left).abs() <= 2.0 && (y - top).abs() <= 2.0 && (w - display.w).abs() <= 2.0 && (h - display.h).abs() <= 2.0 {
                fills = true;
                break;
            }
        }
        if !list.is_null() {
            let _: () = msg_send![list, release];
        }
        let _: () = msg_send![pool, drain];
        fills
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = display;
        false
    }
}
