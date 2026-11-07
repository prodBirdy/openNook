//! Where openNook stands with macOS's privacy permissions, for Settings › Permissions, and a
//! way to ask. System Audio Recording has no public status call: like every app with a Core
//! Audio tap, this asks TCC itself (`TCCAccessPreflight` / `TCCAccessRequest`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Allowed,
    Denied,
    /// Never asked (or macOS won't say).
    NotAsked,
}

/// System Settings panes, by permission.
pub const AUDIO_CAPTURE_PANE: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture";
pub const LOCATION_PANE: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices";
pub const AUTOMATION_PANE: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation";

/// System Audio Recording, which the music meter's tap needs.
pub fn audio_capture() -> Access {
    #[cfg(target_os = "macos")]
    return macos::audio_capture();
    #[cfg(not(target_os = "macos"))]
    Access::NotAsked
}

/// Asks for System Audio Recording (macOS shows its prompt once; after that it's System
/// Settings). Returns at once; read [`audio_capture`] again for the answer.
pub fn request_audio_capture() {
    #[cfg(target_os = "macos")]
    macos::request_audio_capture();
}

/// Location Services for openNook.
pub fn location() -> Access {
    #[cfg(target_os = "macos")]
    return macos::location();
    #[cfg(not(target_os = "macos"))]
    Access::NotAsked
}

/// The app macOS asks on openNook's behalf, when that isn't openNook: a development build
/// started from a terminal is that terminal's, and its permissions are the terminal's.
pub fn asked_for() -> Option<String> {
    #[cfg(target_os = "macos")]
    return macos::asked_for();
    #[cfg(not(target_os = "macos"))]
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use super::Access;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    use std::ffi::{c_void, CStr};

    const AUDIO_CAPTURE: &str = "kTCCServiceAudioCapture";

    fn tcc(symbol: &CStr) -> *mut c_void {
        unsafe extern "C" {
            fn dlopen(path: *const i8, mode: i32) -> *mut c_void;
            fn dlsym(handle: *mut c_void, name: *const i8) -> *mut c_void;
        }
        const RTLD_NOW: i32 = 2;
        unsafe {
            let handle = dlopen(c"/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC".as_ptr(), RTLD_NOW);
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            dlsym(handle, symbol.as_ptr())
        }
    }

    pub fn audio_capture() -> Access {
        let f = tcc(c"TCCAccessPreflight");
        if f.is_null() {
            return Access::NotAsked;
        }
        let preflight: unsafe extern "C" fn(*const NSString, *const c_void) -> i64 = unsafe { std::mem::transmute(f) };
        let service = NSString::from_str(AUDIO_CAPTURE);
        match unsafe { preflight(&*service, std::ptr::null()) } {
            0 => Access::Allowed,
            1 => Access::Denied,
            _ => Access::NotAsked,
        }
    }

    pub fn request_audio_capture() {
        let f = tcc(c"TCCAccessRequest");
        if f.is_null() {
            return;
        }
        let request: unsafe extern "C" fn(*const NSString, *const c_void, *const block2::Block<dyn Fn(Bool)>) =
            unsafe { std::mem::transmute(f) };
        let service = NSString::from_str(AUDIO_CAPTURE);
        let done = block2::RcBlock::new(|granted: Bool| log::info!("permissions: system audio recording {}", if granted.as_bool() { "allowed" } else { "denied" }));
        unsafe { request(&*service, std::ptr::null(), &*done) };
    }

    pub fn location() -> Access {
        unsafe {
            let manager: *mut AnyObject = msg_send![class!(CLLocationManager), new];
            if manager.is_null() {
                return Access::NotAsked;
            }
            let status: i32 = msg_send![manager, authorizationStatus];
            let _: () = msg_send![manager, release];
            // kCLAuthorizationStatus: 0 not determined, 1 restricted, 2 denied, 3 always, 4 when in use.
            match status {
                3 | 4 => Access::Allowed,
                1 | 2 => Access::Denied,
                _ => Access::NotAsked,
            }
        }
    }

    pub fn asked_for() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        if exe.to_string_lossy().contains(".app/Contents/MacOS/") {
            return None;
        }
        unsafe extern "C" {
            fn dlsym(handle: *mut c_void, name: *const i8) -> *mut c_void;
        }
        const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
        let f = unsafe { dlsym(RTLD_DEFAULT, c"responsibility_get_pid_responsible_for_pid".as_ptr()) };
        let pid = std::process::id() as i32;
        let responsible = if f.is_null() {
            pid
        } else {
            let get: unsafe extern "C" fn(i32) -> i32 = unsafe { std::mem::transmute(f) };
            unsafe { get(pid) }
        };
        let named = (responsible > 0 && responsible != pid).then(|| unsafe {
            let app: *mut AnyObject = msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: responsible];
            if app.is_null() {
                return None;
            }
            let name: *mut NSString = msg_send![app, localizedName];
            name.as_ref().map(|n| n.to_string())
        });
        named.flatten().or_else(terminal).or_else(|| Some("your terminal".into()))
    }

    /// The terminal by `TERM_PROGRAM`, as the app is called.
    fn terminal() -> Option<String> {
        let program = std::env::var("TERM_PROGRAM").ok()?;
        Some(match program.as_str() {
            "ghostty" => "Ghostty".into(),
            "Apple_Terminal" => "Terminal".into(),
            "iTerm.app" => "iTerm".into(),
            "WarpTerminal" => "Warp".into(),
            "vscode" => "Visual Studio Code".into(),
            other => other.to_owned(),
        })
    }
}
