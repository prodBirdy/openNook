use crate::database::{get_connection, log_sql};
use crate::models::NotchInfo;
use log;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window,
};

#[tauri::command]
pub fn open_settings(app_handle: tauri::AppHandle) -> Result<(), String> {
    let _window = if let Some(window) = app_handle.get_webview_window("settings") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        window
    } else {
        WebviewWindowBuilder::new(&app_handle, "settings", WebviewUrl::App("settings".into()))
            .title("Settings")
            .inner_size(600.0, 450.0)
            .resizable(false)
            .visible(true)
            .build()
            .map_err(|e| e.to_string())?
    };

    // Activate the app to ensure the new window is visible and focused
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::*;

        unsafe {
            // Get NSApplication shared instance and activate it
            let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
        }
    }

    Ok(())
}

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
    /// Whether "non notch mode" is active (hides wings, tighter collision)
    #[serde(default)]
    pub non_notch_mode: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            extra_width: 400.0,
            extra_height: 800.0,
            non_notch_mode: false,
        }
    }
}

/// Helper to save settings to DB
fn persist_window_settings(app_handle: &AppHandle, settings: &WindowSettings) {
    if let Ok(conn) = get_connection(app_handle) {
        if let Ok(json) = serde_json::to_string(settings) {
            let sql = "INSERT OR REPLACE INTO settings (key, value) VALUES ('window_settings', ?1)";
            log_sql(sql);
            let _ = conn.execute(sql, rusqlite::params![json]);
        }
    }
}

/// Helper to load settings from DB
fn load_window_settings_from_db(app_handle: &AppHandle) -> WindowSettings {
    if let Ok(conn) = get_connection(app_handle) {
        let sql = "SELECT value FROM settings WHERE key = 'window_settings'";
        log_sql(sql);
        if let Ok(mut stmt) = conn.prepare(sql) {
            let json: Result<String, _> = stmt.query_row([], |row| row.get(0));
            if let Ok(json_str) = json {
                if let Ok(settings) = serde_json::from_str(&json_str) {
                    return settings;
                }
            }
        }
    }
    WindowSettings::default()
}

