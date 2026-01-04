use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{window::Color, Emitter, LogicalPosition, LogicalSize, Manager, WebviewWindow};

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

/// Now Playing track information
#[derive(Debug, Serialize, Clone, Default)]
pub struct NowPlayingData {
    /// Track title
    pub title: Option<String>,
    /// Artist name
    pub artist: Option<String>,
    /// Album name
    pub album: Option<String>,
    /// Base64 encoded artwork (PNG)
    pub artwork_base64: Option<String>,
    /// Track duration in seconds
    pub duration: Option<f64>,
    /// Elapsed time in seconds
    pub elapsed_time: Option<f64>,
    /// Whether music is currently playing
    pub is_playing: bool,
    /// Audio levels for visualizer (6 frequency bands, 0.0-1.0)
    pub audio_levels: Option<Vec<f64>>,
}

/// Global state for audio levels (updated by audio monitoring thread)
static AUDIO_LEVELS: std::sync::OnceLock<std::sync::Mutex<Vec<f64>>> = std::sync::OnceLock::new();

/// Cache for current track info to avoid refetching artwork
/// Format: (title, artist, artwork_base64)
static TRACK_CACHE: std::sync::OnceLock<
    std::sync::Mutex<(Option<String>, Option<String>, Option<String>)>,
> = std::sync::OnceLock::new();

fn get_audio_levels_internal() -> Vec<f64> {
    AUDIO_LEVELS
        .get()
        .map(|m| m.lock().unwrap().clone())
        .unwrap_or_else(|| vec![0.15; 6])
}

fn set_audio_levels(levels: Vec<f64>) {
    if let Some(m) = AUDIO_LEVELS.get() {
        *m.lock().unwrap() = levels;
    }
}

/// Get current audio levels for visualizer (lightweight, no AppleScript calls)
#[tauri::command]
fn get_audio_levels() -> Vec<f64> {
    get_audio_levels_internal()
}

fn get_cached_track() -> (Option<String>, Option<String>, Option<String>) {
    TRACK_CACHE
        .get()
        .map(|m| m.lock().unwrap().clone())
        .unwrap_or((None, None, None))
}

fn set_cached_track(title: Option<String>, artist: Option<String>, artwork: Option<String>) {
    if let Some(m) = TRACK_CACHE.get() {
        *m.lock().unwrap() = (title, artist, artwork);
    }
}

fn is_track_changed(title: &Option<String>, artist: &Option<String>) -> bool {
    let cached = get_cached_track();
    cached.0 != *title || cached.1 != *artist
}

