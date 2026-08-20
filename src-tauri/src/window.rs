use crate::database::{get_connection, log_sql};
use crate::models::NotchInfo;
use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::Emitter;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow,
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

fn default_position_x() -> f64 {
    50.0
}

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
    /// Horizontal island placement: 0 = left, 50 = center, 100 = right
    #[serde(default = "default_position_x")]
    pub position_x: f64,
    /// Vertical island placement: 0 = top, 100 = bottom
    #[serde(default)]
    pub position_y: f64,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            extra_width: 400.0,
            extra_height: 800.0,
            non_notch_mode: false,
            position_x: 50.0,
            position_y: 0.0,
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
///
/// Receives window-relative coordinates from getBoundingClientRect():
/// - x, y: Position relative to the window's top-left corner
/// - width, height: Element dimensions
///
/// These will be converted to screen coordinates during mouse collision detection
/// by adding the window's screen position.
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

/// Compact overlay sized for the expanded island. A full-screen window would
/// intercept file-drags everywhere; a top-centered strip cannot travel.
const ISLAND_WINDOW_WIDTH: f64 = 640.0;
const ISLAND_WINDOW_HEIGHT: f64 = 320.0;

fn clamp_position_pct(value: f64) -> f64 {
    value.clamp(0.0, 100.0)
}

fn island_window_size(screen_width: f64, screen_height: f64, notch_height: f64) -> (f64, f64) {
    let width = screen_width.min(ISLAND_WINDOW_WIDTH);
    let height = screen_height.min(notch_height + ISLAND_WINDOW_HEIGHT).max(1.0);
    (width, height)
}

fn island_window_origin(
    screen_width: f64,
    screen_height: f64,
    width: f64,
    height: f64,
    settings: &WindowSettings,
) -> (f64, f64) {
    let x = (screen_width - width).max(0.0) * (clamp_position_pct(settings.position_x) / 100.0);
    let y = (screen_height - height).max(0.0) * (clamp_position_pct(settings.position_y) / 100.0);
    (x, y)
}

/// Logical (x, y, width, height) of the main window.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main_window_logical_frame(
    app_handle: &AppHandle,
    screen_width: f64,
    screen_height: f64,
    notch_height: f64,
) -> (f64, f64, f64, f64) {
    if let Some(window) = app_handle.get_webview_window("main") {
        if let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) {
            let scale = window.scale_factor().unwrap_or(1.0);
            return (
                pos.x as f64 / scale,
                pos.y as f64 / scale,
                size.width as f64 / scale,
                size.height as f64 / scale,
            );
        }
    }

    let settings = get_window_settings();
    let (width, height) = island_window_size(screen_width, screen_height, notch_height);
    let (x, y) = island_window_origin(screen_width, screen_height, width, height, &settings);
    (x, y, width, height)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn point_in_rect(px: f64, py: f64, x: f64, y: f64, w: f64, h: f64, padding: f64) -> bool {
    px >= (x - padding) && px <= (x + w + padding) && py >= (y - padding) && py <= (y + h + padding)
}

/// Screen-space UI hit rect. Uses the frontend's window-relative bounds as-is
/// (no recentering) so a moved island stays hoverable.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn ui_hit_rect(
    bounds: Option<UiBounds>,
    window_x: f64,
    window_y: f64,
    win_width: f64,
    notch_width: f64,
    settings: &WindowSettings,
) -> (f64, f64, f64, f64) {
    if let Some(bounds) = bounds {
        return (
            window_x + bounds.x,
            window_y + bounds.y,
            bounds.width,
            bounds.height,
        );
    }

    let fallback_w = if settings.non_notch_mode {
        0.0
    } else {
        notch_width.min(win_width)
    };
    let fallback_h = if settings.non_notch_mode { 1.0 } else { 100.0 };
    let fallback_x = window_x + (win_width - fallback_w) / 2.0;
    (fallback_x, window_y, fallback_w, fallback_h)
}

/// Helper to calculate dynamic notch width based on screen width
/// Returns a width between 200.0 and 260.0 (10% of screen width)
fn calculate_dynamic_notch_width(screen_width: f64) -> f64 {
    (screen_width * 0.1).clamp(200.0, 260.0)
}

