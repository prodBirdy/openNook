use crate::models::NotchInfo;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use tauri::{Emitter, LogicalPosition, LogicalSize, Manager, WebviewWindow, Window};

#[cfg(target_os = "macos")]
use objc2::{Encode, Encoding};

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
unsafe impl Encode for CGSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
unsafe impl Encode for CGPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[cfg(target_os = "macos")]
unsafe impl Encode for CGRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NSEdgeInsets {
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
}

#[cfg(target_os = "macos")]
unsafe impl Encode for NSEdgeInsets {
    const ENCODING: Encoding = Encoding::Struct(
        "NSEdgeInsets",
        &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING],
    );
}

/// Global storage for the actual UI element bounds (set by frontend)
/// Format: (x, y, width, height) in screen coordinates
static UI_BOUNDS: std::sync::OnceLock<RwLock<Option<UiBounds>>> = std::sync::OnceLock::new();

/// Global storage for window settings
static WINDOW_SETTINGS: std::sync::OnceLock<RwLock<WindowSettings>> = std::sync::OnceLock::new();

/// Window size settings (adjustable by the user)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowSettings {
    /// Extra width added to the base window size (default: 400.0)
    pub extra_width: f64,
    /// Extra height added to the base window size (default: 200.0)
    pub extra_height: f64,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            extra_width: 400.0,
            extra_height: 200.0,
        }
    }
}

fn get_window_settings_store() -> &'static RwLock<WindowSettings> {
    WINDOW_SETTINGS.get_or_init(|| RwLock::new(WindowSettings::default()))
}

#[derive(Debug, Clone, Copy)]
pub struct UiBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

fn get_ui_bounds_store() -> &'static RwLock<Option<UiBounds>> {
    UI_BOUNDS.get_or_init(|| RwLock::new(None))
}

/// Update the actual UI element bounds (called from frontend when element resizes)
#[tauri::command]
pub fn update_ui_bounds(x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
    let store = get_ui_bounds_store();
    let mut bounds = store.write().map_err(|e| e.to_string())?;
    *bounds = Some(UiBounds {
        x,
        y,
        width,
        height,
    });
    Ok(())
}

/// Get screen dimensions on macOS including notch width
/// Returns (screen_width, screen_height, notch_height, notch_width)
#[cfg(target_os = "macos")]
fn get_screen_info() -> (f64, f64, f64, f64) {
    // Define our own CGSize/CGRect to avoid deprecated cocoa crate fields

    use objc2::runtime::AnyObject;
    use objc2::*;

    unsafe {
        let main_screen: *mut AnyObject = msg_send![class!(NSScreen), mainScreen];

        if main_screen.is_null() {
            return (0.0, 0.0, 0.0, 0.0);
        }

        // Get screen frame
        let frame: CGRect = msg_send![main_screen, frame];
        let screen_width = frame.size.width;
        let screen_height = frame.size.height;

        // Get safeAreaInsets (macOS 12.0+)

        let insets: NSEdgeInsets = msg_send![main_screen, safeAreaInsets];
        let safe_area_top = insets.top;
        // safe_area_bottom unused

        // Calculate notch height
        // The notch height on MacBooks is typically around 30-40px
        // The Dynamic Island style expands beyond just the notch
        // We use a slightly larger value for the Dynamic Island effect
        let notch_height = if safe_area_top >= 0.0 {
            // Approximate island height based on screen height
            (screen_height * 0.1).max(38.0).min(52.0)
        } else {
            0.0
        };
        // Calculate notch width
        // The notch width on MacBooks is typically around 180-200px
        // The Dynamic Island style expands beyond just the notch
        // We use a wider calculation to match the expanded Island look
        let notch_width = if safe_area_top > 0.0 {
            // Approximate notch width based on screen width
            // We use a slightly larger value for the Dynamic Island effect
            (screen_width * 0.1).max(200.0).min(260.0)
        } else {
            // Fallback width
            180.0
        };

        (screen_width, screen_height, notch_height, notch_width)
    }
}

/// Get notch information from the main screen using NSScreen.safeAreaInsets (macOS 12.0+)
#[tauri::command]
pub fn get_notch_info() -> NotchInfo {
    let (screen_width, screen_height, notch_height, notch_width) = get_screen_info();
    let has_notch = notch_height > 0.0;
    let visible_height = screen_height - notch_height;

    NotchInfo {
        has_notch,
        notch_height,
        notch_width,
        screen_width,
        screen_height,
        visible_height,
    }
}

