//! Built-in panel brightness via private DisplayServices (dlopen / dlsym).
//!
//! External DDC/CI displays are out of scope. If the private symbols vanish
//! on a future macOS, [`available`] stays false and the brightness HUD hides.

use crate::sysvol::{self, HudKind};
use std::sync::atomic::{AtomicBool, Ordering};

/// True when get/set symbols resolved and the internal panel answered.
pub fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

pub fn brightness() -> Option<f32> {
    #[cfg(target_os = "macos")]
    {
        macos::read()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn set_brightness(value: f32) {
    let value = sysvol::clamp_unit(value);
    #[cfg(target_os = "macos")]
    macos::write(value);
    let _ = value;
}

/// Probe private frameworks and subscribe to brightness-change notifications.
pub fn start() {
    #[cfg(target_os = "macos")]
    macos::start();
}

static AVAILABLE: AtomicBool = AtomicBool::new(false);

/// DisplayServices / CoreDisplay symbol names we probe at startup.
pub const DISPLAY_SERVICES_PATH: &str =
    "/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices";
pub const CORE_DISPLAY_PATH: &str = "/System/Library/Frameworks/CoreDisplay.framework/CoreDisplay";
pub const GET_BRIGHTNESS_SYMBOL: &str = "DisplayServicesGetBrightness";
pub const SET_BRIGHTNESS_SYMBOL: &str = "DisplayServicesSetBrightness";
pub const REGISTER_BRIGHTNESS_SYMBOL: &str =
    "DisplayServicesRegisterForBrightnessChangeNotifications";
pub const CORE_GET_SYMBOL: &str = "CoreDisplay_Display_GetUserBrightness";
pub const CORE_SET_SYMBOL: &str = "CoreDisplay_Display_SetUserBrightness";

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use crate::sysvol::clamp_unit;
    use std::ffi::{c_char, c_void, CString};
    use std::sync::Once;

    const RTLD_LAZY: i32 = 1;
    const OBSERVER_ID: u32 = 0x4e4f4b31; // "NOK1"

    type GetBrightnessFn = unsafe extern "C" fn(u32, *mut f32) -> i32;
    type SetBrightnessFn = unsafe extern "C" fn(u32, f32) -> i32;
    type RegisterFn = unsafe extern "C" fn(u32, u32, BrightnessCallback) -> i32;
    type BrightnessCallback =
        unsafe extern "C" fn(*mut c_void, u32, *mut c_void, *const c_void, *const c_void);
    type CoreGetFn = unsafe extern "C" fn(u32) -> f64;
    type CoreSetFn = unsafe extern "C" fn(u32, f64);

    struct Api {
        get: Option<GetBrightnessFn>,
        set: Option<SetBrightnessFn>,
        register: Option<RegisterFn>,
        core_get: Option<CoreGetFn>,
        core_set: Option<CoreSetFn>,
    }

    static STARTED: Once = Once::new();
    static API: std::sync::OnceLock<Api> = std::sync::OnceLock::new();

    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
    }

    fn load_sym<T>(handle: *mut c_void, name: &str) -> Option<T> {
        if handle.is_null() {
            return None;
        }
        let c_name = CString::new(name).ok()?;
        let ptr = unsafe { dlsym(handle, c_name.as_ptr()) };
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute_copy(&ptr) })
        }
    }

    fn open(path: &str) -> *mut c_void {
        let c_path = match CString::new(path) {
            Ok(p) => p,
            Err(_) => return std::ptr::null_mut(),
        };
        unsafe { dlopen(c_path.as_ptr(), RTLD_LAZY) }
    }

    fn probe() -> Api {
        let ds = open(super::DISPLAY_SERVICES_PATH);
        let cd = open(super::CORE_DISPLAY_PATH);
        let api = Api {
            get: load_sym(ds, super::GET_BRIGHTNESS_SYMBOL),
            set: load_sym(ds, super::SET_BRIGHTNESS_SYMBOL),
            register: load_sym(ds, super::REGISTER_BRIGHTNESS_SYMBOL),
            core_get: load_sym(cd, super::CORE_GET_SYMBOL),
            core_set: load_sym(cd, super::CORE_SET_SYMBOL),
        };
        if api.get.is_none() && api.core_get.is_none() {
            log::warn!(
                "DisplayServices/CoreDisplay brightness symbols missing; brightness HUD hidden"
            );
        }
        api
    }

    fn api() -> &'static Api {
        API.get_or_init(probe)
    }

    fn display_id() -> u32 {
        unsafe { CGMainDisplayID() }
    }

    pub(super) fn read() -> Option<f32> {
        let api = api();
        let id = display_id();
        if let Some(get) = api.get {
            let mut value = 0.0f32;
            let status = unsafe { get(id, &mut value) };
            if status == 0 && value.is_finite() {
                return Some(clamp_unit(value));
            }
        }
        if let Some(get) = api.core_get {
            let value = unsafe { get(id) } as f32;
            if value.is_finite() {
                return Some(clamp_unit(value));
            }
        }
        None
    }

    pub(super) fn write(value: f32) {
        let api = api();
        let id = display_id();
        let mut ok = false;
        if let Some(set) = api.set {
            ok = unsafe { set(id, value) } == 0;
        }
        if !ok {
            if let Some(set) = api.core_set {
                unsafe { set(id, value as f64) };
            }
        }
    }

    unsafe extern "C" fn on_brightness_changed(
        _passthrough: *mut c_void,
        _display: u32,
        _name: *mut c_void,
        _sender: *const c_void,
        _info: *const c_void,
    ) {
        if let Some(value) = read() {
            sysvol::publish(HudKind::Brightness, value);
        }
    }

    pub(super) fn start() {
        STARTED.call_once(|| {
            let api = api();
            match read() {
                Some(_) => {
                    AVAILABLE.store(true, Ordering::Relaxed);
                    log::info!("brightness HUD available (DisplayServices/CoreDisplay)");
                }
                None => {
                    AVAILABLE.store(false, Ordering::Relaxed);
                    log::warn!("internal panel brightness unread; brightness HUD hidden");
                    return;
                }
            }
            if let Some(register) = api.register {
                let status = unsafe { register(display_id(), OBSERVER_ID, on_brightness_changed) };
                if status != 0 {
                    log::warn!(
                        "DisplayServicesRegisterForBrightnessChangeNotifications failed ({status}); brightness HUD will only update from the island slider"
                    );
                }
            } else {
                log::warn!("brightness-change notifications unavailable; no brightness HUD from the keys");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_symbol_names_match_monitorcontrol() {
        assert!(DISPLAY_SERVICES_PATH.contains("DisplayServices.framework"));
        assert_eq!(GET_BRIGHTNESS_SYMBOL, "DisplayServicesGetBrightness");
        assert_eq!(SET_BRIGHTNESS_SYMBOL, "DisplayServicesSetBrightness");
        assert_eq!(
            REGISTER_BRIGHTNESS_SYMBOL,
            "DisplayServicesRegisterForBrightnessChangeNotifications"
        );
        assert_eq!(CORE_GET_SYMBOL, "CoreDisplay_Display_GetUserBrightness");
        assert_eq!(CORE_SET_SYMBOL, "CoreDisplay_Display_SetUserBrightness");
    }

    #[test]
    fn setters_are_safe_off_macos() {
        set_brightness(0.3);
        #[cfg(not(target_os = "macos"))]
        {
            assert!(!available());
            assert_eq!(brightness(), None);
        }
    }
}
