//! Shared CGEventTap thread for Mechey (listen-only keys) and LiquidMouse
//! (active scroll). Taps exist only while the matching setting is on —
//! disabled means `CFMachPortInvalidate` and the runloop thread joins.

#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
#[cfg(target_os = "macos")]
use std::thread;
use std::thread::JoinHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    Unsupported,
}

impl PermissionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Granted => "Granted",
            Self::Denied => "Not granted",
            Self::Unsupported => "macOS only",
        }
    }

    pub fn granted(self) -> bool {
        matches!(self, Self::Granted)
    }
}

struct TapRuntime {
    want_keys: bool,
    want_scroll: bool,
    handle: Option<JoinHandle<()>>,
}

fn runtime() -> &'static Mutex<TapRuntime> {
    static RUNTIME: OnceLock<Mutex<TapRuntime>> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Mutex::new(TapRuntime {
            want_keys: false,
            want_scroll: false,
            handle: None,
        })
    })
}

/// Create or tear down taps from the current settings. No-op when unchanged.
pub fn sync() {
    let settings = crate::settings::get_app_settings();
    let want_keys = settings.keysounds_enabled;
    let want_scroll = settings.smooth_scroll_enabled || settings.reverse_mouse_scroll;
    let mut guard = runtime().lock().unwrap_or_else(|e| e.into_inner());
    let live = guard.handle.is_some();
    let want_any = want_keys || want_scroll;
    if guard.want_keys == want_keys && guard.want_scroll == want_scroll && live == want_any {
        return;
    }
    stop_locked(&mut guard);
    guard.want_keys = want_keys;
    guard.want_scroll = want_scroll;
    if !want_keys && !want_scroll {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let keys = want_keys;
        let scroll = want_scroll;
        guard.handle = Some(
            thread::Builder::new()
                .name("nook-eventtap".into())
                .spawn(move || unsafe { runloop_thread(keys, scroll) })
                .expect("eventtap thread"),
        );
    }
}

fn stop_locked(guard: &mut TapRuntime) {
    #[cfg(target_os = "macos")]
    unsafe {
        stop_runloop();
    }
    if let Some(handle) = guard.handle.take() {
        let _ = handle.join();
    }
}

pub fn input_monitoring_status() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    unsafe {
        match ffi::IOHIDCheckAccess(ffi::kIOHIDRequestTypeListenEvent) {
            0 => PermissionStatus::Granted,
            _ => PermissionStatus::Denied,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionStatus::Unsupported
    }
}

pub fn request_input_monitoring() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        ffi::IOHIDRequestAccess(ffi::kIOHIDRequestTypeListenEvent)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn open_input_monitoring_settings() {
    #[cfg(target_os = "macos")]
    {
        let _ = open::that(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
        );
    }
}

pub fn accessibility_status() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    unsafe {
        if ffi::AXIsProcessTrusted() {
            PermissionStatus::Granted
        } else {
            PermissionStatus::Denied
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionStatus::Unsupported
    }
}

pub fn request_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        prompt_accessibility_macos()
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

pub fn is_secure_input() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        ffi::IsSecureEventInputEnabled()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn frontmost_bundle_id() -> Option<String> {
    #[cfg(target_os = "macos")]
    unsafe {
        frontmost_bundle_id_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn running_conflict_ids() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        crate::scroll::CONFLICT_BUNDLES
            .iter()
            .filter(|id| bundle_is_running(id))
            .map(|id| (*id).to_string())
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
fn bundle_is_running(id: &str) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::CString;
    let Ok(c_id) = CString::new(id) else {
        return false;
    };
    unsafe {
        let ident: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c_id.as_ptr()];
        let apps: *mut AnyObject = msg_send![
            class!(NSRunningApplication),
            runningApplicationsWithBundleIdentifier: ident
        ];
        if apps.is_null() {
            return false;
        }
        let count: usize = msg_send![apps, count];
        count > 0
    }
}

