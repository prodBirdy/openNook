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
                                                    var audio = document.querySelector('audio');
                                                    var activeMedia = (video && !video.paused && !video.ended) ? video : (audio && !audio.paused && !audio.ended) ? audio : null;

                                                    if (!activeMedia) return 'paused';

                                                    var duration = activeMedia.duration || '';
                                                    var currentTime = activeMedia.currentTime || '';

                                                    var art = '';
                                                    try {
                                                        if (location.hostname.includes('spotify')) {
                                                            var img = document.querySelector('img[alt^=\"Now playing\"]') || document.querySelector('img[data-testid=\"cover-art-image\"]');
                                                            if (img) art = img.src;
                                                        } else if (location.hostname.includes('music.youtube')) {
                                                            var img = document.querySelector('.ytmusic-player-bar.style-scope img');
                                                            if (img) art = img.src;
                                                        }

                                                        if (!art) {
                                                            var icon = document.querySelector('link[rel*=\"icon\"]');
                                                            if (icon) art = icon.href;
                                                        }
                                                    } catch (e) {}

                                                    return 'playing|' + art + '|' + duration + '|' + currentTime;
                                                })();
                                            " in t

                                            if jsResult starts with "playing" then
                                                set AppleScript's text item delimiters to "|"
                                                set jsParts to text items of jsResult
                                                set artUrl to ""
                                                set trackDuration to ""
                                                set trackPosition to ""

                                                if (count of jsParts) > 1 then
                                                    set artUrl to item 2 of jsParts
                                                end if
                                                if (count of jsParts) > 2 then
                                                    set trackDuration to item 3 of jsParts
                                                end if
                                                if (count of jsParts) > 3 then
                                                    set trackPosition to item 4 of jsParts
                                                end if

                                                return "playing|" & tabName & "|Safari|" & tabURL & "|" & trackDuration & "|" & trackPosition & "|" & artUrl & "|safari"
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

    #[cfg(target_os = "windows")]
    {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        use windows::Storage::Streams::DataReader;

        if let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
            if let Ok(manager) = manager.await {
                if let Ok(session) = manager.GetCurrentSession() {
                    if let Ok(properties) = session.TryGetMediaPropertiesAsync().unwrap().await {
                        let title = properties.Title().ok().map(|h| h.to_string());
                        let artist = properties.Artist().ok().map(|h| h.to_string());
                        let album = properties.AlbumTitle().ok().map(|h| h.to_string());

                        // Check playback status
                        let playback_info = session.GetPlaybackInfo().unwrap();
                        let is_playing = playback_info.PlaybackStatus().unwrap() == windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;

                        // Get timeline
                        let timeline = session.GetTimelineProperties().unwrap();
                        let duration = timeline
                            .EndTime()
                            .ok()
                            .map(|t| t.Duration as f64 / 10_000_000.0);
                        let position = timeline
                            .Position()
                            .ok()
                            .map(|t| t.Duration as f64 / 10_000_000.0);

                        // Artwork
                        // Getting stream from IRandomAccessStreamReference
                        let mut artwork_base64 = None;
                        if let Ok(thumb_ref) = properties.Thumbnail() {
                            if let Ok(stream) = thumb_ref.OpenReadAsync().unwrap().await {
                                let size = stream.Size().unwrap() as usize;
                                let reader = DataReader::CreateDataReader(&stream).unwrap();
                                if reader.LoadAsync(size as u32).unwrap().await.is_ok() {
                                    let mut buffer = vec![0u8; size];
                                    if reader.ReadBytes(&mut buffer).is_ok() {
                                        artwork_base64 = Some(base64_encode(&buffer));
                                    }
                                }
                            }
                        }

                        IS_PLAYING.store(is_playing, Ordering::Relaxed);

                        let data = NowPlayingData {
                            title,
                            artist,
                            album,
                            artwork_base64,
                            duration,
                            elapsed_time: position,
                            is_playing,
                            audio_levels: Some(get_audio_levels_internal()),
                            app_name: Some("System".to_string()),
                        };

                        save_last_played(&data);
                        return data;
                    }
                }
            }
        }

        get_last_played_or_default(get_audio_levels())
    }

    #[cfg(target_os = "linux")]
    {
        // Use zbus to query MPRIS
        // This is a simplified implementation, ideally we'd iterate over names
        use zbus::zvariant::Value;
        use zbus::{proxy, Connection};

        // Define a simple proxy for MPRIS Player
        #[proxy(
            interface = "org.mpris.MediaPlayer2.Player",
            default_path = "/org/mpris/MediaPlayer2"
        )]
        trait Player {
            #[zbus(property)]
            fn playback_status(&self) -> zbus::Result<String>;
            #[zbus(property)]
            fn metadata(
                &self,
            ) -> zbus::Result<std::collections::HashMap<String, zbus::zvariant::OwnedValue>>;
            #[zbus(property)]
            fn position(&self) -> zbus::Result<i64>;
        }

        if let Ok(conn) = Connection::session().await {
            // Find a media player (simplified: grabbing first one or a specific one)
            // Real impl should list names org.mpris.MediaPlayer2.*

            // We can list names
            let proxy = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
            let names = proxy.list_names().await.unwrap();

            for name in names {
                if name.starts_with("org.mpris.MediaPlayer2.") {
                    let player = PlayerProxy::builder(&conn)
                        .destination(name.clone())
                        .unwrap()
                        .build()
                        .await
                        .unwrap();

                    if let Ok(status) = player.playback_status().await {
                        if status == "Playing" {
                            IS_PLAYING.store(true, Ordering::Relaxed);

                            let mut title = None;
                            let mut artist = None;
                            let mut album = None;
                            let mut duration = None;
                            let mut artwork_url = None;

                            if let Ok(metadata) = player.metadata().await {
                                if let Some(t) = metadata.get("xesam:title") {
                                    if let Value::Str(v) = &**t {
                                        title = Some(v.to_string());
                                    }
                                }
                                if let Some(a) = metadata.get("xesam:artist") {
                                    // artist is often array of strings
                                    if let Value::Array(v) = &**a {
                                        if let Ok(Some(Value::Str(s))) = v.get(0) {
                                            artist = Some(s.to_string());
                                        }
                                    }
                                }
                                if let Some(a) = metadata.get("xesam:album") {
                                    if let Value::Str(v) = &**a {
                                        album = Some(v.to_string());
                                    }
                                }
                                if let Some(d) = metadata.get("mpris:length") {
                                    if let Value::I64(v) = &**d {
                                        duration = Some(*v as f64 / 1_000_000.0);
                                    } else if let Value::U64(v) = &**d {
                                        duration = Some(*v as f64 / 1_000_000.0);
                                    }
                                }
                                if let Some(u) = metadata.get("mpris:artUrl") {
                                    if let Value::Str(v) = &**u {
                                        artwork_url = Some(v.to_string());
                                    }
                                }
                            }

                            let position =
                                player.position().await.ok().map(|p| p as f64 / 1_000_000.0);

                            let artwork_base64 = if let Some(url) = artwork_url {
                                fetch_artwork_from_url(&url)
                            } else {
                                None
                            };

                            let data = NowPlayingData {
                                title,
                                artist,
                                album,
                                artwork_base64,
                                duration,
                                elapsed_time: position,
                                is_playing: true,
                                audio_levels: Some(get_audio_levels_internal()),
                                app_name: Some(name.replace("org.mpris.MediaPlayer2.", "")),
                            };
                            save_last_played(&data);
                            return data;
                        }
                    }
                }
            }
        }

        get_last_played_or_default(get_audio_levels())
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
pub async fn media_play_pause() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        // Script that controls whichever app is playing
        let script = r#"
            tell application "System Events"
                set spotifyRunning to (name of processes) contains "Spotify"
                set musicRunning to (name of processes) contains "Music"
                set safariRunning to (name of processes) contains "Safari"
            end tell

            if spotifyRunning then
                tell application "Spotify"
                    if player state is playing then
                        playpause
                        return "spotify"
                    end if
                end tell
            end if

            if musicRunning then
                tell application "Music"
                    if player state is playing then
                        playpause
                        return "music"
                    end if
                end tell
            end if

            if safariRunning then
                tell application "Safari"
                    try
                        repeat with w in windows
                            repeat with t in tabs of w
                                try
                                    set tabURL to URL of t
                                    if tabURL contains "youtube.com" or tabURL contains "music.youtube.com" or tabURL contains "open.spotify.com" or tabURL contains "soundcloud.com" then
                                        do JavaScript "
                                            (function() {
                                                var video = document.querySelector('video');
                                                var audio = document.querySelector('audio');
                                                var activeMedia = (video && !video.paused && !video.ended) ? video : (audio && !audio.paused && !audio.ended) ? audio : null;
                                                // Fallback if paused but visible
                                                if (!activeMedia) activeMedia = video || audio;

                                                if (activeMedia) {
                                                    if (activeMedia.paused) { activeMedia.play(); } else { activeMedia.pause(); }
                                                    return 'success';
                                                }
                                                return 'no_media';
                                            })();
                                        " in t
                                        return "safari"
                                    end if
                                end try
                            end repeat
                        end repeat
                    end try
                end tell
            end if

            -- Fallback: If nothing was specifically playing, just try to toggle Spotify then Music
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

    #[cfg(target_os = "windows")]
    {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        if let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
            if let Ok(manager) = manager.await {
                if let Ok(session) = manager.GetCurrentSession() {
                    let _ = session.TryTogglePlayPauseAsync().unwrap().await;
                }
            }
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        use zbus::{proxy, Connection};
        #[proxy(
            interface = "org.mpris.MediaPlayer2.Player",
            default_path = "/org/mpris/MediaPlayer2"
        )]
        trait Player {
            fn play_pause(&self) -> zbus::Result<()>;
        }

        if let Ok(conn) = Connection::session().await {
            let proxy = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
            let names = proxy.list_names().await.unwrap();
            for name in names {
                if name.starts_with("org.mpris.MediaPlayer2.") {
                    let player = PlayerProxy::builder(&conn)
                        .destination(name)
                        .unwrap()
                        .build()
                        .await
                        .unwrap();
                    let _ = player.play_pause().await;
                }
            }
        }
        Ok(())
    }
}