/// Get screen dimensions
/// Returns (screen_width, screen_height, notch_height, notch_width)
fn get_screen_info(_app_handle: Option<&tauri::AppHandle>) -> (f64, f64, f64, f64) {
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
                // OLD: 180.0
                calculate_dynamic_notch_width(screen_width)
            };

            (screen_width, screen_height, notch_height, notch_width)
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Use primary_monitor for DPI-aware screen dimensions
        // GetSystemMetrics returns scaled values on high-DPI screens (e.g., 4K displays)
        if let Some(handle) = _app_handle {
            if let Ok(Some(monitor)) = handle.primary_monitor() {
                let size = monitor.size();
                let scale_factor = monitor.scale_factor();
                let width = size.width as f64 / scale_factor;
                let height = size.height as f64 / scale_factor;
                let notch_width = calculate_dynamic_notch_width(width);
                return (width, height, 0.0, notch_width);
            }
        }

        // Fallback to GetSystemMetrics if monitor API unavailable
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        unsafe {
            let width = GetSystemMetrics(SM_CXSCREEN) as f64;
            let height = GetSystemMetrics(SM_CYSCREEN) as f64;
            let notch_width = calculate_dynamic_notch_width(width);
            (width, height, 0.0, notch_width)
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Try to get from app handle if available
        if let Some(handle) = _app_handle {
            if let Ok(Some(monitor)) = handle.primary_monitor() {
                let size = monitor.size();
                let scale_factor = monitor.scale_factor();
                let width = size.width as f64 / scale_factor;
                let height = size.height as f64 / scale_factor;
                let notch_width = calculate_dynamic_notch_width(width);
                return (width, height, 0.0, notch_width);
            }
        }
        let notch_width = calculate_dynamic_notch_width(1920.0);
        (1920.0, 1080.0, 0.0, notch_width)
    }
}

#[tauri::command]
pub fn get_system_accent_color() -> String {
    #[cfg(target_os = "macos")]
    return crate::utils::get_macos_accent_color();

    #[cfg(target_os = "windows")]
    return crate::utils::get_windows_accent_color();

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
    position_x: f64,
    position_y: f64,
) -> Result<(), String> {
    // Update the stored settings
    {
        let store = get_window_settings_store();
        let mut settings = store.write().map_err(|e| e.to_string())?;
        settings.extra_width = extra_width;
        settings.extra_height = extra_height;
        settings.non_notch_mode = non_notch_mode;
        settings.position_x = clamp_position_pct(position_x);
        settings.position_y = clamp_position_pct(position_y);

        persist_window_settings(window.app_handle(), &settings);
    }

    // Apply the new window size to the MAIN window, not the settings window
    if let Some(main_window) = window.app_handle().get_webview_window("main") {
        setup_fixed_window_size(&main_window)?;
    }

    Ok(())
}

