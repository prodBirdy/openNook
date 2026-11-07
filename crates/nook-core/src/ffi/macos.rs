//! Canonical macOS framework FFI — one signature per symbol.
//!
//! Call sites must import from here instead of declaring their own
//! `extern "C"` blocks. Duplicate declarations with mismatched ABI
//! (bool vs Boolean, `*const` vs `*mut`) are undefined behavior at link time.

#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code
)]

use std::ffi::{c_char, c_void};

// --- Types -----------------------------------------------------------------

pub type Boolean = u8;
pub type CFIndex = isize;
pub type CFTypeID = usize;
pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFAllocatorRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFNumberRef = *const c_void;
pub type CFBooleanRef = *const c_void;
pub type CFRunLoopRef = *mut c_void;
pub type CFRunLoopSourceRef = *mut c_void;
pub type CFMachPortRef = *mut c_void;

pub type AXUIElementRef = *const c_void;
pub type AXObserverRef = *const c_void;
pub type AXValueRef = *const c_void;

pub type CGEventRef = *mut c_void;
pub type CGEventSourceRef = *mut c_void;
pub type CGEventTapProxy = *mut c_void;
pub type CGEventTapCallBack =
    unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

pub type AudioObjectID = u32;
pub type OSStatus = i32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AudioObjectPropertyAddress {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}

pub type AudioObjectPropertyListenerProc = Option<
    unsafe extern "C" fn(
        AudioObjectID,
        u32,
        *const AudioObjectPropertyAddress,
        *mut c_void,
    ) -> OSStatus,
>;

pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;

// CGEventTap / keyboard constants used by eventtap + scroll
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

// --- CoreFoundation --------------------------------------------------------

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    pub fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        cStr: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    pub fn CFStringGetCString(
        s: CFStringRef,
        buf: *mut c_char,
        size: CFIndex,
        encoding: u32,
    ) -> Boolean;
    pub fn CFStringGetLength(s: CFStringRef) -> CFIndex;
    pub fn CFStringGetMaximumSizeForEncoding(len: CFIndex, encoding: u32) -> CFIndex;
    pub fn CFStringGetTypeID() -> CFTypeID;
    pub fn CFArrayGetTypeID() -> CFTypeID;
    pub fn CFArrayGetCount(arr: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(arr: CFArrayRef, idx: CFIndex) -> CFTypeRef;
    pub fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    pub fn CFDictionaryCreate(
        allocator: CFAllocatorRef,
        keys: *const *const c_void,
        values: *const *const c_void,
        n: CFIndex,
        key_cb: *const c_void,
        value_cb: *const c_void,
    ) -> CFDictionaryRef;
    pub fn CFNumberGetTypeID() -> CFTypeID;
    pub fn CFNumberGetValue(number: CFNumberRef, the_type: i32, value_ptr: *mut c_void) -> Boolean;
    pub fn CFBooleanGetTypeID() -> CFTypeID;
    pub fn CFBooleanGetValue(boolean: CFBooleanRef) -> Boolean;
    pub fn CFPreferencesAppSynchronize(applicationID: CFStringRef) -> Boolean;
    pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub fn CFRunLoopGetMain() -> CFRunLoopRef;
    pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFRunLoopRemoveSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFRunLoopRun();
    pub fn CFRunLoopStop(rl: CFRunLoopRef);
    pub fn CFRunLoopRunInMode(
        mode: CFStringRef,
        seconds: f64,
        returnAfterSourceHandled: Boolean,
    ) -> i32;
    pub fn CFMachPortCreateRunLoopSource(
        allocator: CFAllocatorRef,
        port: CFMachPortRef,
        order: CFIndex,
    ) -> CFRunLoopSourceRef;
    pub fn CFMachPortInvalidate(port: CFMachPortRef);

    pub static kCFBooleanTrue: CFTypeRef;
    pub static kCFBooleanFalse: CFTypeRef;
    pub static kCFRunLoopDefaultMode: CFStringRef;
    pub static kCFRunLoopCommonModes: CFStringRef;
    pub static kCFTypeDictionaryKeyCallBacks: c_void;
    pub static kCFTypeDictionaryValueCallBacks: c_void;
}

// --- Accessibility (ApplicationServices) -----------------------------------

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub fn AXIsProcessTrusted() -> Boolean;
    pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
    pub fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    pub fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> i32;
    pub fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> i32;
    pub fn AXValueCreate(the_type: u32, value_ptr: *const c_void) -> AXValueRef;
    pub fn AXObserverCreate(
        pid: i32,
        callback: extern "C" fn(AXObserverRef, AXUIElementRef, CFStringRef, *mut c_void),
        out_observer: *mut AXObserverRef,
    ) -> i32;
    pub fn AXObserverAddNotification(
        observer: AXObserverRef,
        element: AXUIElementRef,
        notification: CFStringRef,
        refcon: *mut c_void,
    ) -> i32;
    pub fn AXObserverGetRunLoopSource(observer: AXObserverRef) -> CFRunLoopSourceRef;
    pub static kAXTrustedCheckOptionPrompt: CFTypeRef;
}

// --- CoreGraphics ----------------------------------------------------------

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub fn CGMainDisplayID() -> u32;
    pub fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        virtualKey: u16,
        keyDown: bool,
    ) -> CGEventRef;
    pub fn CGEventSetFlags(event: CGEventRef, flags: u64);
    pub fn CGEventPost(tap: u32, event: CGEventRef);
    pub fn CGEventPostToPid(pid: i32, event: CGEventRef);
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
    pub fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
}

// --- CoreAudio -------------------------------------------------------------

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    pub fn AudioObjectHasProperty(
        object: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
    ) -> Boolean;
    pub fn AudioObjectGetPropertyDataSize(
        object: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> OSStatus;
    pub fn AudioObjectGetPropertyData(
        object: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> OSStatus;
    pub fn AudioObjectSetPropertyData(
        object: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: u32,
        data: *const c_void,
    ) -> OSStatus;
    pub fn AudioObjectAddPropertyListener(
        object: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        listener: AudioObjectPropertyListenerProc,
        client: *mut c_void,
    ) -> OSStatus;
    pub fn AudioObjectRemovePropertyListener(
        object: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        listener: AudioObjectPropertyListenerProc,
        client: *mut c_void,
    ) -> OSStatus;
}

// --- IOKit (HID + power) ---------------------------------------------------

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    pub fn IOHIDCheckAccess(requestType: u32) -> u32;
    pub fn IOHIDRequestAccess(requestType: u32) -> Boolean;
    pub fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    pub fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
    pub fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
    pub fn IOPSGetBatteryWarningLevel() -> i32;
    pub fn IOPSGetTimeRemainingEstimate() -> f64;
    pub fn IOPSNotificationCreateRunLoopSource(
        callback: Option<unsafe extern "C" fn(*mut c_void)>,
        context: *mut c_void,
    ) -> CFRunLoopSourceRef;
    pub fn IOPMAssertionCreateWithDescription(
        assertion_type: CFStringRef,
        name: CFStringRef,
        details: CFStringRef,
        human_readable_reason: CFStringRef,
        localization_bundle_path: CFStringRef,
        timeout: f64,
        timeout_action: CFStringRef,
        assertion: *mut u32,
    ) -> i32;
    pub fn IOPMAssertionRelease(assertion: u32) -> i32;
}
