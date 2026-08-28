//! Tier-1 Thaw: hide other menu-bar extras by stretching our own separator.
//!
//! Other processes' `NSStatusItem`s are untouchable. Dozer / Hidden Bar / Ice
//! all do the same public trick: a separator item whose `length` is set to a
//! huge value so everything the user ⌘-dragged to its *left* is pushed past
//! the screen edge. Show restores a few points. No TCC, no idle work, no
//! Screen Recording (Ice-bar capture is a later flag).

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicPtr;

/// `NSStatusItem` length that shoves left-side extras off the display.
pub const HIDDEN_LENGTH: f64 = 10_000.0;
/// `NSVariableStatusItemLength` — shrinks to the chevron while extras are shown.
pub const SHOWN_LENGTH: f64 = -1.0;

static INSTALLED: AtomicBool = AtomicBool::new(false);
static HIDDEN: AtomicBool = AtomicBool::new(false);

/// Length the separator should use for this settings pair.
pub fn desired_length(enabled: bool, hidden: bool) -> Option<f64> {
    if !enabled {
        return None;
    }
    Some(if hidden { HIDDEN_LENGTH } else { SHOWN_LENGTH })
}

pub fn is_hidden() -> bool {
    HIDDEN.load(Ordering::Relaxed)
}

/// Create / remove / resize the separator from the current settings.
/// Main-thread only (AppKit). Does not recreate an existing item so a user's
/// ⌘-drag arrangement survives hide/show toggles.
pub fn install() {
    sync();
}

pub fn sync() {
    #[cfg(target_os = "macos")]
    {
        sync_macos();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let settings = crate::settings::get_app_settings();
        HIDDEN.store(settings.thaw_enabled && settings.thaw_hidden, Ordering::Relaxed);
        INSTALLED.store(settings.thaw_enabled, Ordering::Relaxed);
    }
}

/// Flip hide/show and persist. No-op when Thaw is off.
pub fn toggle() {
    crate::settings::tweak_app_settings(|settings| {
        if settings.thaw_enabled {
            settings.thaw_hidden = !settings.thaw_hidden;
        }
    });
}

#[cfg(target_os = "macos")]
static SEPARATOR: AtomicPtr<objc2::runtime::AnyObject> =
    AtomicPtr::new(std::ptr::null_mut());

#[cfg(target_os = "macos")]
fn sync_macos() {
    let settings = crate::settings::get_app_settings();
    if !settings.thaw_enabled {
        remove_item();
        HIDDEN.store(false, Ordering::Relaxed);
        return;
    }
    ensure_item();
    apply_length(settings.thaw_hidden);
    HIDDEN.store(settings.thaw_hidden, Ordering::Relaxed);
}

#[cfg(target_os = "macos")]
fn ensure_item() {
    use objc2::runtime::AnyObject;
    use objc2::*;

    if !SEPARATOR.load(Ordering::Relaxed).is_null() {
        INSTALLED.store(true, Ordering::Relaxed);
        return;
    }
    install_target();
    unsafe {
        let bar: *mut AnyObject = msg_send![class!(NSStatusBar), systemStatusBar];
        if bar.is_null() {
            log::error!("NSStatusBar missing; Thaw separator not created");
            return;
        }
        let item: *mut AnyObject = msg_send![bar, statusItemWithLength: SHOWN_LENGTH];
        if item.is_null() {
            log::error!("failed to create Thaw NSStatusItem");
            return;
        }
        let _: *mut AnyObject = msg_send![item, retain];
        SEPARATOR.store(item, Ordering::Relaxed);

        let title: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"\u{25C1}".as_ptr()];
        let button: *mut AnyObject = msg_send![item, button];
        if !button.is_null() {
            let _: () = msg_send![button, setTitle: title];
            if let Some(cls) = objc2::runtime::AnyClass::get(c"NookThawTarget") {
                let target: *mut AnyObject = msg_send![cls, new];
                if !target.is_null() {
                    let _: () = msg_send![button, setTarget: target];
                    let _: () = msg_send![button, setAction: sel!(toggleThaw:)];
                }
            }
        }
        INSTALLED.store(true, Ordering::Relaxed);
        log::info!("thaw separator installed");
    }
}

#[cfg(target_os = "macos")]
fn apply_length(hidden: bool) {
    use objc2::runtime::AnyObject;
    use objc2::*;

    let item = SEPARATOR.load(Ordering::Relaxed);
    if item.is_null() {
        return;
    }
    let length = if hidden { HIDDEN_LENGTH } else { SHOWN_LENGTH };
    unsafe {
        let _: () = msg_send![item, setLength: length];
        let title = if hidden { c"\u{25B7}" } else { c"\u{25C1}" };
        let ns: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: title.as_ptr()];
        let button: *mut AnyObject = msg_send![item, button];
        if !button.is_null() && !ns.is_null() {
            let _: () = msg_send![button, setTitle: ns];
        }
    }
}

#[cfg(target_os = "macos")]
fn remove_item() {
    use objc2::runtime::AnyObject;
    use objc2::*;

    let item = SEPARATOR.swap(std::ptr::null_mut(), Ordering::Relaxed);
    INSTALLED.store(false, Ordering::Relaxed);
    if item.is_null() {
        return;
    }
    unsafe {
        let bar: *mut AnyObject = msg_send![class!(NSStatusBar), systemStatusBar];
        if !bar.is_null() {
            let _: () = msg_send![bar, removeStatusItem: item];
        }
        let _: () = msg_send![item, release];
    }
}

#[cfg(target_os = "macos")]
fn install_target() {
    use objc2::ffi::{class_addMethod, objc_allocateClassPair, objc_registerClassPair};
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{class, sel};
    use std::ffi::CString;

    unsafe {
        if AnyClass::get(c"NookThawTarget").is_some() {
            return;
        }
        extern "C" fn toggle_thaw(_this: *mut AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
            toggle();
        }
        let super_cls = class!(NSObject) as *const AnyClass as *mut AnyClass;
        let cls = objc_allocateClassPair(super_cls, c"NookThawTarget".as_ptr(), 0);
        if cls.is_null() {
            log::error!("could not allocate NookThawTarget");
            return;
        }
        let types = CString::new("v@:@").unwrap();
        let imp: Imp =
            std::mem::transmute(toggle_thaw as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject));
        let _ = class_addMethod(cls, sel!(toggleThaw:), imp, types.as_ptr());
        objc_registerClassPair(cls);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_length_is_none_when_disabled() {
        assert_eq!(desired_length(false, false), None);
        assert_eq!(desired_length(false, true), None);
    }

    #[test]
    fn desired_length_stretches_only_while_hidden() {
        assert_eq!(desired_length(true, false), Some(SHOWN_LENGTH));
        assert_eq!(desired_length(true, true), Some(HIDDEN_LENGTH));
        assert!(HIDDEN_LENGTH > 1_000.0);
        assert!(SHOWN_LENGTH < 0.0);
    }
}
