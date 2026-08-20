use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::{Emitter, Manager};

/// User setting: hide the island when another app is maximized/fullscreen.
/// Defaults to true so the requested behavior is on unless the user opts out.
static HIDE_WHEN_MAXIMIZED_ENABLED: AtomicBool = AtomicBool::new(true);

/// Latest OS detection result (another app is maximized/fullscreen on the island's display).
static OTHER_APP_MAXIMIZED: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn set_hide_when_maximized(enabled: bool) {
    HIDE_WHEN_MAXIMIZED_ENABLED.store(enabled, Ordering::Relaxed);
}

/// True when the island should stay hidden and click-through.
pub fn should_hide_for_maximized() -> bool {
    HIDE_WHEN_MAXIMIZED_ENABLED.load(Ordering::Relaxed)
        && OTHER_APP_MAXIMIZED.load(Ordering::Relaxed)
}

pub fn setup_maximized_monitoring(app_handle: AppHandle) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        std::thread::spawn(move || {
            const POLL_MS: u64 = 200;
            loop {
                let maximized = other_app_is_maximized();
                let was_maximized = OTHER_APP_MAXIMIZED.swap(maximized, Ordering::Relaxed);
                if maximized != was_maximized {
                    if maximized {
                        log::debug!("[maximized] another app is maximized/fullscreen");
                        let _ = app_handle.emit("app-maximized", ());
                    } else {
                        log::debug!("[maximized] no maximized/fullscreen app");
                        let _ = app_handle.emit("app-unmaximized", ());
                    }

                    // Keep the overlay click-through while hidden so hover monitoring
                    // cannot steal clicks from the maximized app.
                    if should_hide_for_maximized() {
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.set_ignore_cursor_events(true);
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
            }
        });
    }

    #[cfg(target_os = "linux")]
    {
        let _ = app_handle;
        let _ = should_hide_for_maximized();
        log::info!("Maximized-window monitoring is not implemented on Linux.");
    }
}

#[cfg(target_os = "macos")]
fn other_app_is_maximized() -> bool {
    use objc2::runtime::AnyObject;
    use objc2::*;
    use objc2::{Encode, Encoding};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    unsafe impl Encode for CGSize {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    unsafe impl Encode for CGPoint {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    unsafe impl Encode for CGRect {
        const ENCODING: Encoding =
            Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(
            option: u32,
            relative_to_window: u32,
        ) -> *mut std::ffi::c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *mut std::ffi::c_void);
    }

    // On-screen windows, exclude desktop wallpaper elements.
    const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0;
    const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;

    let our_pid = std::process::id() as i32;

    unsafe {
        let pool: *mut AnyObject = msg_send![class!(NSAutoreleasePool), new];

        let main_screen: *mut AnyObject = msg_send![class!(NSScreen), mainScreen];
        if main_screen.is_null() {
            let _: () = msg_send![pool, drain];
            return false;
        }

        let frame: CGRect = msg_send![main_screen, frame];
        let visible: CGRect = msg_send![main_screen, visibleFrame];
        let screen_width = frame.size.width;
        let screen_height = frame.size.height;
        // Zoomed windows fill visibleFrame; fullscreen windows are at least that large.
        let target_width = visible.size.width;
        let target_height = visible.size.height;

        let info = CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            0,
        );
        if info.is_null() {
            let _: () = msg_send![pool, drain];
            return false;
        }

        let array = info as *mut AnyObject;
        let count: usize = msg_send![array, count];

        let pid_key: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: b"kCGWindowOwnerPID\0".as_ptr() as *const i8
        ];
        let layer_key: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: b"kCGWindowLayer\0".as_ptr() as *const i8
        ];
        let bounds_key: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: b"kCGWindowBounds\0".as_ptr() as *const i8
        ];
        let x_key: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: b"X\0".as_ptr() as *const i8];
        let y_key: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: b"Y\0".as_ptr() as *const i8];
        let w_key: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: b"Width\0".as_ptr() as *const i8];
        let h_key: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: b"Height\0".as_ptr() as *const i8];

        let mut found = false;
        const SIZE_SLOP: f64 = 40.0;

        for i in 0..count {
            let dict: *mut AnyObject = msg_send![array, objectAtIndex: i];
            if dict.is_null() {
                continue;
            }

            let layer_obj: *mut AnyObject = msg_send![dict, objectForKey: layer_key];
            if !layer_obj.is_null() {
                let layer: i32 = msg_send![layer_obj, intValue];
                // Layer 0 is normal app windows (Dock/menu bar/overlays use higher layers).
                if layer != 0 {
                    continue;
                }
            }

            let pid_obj: *mut AnyObject = msg_send![dict, objectForKey: pid_key];
            if pid_obj.is_null() {
                continue;
            }
            let pid: i32 = msg_send![pid_obj, intValue];
            if pid == our_pid || pid <= 0 {
                continue;
            }

            let bounds: *mut AnyObject = msg_send![dict, objectForKey: bounds_key];
            if bounds.is_null() {
                continue;
            }

            let x_obj: *mut AnyObject = msg_send![bounds, objectForKey: x_key];
            let y_obj: *mut AnyObject = msg_send![bounds, objectForKey: y_key];
            let w_obj: *mut AnyObject = msg_send![bounds, objectForKey: w_key];
            let h_obj: *mut AnyObject = msg_send![bounds, objectForKey: h_key];
            if w_obj.is_null() || h_obj.is_null() || x_obj.is_null() || y_obj.is_null() {
                continue;
            }

            let x: f64 = msg_send![x_obj, doubleValue];
            let y: f64 = msg_send![y_obj, doubleValue];
            let width: f64 = msg_send![w_obj, doubleValue];
            let height: f64 = msg_send![h_obj, doubleValue];

            // CGWindow bounds origin is top-left of the primary display.
            let cx = x + width / 2.0;
            let cy = y + height / 2.0;
            let on_main = cx >= 0.0 && cx < screen_width && cy >= 0.0 && cy < screen_height;
            if !on_main {
                continue;
            }

            if width >= target_width - SIZE_SLOP && height >= target_height - SIZE_SLOP {
                found = true;
                break;
            }
        }

        CFRelease(info);
        let _: () = msg_send![pool, drain];
        found
    }
}

