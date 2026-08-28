//! Keep-awake via IOPM assertions (High Alert).
//!
//! Creating an assertion is one IPC into powerd; after that the row lives in
//! powerd and this process takes no wakeups. Timed expiry uses
//! `kIOPMAssertionTimeoutActionRelease` so powerd drops the assertion itself.
//! Manual High Alert and pomodoro work-phase auto-awake share one assertion
//! through owner bits — releasing one must not kill the other.
//!
//! Self-contained: WP05's battery/`power.rs` is a different module on another
//! branch. Low-battery auto-release registers an IOPS run-loop source only
//! while an assertion is live.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::time::Duration;

const OWNER_MANUAL: u32 = 1;
const OWNER_POMODORO: u32 = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HighAlertKind {
    #[default]
    Display,
    System,
}

impl HighAlertKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Display => "Display",
            Self::System => "System",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighAlertOwner {
    Manual,
    Pomodoro,
}

impl HighAlertOwner {
    fn bit(self) -> u32 {
        match self {
            Self::Manual => OWNER_MANUAL,
            Self::Pomodoro => OWNER_POMODORO,
        }
    }
}

static OWNERS: AtomicU32 = AtomicU32::new(0);
static KIND: AtomicU8 = AtomicU8::new(0);
static TIMEOUT_SECS: AtomicU32 = AtomicU32::new(0);
static LOW_BATTERY_PCT: AtomicU8 = AtomicU8::new(10);
static UI_STALE: AtomicBool = AtomicBool::new(false);

pub fn is_active() -> bool {
    OWNERS.load(Ordering::Relaxed) != 0
}

pub fn is_held_by(owner: HighAlertOwner) -> bool {
    OWNERS.load(Ordering::Relaxed) & owner.bit() != 0
}

/// Island tick / render: powerd may have released a timed assertion, or the
/// low-battery callback cleared owners. Cheap atomic; no IOKit.
pub fn take_ui_stale() -> bool {
    UI_STALE.swap(false, Ordering::SeqCst)
}

pub fn set_low_battery_release_pct(pct: u8) {
    LOW_BATTERY_PCT.store(pct, Ordering::Relaxed);
}

/// Acquire or refresh `owner`'s hold. `timeout` is stored for Manual so a
/// later pomodoro release can recreate the timed assertion. Pomodoro passes
/// `None` (held until the work phase ends).
pub fn acquire(
    owner: HighAlertOwner,
    kind: HighAlertKind,
    timeout: Option<Duration>,
) -> Result<(), String> {
    KIND.store(kind as u8, Ordering::Relaxed);
    if owner == HighAlertOwner::Manual {
        TIMEOUT_SECS.store(
            timeout
                .map(|d| d.as_secs().min(u32::MAX as u64) as u32)
                .unwrap_or(0),
            Ordering::Relaxed,
        );
    }
    OWNERS.fetch_or(owner.bit(), Ordering::SeqCst);
    apply_assertion()
}

pub fn release(owner: HighAlertOwner) {
    OWNERS.fetch_and(!owner.bit(), Ordering::SeqCst);
    let _ = apply_assertion();
}

pub fn release_all() {
    OWNERS.store(0, Ordering::SeqCst);
    TIMEOUT_SECS.store(0, Ordering::Relaxed);
    native_release();
    stop_battery_watch();
}

/// Drop the IOPS source once no owner remains (safe outside the callback).
pub fn reap_idle() {
    if OWNERS.load(Ordering::Relaxed) == 0 {
        stop_battery_watch();
    }
}

fn current_kind() -> HighAlertKind {
    if KIND.load(Ordering::Relaxed) == HighAlertKind::System as u8 {
        HighAlertKind::System
    } else {
        HighAlertKind::Display
    }
}

