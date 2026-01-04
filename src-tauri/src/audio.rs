use crate::models::NowPlayingData;
use crate::utils::{base64_encode, fetch_artwork_from_url};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;

/// Global state for audio levels (updated by audio monitoring thread)
static AUDIO_LEVELS: std::sync::OnceLock<std::sync::Mutex<Vec<f64>>> = std::sync::OnceLock::new();

/// Global state to track if media is playing (to pause simulation)
static IS_PLAYING: AtomicBool = AtomicBool::new(false);

/// Cache for current track info to avoid refetching artwork
/// Format: (title, artist, artwork_base64)
static TRACK_CACHE: std::sync::OnceLock<
    std::sync::Mutex<(Option<String>, Option<String>, Option<String>)>,
> = std::sync::OnceLock::new();

/// Cache for the last played track to display when paused/idle
static LAST_PLAYED: std::sync::OnceLock<std::sync::Mutex<Option<NowPlayingData>>> =
    std::sync::OnceLock::new();

pub fn init_audio_state() {
    let _ = AUDIO_LEVELS.set(std::sync::Mutex::new(vec![0.15; 6]));
    let _ = TRACK_CACHE.set(std::sync::Mutex::new((None, None, None)));
    let _ = LAST_PLAYED.set(std::sync::Mutex::new(None));
}

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
pub fn get_audio_levels() -> Vec<f64> {
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

fn save_last_played(data: &NowPlayingData) {
    if let Some(m) = LAST_PLAYED.get() {
        *m.lock().unwrap() = Some(data.clone());
    }
}

fn get_last_played_or_default(levels: Vec<f64>) -> NowPlayingData {
    if let Some(m) = LAST_PLAYED.get() {
        if let Ok(guard) = m.lock() {
            if let Some(last) = &*guard {
                let mut data = last.clone();
                data.is_playing = false;
                data.audio_levels = Some(levels);
                return data;
            }
        }
    }
    NowPlayingData {
        audio_levels: Some(levels),
        ..Default::default()
    }
}

/// Get currently playing music information
/// Tries multiple sources: Spotify, Music.app, Safari
#[tauri::command]
pub async fn get_now_playing() -> NowPlayingData {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        use sysinfo::System;

        // Static system monitor to avoid re-initialization
        static SYSTEM: std::sync::OnceLock<std::sync::Mutex<System>> = std::sync::OnceLock::new();

        let apps_running = {
            let sys_lock = SYSTEM.get_or_init(|| std::sync::Mutex::new(System::new_all()));

            if let Ok(mut sys) = sys_lock.lock() {
                sys.refresh_all();
                let processes = sys.processes();

                let spotify = processes.values().any(|p| p.name() == "Spotify");
                let music = processes.values().any(|p| p.name() == "Music");
                let safari = processes.values().any(|p| p.name() == "Safari");

                Some((spotify, music, safari))
            } else {
                None
            }
        };

        let (spotify_running, music_running, safari_running) =
            apps_running.unwrap_or((false, false, false));

        // If no relevant apps are running, return early with no overhead
        if !spotify_running && !music_running && !safari_running {
            IS_PLAYING.store(false, Ordering::Relaxed);
            return get_last_played_or_default(get_audio_levels());
        }

        // Default to not playing before check
        IS_PLAYING.store(false, Ordering::Relaxed);

        // Build dynamic script based on running apps only
        let mut script = String::new();

        // Check Spotify
        if spotify_running {
            script.push_str(r#"
                tell application "Spotify"
                    if player state is playing then
                        set trackName to name of current track
                        set artistName to artist of current track
                        set albumName to album of current track
                        set trackDuration to (duration of current track) / 1000
                        set trackPosition to player position
                        set artUrl to artwork url of current track
                        return "playing|" & trackName & "|" & artistName & "|" & albumName & "|" & trackDuration & "|" & trackPosition & "|" & artUrl & "|spotify"
                    end if
                end tell
            "#);
        }

        // Check Music
        if music_running {
            script.push_str(r#"
                tell application "Music"
                    if player state is playing then
                        set trackName to name of current track
                        set artistName to artist of current track
                        set albumName to album of current track
                        set trackDuration to duration of current track
                        set trackPosition to player position
                        return "playing|" & trackName & "|" & artistName & "|" & albumName & "|" & trackDuration & "|" & trackPosition & "||music"
                    end if
                end tell
            "#);
        }

        // Check Safari
        if safari_running {
            script.push_str(r#"
                tell application "Safari"
                    try
                        repeat with w in windows
                            repeat with t in tabs of w
                                try
                                    set tabURL to URL of t
                                    set tabName to name of t

                                    if tabURL contains "youtube.com" or tabURL contains "music.youtube.com" or tabURL contains "open.spotify.com" or tabURL contains "soundcloud.com" then
                                        try
                                            set jsResult to do JavaScript "
                                                (function() {
                                                    var video = document.querySelector('video');
                                                    if (video && !video.paused && !video.ended) return 'playing';
                                                    var audio = document.querySelector('audio');
                                                    if (audio && !audio.paused && !audio.ended) return 'playing';
                                                    return 'paused';
                                                })();
                                            " in t

                                            if jsResult is "playing" then
                                                return "playing|" & tabName & "|Safari|" & tabURL & "|||safari"
                                            end if
                                        on error
                                            if tabName starts with "▶" then
                                                return "playing|" & tabName & "|Safari|" & tabURL & "|||safari"
                                            end if
                                        end try
                                    end if
                                end try
                            end repeat
                        end repeat
                    end try
                end tell
            "#);
        }

        script.push_str("\nreturn \"not_playing\"");

        if let Ok(result) = Command::new("osascript").arg("-e").arg(&script).output() {
            let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();
            let parts: Vec<&str> = stdout.split('|').collect();

            if parts.len() >= 8 && parts[0] == "playing" {
                IS_PLAYING.store(true, Ordering::Relaxed);
                let app_id = parts.last().unwrap_or(&"");

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

                // Fetch artwork if track changed
                let artwork = if is_track_changed(&title, &artist) {
                    if *app_id == "music" {
                        get_music_app_artwork()
                    } else if !parts[6].is_empty() {
                        let art = fetch_artwork_from_url(parts[6]);
                        art
                    } else {
                        None
                    }
                } else {
                    get_cached_track().2
                };

                if is_track_changed(&title, &artist) {
                    set_cached_track(title.clone(), artist.clone(), artwork.clone());
                }

                let data = NowPlayingData {
                    title: if *app_id == "safari" {
                        // Clean YouTube title
                        title.map(|t| t.trim_end_matches(" - YouTube").to_string())
                    } else {
                        title
                    },
                    artist,
                    album: if parts[3].is_empty() {
                        None
                    } else {
                        Some(parts[3].to_string())
                    },
                    artwork_base64: artwork,
                    duration: parts[4].replace(',', ".").parse().ok(),
                    elapsed_time: parts[5].replace(',', ".").parse().ok(),
                    is_playing: true,
                    audio_levels: Some(get_audio_levels_internal()),
                    app_name: match *app_id {
                        "spotify" => Some("Spotify".to_string()),
                        "music" => Some("Music".to_string()),
                        "safari" => Some("Safari".to_string()),
                        _ => None,
                    },
                };

                save_last_played(&data);
                return data;
            }
        }

        get_last_played_or_default(get_audio_levels())
    }

    #[cfg(not(target_os = "macos"))]
    {
        NowPlayingData::default()
    }
}

/// Get artwork from Music.app using AppleScript to write to temp file
#[cfg(target_os = "macos")]
fn get_music_app_artwork() -> Option<String> {
    use std::fs;
    use std::process::Command;

    // Use a unique path to avoid conflicts
    let temp_path = "/tmp/overdone_music_art_v3.data";
    // Ensure cleanup
    let _ = fs::remove_file(temp_path);

    // AppleScript to extract artwork to a file
    // Tries 'data' first, then 'raw data' as fallback
    let script = format!(
        r#"
        tell application "Music"
            try
                if (count of artworks of current track) < 1 then return "no_artwork"

                set artData to missing value

                -- Try getting 'data' (image object/data) first
                try
                    set artData to data of artwork 1 of current track
                end try

                -- Fallback to 'raw data' if 'data' failed or is missing
                if artData is missing value then
                    try
                        set artData to raw data of artwork 1 of current track
                    end try
                end if

                if artData is missing value then return "no_data_found"

                set dest to POSIX file "{}"
                set f to open for access dest with write permission
                set eof f to 0
                write artData to f
                close access f
                return "success"
            on error errStr
                try
                    close access (POSIX file "{}")
                end try
                return "error: " & errStr
            end try
        end tell
    "#,
        temp_path, temp_path
    );

    if let Ok(result) = Command::new("osascript").arg("-e").arg(&script).output() {
        let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();

        // println!("Artwork fetch result: {}", stdout); // Uncomment for debugging

        if stdout == "success" {
            if let Ok(data) = fs::read(temp_path) {
                let _ = fs::remove_file(temp_path);
                if !data.is_empty() {
                    return Some(base64_encode(&data));
                }
            }
        }
    }

    // Ensure cleanup on failure
    let _ = fs::remove_file(temp_path);
    None
}

/// Toggle play/pause for the currently playing media
#[tauri::command]
pub fn media_play_pause() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        // Script that controls whichever app is playing
        let script = r#"
            tell application "System Events"
                set spotifyRunning to (name of processes) contains "Spotify"
                set musicRunning to (name of processes) contains "Music"
            end tell

            if spotifyRunning then
                tell application "Spotify" to playpause
                return "spotify"
            else if musicRunning then
                tell application "Music" to playpause
                return "music"
            else
                return "no_app"
            end if
        "#;

        Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("Media controls not supported on this platform".to_string())
    }
}

/// Skip to the next track
#[tauri::command]
pub fn media_next_track() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        let script = r#"
            tell application "System Events"
                set spotifyRunning to (name of processes) contains "Spotify"
                set musicRunning to (name of processes) contains "Music"
            end tell

            if spotifyRunning then
                tell application "Spotify" to next track
                return "spotify"
            else if musicRunning then
                tell application "Music" to next track
                return "music"
            else
                return "no_app"
            end if
        "#;

        Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("Media controls not supported on this platform".to_string())
    }
}