/// Initialize window settings from DB into memory (call on app setup)
pub fn initialize_window_settings_from_db(app_handle: &AppHandle) {
    let settings = load_window_settings_from_db(app_handle);
    let store = get_window_settings_store();
    if let Ok(mut guard) = store.write() {
        *guard = settings;
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

/// Get screen dimensions
/// Returns (screen_width, screen_height, notch_height, notch_width)
fn get_screen_info(app_handle: Option<&tauri::AppHandle>) -> (f64, f64, f64, f64) {
    #[cfg(target_os = "macos")]
    {
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

            let notch_height = if safe_area_top >= 0.0 {
                (screen_height * 0.1).max(38.0).min(52.0)
            } else {
                0.0
            };

            let notch_width = if safe_area_top > 0.0 {
                (screen_width * 0.1).max(200.0).min(260.0)
            } else {
                180.0
            };

            (screen_width, screen_height, notch_height, notch_width)
        }
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

        unsafe {
            let width = GetSystemMetrics(SM_CXSCREEN) as f64;
            let height = GetSystemMetrics(SM_CYSCREEN) as f64;
            (width, height, 0.0, 0.0)
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Try to get from app handle if available
        if let Some(handle) = app_handle {
            if let Ok(Some(monitor)) = handle.primary_monitor() {
                let size = monitor.size();
                let scale_factor = monitor.scale_factor();
                let width = size.width as f64 / scale_factor;
                let height = size.height as f64 / scale_factor;
                return (width, height, 0.0, 0.0);
            }
        }
        (1920.0, 1080.0, 0.0, 0.0)
    }
}

#[tauri::command]
pub fn get_system_accent_color() -> String {
    #[cfg(target_os = "macos")]
    return crate::utils::get_macos_accent_color();

    #[cfg(target_os = "windows")]
    {
        // Fallback to blue for now, retrieving registry value is a bit more involved
        // and dwmapi GetWindowAttribute uses BGR which needs conversion
        "#007AFF".to_string()
    }

    #[cfg(target_os = "linux")]
    return "#007AFF".to_string();
}

/// Get notch information from the main screen using NSScreen.safeAreaInsets (macOS 12.0+)
#[tauri::command]
pub fn get_notch_info(app_handle: tauri::AppHandle) -> Option<NotchInfo> {
    let (screen_width, screen_height, notch_height, notch_width) =
        get_screen_info(Some(&app_handle));
    let has_notch = notch_height > 0.0;
    let visible_height = screen_height - notch_height;

    Some(NotchInfo {
        has_notch,
        notch_height,
        notch_width,
        screen_width,
        screen_height,
        visible_height,
    })
}

/// Position the window at the notch location (centered at top of screen)
#[tauri::command]
pub fn position_at_notch(window: Window) -> Result<(), String> {
    let (screen_width, _screen_height, _notch_height, notch_width) =
        get_screen_info(Some(window.app_handle()));

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
    let (screen_width, _screen_height, _notch_height, _notch_width) =
        get_screen_info(Some(window.app_handle()));

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
    non_notch_mode: bool,
) -> Result<(), String> {
    // Update the stored settings
    {
        let store = get_window_settings_store();
        let mut settings = store.write().map_err(|e| e.to_string())?;
        settings.extra_width = extra_width;
        settings.extra_height = extra_height;
        settings.non_notch_mode = non_notch_mode;

        persist_window_settings(window.app_handle(), &settings);
    }

    // Apply the new window size to the MAIN window, not the settings window
    if let Some(main_window) = window.app_handle().get_webview_window("main") {
        setup_fixed_window_size(&main_window)?;
    }

    Ok(())
}

/// Set up the window with a fixed size based on notch dimensions and settings.
/// The window always uses: width = (notch_width + 160) + extra_width, height = notch_height + extra_height
pub fn setup_fixed_window_size(window: &WebviewWindow) -> Result<(), String> {
    let (screen_width, _screen_height, notch_height, notch_width) =
        get_screen_info(Some(window.app_handle()));
    let settings = get_window_settings();

    // Calculate fixed window dimensions
    // In non-notch mode, we might want a smaller fixed window if possible, but keeping it consistent is safer for now
    // unless the "too big" comment refers to the window size itself blocking things?
    // If the window is transparent and click-through, size shouldn't matter much visually, but might block clicks if implementation is wrong.
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
            // NSApplicationActivationPolicyRegular = 0
            // Ensure the app is in Regular mode so it appears in the Dock and App Switcher
            let _: () = msg_send![ns_app, setActivationPolicy: 0_i64];
            let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];

            // Re-apply styles to the main notch window to prevent it from disappearing/resetting
            if let Some(main_window) = window.app_handle().get_webview_window("main") {
                if let Ok(handle) = main_window.window_handle() {
                    if let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) =
                        handle.as_raw()
                    {
                        let ns_view = appkit_handle.ns_view.as_ptr() as *mut AnyObject;
                        let ns_win: *mut AnyObject = msg_send![ns_view, window];

                        // Re-apply level and collection behavior
                        let _: () = msg_send![ns_win, setLevel: 25_i64];
                        let _: () = msg_send![ns_win, setCollectionBehavior: 17_u64];
                    }
                }
            }

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

    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::HasWindowHandle;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetForegroundWindow, ShowWindow, SW_RESTORE,
        };

        if let Ok(handle) = window.window_handle() {
            if let raw_window_handle::RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                unsafe {
                    // Non-zero handle
                    let hwnd = HWND(win32_handle.hwnd.get() as _);
                    // Force restore if minimized and bring to front
                    ShowWindow(hwnd, SW_RESTORE);
                    SetForegroundWindow(hwnd);
                }
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        window.set_focus().map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Deactivate the window and reset activation policy (hide from dock)
#[tauri::command]
pub fn deactivate_window(window: Window) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::*;
        use raw_window_handle::HasWindowHandle;

        unsafe {
            // Get NSApplication shared instance
            let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            // NSApplicationActivationPolicyAccessory = 1
            // Revert to Accessory mode so it hides from Dock
            let _: () = msg_send![ns_app, setActivationPolicy: 1_i64];

            // Re-apply styles to the main notch window explicitly
            if let Some(main_window) = window.app_handle().get_webview_window("main") {
                if let Ok(handle) = main_window.window_handle() {
                    if let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) =
                        handle.as_raw()
                    {
                        let ns_view = appkit_handle.ns_view.as_ptr() as *mut AnyObject;
                        let ns_win: *mut AnyObject = msg_send![ns_view, window];

                        let _: () = msg_send![ns_win, setLevel: 25_i64];
                        let _: () = msg_send![ns_win, setCollectionBehavior: 17_u64];
                    }
                }
            }
        }
    }

    Ok(())
}

