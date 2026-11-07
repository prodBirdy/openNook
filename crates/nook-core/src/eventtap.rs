//! Accessibility permission helpers for widget controls.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    Unsupported,
}

/// Accessibility TCC via `AXIsProcessTrustedWithOptions`. `prompt` shows the
/// system dialog (and deep-links if the user agrees).
pub fn ax_trusted(prompt: bool) -> bool {
    #[cfg(target_os = "macos")]
    {
        use crate::ffi::macos::{AXIsProcessTrustedWithOptions, CFDictionaryRef};
        use objc2::runtime::AnyObject;
        use objc2::*;
        use std::ptr;
        unsafe {
            let opts = if prompt {
                let key: *mut AnyObject = msg_send![
                    class!(NSString),
                    stringWithUTF8String: c"AXTrustedCheckOptionPrompt".as_ptr()
                ];
                let yes: *mut AnyObject = msg_send![class!(NSNumber), numberWithBool: true];
                let dict: *mut AnyObject =
                    msg_send![class!(NSDictionary), dictionaryWithObject: yes, forKey: key];
                dict as CFDictionaryRef
            } else {
                ptr::null()
            };
            AXIsProcessTrustedWithOptions(opts) != 0
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = prompt;
        false
    }
}

pub fn accessibility_status() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        if ax_trusted(false) {
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
    ax_trusted(true)
}

pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    {
        let _ = open::that(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        );
    }
}