/// Go to the previous track
#[tauri::command]
pub fn media_previous_track() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        let script = r#"
            tell application "System Events"
                set spotifyRunning to (name of processes) contains "Spotify"
                set musicRunning to (name of processes) contains "Music"
            end tell

            if spotifyRunning then
                tell application "Spotify" to previous track
                return "spotify"
            else if musicRunning then
                tell application "Music" to back track
                return "music"
            else
                return "no_app"
            end if
        "#;

        Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("Media controls not supported on this platform".to_string())
    }
}

/// Seek to a specific position in the track (in seconds)
#[tauri::command]
pub async fn media_seek(position: f64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        let script = format!(
            r#"
            tell application "System Events"
                set spotifyRunning to (name of processes) contains "Spotify"
                set musicRunning to (name of processes) contains "Music"
            end tell

            if spotifyRunning then
                tell application "Spotify" to set player position to {}
                return "spotify"
            else if musicRunning then
                tell application "Music" to set player position to {}
                return "music"
            else
                return "no_app"
            end if
            "#,
            position, position
        );

        Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("Media controls not supported on this platform".to_string())
    }
}

/// Activate the media application
#[tauri::command]
pub fn activate_media_app(app_name: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        // Map common names if necessary, but open -a usually handles "Spotify", "Music", "Safari" fine.
        // "Music" might need to be "Music" (it's the app name).

        let app = match app_name.to_lowercase().as_str() {
            "music" => "Music",
            "spotify" => "Spotify",
            "safari" => "Safari",
            _ => &app_name,
        };

        Command::new("open")
            .arg("-a")
            .arg(app)
            .output()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("Not supported on this platform".to_string())
    }
}

