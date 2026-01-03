use serde::Serialize;
use tauri::{LogicalPosition, LogicalSize, Manager};

/// Notch and screen information returned to the frontend
#[derive(Debug, Serialize, Clone)]
pub struct NotchInfo {
    /// Whether the screen has a notch (safeAreaInsets.top > 0)
    pub has_notch: bool,
    /// Height of the notch/safe area inset from the top (typically 30-40px on notched MacBooks)
    pub notch_height: f64,
    /// Width of the notch (the black area at the top center)
    pub notch_width: f64,
    /// Full screen width
    pub screen_width: f64,
    /// Full screen height
    pub screen_height: f64,
    /// The visible (usable) height below the notch
    pub visible_height: f64,
}

/// Get screen dimensions on macOS including notch width
/// Returns (screen_width, screen_height, notch_height, notch_width)
#[cfg(target_os = "macos")]
fn get_screen_info() -> (f64, f64, f64, f64) {
    use cocoa::foundation::NSRect;
    use objc::runtime::Object;
    use objc::*;

    unsafe {
        let main_screen: *mut Object = msg_send![class!(NSScreen), mainScreen];

        if main_screen.is_null() {
            return (0.0, 0.0, 0.0, 0.0);
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
        let notch_height = insets.top;

        // Calculate notch width
        // The notch width on MacBooks is typically around 180-200px
        // The Dynamic Island style expands beyond just the notch
        // We use a wider calculation to match the expanded Island look
        let notch_width = if notch_height > 0.0 {
            // Approximate notch width based on screen width
            // We use a slightly larger value for the Dynamic Island effect
            (screen_width * 0.121).max(200.0).min(260.0)
        } else {
            0.0
        };

        (screen_width, screen_height, notch_height, notch_width)
    }
}

#[cfg(not(target_os = "macos"))]
fn get_screen_info() -> (f64, f64, f64, f64) {
    (1920.0, 1080.0, 0.0, 0.0)
}

/// Get notch information from the main screen using NSScreen.safeAreaInsets (macOS 12.0+)
#[tauri::command]
fn get_notch_info() -> NotchInfo {
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
fn position_at_notch(window: tauri::Window) -> Result<(), String> {
    let (screen_width, _screen_height, _notch_height, notch_width) = get_screen_info();

    // Use notch width if available, otherwise fall back to current window width
    let target_width = if notch_width > 0.0 { notch_width } else {
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
fn fit_to_notch(window: tauri::Window, width: f64, height: f64) -> Result<(), String> {
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
            // Auto-position and resize window to match notch on startup
            if let Some(window) = app.get_webview_window("main") {
                let (screen_width, _screen_height, notch_height, notch_width) = get_screen_info();

                // Use notch width if available, otherwise use configured window width
                let window_width = if notch_width > 0.0 {
                    notch_width
                } else {
                    window.outer_size().map(|s| {
                        let scale = window.scale_factor().unwrap_or(1.0);
                        s.width as f64 / scale
                    }).unwrap_or(250.0)
                };

                // Use notch height for window height (plus a bit for the notification to grow into)
                let window_height = notch_height.max(38.0) + 20.0;

                // Resize window to match notch dimensions
                let _ = window.set_size(LogicalSize::new(window_width, window_height));

                // Set window level above menu bar on macOS
                // This allows the window to be positioned over the notch
                #[cfg(target_os = "macos")]
                {
                    use objc::runtime::Object;
                    use objc::*;
                    use raw_window_handle::HasWindowHandle;

                    if let Ok(handle) = window.window_handle() {
                        if let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() {
                            unsafe {
                                let ns_view = appkit_handle.ns_view.as_ptr() as *mut Object;
                                let ns_win: *mut Object = msg_send![ns_view, window];

                                // NSStatusWindowLevel = 25, which is above the menu bar (24)
                                // This allows positioning in the notch area
                                let _: () = msg_send![ns_win, setLevel: 25_i64];

                                // Also set collection behavior to allow appearing on all spaces
                                // NSWindowCollectionBehaviorCanJoinAllSpaces = 1 << 0
                                // NSWindowCollectionBehaviorStationary = 1 << 4
                                let _: () = msg_send![ns_win, setCollectionBehavior: 17_u64];
                            }
                        }
                    }
                }

                // Center horizontally, position at top (now can go negative)
                let x = (screen_width - window_width) / 2.0;
                // Position window so it overlaps with the notch
                // Negative y moves it up into the notch area
                let y = (notch_height.max(38.0) - 10.0);
                let _ = window.set_position(LogicalPosition::new(x, y));

                // Enable click-through by default (no notification showing)
                let _ = window.set_ignore_cursor_events(true);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