/// Get screen dimensions on macOS including notch width
/// Returns (screen_width, screen_height, notch_height, notch_width)
#[cfg(target_os = "macos")]
fn get_screen_info() -> (f64, f64, f64, f64) {
    // Define our own CGSize/CGRect to avoid deprecated cocoa crate fields
    #[repr(C)]
    #[derive(Debug, Copy, Clone)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Debug, Copy, Clone)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Debug, Copy, Clone)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    use objc::runtime::Object;
    use objc::*;

    unsafe {
        let main_screen: *mut Object = msg_send![class!(NSScreen), mainScreen];

        if main_screen.is_null() {
            return (0.0, 0.0, 0.0, 0.0);
        }

        // Get screen frame
        let frame: CGRect = msg_send![main_screen, frame];
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
        let safe_area_top = insets.top;

        // Calculate notch height
        // The notch height on MacBooks is typically around 30-40px
        // The Dynamic Island style expands beyond just the notch
        // We use a slightly larger value for the Dynamic Island effect
        let notch_height = if safe_area_top > 0.0 {
            // Approximate island height based on screen height
            (screen_height * 0.07).max(38.0).min(52.0)
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
            (screen_width * 0.222).max(200.0).min(260.0)
        } else {
            0.0
        };

        (screen_width, screen_height, notch_height, notch_width)
    }
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

/// Helper function to resize and position window based on hover state
/// When not hovered: window is wide enough for media content (notch + 120px for side panels)
/// When hovered: window is 200px wider on each side (400px total) and 40px taller
fn resize_window_for_hover(window: &WebviewWindow, is_hovered: bool) -> Result<(), String> {
    let (screen_width, _screen_height, notch_height, notch_width) = get_screen_info();

    let (target_width, target_height) = if is_hovered {
        // Hovered: 200px wider on each side (400px total) and 40px taller
        let width = notch_width + 400.0;
        let height = notch_height + 40.0;
        (width, height)
    } else {
        // Not hovered: wide enough for media content (60px album cover + notch + 60px visualizer)
        // This ensures media content is visible even when not hovered
        let width = notch_width + 120.0; // 60px left + 60px right
        let height = notch_height;
        (width, height)
    };

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

/// Resize and position window based on hover state (Tauri command wrapper)
#[tauri::command]
fn resize_for_hover(window: WebviewWindow, is_hovered: bool) -> Result<(), String> {
    resize_window_for_hover(&window, is_hovered)
}

/// Activate the window (focus it)
/// Uses native macOS APIs to properly activate an accessory app
#[tauri::command]
fn activate_window(window: tauri::Window) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use objc::runtime::Object;
        use objc::*;
        use raw_window_handle::HasWindowHandle;

        unsafe {
            // Get NSApplication shared instance and activate it
            let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];

            // Also make the window key and bring it to front
            if let Ok(handle) = window.window_handle() {
                if let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() {
                    let ns_view = appkit_handle.ns_view.as_ptr() as *mut Object;
                    let ns_win: *mut Object = msg_send![ns_view, window];
                    let _: () = msg_send![ns_win, makeKeyAndOrderFront: std::ptr::null::<Object>()];
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
fn trigger_haptics() {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc::runtime::Object;
        use objc::*;

        let manager: *mut Object = msg_send![class!(NSHapticFeedbackManager), defaultPerformer];
        let _: () = msg_send![manager, performFeedbackPattern:0_i64 performanceTime:1_i64];
    }
}

/// Get currently playing music information using AppleScript
/// Queries Spotify first, then Music.app (Apple Music)
#[tauri::command]
fn get_now_playing() -> NowPlayingData {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        // Try Spotify first
        let spotify_script = r#"
            tell application "System Events"
                set spotifyRunning to (name of processes) contains "Spotify"
            end tell

            if spotifyRunning then
                tell application "Spotify"
                    if player state is playing then
                        set trackName to name of current track
                        set artistName to artist of current track
                        set albumName to album of current track
                        set trackDuration to (duration of current track) / 1000
                        set trackPosition to player position
                        set artUrl to artwork url of current track
                        return "playing|" & trackName & "|" & artistName & "|" & albumName & "|" & trackDuration & "|" & trackPosition & "|" & artUrl & "|spotify"
                    else if player state is paused then
                        return "paused||||||spotify"
                    else
                        return "stopped||||||spotify"
                    end if
                end tell
            else
                return "not_running||||||spotify"
            end if
        "#;

        if let Ok(result) = Command::new("osascript")
            .arg("-e")
            .arg(spotify_script)
            .output()
        {
            let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();
            let parts: Vec<&str> = stdout.split('|').collect();

            if parts.len() >= 8 && parts[0] == "playing" {
                let title = if parts[1].is_empty() {
                    None
                } else {
                    Some(parts[1].to_string())
                };
                let artist = if parts[2].is_empty() {
                    None
                } else {
                    Some(parts[2].to_string())
                };

                // Only fetch artwork if track changed
                let artwork = if is_track_changed(&title, &artist) {
                    println!("🎵 Spotify playing: {} - {}", parts[1], parts[2]);
                    let art = fetch_artwork_from_url(parts[6]);
                    println!("🖼️ Artwork fetched: {}", art.is_some());
                    set_cached_track(title.clone(), artist.clone(), art.clone());
                    art
                } else {
                    // Track hasn't changed, use cached artwork
                    get_cached_track().2
                };

                return NowPlayingData {
                    title,
                    artist,
                    album: if parts[3].is_empty() {
                        None
                    } else {
                        Some(parts[3].to_string())
                    },
                    artwork_base64: artwork,
                    duration: parts[4].parse().ok(),
                    elapsed_time: parts[5].parse().ok(),
                    is_playing: true,
                    audio_levels: Some(get_audio_levels_internal()),
                };
            }
        }

        // Fall back to Music.app (Apple Music)
        let music_script = r#"
            tell application "System Events"
                set musicRunning to (name of processes) contains "Music"
            end tell

            if musicRunning then
                tell application "Music"
                    if player state is playing then
                        set trackName to name of current track
                        set artistName to artist of current track
                        set albumName to album of current track
                        set trackDuration to duration of current track
                        set trackPosition to player position

                        -- Get artwork data
                        set artData to ""
                        try
                            set artworks to artworks of current track
                            if (count of artworks) > 0 then
                                set artData to data of (item 1 of artworks)
                            end if
                        end try

                        return "playing|" & trackName & "|" & artistName & "|" & albumName & "|" & trackDuration & "|" & trackPosition & "|music"
                    else if player state is paused then
                        return "paused|||||music"
                    else
                        return "stopped|||||music"
                    end if
                end tell
            else
                return "not_running|||||music"
            end if
        "#;

        if let Ok(result) = Command::new("osascript")
            .arg("-e")
            .arg(music_script)
            .output()
        {
            let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();
            let parts: Vec<&str> = stdout.split('|').collect();

            if parts.len() >= 7 && parts[0] == "playing" {
                let title = if parts[1].is_empty() {
                    None
                } else {
                    Some(parts[1].to_string())
                };
                let artist = if parts[2].is_empty() {
                    None
                } else {
                    Some(parts[2].to_string())
                };

                // Only fetch artwork if track changed
                let artwork = if is_track_changed(&title, &artist) {
                    println!("🎵 Music.app playing: {} - {}", parts[1], parts[2]);
                    let art = get_music_app_artwork();
                    println!("🖼️ Artwork fetched: {}", art.is_some());
                    set_cached_track(title.clone(), artist.clone(), art.clone());
                    art
                } else {
                    // Track hasn't changed, use cached artwork
                    get_cached_track().2
                };

                return NowPlayingData {
                    title,
                    artist,
                    album: if parts[3].is_empty() {
                        None
                    } else {
                        Some(parts[3].to_string())
                    },
                    artwork_base64: artwork,
                    duration: parts[4].parse().ok(),
                    elapsed_time: parts[5].parse().ok(),
                    is_playing: true,
                    audio_levels: Some(get_audio_levels_internal()),
                };
            }
        }

        NowPlayingData {
            audio_levels: Some(get_audio_levels()),
            ..Default::default()
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        NowPlayingData::default()
    }
}

/// Fetch artwork from a URL (used for Spotify)
#[cfg(target_os = "macos")]
fn fetch_artwork_from_url(url: &str) -> Option<String> {
    use std::process::Command;

    if url.is_empty() {
        return None;
    }

    // Use curl to fetch the image and convert to base64
    let output = Command::new("curl")
        .args(["-s", "-L", "--max-time", "2", url])
        .output()
        .ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        // Encode to base64
        let base64 = base64_encode(&output.stdout);
        Some(base64)
    } else {
        None
    }
}

/// Get artwork from Music.app using AppleScript to write to temp file
#[cfg(target_os = "macos")]
fn get_music_app_artwork() -> Option<String> {
    use std::fs;
    use std::process::Command;

    let temp_path = "/tmp/overdone_artwork.png";

    // AppleScript to extract artwork to a file
    let script = format!(
        r#"
        tell application "Music"
            try
                set currentTrack to current track
                set artworks to artworks of currentTrack
                if (count of artworks) > 0 then
                    set artworkData to raw data of (item 1 of artworks)
                    set fileRef to open for access POSIX file "{}" with write permission
                    set eof fileRef to 0
                    write artworkData to fileRef
                    close access fileRef
                    return "success"
                else
                    return "no_artwork"
                end if
            on error
                return "error"
            end try
        end tell
    "#,
        temp_path
    );

    if let Ok(result) = Command::new("osascript").arg("-e").arg(&script).output() {
        let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();
        if stdout == "success" {
            if let Ok(data) = fs::read(temp_path) {
                let _ = fs::remove_file(temp_path);
                return Some(base64_encode(&data));
            }
        }
    }
    None
}

/// Simple base64 encoding
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::new();
    let chunks = data.chunks(3);

    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;

        let n = (b0 << 16) | (b1 << 8) | b2;

        result.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        result.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(ALPHABET[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

/// Setup global mouse monitoring for the window
/// This uses a polling thread to detect mouse position and controls window state
#[cfg(target_os = "macos")]
fn setup_mouse_monitoring(app_handle: tauri::AppHandle) {
    use objc::runtime::Object;
    use objc::*;

    // Track whether mouse is currently in the window area
    let is_inside = Arc::new(AtomicBool::new(false));
    let is_inside_clone = is_inside.clone();

    // Get screen info for notch area detection
    let (screen_width, _screen_height, notch_height, notch_width) = get_screen_info();

    // Window sizes
    let _not_hovered_width = notch_width + 120.0; // 60px left + 60px right for media content
    let hovered_width = notch_width + 400.0; // 200px wider on each side when hovered
    let hovered_height = notch_height + 40.0;

    // Calculate detection area - use the larger hovered width to catch mouse approaching
    // This ensures we detect hover even when window is in non-hovered state
    let detection_x_start = (screen_width - hovered_width) / 2.0 - 10.0;
    let detection_x_end = detection_x_start + hovered_width + 20.0;
    let detection_y_end = hovered_height + 10.0; // Detection extends slightly below hovered window

    let app_handle_clone = app_handle.clone();

    // Spawn a thread to poll mouse position and control window
    std::thread::spawn(move || {
        loop {
            let in_notch_area = unsafe {
                // Get current mouse location in screen coordinates
                let mouse_loc: CGPoint = msg_send![class!(NSEvent), mouseLocation];

                // macOS uses bottom-left origin, so we need to flip Y
                let screens: *mut Object = msg_send![class!(NSScreen), screens];
                let primary_screen: *mut Object = msg_send![screens, objectAtIndex: 0_u64];
                let screen_frame: CGRect = msg_send![primary_screen, frame];
                let flipped_y = screen_frame.size.height - mouse_loc.y;

                // Check if mouse is in the detection area
                mouse_loc.x >= detection_x_start
                    && mouse_loc.x <= detection_x_end
                    && flipped_y >= 0.0
                    && flipped_y <= detection_y_end
            };

            let was_inside = is_inside_clone.load(Ordering::Relaxed);

            if in_notch_area && !was_inside {
                // Mouse entered notch area - expand window
                println!(
                    "🖱️ Mouse ENTERED notch area - expanding window and disabling click-through"
                );
                is_inside_clone.store(true, Ordering::Relaxed);

                if let Some(window) = app_handle_clone.get_webview_window("main") {
                    // Resize window to hovered state (200px wider on each side, 40px taller)
                    let _ = resize_window_for_hover(&window, true);
                    // Disable click-through so window receives events
                    let _ = window.set_ignore_cursor_events(false);
                    // Focus the window using thread-safe Tauri API
                    let _ = window.set_focus();
                }

                let _ = app_handle_clone.emit("mouse-entered-notch", ());
            } else if !in_notch_area && was_inside {
                // Mouse exited notch area - contract window
                println!(
                    "🖱️ Mouse EXITED notch area - contracting window and enabling click-through"
                );
                is_inside_clone.store(false, Ordering::Relaxed);

                if let Some(window) = app_handle_clone.get_webview_window("main") {
                    // Resize window to not-hovered state (same size as notch)
                    let _ = resize_window_for_hover(&window, false);
                    // Enable click-through so clicks pass through
                    let _ = window.set_ignore_cursor_events(true);
                }

                let _ = app_handle_clone.emit("mouse-exited-notch", ());
            }

            // Poll at ~60fps
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });

    println!("Mouse monitoring thread started for notch area detection");
}

/// Setup audio level monitoring using real system audio via ScreenCaptureKit
/// Captures system audio and performs FFT to extract frequency bands
/// Setup audio level monitoring
/// Uses Core Audio HAL Taps (placeholder) and Fallback Simulation
#[cfg(target_os = "macos")]
fn setup_audio_monitoring(app_handle: tauri::AppHandle) {
    use spectrum_analyzer::windows::hann_window;
    use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
    // use coreaudio_sys::*; // Ready for HAL implementation
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // Initialize the audio levels storage
    let _ = AUDIO_LEVELS.set(std::sync::Mutex::new(vec![0.15; 6]));

    let bar_count: usize = 6;
    let floor: f64 = 0.15;

    // Shared buffer for audio samples
    let audio_buffer: Arc<std::sync::Mutex<Vec<f32>>> =
        Arc::new(std::sync::Mutex::new(Vec::with_capacity(2048)));

    // Processing thread - runs FFT or Simulation at 60fps
    let app_handle_clone = app_handle.clone();
    std::thread::spawn(move || {
        let mut band_energy = vec![0.0_f64; bar_count];
        // let fft_size = 1024; // Unused until HAL Tap is active

        // Custom gain per band to balance the spectrum (Pink noise compensation)
        // Bass needs less gain, Highs need more
        let _band_gains = [1.0, 1.2, 1.8, 2.5, 4.0, 6.0];

        // Fallback simulation state
        let mut rng_state: u64 = 0xC0FFEE_u64
            ^ (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64);

        fn next_u64(state: &mut u64) -> u64 {
            let mut x = *state;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            *state = x;
            x.wrapping_mul(0x2545F4914F6CDD1D)
        }

        fn next_f64_01(state: &mut u64) -> f64 {
            (next_u64(state) >> 11) as f64 * (1.0 / ((1_u64 << 53) as f64))
        }

        fn is_music_playing() -> bool {
            use std::process::Command;
            let script = r#"
                set isPlaying to false
                tell application "System Events"
                    if (name of processes) contains "Spotify" then
                        tell application "Spotify" to if player state is playing then set isPlaying to true
                    else if (name of processes) contains "Music" then
                        tell application "Music" to if player state is playing then set isPlaying to true
                    end if
                end tell
                if isPlaying then return "playing" else return "stopped"
            "#;
            Command::new("osascript")
                .arg("-e")
                .arg(script)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "playing")
                .unwrap_or(false)
        }

        let mut last_music_check = std::time::Instant::now();
        let mut music_playing = false;

        loop {
            let mut levels = vec![floor; bar_count];
            let has_real_audio = false; // Force simulation for now until HAL Tap is ready

            // TODO: Implement Core Audio HAL Tap here
            // Access `audio_buffer` and run FFT if data available

            // Fallback simulation
            if !has_real_audio {
                if last_music_check.elapsed() > std::time::Duration::from_millis(500) {
                    music_playing = is_music_playing();
                    last_music_check = std::time::Instant::now();
                }

                if music_playing {
                    let t = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64();

                    for i in 0..bar_count {
                        let band_pos = (i as f64) / ((bar_count - 1) as f64);
                        band_energy[i] *= 0.97 - band_pos * 0.12;
                        if next_f64_01(&mut rng_state) < 0.015 + band_pos * 0.09 {
                            band_energy[i] += (0.45 - band_pos * 0.25)
                                * (0.5 + 0.5 * next_f64_01(&mut rng_state));
                        }
                        band_energy[i] += ((t * (1.6 + 0.6 * (1.0 - band_pos))).sin() * 0.5 + 0.5)
                            * 0.05
                            * (1.0 - band_pos).powf(1.4);
                        band_energy[i] = band_energy[i].clamp(0.0, 0.85);
                        levels[i] = (floor + band_energy[i]).clamp(floor, 1.0);
                    }
                } else {
                    for e in &mut band_energy {
                        *e *= 0.90;
                        if *e < 0.0005 {
                            *e = 0.0;
                        }
                    }
                    for i in 0..bar_count {
                        levels[i] = (floor + band_energy[i]).clamp(floor, 1.0);
                    }
                }
            }

            set_audio_levels(levels.clone());
            let _ = app_handle_clone.emit("audio-levels-update", levels);
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });

    println!("Audio monitoring started (Core Audio HAL Tap foundation + Simulation)");
}

/// CGRect definition for mouse monitoring
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGSize {
    width: f64,
    height: f64,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_notch_info,
            position_at_notch,
            fit_to_notch,
            set_click_through,
            activate_window,
            trigger_haptics,
            get_now_playing,
            resize_for_hover,
            get_audio_levels
        ])
        .setup(|app| {
            // Auto-position and resize window to match notch on startup
            if let Some(window) = app.get_webview_window("main") {
                // Initially set window to not-hovered state (behind the notch)
                let _ = resize_window_for_hover(&window, false);
                println!("Window initialized to not-hovered state (behind notch)");

                // Set window level above menu bar on macOS
                // This allows the window to be positioned over the notch
                #[cfg(target_os = "macos")]
                {
                    use objc::runtime::Object;
                    use objc::*;
                    use raw_window_handle::HasWindowHandle;

                    if let Ok(handle) = window.window_handle() {
                        if let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) =
                            handle.as_raw()
                        {
                            unsafe {
                                let ns_view = appkit_handle.ns_view.as_ptr() as *mut Object;
                                let ns_win: *mut Object = msg_send![ns_view, window];

                                // Set activation policy to Accessory (1) to hide dock icon and menu bar
                                // This is needed at runtime for tauri dev, Info.plist only works for bundled builds
                                let ns_app: *mut Object =
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

                // Setup mouse monitoring to detect hover over notch area
                #[cfg(target_os = "macos")]
                {
                    // Initialize caches
                    let _ = TRACK_CACHE.set(std::sync::Mutex::new((None, None, None)));

                    setup_mouse_monitoring(app.handle().clone());
                    setup_audio_monitoring(app.handle().clone());
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