use std::thread;

/// Setup audio level monitoring using simulated audio visualization
#[cfg(target_os = "macos")]
pub fn setup_audio_monitoring(app_handle: tauri::AppHandle) {
    // Initialize the audio levels storage if not already done
    if AUDIO_LEVELS.get().is_none() {
        let _ = AUDIO_LEVELS.set(std::sync::Mutex::new(vec![0.15; 6]));
    }

    // Spawn simulation thread
    thread::spawn(move || {
        println!("🎭 Starting audio visualization simulation");

        let mut t = 0.0f64;
        let mut prev_levels = vec![0.15; 6];
        let mut beat_phase = 0.0f64;
        let mut energy = 0.5f64;

        // Reduce to 30fps to save IPC overhead and make transitions smoother
        let frame_duration = std::time::Duration::from_micros(33333); // ~30fps
        let mut next_frame = std::time::Instant::now();

        loop {
            if !IS_PLAYING.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(200));
                next_frame = std::time::Instant::now();
                continue;
            }

            t += 0.0333; // Time increment per frame (30fps)

            // Simulate varying energy levels (like quiet vs loud parts of a song)
            let energy_wave = (t * 0.15).sin() * 0.3 + 0.9;
            energy = energy * 0.995 + energy_wave * 0.005;

            // Simulate beat at ~160 BPM (2.67 beats per second)
            beat_phase += 0.0333 * 2.67 * std::f64::consts::PI * 2.67;
            let beat = (beat_phase.sin().max(0.0)).powf(4.0); // Sharp beat pulse

            // Add some randomness for realism
            let noise = || -> f64 {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                ((t * 10000.0) as u64).hash(&mut hasher);
                (hasher.finish() % 1000) as f64 / 1000.0 - 0.5
            };

            let mut levels = vec![0.0; 6];

            // Bass (20-150 Hz) - strongest on beat
            levels[0] = energy * (0.4 + beat * 0.5 + noise() * 0.1);

            // Low-mid (150-400 Hz) - follows bass with slight delay
            levels[1] = energy * (0.35 + beat * 0.3 + (t * 3.2).sin() * 0.15 + noise() * 0.08);

            // Mid (400-1000 Hz) - melodic content
            levels[2] =
                energy * (0.3 + (t * 5.7).sin() * 0.2 + (t * 7.3).cos() * 0.1 + noise() * 0.1);

            // High-mid (1000-2500 Hz) - vocals, instruments
            levels[3] =
                energy * (0.28 + (t * 4.1).sin() * 0.18 + (t * 6.8).cos() * 0.12 + noise() * 0.08);

            // Presence (2500-6000 Hz) - clarity, attack
            levels[4] = energy * (0.22 + (t * 8.3).sin() * 0.15 + beat * 0.1 + noise() * 0.1);

            // Brilliance (6000-20000 Hz) - air, shimmer (generally lower)
            levels[5] =
                energy * (0.18 + (t * 11.2).sin() * 0.1 + (t * 9.7).cos() * 0.08 + noise() * 0.06);

            // Smooth transitions (exponential moving average)
            for i in 0..6 {
                // Adjusted smoothing for 30fps (needs to be slightly higher to match speed of 60fps)
                let smoothing = if levels[i] > prev_levels[i] {
                    0.5 // faster attack
                } else {
                    0.25 // slower decay
                };
                levels[i] = prev_levels[i] + (levels[i] - prev_levels[i]) * smoothing;
                // Clamp to valid range
                levels[i] = levels[i].clamp(0.08, 0.92);
            }

            prev_levels = levels.clone();

            set_audio_levels(levels.clone());
            let _ = app_handle.emit("audio-levels-update", levels);

            // Precise timing for consistent 60fps
            next_frame += frame_duration;
            let now = std::time::Instant::now();
            if next_frame > now {
                std::thread::sleep(next_frame - now);
            } else {
                // If we're behind, reset timing
                next_frame = now + frame_duration;
            }
        }
    });
}