#[cfg(target_os = "windows")]
fn other_app_is_maximized() -> bool {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowRect, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, IsZoomed, MONITORINFOF_PRIMARY,
    };

    struct EnumState {
        our_pid: u32,
        found: bool,
    }

    fn is_cloaked(hwnd: HWND) -> bool {
        let mut cloaked: u32 = 0;
        let result = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            )
        };
        result.is_ok() && cloaked != 0
    }

    fn class_name(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let len = unsafe { GetClassNameW(hwnd, &mut buf) };
        if len <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }

    fn is_shell_window(class: &str) -> bool {
        matches!(
            class,
            "Progman"
                | "WorkerW"
                | "Shell_TrayWnd"
                | "Shell_SecondaryTrayWnd"
                | "NotifyIconOverflowWindow"
                | "ForegroundStaging"
                | "XamlExplorerHostIslandWindow"
        )
    }

    fn is_on_primary(hwnd: HWND) -> bool {
        unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                rcMonitor: RECT::default(),
                rcWork: RECT::default(),
                dwFlags: 0,
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return false;
            }
            info.dwFlags & MONITORINFOF_PRIMARY != 0
        }
    }

    fn covers_monitor(hwnd: HWND) -> bool {
        unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                rcMonitor: RECT::default(),
                rcWork: RECT::default(),
                dwFlags: 0,
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return false;
            }
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return false;
            }
            const SLOP: i32 = 4;
            rect.left <= info.rcMonitor.left + SLOP
                && rect.top <= info.rcMonitor.top + SLOP
                && rect.right >= info.rcMonitor.right - SLOP
                && rect.bottom >= info.rcMonitor.bottom - SLOP
        }
    }

    fn is_hide_worthy(hwnd: HWND, our_pid: u32) -> bool {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                return false;
            }
            if is_cloaked(hwnd) {
                return false;
            }

            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
            if pid == 0 || pid == our_pid {
                return false;
            }

            let class = class_name(hwnd);
            if is_shell_window(&class) {
                return false;
            }

            if !is_on_primary(hwnd) {
                return false;
            }

            IsZoomed(hwnd).as_bool() || covers_monitor(hwnd)
        }
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &mut *(lparam.0 as *mut EnumState) };
        if is_hide_worthy(hwnd, state.our_pid) {
            state.found = true;
            BOOL(0)
        } else {
            BOOL(1)
        }
    }

    let mut state = EnumState {
        our_pid: std::process::id(),
        found: false,
    };
    let _ = unsafe { EnumWindows(Some(enum_cb), LPARAM(&mut state as *mut EnumState as isize)) };
    state.found
}