/// Skip to the next track
#[tauri::command]
pub async fn media_next_track() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        let script = r#"
            tell application "System Events"
                set spotifyRunning to (name of processes) contains "Spotify"
                set musicRunning to (name of processes) contains "Music"
                set safariRunning to (name of processes) contains "Safari"
            end tell

            if spotifyRunning then
                tell application "Spotify"
                    if player state is playing then
                        next track
                        return "spotify"
                    end if
                end tell
            end if

            if musicRunning then
                tell application "Music"
                    if player state is playing then
                        next track
                        return "music"
                    end if
                end tell
            end if

            if safariRunning then
                tell application "Safari"
                    try
                        repeat with w in windows
                            repeat with t in tabs of w
                                try
                                    set tabURL to URL of t
                                    if tabURL contains "youtube.com" or tabURL contains "music.youtube.com" then
                                        do JavaScript "
                                            (function() {
                                                // YouTube specific next button
                                                var nextBtn = document.querySelector('.ytp-next-button') || document.querySelector('[title=\"Next button\"]') || document.querySelector('.next-button');
                                                if (nextBtn) {
                                                    nextBtn.click();
                                                    return 'success';
                                                }
                                                return 'no_btn';
                                            })();
                                        " in t
                                        return "safari"
                                    else if tabURL contains "open.spotify.com" then
                                        do JavaScript "
                                            (function() {
                                                var nextBtn = document.querySelector('[data-testid=\"control-button-skip-forward\"]');
                                                if (nextBtn) {
                                                    nextBtn.click();
                                                    return 'success';
                                                }
                                                return 'no_btn';
                                            })();
                                        " in t
                                        return "safari"
                                    end if
                                end try
                            end repeat
                        end repeat
                    end try
                end tell
            end if

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

    #[cfg(target_os = "windows")]
    {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        if let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
            if let Ok(manager) = manager.await {
                if let Ok(session) = manager.GetCurrentSession() {
                    let _ = session.TrySkipNextAsync().unwrap().await;
                }
            }
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        use zbus::{proxy, Connection};
        #[proxy(
            interface = "org.mpris.MediaPlayer2.Player",
            default_path = "/org/mpris/MediaPlayer2"
        )]
        trait Player {
            fn next(&self) -> zbus::Result<()>;
        }
        if let Ok(conn) = Connection::session().await {
            let proxy = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
            let names = proxy.list_names().await.unwrap();
            for name in names {
                if name.starts_with("org.mpris.MediaPlayer2.") {
                    let player = PlayerProxy::builder(&conn)
                        .destination(name)
                        .unwrap()
                        .build()
                        .await
                        .unwrap();
                    let _ = player.next().await;
                }
            }
        }
        Ok(())
    }
}