#[cfg(target_os = "macos")]
unsafe fn frontmost_bundle_id_macos() -> Option<String> {
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::CStr;
    let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
    if workspace.is_null() {
        return None;
    }
    let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
    if app.is_null() {
        return None;
    }
    let ident: *mut AnyObject = msg_send![app, bundleIdentifier];
    if ident.is_null() {
        return None;
    }
    let utf8: *const i8 = msg_send![ident, UTF8String];
    if utf8.is_null() {
        return None;
    }
    CStr::from_ptr(utf8).to_str().ok().map(str::to_string)
}

#[cfg(target_os = "macos")]
unsafe fn prompt_accessibility_macos() -> bool {
    use objc2::runtime::AnyObject;
    use objc2::*;
    let key: *mut AnyObject = msg_send![
        class!(NSString),
        stringWithUTF8String: c"AXTrustedCheckOptionPrompt".as_ptr()
    ];
    let yes: *mut AnyObject = msg_send![class!(NSNumber), numberWithBool: true];
    // `use objc2::*` shadows this module's `ffi` with `objc2::ffi`, which
    // omits Accessibility. Call the local ApplicationServices declarations.
    if key.is_null() || yes.is_null() {
        return self::ffi::AXIsProcessTrusted();
    }
    let options: *mut AnyObject =
        msg_send![class!(NSDictionary), dictionaryWithObject: yes, forKey: key];
    if options.is_null() {
        return self::ffi::AXIsProcessTrusted();
    }
    self::ffi::AXIsProcessTrustedWithOptions(options as *const std::ffi::c_void)
}

/// Opaque CFRunLoop pointer. `CFRunLoopStop` is documented as safe to call
/// from another thread; the runloop thread owns the object lifetime.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct SendCfPtr(ffi::CFRunLoopRef);

// SAFETY: stored only as an identity for CFRunLoopStop / clear. Never
// dereferenced as Rust data; the runloop thread creates and tears it down.
#[cfg(target_os = "macos")]
unsafe impl Send for SendCfPtr {}

#[cfg(target_os = "macos")]
static RUNLOOP: Mutex<Option<SendCfPtr>> = Mutex::new(None);

