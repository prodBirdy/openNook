pub mod audio;
pub mod calendar;
pub mod models;
pub mod notes;
pub mod utils;
pub mod window;
pub mod files;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            window::get_notch_info,
            window::position_at_notch,
            window::fit_to_notch,
            window::set_click_through,
            window::activate_window,
            window::trigger_haptics,
            window::update_ui_bounds,
            window::get_window_settings,
            window::update_window_settings,
            window::open_settings,
            audio::get_now_playing,
            audio::get_audio_levels,
            audio::media_play_pause,
            audio::media_next_track,
            audio::media_previous_track,
            audio::media_seek,
            audio::activate_media_app,
            notes::save_notes,
            notes::load_notes,
            calendar::request_calendar_access,
            calendar::get_upcoming_events,
            calendar::get_reminders,
            files::open_file,
            files::reveal_file,
            files::on_file_drop
        ])
        .setup(|app| {
            // Auto-position and resize window to match notch on startup
            if let Some(window) = app.get_webview_window("main") {
                // Set window level above menu bar on macOS
                // This allows the window to be positioned over the notch
                #[cfg(target_os = "macos")]
                {
                    use objc2::runtime::AnyObject;
                    use objc2::*;
                    use raw_window_handle::HasWindowHandle;

                    if let Ok(handle) = window.window_handle() {
                        if let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) =
                            handle.as_raw()
                        {
                            unsafe {
                                let ns_view = appkit_handle.ns_view.as_ptr() as *mut AnyObject;
                                let ns_win: *mut AnyObject = msg_send![ns_view, window];

                                // Set activation policy to Accessory (1) to hide dock icon and menu bar
                                // This is needed at runtime for tauri dev, Info.plist only works for bundled builds
                                let ns_app: *mut AnyObject =
                                    msg_send![class!(NSApplication), sharedApplication];
                                // NSApplicationActivationPolicyAccessory = 1
                                let _: () = msg_send![ns_app, setActivationPolicy: 1_i64];

                                // NSStatusWindowLevel = 25, which is above the menu bar (24)
                                // This allows positioning in the notch area
                                let _: () = msg_send![ns_win, setLevel: 25_i64];

                                // Also set collection behavior to allow appearing on all spaces
                                // NSWindowCollectionBehaviorCanJoinAllSpaces = 1 << 0
                                // NSWindowCollectionBehaviorStationary = 1 << 4
                                let _: () = msg_send![ns_win, setCollectionBehavior: 17_u64];

                                // Remove window shadow to prevent border effect
                                let _: () = msg_send![ns_win, setHasShadow: 0];
                            }
                        }
                    }
                }

                let _ = window.set_decorations(false);
                // Enable click-through by default (no notification showing)
                let _ = window.set_ignore_cursor_events(true);

                // Setup monitors
                #[cfg(target_os = "macos")]
                {
                    // Initialize audio caches
                    audio::init_audio_state();

                    // Initial positioning and sizing - window is always fixed size
                    let _ = window::setup_fixed_window_size(&window);

                    window::setup_mouse_monitoring(app.handle().clone());
                    audio::setup_audio_monitoring(app.handle().clone());
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