/// Position the window at the notch location (centered at top of screen)
#[tauri::command]
pub fn position_at_notch(window: Window) -> Result<(), String> {
    let (screen_width, _screen_height, _notch_height, notch_width) = get_screen_info();

    // Use notch width if available, otherwise fall back to current window width
    let target_width = if notch_width > 0.0 {
        notch_width
    } else {
        let window_size = window.outer_size().map_err(|e| e.to_string())?;
        let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;
        window_size.width as f64 / scale_factor
    };

    // Center horizontally, position at very top (y=0)
    let x = (screen_width - target_width) / 2.0;
    let y = 0.0;

    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Resize and position window to fit the notch area
/// The window is positioned at y=0 (top of screen) to overlap with the notch
#[tauri::command]
pub fn fit_to_notch(window: Window, width: f64, height: f64) -> Result<(), String> {
    let (screen_width, _screen_height, _notch_height, _notch_width) = get_screen_info();

    // Resize the window
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;

    // Center horizontally, position at very top (y=0) to overlap with notch
    let x = (screen_width - width) / 2.0;
    let y = 0.0;

    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Set whether the window should ignore mouse events (click-through)
/// When true, clicks pass through to the underlying application
#[tauri::command]
pub fn set_click_through(window: Window, ignore: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(ignore)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Get current window settings
#[tauri::command]
pub fn get_window_settings() -> WindowSettings {
    let store = get_window_settings_store();
    *store.read().unwrap_or_else(|e| e.into_inner())
}

/// Update window settings
#[tauri::command]
pub fn update_window_settings(
    window: WebviewWindow,
    extra_width: f64,
    extra_height: f64,
) -> Result<(), String> {
    // Update the stored settings
    {
        let store = get_window_settings_store();
        let mut settings = store.write().map_err(|e| e.to_string())?;
        settings.extra_width = extra_width;
        settings.extra_height = extra_height;
    }

    // Apply the new window size
    setup_fixed_window_size(&window)
}

/// Set up the window with a fixed size based on notch dimensions and settings.
/// The window always uses: width = (notch_width + 160) + extra_width, height = notch_height + extra_height
pub fn setup_fixed_window_size(window: &WebviewWindow) -> Result<(), String> {
    let (screen_width, _screen_height, notch_height, notch_width) = get_screen_info();
    let settings = get_window_settings();

    // Calculate fixed window dimensions
    let target_width = (notch_width + 160.0) + settings.extra_width;
    let target_height = notch_height + settings.extra_height;

    // Resize the window
    window
        .set_size(LogicalSize::new(target_width, target_height))
        .map_err(|e| e.to_string())?;

    // Center horizontally, position at very top (y=0) to overlap with notch
    let x = (screen_width - target_width) / 2.0;
    let y = 0.0;

    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Legacy function for backwards compatibility - now just sets up fixed window size
#[deprecated(note = "Window is now always fixed size. Use setup_fixed_window_size instead.")]
pub fn resize_window_for_hover(window: &WebviewWindow, _is_hovered: bool) -> Result<(), String> {
    setup_fixed_window_size(window)
}

/// Resize window to fixed size (Tauri command wrapper)
/// Note: is_hovered parameter is ignored - window is always fixed size now
#[tauri::command]
pub fn resize_for_hover(window: WebviewWindow, _is_hovered: bool) -> Result<(), String> {
    setup_fixed_window_size(&window)
}

/// Activate the window (focus it)
/// Uses native macOS APIs to properly activate an accessory app
#[tauri::command]
pub fn activate_window(window: Window) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::*;
        use raw_window_handle::HasWindowHandle;

        unsafe {
            // Get NSApplication shared instance and activate it
            let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];

            // Also make the window key and bring it to front
            if let Ok(handle) = window.window_handle() {
                if let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() {
                    let ns_view = appkit_handle.ns_view.as_ptr() as *mut AnyObject;
                    let ns_win: *mut AnyObject = msg_send![ns_view, window];
                    let _: () =
                        msg_send![ns_win, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
                }
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        window.set_focus().map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Trigger haptic feedback on macOS
#[tauri::command]
pub fn trigger_haptics() {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::*;

        let manager: *mut AnyObject = msg_send![class!(NSHapticFeedbackManager), defaultPerformer];
        let _: () = msg_send![manager, performFeedbackPattern: 0_i64, performanceTime: 1_i64];
    }
}

/// Setup global mouse monitoring for the window
/// Uses fast polling for minimal latency hover detection
#[cfg(target_os = "macos")]
pub fn setup_mouse_monitoring(app_handle: tauri::AppHandle) {
    use objc2::runtime::AnyObject;
    use objc2::*;

    // Track whether mouse is currently in the UI area
    static IS_INSIDE: AtomicBool = AtomicBool::new(false);

    // Get initial screen info
    let (screen_width, screen_height, notch_height, notch_width) = get_screen_info();

    // Pre-compute window position (window is centered at top)
    let settings = get_window_settings();
    let win_width = notch_width + 160.0 + settings.extra_width;
    let window_x = (screen_width - win_width) / 2.0;

    // Compute fallback detection area
    let fallback_x_start = (screen_width - notch_width) / 2.0;
    let fallback_x_end = fallback_x_start + notch_width;
    let fallback_y_end = notch_height;

    // Spawn monitoring thread
    std::thread::spawn(move || {
        let mut cached_screen_height = screen_height;
        let mut refresh_counter: u16 = 0;

        // Hysteresis to prevent flicker
        const PADDING_ENTER: f64 = 3.0;
        const PADDING_EXIT: f64 = 8.0;

        // Fast polling for low latency
        const POLL_MS: u64 = 20; // ~50fps

        loop {
            // Get mouse position
            let (mouse_x, flipped_y) = unsafe {
                let mouse_loc: CGPoint = msg_send![class!(NSEvent), mouseLocation];

                // Refresh screen height occasionally
                refresh_counter = refresh_counter.wrapping_add(1);
                if refresh_counter % 500 == 0 {
                    let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
                    if !screens.is_null() {
                        let primary: *mut AnyObject = msg_send![screens, objectAtIndex: 0_u64];
                        if !primary.is_null() {
                            let frame: CGRect = msg_send![primary, frame];
                            cached_screen_height = frame.size.height;
                        }
                    }
                }

                (mouse_loc.x, cached_screen_height - mouse_loc.y)
            };

            let was_inside = IS_INSIDE.load(Ordering::Relaxed);

            // OPTIMIZATION: Broad interaction zone check.
            // Only perform precise bounds checks if the mouse is roughly in the top-middle area.
            // If we're far away and not already "inside", skip the heavy logic.
            let broad_padding_x = 300.0;
            let broad_limit_y = 250.0;
            let is_in_interaction_zone = mouse_x >= (fallback_x_start - broad_padding_x)
                && mouse_x <= (fallback_x_end + broad_padding_x)
                && flipped_y <= broad_limit_y;

            if !is_in_interaction_zone && !was_inside {
                std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
                continue;
            }

            let padding = if was_inside {
                PADDING_EXIT
            } else {
                PADDING_ENTER
            };

            // Check UI bounds or fallback to notch area
            let in_ui_area = if let Ok(guard) = get_ui_bounds_store().try_read() {
                if let Some(bounds) = *guard {
                    let sx = window_x + bounds.x;
                    let sy = bounds.y;
                    mouse_x >= (sx - padding)
                        && mouse_x <= (sx + bounds.width + padding)
                        && flipped_y >= (sy - padding)
                        && flipped_y <= (sy + bounds.height + padding)
                } else {
                    mouse_x >= (fallback_x_start - padding)
                        && mouse_x <= (fallback_x_end + padding)
                        && flipped_y >= -padding
                        && flipped_y <= (fallback_y_end + padding)
                }
            } else {
                mouse_x >= (fallback_x_start - padding)
                    && mouse_x <= (fallback_x_end + padding)
                    && flipped_y >= -padding
                    && flipped_y <= (fallback_y_end + padding)
            };

            // State transitions - emit events immediately
            if in_ui_area && !was_inside {
                IS_INSIDE.store(true, Ordering::Relaxed);

                // Emit event first for UI responsiveness
                let _ = app_handle.emit("mouse-entered-notch", ());

                // Set cursor events and activate using native APIs (non-blocking)
                unsafe {
                    // Activate app
                    let ns_app: *mut AnyObject =
                        msg_send![class!(NSApplication), sharedApplication];
                    let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
                }

                // Set ignore cursor events via Tauri (this is fast)
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.set_ignore_cursor_events(false);
                }
            } else if !in_ui_area && was_inside {
                IS_INSIDE.store(false, Ordering::Relaxed);

                // Emit event first
                let _ = app_handle.emit("mouse-exited-notch", ());

                // Disable cursor events
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.set_ignore_cursor_events(true);
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
        }
    });
}