#[cfg(target_os = "macos")]
unsafe fn stop_runloop() {
    if let Ok(slot) = RUNLOOP.lock() {
        if let Some(SendCfPtr(rl)) = *slot {
            ffi::CFRunLoopStop(rl);
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn runloop_thread(want_keys: bool, want_scroll: bool) {
    let rl = ffi::CFRunLoopGetCurrent();
    if let Ok(mut slot) = RUNLOOP.lock() {
        *slot = Some(SendCfPtr(rl));
    }

    let mut ports = Vec::new();
    if want_keys {
        if let Some(port) = create_tap(
            ffi::kCGEventTapOptionListenOnly,
            key_mask(),
            key_callback,
            TapKind::Keys as usize,
        ) {
            if let Some(src) = attach(rl, port) {
                ports.push((port, src));
            }
        } else {
            log::info!("keyboard tap unavailable (Input Monitoring denied?)");
        }
    }
    if want_scroll {
        if let Some(port) = create_tap(
            ffi::kCGEventTapOptionDefault,
            scroll_mask(),
            scroll_callback,
            TapKind::Scroll as usize,
        ) {
            if let Some(src) = attach(rl, port) {
                ports.push((port, src));
            }
        } else {
            log::info!("scroll tap unavailable (Accessibility denied?)");
        }
    }

    ffi::CFRunLoopRun();

    for (port, src) in ports {
        ffi::CGEventTapEnable(port, false);
        ffi::CFRunLoopRemoveSource(rl, src, ffi::kCFRunLoopCommonModes);
        ffi::CFMachPortInvalidate(port);
        ffi::CFRelease(src);
        ffi::CFRelease(port);
    }
    if let Ok(mut slot) = RUNLOOP.lock() {
        *slot = None;
    }
}

#[cfg(target_os = "macos")]
fn key_mask() -> u64 {
    // Disabled-by-timeout / user-input are delivered to the callback
    // regardless of mask; their type codes are not valid bit positions.
    (1u64 << ffi::kCGEventKeyDown)
        | (1u64 << ffi::kCGEventKeyUp)
        | (1u64 << ffi::kCGEventFlagsChanged)
}

#[cfg(target_os = "macos")]
fn scroll_mask() -> u64 {
    1u64 << ffi::kCGEventScrollWheel
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(usize)]
enum TapKind {
    Keys = 1,
    Scroll = 2,
}

#[cfg(target_os = "macos")]
static KEY_PORT: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
#[cfg(target_os = "macos")]
static SCROLL_PORT: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[cfg(target_os = "macos")]
unsafe fn create_tap(
    options: u32,
    mask: u64,
    callback: ffi::CGEventTapCallBack,
    kind: usize,
) -> Option<ffi::CFMachPortRef> {
    let port = ffi::CGEventTapCreate(
        ffi::kCGSessionEventTap,
        ffi::kCGHeadInsertEventTap,
        options,
        mask,
        callback,
        kind as *mut std::ffi::c_void,
    );
    if port.is_null() {
        return None;
    }
    ffi::CGEventTapEnable(port, true);
    if kind == TapKind::Keys as usize {
        KEY_PORT.store(port, Ordering::SeqCst);
    } else {
        SCROLL_PORT.store(port, Ordering::SeqCst);
    }
    Some(port)
}

#[cfg(target_os = "macos")]
unsafe fn attach(rl: ffi::CFRunLoopRef, port: ffi::CFMachPortRef) -> Option<ffi::CFRunLoopSourceRef> {
    let src = ffi::CFMachPortCreateRunLoopSource(std::ptr::null_mut(), port, 0);
    if src.is_null() {
        ffi::CFMachPortInvalidate(port);
        ffi::CFRelease(port);
        return None;
    }
    ffi::CFRunLoopAddSource(rl, src, ffi::kCFRunLoopCommonModes);
    Some(src)
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn key_callback(
    _proxy: ffi::CGEventTapProxy,
    ty: u32,
    event: ffi::CGEventRef,
    _info: *mut std::ffi::c_void,
) -> ffi::CGEventRef {
    if ty == ffi::kCGEventTapDisabledByTimeout || ty == ffi::kCGEventTapDisabledByUserInput {
        let port = KEY_PORT.load(Ordering::SeqCst);
        if !port.is_null() {
            ffi::CGEventTapEnable(port, true);
        }
        return event;
    }
    let keycode = ffi::CGEventGetIntegerValueField(event, ffi::kCGKeyboardEventKeycode) as u16;
    let autorepeat = ffi::CGEventGetIntegerValueField(event, ffi::kCGKeyboardEventAutorepeat) != 0;
    match ty {
        ffi::kCGEventKeyDown => crate::keysounds::handle_key(keycode, true, autorepeat),
        ffi::kCGEventKeyUp => crate::keysounds::handle_key(keycode, false, false),
        ffi::kCGEventFlagsChanged => {
            crate::keysounds::handle_key(keycode, modifier_is_down(event, keycode), false);
        }
        _ => {}
    }
    event
}

#[cfg(target_os = "macos")]
unsafe fn modifier_is_down(event: ffi::CGEventRef, keycode: u16) -> bool {
    let flags = ffi::CGEventGetFlags(event);
    match keycode {
        56 | 60 => flags & ffi::kCGEventFlagMaskShift != 0,
        59 | 62 => flags & ffi::kCGEventFlagMaskControl != 0,
        58 | 61 => flags & ffi::kCGEventFlagMaskAlternate != 0,
        54 | 55 => flags & ffi::kCGEventFlagMaskCommand != 0,
        _ => flags != 0,
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn scroll_callback(
    _proxy: ffi::CGEventTapProxy,
    ty: u32,
    event: ffi::CGEventRef,
    _info: *mut std::ffi::c_void,
) -> ffi::CGEventRef {
    if ty == ffi::kCGEventTapDisabledByTimeout || ty == ffi::kCGEventTapDisabledByUserInput {
        let port = SCROLL_PORT.load(Ordering::SeqCst);
        if !port.is_null() {
            ffi::CGEventTapEnable(port, true);
        }
        return event;
    }
    if ty != ffi::kCGEventScrollWheel {
        return event;
    }
    let tag = ffi::CGEventGetIntegerValueField(event, ffi::kCGEventSourceUserData);
    if tag == crate::scroll::synthetic_tag() {
        return event;
    }
    let settings = crate::settings::get_app_settings();
    if !settings.smooth_scroll_enabled && !settings.reverse_mouse_scroll {
        return event;
    }
    if let Some(bundle) = frontmost_bundle_id() {
        if crate::scroll::is_excluded(&bundle, &settings.scroll_excluded_apps) {
            return event;
        }
    }
    let continuous =
        ffi::CGEventGetIntegerValueField(event, ffi::kCGScrollWheelEventIsContinuous) != 0;
    match crate::scroll::classify_wheel(continuous, false) {
        crate::scroll::WheelKind::PassThrough | crate::scroll::WheelKind::Excluded => event,
        crate::scroll::WheelKind::Smooth => {
            if !settings.smooth_scroll_enabled {
                if settings.reverse_mouse_scroll {
                    reverse_event(event);
                }
                return event;
            }
            let dy = ffi::CGEventGetIntegerValueField(event, ffi::kCGScrollWheelEventDeltaAxis1);
            let dx = ffi::CGEventGetIntegerValueField(event, ffi::kCGScrollWheelEventDeltaAxis2);
            let flags = ffi::CGEventGetFlags(event);
            let shift = flags & ffi::kCGEventFlagMaskShift != 0;
            crate::scroll::ingest_wheel(dx as f64, dy as f64, shift);
            std::ptr::null_mut()
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn reverse_event(event: ffi::CGEventRef) {
    for field in [
        ffi::kCGScrollWheelEventDeltaAxis1,
        ffi::kCGScrollWheelEventDeltaAxis2,
        ffi::kCGScrollWheelEventPointDeltaAxis1,
        ffi::kCGScrollWheelEventPointDeltaAxis2,
    ] {
        let value = ffi::CGEventGetIntegerValueField(event, field);
        ffi::CGEventSetIntegerValueField(event, field, -value);
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod ffi {
    #![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

    use std::ffi::c_void;

    pub type CGEventRef = *mut c_void;
    pub type CFMachPortRef = *mut c_void;
    pub type CFRunLoopSourceRef = *mut c_void;
    pub type CFRunLoopRef = *mut c_void;
    pub type CFAllocatorRef = *mut c_void;
    pub type CFStringRef = *const c_void;
    pub type CGEventTapProxy = *mut c_void;
    pub type CGEventTapCallBack = unsafe extern "C" fn(
        CGEventTapProxy,
        u32,
        CGEventRef,
        *mut c_void,
    ) -> CGEventRef;

    pub const kCGSessionEventTap: u32 = 1;
    pub const kCGHIDEventTap: u32 = 0;
    pub const kCGHeadInsertEventTap: u32 = 0;
    pub const kCGEventTapOptionDefault: u32 = 0;
    pub const kCGEventTapOptionListenOnly: u32 = 1;

    pub const kCGEventKeyDown: u32 = 10;
    pub const kCGEventKeyUp: u32 = 11;
    pub const kCGEventFlagsChanged: u32 = 12;
    pub const kCGEventScrollWheel: u32 = 22;
    pub const kCGEventTapDisabledByTimeout: u32 = 0xFFFF_FFFE;
    pub const kCGEventTapDisabledByUserInput: u32 = 0xFFFF_FFFF;

    pub const kCGKeyboardEventAutorepeat: u32 = 8;
    pub const kCGKeyboardEventKeycode: u32 = 9;
    pub const kCGScrollWheelEventDeltaAxis1: u32 = 11;
    pub const kCGScrollWheelEventDeltaAxis2: u32 = 12;
    pub const kCGScrollWheelEventIsContinuous: u32 = 88;
    pub const kCGScrollWheelEventScrollPhase: u32 = 99;
    pub const kCGScrollWheelEventPointDeltaAxis1: u32 = 96;
    pub const kCGScrollWheelEventPointDeltaAxis2: u32 = 97;
    pub const kCGScrollWheelEventMomentumPhase: u32 = 123;
    pub const kCGEventSourceUserData: u32 = 42;
    pub const kCGScrollEventUnitPixel: u32 = 0;

    pub const kCGEventFlagMaskShift: u64 = 0x0002_0000;
    pub const kCGEventFlagMaskControl: u64 = 0x0004_0000;
    pub const kCGEventFlagMaskAlternate: u64 = 0x0008_0000;
    pub const kCGEventFlagMaskCommand: u64 = 0x0010_0000;

    pub const kIOHIDRequestTypeListenEvent: u32 = 1;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            eventsOfInterest: u64,
            callback: CGEventTapCallBack,
            userInfo: *mut c_void,
        ) -> CFMachPortRef;
        pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        pub fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        pub fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
        pub fn CGEventGetDoubleValueField(event: CGEventRef, field: u32) -> f64;
        pub fn CGEventSetDoubleValueField(event: CGEventRef, field: u32, value: f64);
        pub fn CGEventGetFlags(event: CGEventRef) -> u64;
        pub fn CGEventCreateScrollWheelEvent2(
            source: *mut c_void,
            units: u32,
            wheelCount: u32,
            wheel1: i32,
            wheel2: i32,
            wheel3: i32,
        ) -> CGEventRef;
        pub fn CGEventPost(tap: u32, event: CGEventRef);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFRelease(cf: *const c_void);
        pub fn CFMachPortCreateRunLoopSource(
            allocator: CFAllocatorRef,
            port: CFMachPortRef,
            order: i64,
        ) -> CFRunLoopSourceRef;
        pub fn CFMachPortInvalidate(port: CFMachPortRef);
        pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        pub fn CFRunLoopRemoveSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        pub fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, returnAfterSourceHandled: bool) -> i32;
        pub fn CFRunLoopRun();
        pub fn CFRunLoopStop(rl: CFRunLoopRef);
        pub static kCFRunLoopDefaultMode: CFStringRef;
        pub static kCFRunLoopCommonModes: CFStringRef;
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        pub fn IOHIDCheckAccess(requestType: u32) -> u32;
        pub fn IOHIDRequestAccess(requestType: u32) -> bool;
    }

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        pub fn IsSecureEventInputEnabled() -> bool;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        pub fn AXIsProcessTrusted() -> bool;
        pub fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_labels() {
        assert_eq!(PermissionStatus::Granted.label(), "Granted");
        assert_eq!(PermissionStatus::Denied.label(), "Not granted");
        assert_eq!(PermissionStatus::Unsupported.label(), "macOS only");
        assert!(PermissionStatus::Granted.granted());
        assert!(!PermissionStatus::Denied.granted());
    }

    #[test]
    fn sync_is_idle_when_features_are_off() {
        crate::eventtap::sync();
        let guard = runtime().lock().unwrap();
        assert!(!guard.want_keys);
        assert!(!guard.want_scroll);
        assert!(guard.handle.is_none());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn linux_reports_unsupported_permissions() {
        assert_eq!(input_monitoring_status(), PermissionStatus::Unsupported);
        assert_eq!(accessibility_status(), PermissionStatus::Unsupported);
        assert!(!is_secure_input());
        assert!(frontmost_bundle_id().is_none());
        assert!(running_conflict_ids().is_empty());
    }
}