/// Set up the window sized for the island and placed from saved position settings.
pub fn setup_fixed_window_size(window: &WebviewWindow) -> Result<(), String> {
    let (screen_width, screen_height, notch_height, _notch_width) =
        get_screen_info(Some(window.app_handle()));
    let settings = get_window_settings();

    let (target_width, target_height) =
        island_window_size(screen_width, screen_height, notch_height);
    let (x, y) = island_window_origin(
        screen_width,
        screen_height,
        target_width,
        target_height,
        &settings,
    );

    window
        .set_size(LogicalSize::new(target_width, target_height))
        .map_err(|e| e.to_string())?;

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
    #[cfg(not(target_os = "macos"))]
    let _ = window;

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
    #[cfg(not(target_os = "macos"))]
    let _ = config;

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
            // Hide-when-maximized: force click-through and skip hover activation.
            if crate::maximized::should_hide_for_maximized() {
                if IS_INSIDE.swap(false, Ordering::Relaxed) {
                    let _ = app_handle.emit("mouse-exited-notch", ());
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.set_ignore_cursor_events(true);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
                continue;
            }

            // Refresh settings and the real window frame so a moved island stays hoverable
            let settings = get_window_settings();
            let (window_x, window_y, win_width, _win_height) = main_window_logical_frame(
                &app_handle,
                screen_width,
                cached_screen_height,
                notch_height,
            );
            let bounds = get_ui_bounds_store().try_read().ok().and_then(|guard| *guard);
            let (hit_x, hit_y, hit_w, hit_h) = ui_hit_rect(
                bounds,
                window_x,
                window_y,
                win_width,
                notch_width,
                &settings,
            );

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

            // Broad zone follows the actual island, not the top-center notch.
            let broad_padding = if settings.non_notch_mode { 60.0 } else { 80.0 };
            let is_in_interaction_zone =
                point_in_rect(mouse_x, flipped_y, hit_x, hit_y, hit_w, hit_h, broad_padding);

            if !is_in_interaction_zone && !was_inside {
                std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
                continue;
            }

            let padding = if was_inside {
                PADDING_EXIT
            } else {
                PADDING_ENTER
            };

            let in_ui_area =
                point_in_rect(mouse_x, flipped_y, hit_x, hit_y, hit_w, hit_h, padding);

            // State transitions - emit events immediately
            if in_ui_area && !was_inside {
                IS_INSIDE.store(true, Ordering::Relaxed);

                if bounds.is_some() {
                    log::debug!("[mouse] ENTERED UI bounds - mouse: ({:.0}, {:.0}), bounds: x={:.0}, y={:.0}, w={:.0}, h={:.0}",
                        mouse_x, flipped_y, hit_x, hit_y, hit_w, hit_h);
                } else {
                    log::debug!(
                        "[mouse] ENTERED UI bounds (fallback) - mouse: ({:.0}, {:.0})",
                        mouse_x,
                        flipped_y
                    );
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

                if bounds.is_some() {
                    log::debug!("[mouse] EXITED UI bounds - mouse: ({:.0}, {:.0}), bounds: x={:.0}, y={:.0}, w={:.0}, h={:.0}",
                        mouse_x, flipped_y, hit_x, hit_y, hit_w, hit_h);
                } else {
                    log::debug!(
                        "[mouse] EXITED UI bounds (fallback) - mouse: ({:.0}, {:.0})",
                        mouse_x,
                        flipped_y
                    );
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

    let (screen_width, screen_height, notch_height, notch_width) =
        get_screen_info(Some(&app_handle));

    std::thread::spawn(move || {
        const POLL_MS: u64 = 20;

        loop {
            // Hide-when-maximized: force click-through and skip hover activation.
            if crate::maximized::should_hide_for_maximized() {
                if IS_INSIDE.swap(false, Ordering::Relaxed) {
                    let _ = app_handle.emit("mouse-exited-notch", ());
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.set_ignore_cursor_events(true);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
                continue;
            }

            // Refresh settings and the real window frame so a moved island stays hoverable
            let settings = get_window_settings();
            let (window_x, window_y, win_width, _win_height) = main_window_logical_frame(
                &app_handle,
                screen_width,
                screen_height,
                notch_height,
            );
            let scale_factor = app_handle
                .get_webview_window("main")
                .and_then(|window| window.scale_factor().ok())
                .unwrap_or(1.0);
            let bounds = get_ui_bounds_store().try_read().ok().and_then(|guard| *guard);
            let (hit_x, hit_y, hit_w, hit_h) = ui_hit_rect(
                bounds,
                window_x,
                window_y,
                win_width,
                notch_width,
                &settings,
            );

            let mut point = POINT::default();
            let success = unsafe { GetCursorPos(&mut point) };

            if success.is_ok() {
                // Convert physical mouse coordinates to logical pixels
                let mouse_x = (point.x as f64) / scale_factor;
                let mouse_y = (point.y as f64) / scale_factor;

                let was_inside = IS_INSIDE.load(Ordering::Relaxed);

                let padding = if was_inside { 30.0 } else { 20.0 };
                let in_ui_area =
                    point_in_rect(mouse_x, mouse_y, hit_x, hit_y, hit_w, hit_h, padding);

                if in_ui_area && !was_inside {
                    IS_INSIDE.store(true, Ordering::Relaxed);
                    let _ = app_handle.emit("mouse-entered-notch", ());

                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.set_ignore_cursor_events(false);
                        // Do NOT activate window on Windows as it cancels Drag & Drop operations
                        // The window is already always-on-top so it will receive events once ignore_cursor_events is false
                        /*
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
                        */
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
    let _ = app_handle;
    // Mouse monitoring on Linux (Wayland/X11) is complex to do globally without heavy dependencies.
    // For now, we will rely on window events if possible, or disable the hover feature.
    // To avoid busy looping or useless threads, we just log a message.
    log::info!("Global mouse monitoring not implemented for Linux yet.");
}
