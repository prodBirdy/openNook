#[cfg(target_os = "macos")]
#[link(name = "ServiceManagement", kind = "framework")]
extern "C" {}

#[cfg(target_os = "macos")]
pub fn is_supported() -> bool {
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
        let identifier: *mut AnyObject = msg_send![bundle, bundleIdentifier];
        !identifier.is_null()
    }
}

#[cfg(not(target_os = "macos"))]
pub fn is_supported() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn is_enabled() -> bool {
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let service: *mut AnyObject = msg_send![class!(SMAppService), mainAppService];
        let status: isize = msg_send![service, status];
        status == 1
    }
}

#[cfg(not(target_os = "macos"))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn set_enabled(on: bool) -> Result<(), String> {
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        let service: *mut AnyObject = msg_send![class!(SMAppService), mainAppService];
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let ok: bool = if on {
            msg_send![service, registerAndReturnError: &mut error]
        } else {
            msg_send![service, unregisterAndReturnError: &mut error]
        };
        if ok {
            return Ok(());
        }
        if !error.is_null() {
            let description: *mut AnyObject = msg_send![error, localizedDescription];
            let utf8: *const std::ffi::c_char = msg_send![description, UTF8String];
            if !utf8.is_null() {
                return Err(std::ffi::CStr::from_ptr(utf8)
                    .to_string_lossy()
                    .into_owned());
            }
        }
        Err("Unable to update Launch at Login".into())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_enabled(_: bool) -> Result<(), String> {
    Err("Launch at Login is unavailable".into())
}