/// Go to the previous track
#[tauri::command]
pub async fn media_previous_track() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        let script = r#"
            tell application "System Events"
                set spotifyRunning to (name of processes) contains "Spotify"
                set musicRunning to (name of processes) contains "Music"
                set safariRunning to (name of processes) contains "Safari"
            end tell

            if spotifyRunning then
                tell application "Spotify"
                    if player state is playing then
                        previous track
                        return "spotify"
                    end if
                end tell
            end if

            if musicRunning then
                tell application "Music"
                    if player state is playing then
                        back track
                        return "music"
                    end if
                end tell
            end if

            if safariRunning then
                tell application "Safari"
                    try
                        repeat with w in windows
                            repeat with t in tabs of w
                                try
                                    set tabURL to URL of t
                                    if tabURL contains "youtube.com" or tabURL contains "music.youtube.com" then
                                        do JavaScript "
                                            (function() {
                                                // Try to go back ~10s or restart video usually, or strictly prev button if playlist
                                                // For now, let's try history back or restart
                                                window.history.back();
                                                return 'success';
                                            })();
                                        " in t
                                        return "safari"
                                    else if tabURL contains "open.spotify.com" then
                                        do JavaScript "
                                            (function() {
                                                var prevBtn = document.querySelector('[data-testid=\"control-button-skip-back\"]');
                                                if (prevBtn) {
                                                    prevBtn.click();
                                                    return 'success';
                                                }
                                                return 'no_btn';
                                            })();
                                        " in t
                                        return "safari"
                                    end if
                                end try
                            end repeat
                        end repeat
                    end try
                end tell
            end if

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

    #[cfg(target_os = "windows")]
    {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        if let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
            if let Ok(manager) = manager.await {
                if let Ok(session) = manager.GetCurrentSession() {
                    let _ = session.TrySkipPreviousAsync().unwrap().await;
                }
            }
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        use zbus::{proxy, Connection};
        #[proxy(
            interface = "org.mpris.MediaPlayer2.Player",
            default_path = "/org/mpris/MediaPlayer2"
        )]
        trait Player {
            fn previous(&self) -> zbus::Result<()>;
        }
        if let Ok(conn) = Connection::session().await {
            let proxy = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
            let names = proxy.list_names().await.unwrap();
            for name in names {
                if name.starts_with("org.mpris.MediaPlayer2.") {
                    let player = PlayerProxy::builder(&conn)
                        .destination(name)
                        .unwrap()
                        .build()
                        .await
                        .unwrap();
                    let _ = player.previous().await;
                }
            }
        }
        Ok(())
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
                set safariRunning to (name of processes) contains "Safari"
            end tell

            if spotifyRunning then
                tell application "Spotify"
                    if player state is playing then
                        set player position to {}
                        return "spotify"
                    end if
                end tell
            end if

            if musicRunning then
                tell application "Music"
                    if player state is playing then
                        set player position to {}
                        return "music"
                    end if
                end tell
            end if

            if safariRunning then
                tell application "Safari"
                    try
                        repeat with w in windows
                            repeat with t in tabs of w
                                try
                                    set tabURL to URL of t
                                    if tabURL contains "youtube.com" or tabURL contains "music.youtube.com" or tabURL contains "open.spotify.com" or tabURL contains "soundcloud.com" then
                                        do JavaScript "
                                            (function() {{
                                                var video = document.querySelector('video');
                                                var audio = document.querySelector('audio');
                                                var activeMedia = (video && !video.paused && !video.ended) ? video : (audio && !audio.paused && !audio.ended) ? audio : null;
                                                // Fallback if paused but visible
                                                if (!activeMedia) activeMedia = video || audio;

                                                if (activeMedia) {{
                                                    activeMedia.currentTime = {};
                                                    return 'success';
                                                }}
                                                return 'no_media';
                                            }})();
                                        " in t
                                        return "safari"
                                    end if
                                end try
                            end repeat
                        end repeat
                    end try
                end tell
            end if

            return "no_app"
            "#,
            position, position, position
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
        // Seeking not easily supported in global transport controls universally
        Ok(())
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
        // Maybe try opening by protocol or process name?
        Ok(())
    }
}

use std::thread;

/// Setup audio level monitoring using simulated audio visualization
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