/// Predefined haptic patterns
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HapticPattern {
    /// Generic haptic (NSHapticFeedbackPattern 0)
    Generic,
    /// Alignment haptic - subtle (NSHapticFeedbackPattern 1)
    Alignment,
    /// Level change haptic - strong (NSHapticFeedbackPattern 2)
    LevelChange,
    /// Light tap
    Light,
    /// Medium tap
    Medium,
    /// Heavy impact
    Heavy,
    /// Selection feedback - quick
    Selection,
    /// Success - double tap pattern
    Success,
    /// Error - triple tap pattern
    Error,
}

/// Haptic configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HapticConfig {
    /// Pattern to use
    pub pattern: HapticPattern,
    /// Intensity (0.0 - 1.0) - maps to pattern selection
    #[serde(default = "default_intensity")]
    pub intensity: f64,
}

fn default_intensity() -> f64 {
    0.6
}

impl Default for HapticConfig {
    fn default() -> Self {
        Self {
            pattern: HapticPattern::Medium,
            intensity: 0.6,
        }
    }
}

/// Trigger haptic feedback on macOS with pattern and intensity control
///
/// # Examples
/// ```typescript
/// // Simple - uses default Medium pattern
/// await invoke('trigger_haptics');
///
/// // With pattern
/// await invoke('trigger_haptics', { config: { pattern: 'light' } });
///
/// // With pattern and intensity
/// await invoke('trigger_haptics', { config: { pattern: 'generic', intensity: 0.8 } });
/// ```
#[tauri::command]
pub fn trigger_haptics(config: Option<HapticConfig>) -> Result<(), String> {
    let config = config.unwrap_or_default();

    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::*;

        let manager: *mut AnyObject = msg_send![class!(NSHapticFeedbackManager), defaultPerformer];

        match config.pattern {
            HapticPattern::Generic => {
                let _: () =
                    msg_send![manager, performFeedbackPattern: 0_i64, performanceTime: 1_i64];
            }
            HapticPattern::Alignment => {
                let _: () =
                    msg_send![manager, performFeedbackPattern: 1_i64, performanceTime: 1_i64];
            }
            HapticPattern::LevelChange => {
                let _: () =
                    msg_send![manager, performFeedbackPattern: 2_i64, performanceTime: 1_i64];
            }
            HapticPattern::Light => {
                // Alignment pattern (subtle)
                let _: () =
                    msg_send![manager, performFeedbackPattern: 1_i64, performanceTime: 1_i64];
            }
            HapticPattern::Medium => {
                // Generic pattern (medium strength)
                let _: () =
                    msg_send![manager, performFeedbackPattern: 0_i64, performanceTime: 1_i64];
            }
            HapticPattern::Heavy => {
                // Level change pattern (strong)
                let _: () =
                    msg_send![manager, performFeedbackPattern: 2_i64, performanceTime: 1_i64];
            }
            HapticPattern::Selection => {
                // Quick alignment (subtle & fast)
                let _: () =
                    msg_send![manager, performFeedbackPattern: 1_i64, performanceTime: 0_i64];
            }
            HapticPattern::Success => {
                // Double tap - alignment then generic
                let _: () =
                    msg_send![manager, performFeedbackPattern: 1_i64, performanceTime: 1_i64];
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _: () =
                    msg_send![manager, performFeedbackPattern: 0_i64, performanceTime: 1_i64];
            }
            HapticPattern::Error => {
                // Triple tap - strong pattern
                for i in 0..3 {
                    let _: () =
                        msg_send![manager, performFeedbackPattern: 2_i64, performanceTime: 1_i64];
                    if i < 2 {
                        std::thread::sleep(std::time::Duration::from_millis(40));
                    }
                }
            }
        }
    }

    Ok(())
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
    let (screen_width, screen_height, notch_height, notch_width) =
        get_screen_info(Some(&app_handle));

    // Pre-compute window position (window is centered at top)
    let settings = get_window_settings();
    let win_width = notch_width + 160.0 + settings.extra_width;
    let window_x = (screen_width - win_width) / 2.0;

    // Compute fallback detection area
    let effective_notch_width = if settings.non_notch_mode {
        0.0
    } else {
        notch_width
    };

    let fallback_x_start = (screen_width - effective_notch_width) / 2.0;
    let _fallback_x_end = fallback_x_start + effective_notch_width;
    let _fallback_y_end = if settings.non_notch_mode {
        1.0
    } else {
        notch_height
    };

    // Spawn monitoring thread
    std::thread::spawn(move || {
        let mut cached_screen_height = screen_height;
        let mut refresh_counter: u16 = 0;

        // Hysteresis to prevent flicker
        const PADDING_ENTER: f64 = 20.0;
        const PADDING_EXIT: f64 = 30.0;

        // Fast polling for low latency
        const POLL_MS: u64 = 20; // ~50fps

        loop {
            // Refresh settings and dimensions on every iteration to handle runtime toggles
            let settings = get_window_settings();
            let effective_notch_width = if settings.non_notch_mode {
                0.0
            } else {
                notch_width
            };

            let fallback_x_start = (screen_width - effective_notch_width) / 2.0;
            let fallback_x_end = fallback_x_start + effective_notch_width;
            let fallback_y_end = if settings.non_notch_mode {
                1.0
            } else {
                notch_height
            };

            // Get mouse position
            let (mouse_x, flipped_y) = unsafe {
                let mouse_loc: CGPoint = msg_send![class!(NSEvent), mouseLocation];

                // Refresh screen height occasionally
                refresh_counter = refresh_counter.wrapping_add(1);
                if refresh_counter % 500 == 0 {
                    let (_, height, _, _) = get_screen_info(None);
                    cached_screen_height = height;
                }

                (mouse_loc.x, cached_screen_height - mouse_loc.y)
            };

            let was_inside = IS_INSIDE.load(Ordering::Relaxed);

            // OPTIMIZATION: Broad interaction zone check.
            // Only perform precise bounds checks if the mouse is roughly in the top-middle area.
            // In non-notch mode, we want a much tighter broad check to avoid accidental triggers.
            let broad_padding_x = if settings.non_notch_mode { 60.0 } else { 300.0 };
            let broad_limit_y = if settings.non_notch_mode { 50.0 } else { 250.0 };

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

                if let Ok(guard) = get_ui_bounds_store().try_read() {
                    if let Some(bounds) = *guard {
                        log::debug!("[mouse] ENTERED UI bounds - mouse: ({:.0}, {:.0}), bounds: x={:.0}, y={:.0}, w={:.0}, h={:.0}",
                            mouse_x, flipped_y, window_x + bounds.x, bounds.y, bounds.width, bounds.height);
                    } else {
                        log::debug!(
                            "[mouse] ENTERED UI bounds (fallback) - mouse: ({:.0}, {:.0})",
                            mouse_x,
                            flipped_y
                        );
                    }
                }

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

                if let Ok(guard) = get_ui_bounds_store().try_read() {
                    if let Some(bounds) = *guard {
                        log::debug!("[mouse] EXITED UI bounds - mouse: ({:.0}, {:.0}), bounds: x={:.0}, y={:.0}, w={:.0}, h={:.0}",
                            mouse_x, flipped_y, window_x + bounds.x, bounds.y, bounds.width, bounds.height);
                    } else {
                        log::debug!(
                            "[mouse] EXITED UI bounds (fallback) - mouse: ({:.0}, {:.0})",
                            mouse_x,
                            flipped_y
                        );
                    }
                }

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

#[cfg(target_os = "windows")]
pub fn setup_mouse_monitoring(app_handle: tauri::AppHandle) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    // Track whether mouse is currently in the UI area
    static IS_INSIDE: AtomicBool = AtomicBool::new(false);

    let (screen_width, _screen_height, notch_height, notch_width) =
        get_screen_info(Some(&app_handle));

    // Pre-compute window position (window is centered at top)
    let settings = get_window_settings();
    let effective_notch_width = if settings.non_notch_mode {
        0.0
    } else {
        notch_width
    };

    let win_width = if effective_notch_width > 0.0 {
        effective_notch_width + 160.0 + settings.extra_width
    } else {
        800.0 + settings.extra_width
    }; // Fallback width

    let window_x = (screen_width - win_width) / 2.0;

    std::thread::spawn(move || {
        const POLL_MS: u64 = 20;

        loop {
            let mut point = POINT::default();
            let success = unsafe { GetCursorPos(&mut point) };

            if success.is_ok() {
                let mouse_x = point.x as f64;
                let mouse_y = point.y as f64;

                let was_inside = IS_INSIDE.load(Ordering::Relaxed);

                // Logic adapted from macOS version
                let padding = if was_inside { 30.0 } else { 20.0 };

                let in_ui_area = if let Ok(guard) = get_ui_bounds_store().try_read() {
                    if let Some(bounds) = *guard {
                        let sx = window_x + bounds.x;
                        let sy = bounds.y;
                        mouse_x >= (sx - padding)
                            && mouse_x <= (sx + bounds.width + padding)
                            && mouse_y >= (sy - padding)
                            && mouse_y <= (sy + bounds.height + padding)
                    } else {
                        // Fallback zone at top center
                        // In non-notch mode we use a very small height for fallback
                        let fallback_height = if settings.non_notch_mode { 1.0 } else { 100.0 };

                        mouse_x >= (window_x - padding)
                            && mouse_x <= (window_x + win_width + padding)
                            && mouse_y >= 0.0
                            && mouse_y <= (fallback_height + padding)
                    }
                } else {
                    let fallback_height = if settings.non_notch_mode { 1.0 } else { 100.0 };

                    mouse_x >= (window_x - padding)
                        && mouse_x <= (window_x + win_width + padding)
                        && mouse_y >= 0.0
                        && mouse_y <= (fallback_height + padding)
                };

                if in_ui_area && !was_inside {
                    IS_INSIDE.store(true, Ordering::Relaxed);
                    let _ = app_handle.emit("mouse-entered-notch", ());

                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.set_ignore_cursor_events(false);
                        // Activate window
                        use raw_window_handle::HasWindowHandle;
                        use windows::Win32::Foundation::HWND;
                        use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

                        if let Ok(handle) = window.window_handle() {
                            if let raw_window_handle::RawWindowHandle::Win32(win32_handle) =
                                handle.as_raw()
                            {
                                unsafe {
                                    let hwnd = HWND(win32_handle.hwnd.get() as _);
                                    SetForegroundWindow(hwnd);
                                }
                            }
                        }
                    }
                } else if !in_ui_area && was_inside {
                    IS_INSIDE.store(false, Ordering::Relaxed);
                    let _ = app_handle.emit("mouse-exited-notch", ());
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
pub fn setup_mouse_monitoring(app_handle: tauri::AppHandle) {
    // Mouse monitoring on Linux (Wayland/X11) is complex to do globally without heavy dependencies.
    // For now, we will rely on window events if possible, or disable the hover feature.
    // To avoid busy looping or useless threads, we just log a message.
    log::info!("Global mouse monitoring not implemented for Linux yet.");
}