fn apply_assertion() -> Result<(), String> {
    let owners = OWNERS.load(Ordering::SeqCst);
    if owners == 0 {
        native_release();
        stop_battery_watch();
        return Ok(());
    }
    // Timed expiry only when Manual is the sole owner. A live pomodoro hold
    // must not be killed by powerd when the chip countdown ends.
    let timeout = if owners == OWNER_MANUAL {
        match TIMEOUT_SECS.load(Ordering::Relaxed) {
            0 => None,
            secs => Some(Duration::from_secs(secs as u64)),
        }
    } else {
        None
    };
    native_acquire(current_kind(), timeout)?;
    start_battery_watch();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn native_acquire(_kind: HighAlertKind, _timeout: Option<Duration>) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn native_release() {}

#[cfg(not(target_os = "macos"))]
fn start_battery_watch() {}

#[cfg(not(target_os = "macos"))]
fn stop_battery_watch() {}

#[cfg(target_os = "macos")]
fn native_acquire(kind: HighAlertKind, timeout: Option<Duration>) -> Result<(), String> {
    macos::acquire(kind, timeout)
}

#[cfg(target_os = "macos")]
fn native_release() {
    macos::release();
}

#[cfg(target_os = "macos")]
fn start_battery_watch() {
    macos::start_battery_watch();
}

#[cfg(target_os = "macos")]
fn stop_battery_watch() {
    macos::stop_battery_watch();
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_void};
    use std::ptr;
    use std::sync::atomic::AtomicPtr;

    type CfTypeRef = *const c_void;
    type CfStringRef = *const c_void;
    type CfArrayRef = *const c_void;
    type CfDictionaryRef = *const c_void;
    type CfNumberRef = *const c_void;
    type CfBooleanRef = *const c_void;
    type CfAllocatorRef = *const c_void;
    type CfRunLoopRef = *mut c_void;
    type CfRunLoopSourceRef = *mut c_void;
    type CfRunLoopMode = *const c_void;

    const K_CFSTRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_CF_NUMBER_SINT32_TYPE: i32 = 3;
    const K_IO_RETURN_SUCCESS: i32 = 0;
    const LEVEL_ON: i32 = 255;

    static ASSERTION_ID: AtomicU32 = AtomicU32::new(0);
    static BATTERY_SRC: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

    #[link(name = "IOKit", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithDescription(
            assertion_type: CfStringRef,
            name: CfStringRef,
            details: CfStringRef,
            human_readable_reason: CfStringRef,
            localization_bundle_path: CfStringRef,
            timeout: f64,
            timeout_action: CfStringRef,
            assertion_id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(assertion_id: u32) -> i32;

        fn IOPSNotificationCreateRunLoopSource(
            callback: Option<unsafe extern "C" fn(*mut c_void)>,
            context: *mut c_void,
        ) -> CfRunLoopSourceRef;
        fn IOPSCopyPowerSourcesInfo() -> CfTypeRef;
        fn IOPSCopyPowerSourcesList(blob: CfTypeRef) -> CfArrayRef;
        fn IOPSGetPowerSourceDescription(blob: CfTypeRef, ps: CfTypeRef) -> CfDictionaryRef;

        fn CFRelease(cf: CfTypeRef);
        fn CFGetTypeID(cf: CfTypeRef) -> usize;
        fn CFStringCreateWithCString(
            alloc: CfAllocatorRef,
            c_str: *const c_char,
            encoding: u32,
        ) -> CfStringRef;
        fn CFArrayGetCount(array: CfArrayRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CfArrayRef, idx: isize) -> CfTypeRef;
        fn CFDictionaryGetValue(dict: CfDictionaryRef, key: CfTypeRef) -> CfTypeRef;
        fn CFNumberGetTypeID() -> usize;
        fn CFNumberGetValue(number: CfNumberRef, the_type: i32, value_ptr: *mut c_void) -> u8;
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(boolean: CfBooleanRef) -> u8;
        fn CFRunLoopGetMain() -> CfRunLoopRef;
        fn CFRunLoopAddSource(rl: CfRunLoopRef, source: CfRunLoopSourceRef, mode: CfRunLoopMode);
        fn CFRunLoopRemoveSource(rl: CfRunLoopRef, source: CfRunLoopSourceRef, mode: CfRunLoopMode);
        static kCFRunLoopDefaultMode: CfStringRef;
    }

    fn cfstr(text: &CStr) -> CfStringRef {
        unsafe { CFStringCreateWithCString(ptr::null(), text.as_ptr(), K_CFSTRING_ENCODING_UTF8) }
    }

    fn cf_release(cf: CfTypeRef) {
        if !cf.is_null() {
            unsafe { CFRelease(cf) };
        }
    }

    pub fn acquire(kind: HighAlertKind, timeout: Option<Duration>) -> Result<(), String> {
        release();
        let type_key = match kind {
            HighAlertKind::Display => c"PreventUserIdleDisplaySleep",
            HighAlertKind::System => c"PreventUserIdleSystemSleep",
        };
        let assertion_type = cfstr(type_key);
        let name = cfstr(c"openNook High Alert");
        let details = cfstr(c"Keeping the Mac awake from the island.");
        let timeout_secs = timeout.map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let action = if timeout_secs > 0.0 {
            cfstr(c"TimeoutActionRelease")
        } else {
            ptr::null()
        };
        let mut id = 0u32;
        let status = unsafe {
            IOPMAssertionCreateWithDescription(
                assertion_type,
                name,
                details,
                ptr::null(),
                ptr::null(),
                timeout_secs,
                action,
                &mut id,
            )
        };
        cf_release(assertion_type);
        cf_release(name);
        cf_release(details);
        cf_release(action);
        if status != K_IO_RETURN_SUCCESS || id == 0 {
            return Err(format!("IOPM assertion failed ({status})"));
        }
        ASSERTION_ID.store(id, Ordering::SeqCst);
        Ok(())
    }

    pub fn release() {
        let id = ASSERTION_ID.swap(0, Ordering::SeqCst);
        if id != 0 {
            unsafe {
                let _ = IOPMAssertionRelease(id);
            }
        }
    }

    pub fn start_battery_watch() {
        if !BATTERY_SRC.load(Ordering::SeqCst).is_null() {
            return;
        }
        let src = unsafe { IOPSNotificationCreateRunLoopSource(Some(on_power_source), ptr::null_mut()) };
        if src.is_null() {
            return;
        }
        unsafe {
            CFRunLoopAddSource(CFRunLoopGetMain(), src, kCFRunLoopDefaultMode);
        }
        BATTERY_SRC.store(src, Ordering::SeqCst);
    }

    pub fn stop_battery_watch() {
        let src = BATTERY_SRC.swap(ptr::null_mut(), Ordering::SeqCst);
        if src.is_null() {
            return;
        }
        unsafe {
            CFRunLoopRemoveSource(CFRunLoopGetMain(), src, kCFRunLoopDefaultMode);
            CFRelease(src as CfTypeRef);
        }
    }

    unsafe extern "C" fn on_power_source(_ctx: *mut c_void) {
        let threshold = LOW_BATTERY_PCT.load(Ordering::Relaxed);
        if threshold == 0 {
            return;
        }
        if OWNERS.load(Ordering::Relaxed) == 0 {
            return;
        }
        if should_release_for_battery(threshold) {
            // Don't tear down the run-loop source from inside its own callback.
            OWNERS.store(0, Ordering::SeqCst);
            release();
            UI_STALE.store(true, Ordering::SeqCst);
            log::info!("High Alert released: battery at or below {threshold}%");
        }
    }

    fn should_release_for_battery(threshold: u8) -> bool {
        let blob = unsafe { IOPSCopyPowerSourcesInfo() };
        if blob.is_null() {
            return false;
        }
        let list = unsafe { IOPSCopyPowerSourcesList(blob) };
        if list.is_null() {
            cf_release(blob);
            return false;
        }
        let mut release = false;
        unsafe {
            let count = CFArrayGetCount(list);
            for i in 0..count {
                let ps = CFArrayGetValueAtIndex(list, i);
                let dict = IOPSGetPowerSourceDescription(blob, ps);
                if dict.is_null() {
                    continue;
                }
                if dict_bool(dict, c"Is Charging") {
                    continue;
                }
                let state = dict_string(dict, c"Power Source State");
                if state.as_deref() == Some("AC Power") {
                    continue;
                }
                let Some(cur) = dict_i32(dict, c"Current Capacity") else {
                    continue;
                };
                let max = dict_i32(dict, c"Max Capacity").unwrap_or(100).max(1);
                let pct = ((cur as i64 * 100) / max as i64) as u8;
                if pct <= threshold {
                    release = true;
                    break;
                }
            }
        }
        cf_release(list as CfTypeRef);
        cf_release(blob);
        release
    }

    fn dict_i32(dict: CfDictionaryRef, key: &CStr) -> Option<i32> {
        let cf_key = cfstr(key);
        let value = unsafe { CFDictionaryGetValue(dict, cf_key) };
        cf_release(cf_key);
        if value.is_null() {
            return None;
        }
        unsafe {
            if CFGetTypeID(value) != CFNumberGetTypeID() {
                return None;
            }
            let mut out: i32 = 0;
            if CFNumberGetValue(value as CfNumberRef, K_CF_NUMBER_SINT32_TYPE, &mut out as *mut i32 as *mut c_void)
                == 0
            {
                return None;
            }
            Some(out)
        }
    }

    fn dict_bool(dict: CfDictionaryRef, key: &CStr) -> bool {
        let cf_key = cfstr(key);
        let value = unsafe { CFDictionaryGetValue(dict, cf_key) };
        cf_release(cf_key);
        if value.is_null() {
            return false;
        }
        unsafe { CFGetTypeID(value) == CFBooleanGetTypeID() && CFBooleanGetValue(value as CfBooleanRef) != 0 }
    }

    fn dict_string(dict: CfDictionaryRef, key: &CStr) -> Option<String> {
        let cf_key = cfstr(key);
        let value = unsafe { CFDictionaryGetValue(dict, cf_key) };
        cf_release(cf_key);
        if value.is_null() {
            return None;
        }
        // Power Source State is a CFString; reuse GetCString via a small stack buf.
        unsafe extern "C" {
            fn CFStringGetTypeID() -> usize;
            fn CFStringGetCString(
                string: CfStringRef,
                buffer: *mut c_char,
                buffer_size: isize,
                encoding: u32,
            ) -> u8;
        }
        unsafe {
            if CFGetTypeID(value) != CFStringGetTypeID() {
                return None;
            }
            let mut buf = [0i8; 64];
            if CFStringGetCString(
                value as CfStringRef,
                buf.as_mut_ptr(),
                buf.len() as isize,
                K_CFSTRING_ENCODING_UTF8,
            ) == 0
            {
                return None;
            }
            CStr::from_ptr(buf.as_ptr()).to_str().ok().map(str::to_string)
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        release_all();
        UI_STALE.store(false, Ordering::SeqCst);
    }

    #[test]
    fn owners_refcount_manual_and_pomodoro() {
        reset();
        assert!(!is_active());
        acquire(HighAlertOwner::Manual, HighAlertKind::Display, Some(Duration::from_secs(60)))
            .unwrap();
        assert!(is_active());
        assert!(is_held_by(HighAlertOwner::Manual));
        acquire(HighAlertOwner::Pomodoro, HighAlertKind::Display, None).unwrap();
        assert!(is_held_by(HighAlertOwner::Pomodoro));
        release(HighAlertOwner::Manual);
        assert!(is_active(), "pomodoro hold survives manual off");
        assert!(!is_held_by(HighAlertOwner::Manual));
        release(HighAlertOwner::Pomodoro);
        assert!(!is_active());
        reset();
    }

    #[test]
    fn release_all_clears_every_owner() {
        reset();
        let _ = acquire(HighAlertOwner::Manual, HighAlertKind::System, None);
        let _ = acquire(HighAlertOwner::Pomodoro, HighAlertKind::System, None);
        release_all();
        assert!(!is_active());
        assert!(!is_held_by(HighAlertOwner::Manual));
        assert!(!is_held_by(HighAlertOwner::Pomodoro));
    }
}
