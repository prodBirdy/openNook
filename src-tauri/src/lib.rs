use serde::Serialize;
use tauri::{LogicalPosition, LogicalSize, Manager};

/// Notch and screen information returned to the frontend
#[derive(Debug, Serialize, Clone)]
pub struct NotchInfo {
    /// Whether the screen has a notch (safeAreaInsets.top > 0)
    pub has_notch: bool,
    /// Height of the notch/safe area inset from the top (typically 30-40px on notched MacBooks)
    pub notch_height: f64,
    /// Full screen width
    pub screen_width: f64,
    /// Full screen height
    pub screen_height: f64,
    /// The visible (usable) height below the notch
    pub visible_height: f64,
}

/// Get screen dimensions on macOS
#[cfg(target_os = "macos")]
fn get_screen_info() -> (f64, f64, f64) {
    use cocoa::foundation::NSRect;
    use objc::runtime::Object;
    use objc::*;

    unsafe {
        let main_screen: *mut Object = msg_send![class!(NSScreen), mainScreen];

        if main_screen.is_null() {
            return (0.0, 0.0, 0.0);
        }

        // Get screen frame
        let frame: NSRect = msg_send![main_screen, frame];
        let screen_width = frame.size.width;
        let screen_height = frame.size.height;

        // Get safeAreaInsets (macOS 12.0+)
        #[repr(C)]
        #[derive(Debug, Copy, Clone)]
        struct NSEdgeInsets {
            top: f64,
            left: f64,
            bottom: f64,
            right: f64,
        }

        let insets: NSEdgeInsets = msg_send![main_screen, safeAreaInsets];
        (screen_width, screen_height, insets.top)
    }
}

#[cfg(not(target_os = "macos"))]
fn get_screen_info() -> (f64, f64, f64) {
    (1920.0, 1080.0, 0.0)
}

/// Get notch information from the main screen using NSScreen.safeAreaInsets (macOS 12.0+)
#[tauri::command]
fn get_notch_info() -> NotchInfo {
    let (screen_width, screen_height, notch_height) = get_screen_info();
    let has_notch = notch_height > 0.0;
    let visible_height = screen_height - notch_height;

    NotchInfo {
        has_notch,
        notch_height,
        screen_width,
        screen_height,
        visible_height,
    }
}

/// Position the window at the notch location (centered at top of screen)
#[tauri::command]
fn position_at_notch(window: tauri::Window) -> Result<(), String> {
    let (screen_width, _screen_height, _notch_height) = get_screen_info();

    // Get current window size
    let window_size = window.outer_size().map_err(|e| e.to_string())?;
    let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;

    let window_width = window_size.width as f64 / scale_factor;

    // Center horizontally, position at very top (y=0)
    let x = (screen_width - window_width) / 2.0;
    let y = 0.0;

    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Resize and position window to fit the notch area
/// The window is positioned so it starts behind the notch, allowing the notification to "grow out"
#[tauri::command]
fn fit_to_notch(window: tauri::Window, width: f64, height: f64) -> Result<(), String> {
    let (screen_width, _screen_height, notch_height) = get_screen_info();

    // Resize the window
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;

    // Center horizontally, position so the top of the window is behind the notch
    // This makes the notification appear to "grow out" of the notch
    let x = (screen_width - width) / 2.0;
    // Position the window so some of it is behind the notch (negative y)
    // We want the "resting" state to be mostly hidden behind the notch
    let y = -(notch_height.max(38.0) - 8.0); // Keep ~8px visible as the "pill"

    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Set whether the window should ignore mouse events (click-through)
/// When true, clicks pass through to the underlying application
#[tauri::command]
fn set_click_through(window: tauri::Window, ignore: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(ignore)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_notch_info,
            position_at_notch,
            fit_to_notch,
            set_click_through
        ])
        .setup(|app| {
            // Auto-position window behind notch on startup
            if let Some(window) = app.get_webview_window("main") {
                let (screen_width, _screen_height, notch_height) = get_screen_info();

                if let Ok(window_size) = window.outer_size() {
                    let scale_factor = window.scale_factor().unwrap_or(1.0);
                    let window_width = window_size.width as f64 / scale_factor;

                    // Center horizontally
                    let x = (screen_width - window_width) / 2.0;
                    // Position behind the notch (negative y)
                    let y = -(notch_height.max(38.0) - 8.0);
                    let _ = window.set_position(LogicalPosition::new(x, y));
                }

                // Enable click-through by default (no notification showing)
                let _ = window.set_ignore_cursor_events(true);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
