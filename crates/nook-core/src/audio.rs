use crate::models::NowPlayingData;
use crate::utils::fetch_artwork_from_url;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Notify;

/// Global state for audio levels (updated by audio monitoring thread)
static AUDIO_LEVELS: std::sync::OnceLock<std::sync::Mutex<Vec<f64>>> = std::sync::OnceLock::new();

/// Global state to track if media is playing (to pause simulation)
static IS_PLAYING: AtomicBool = AtomicBool::new(false);

/// Track if we were playing in the previous poll cycle (to detect resume)
static WAS_PLAYING: AtomicBool = AtomicBool::new(false);

/// Cache for current track info to avoid refetching artwork
/// Format: (title, artist, artwork_base64)
type TrackCache = (Option<String>, Option<String>, Option<String>);
static TRACK_CACHE: std::sync::OnceLock<std::sync::Mutex<TrackCache>> = std::sync::OnceLock::new();

/// Cache for the last played track to display when paused/idle
static LAST_PLAYED: std::sync::OnceLock<std::sync::Mutex<Option<NowPlayingData>>> =
    std::sync::OnceLock::new();

pub fn init_audio_state() {
    let _ = AUDIO_LEVELS.set(std::sync::Mutex::new(vec![0.15; 6]));
    let _ = TRACK_CACHE.set(std::sync::Mutex::new((None, None, None)));
    let _ = LAST_PLAYED.set(std::sync::Mutex::new(None));
    #[cfg(target_os = "macos")]
    crate::mediaremote::ensure_stream();
}

fn lock_mutex<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn get_audio_levels_internal() -> Vec<f64> {
    AUDIO_LEVELS
        .get()
        .map(|m| lock_mutex(m).clone())
        .unwrap_or_else(|| vec![0.15; 6])
}

fn set_audio_levels(levels: Vec<f64>) {
    if let Some(m) = AUDIO_LEVELS.get() {
        *lock_mutex(m) = levels;
    }
}

/// Get current audio levels for visualizer (lightweight, no AppleScript calls)
pub fn get_audio_levels() -> Vec<f64> {
    get_audio_levels_internal()
}

#[cfg(target_os = "macos")]
fn get_cached_track() -> (Option<String>, Option<String>, Option<String>) {
    TRACK_CACHE
        .get()
        .map(|m| lock_mutex(m).clone())
        .unwrap_or((None, None, None))
}

#[cfg(target_os = "macos")]
fn set_cached_track(title: Option<String>, artist: Option<String>, artwork: Option<String>) {
    if let Some(m) = TRACK_CACHE.get() {
        *lock_mutex(m) = (title, artist, artwork);
    }
}

#[cfg(target_os = "macos")]
fn is_track_changed(title: &Option<String>, artist: &Option<String>) -> bool {
    let cached = get_cached_track();
    cached.0 != *title || cached.1 != *artist
}

fn save_last_played(data: &NowPlayingData) {
    if let Some(m) = LAST_PLAYED.get() {
        *lock_mutex(m) = Some(data.clone());
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

#[cfg(target_os = "macos")]
async fn now_playing_from_adapter(track: crate::mediaremote::AdapterTrack) -> NowPlayingData {
    IS_PLAYING.store(track.is_playing, Ordering::Relaxed);
    let just_started = track.is_playing && !WAS_PLAYING.swap(track.is_playing, Ordering::Relaxed);
    if !track.is_playing {
        WAS_PLAYING.store(false, Ordering::Relaxed);
    }

    let track_changed = is_track_changed(&track.title, &track.artist);
    let mut artwork = if !track_changed {
        get_cached_track().2
    } else {
        None
    };
    if artwork.is_none() {
        artwork = track.artwork_base64.clone();
    }
    if crate::browser_media::is_browser(track.app_name.as_deref(), track.bundle_id.as_deref())
        && (track_changed || just_started || artwork.is_none())
    {
        if let Some(resolved) = crate::browser_media::resolve_artwork(
            track.app_name.as_deref(),
            track.bundle_id.as_deref(),
            track.title.as_deref(),
        )
        .await
        {
            artwork = Some(resolved);
        }
    }
    if track_changed || just_started || artwork.is_some() {
        set_cached_track(track.title.clone(), track.artist.clone(), artwork.clone());
    }

    let data = NowPlayingData {
        title: track.title,
        artist: track.artist,
        album: track.album,
        artwork_base64: artwork,
        duration: track.duration,
        elapsed_time: track.elapsed_time,
        is_playing: track.is_playing,
        audio_levels: Some(get_audio_levels_internal()),
        app_name: track.app_name,
        bundle_id: track.bundle_id,
        motion_artwork_url: None,
    };
    save_last_played(&data);
    data
}

#[cfg(target_os = "macos")]
fn build_now_playing_script(
    spotify_running: bool,
    music_running: bool,
    safari_running: bool,
) -> String {
    let mut script = String::new();

    // Check Spotify
    if spotify_running {
        script.push_str(
            r#"
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
        "#,
        );
    }

    // Check Music
    if music_running {
        script.push_str(
            r#"
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
        "#,
        );
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
    script
}

fn safari_artwork_url<'a>(page_url: &str, artwork_url: &'a str) -> Option<&'a str> {
    let page = reqwest::Url::parse(page_url).ok()?;
    let artwork = reqwest::Url::parse(artwork_url).ok()?;
    if page.scheme() != "https" || artwork.scheme() != "https" {
        return None;
    }
    let page_host = page.host_str()?;
    let artwork_host = artwork.host_str()?;
    let allowed_art_hosts: &[&str] = match page_host {
        "youtube.com" | "www.youtube.com" | "music.youtube.com" => &[
            "youtube.com",
            "www.youtube.com",
            "music.youtube.com",
            "i.ytimg.com",
            "yt3.ggpht.com",
            "lh3.googleusercontent.com",
        ],
        "open.spotify.com" => &["open.spotify.com", "i.scdn.co", "mosaic.scdn.co"],
        "soundcloud.com" | "www.soundcloud.com" => &[
            "soundcloud.com",
            "www.soundcloud.com",
            "i1.sndcdn.com",
            "a1.sndcdn.com",
        ],
        _ => return None,
    };
    allowed_art_hosts
        .contains(&artwork_host)
        .then_some(artwork_url)
}

/// Something changed (or was told to change) in playback: a Spotify/Music
/// distributed notification arrived, or the island itself sent a transport
/// command. The now-playing loop consumes this to poll immediately instead of
/// waiting out its idle cadence.
static MEDIA_EVENT: AtomicBool = AtomicBool::new(false);

/// Launch Services snapshot of the AppleScript fallback players, refreshed
/// from `NSWorkspace` launch/terminate notifications instead of walking the
/// process table. Bit0 = primed, bit1 = Spotify, bit2 = Music, bit3 = Safari.
static MEDIA_APPS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
const MEDIA_APPS_PRIMED: u8 = 1 << 0;
const MEDIA_APP_SPOTIFY: u8 = 1 << 1;
const MEDIA_APP_MUSIC: u8 = 1 << 2;
const MEDIA_APP_SAFARI: u8 = 1 << 3;

static MEDIA_WAKE: OnceLock<Notify> = OnceLock::new();

fn media_wake() -> &'static Notify {
    MEDIA_WAKE.get_or_init(Notify::new)
}

pub fn note_media_event() {
    MEDIA_EVENT.store(true, Ordering::Relaxed);
    media_wake().notify_waiters();
}

pub fn take_media_event() -> bool {
    MEDIA_EVENT.swap(false, Ordering::Relaxed)
}

/// Park until a playback-change poke or `timeout`.
///
/// The island now-playing loop uses this so idle is one timer, not a 250 ms
/// poll of the event flag. Subscribe before checking the flag so a poke that
/// lands in between cannot be missed.
pub async fn wait_media_or_timeout(timeout: Duration) {
    let notified = media_wake().notified();
    tokio::pin!(notified);
    if MEDIA_EVENT.swap(false, Ordering::Relaxed) {
        return;
    }
    tokio::select! {
        _ = notified => {
            let _ = MEDIA_EVENT.swap(false, Ordering::Relaxed);
        }
        _ = tokio::time::sleep(timeout) => {}
    }
}

/// Record that a media-capable app launched or quit. Unknown bundle ids are
/// ignored so the workspace observers can be wired to every app event.
pub fn note_media_app_running(bundle_id: &str, running: bool) {
    let bit = match bundle_id {
        "com.spotify.client" => MEDIA_APP_SPOTIFY,
        "com.apple.Music" => MEDIA_APP_MUSIC,
        "com.apple.Safari" => MEDIA_APP_SAFARI,
        _ => return,
    };
    let mut packed = MEDIA_APPS.load(Ordering::Relaxed) | MEDIA_APPS_PRIMED;
    if running {
        packed |= bit;
    } else {
        packed &= !bit;
    }
    MEDIA_APPS.store(packed, Ordering::Relaxed);
}

/// `(spotify, music, safari)` from the launch/terminate cache, priming via
/// `NSRunningApplication` once if no observer has run yet.
#[cfg(target_os = "macos")]
fn media_players_running() -> (bool, bool, bool) {
    let packed = MEDIA_APPS.load(Ordering::Relaxed);
    if packed & MEDIA_APPS_PRIMED == 0 {
        return prime_media_apps();
    }
    (
        packed & MEDIA_APP_SPOTIFY != 0,
        packed & MEDIA_APP_MUSIC != 0,
        packed & MEDIA_APP_SAFARI != 0,
    )
}

/// Seed the media-app bits from Launch Services. Called from
/// `install_media_observers` so the first AppleScript poll does not race.
#[cfg(target_os = "macos")]
pub fn prime_media_apps() -> (bool, bool, bool) {
    let spotify = app_running(c"com.spotify.client");
    let music = app_running(c"com.apple.Music");
    let safari = app_running(c"com.apple.Safari");
    let mut packed = MEDIA_APPS_PRIMED;
    if spotify {
        packed |= MEDIA_APP_SPOTIFY;
    }
    if music {
        packed |= MEDIA_APP_MUSIC;
    }
    if safari {
        packed |= MEDIA_APP_SAFARI;
    }
    MEDIA_APPS.store(packed, Ordering::Relaxed);
    (spotify, music, safari)
}

/// Whether an app with this bundle id is running, via Launch Services.
#[cfg(target_os = "macos")]
fn app_running(bundle_id: &std::ffi::CStr) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::*;
    unsafe {
        let ident: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: bundle_id.as_ptr()];
        let apps: *mut AnyObject = msg_send![
            class!(NSRunningApplication),
            runningApplicationsWithBundleIdentifier: ident
        ];
        if apps.is_null() {
            return false;
        }
        let count: usize = msg_send![apps, count];
        count > 0
    }
}

/// Get currently playing music information.
///
/// On macOS this prefers MediaRemote via [mediaremote-adapter]
/// (live `stream` cell, `get --now` only as primer / fallback),
/// which works for any now-playing app on 15.4+.
/// AppleScript (Spotify / Music / Safari) is the fallback.
///
/// [mediaremote-adapter]: https://github.com/ungive/mediaremote-adapter
pub async fn get_now_playing() -> NowPlayingData {
    #[cfg(target_os = "macos")]
    {
        if crate::mediaremote::is_available() {
            match crate::mediaremote::get_now_playing() {
                Ok(Some(track)) => return now_playing_from_adapter(track).await,
                Ok(None) => {
                    IS_PLAYING.store(false, Ordering::Relaxed);
                    WAS_PLAYING.store(false, Ordering::Relaxed);
                    return get_last_played_or_default(get_audio_levels());
                }
                Err(err) => {
                    log::debug!("MediaRemote adapter get failed: {err}");
                }
            }
        }

        // NSRunningApplication is documented thread-safe and reads Launch
        // Services' in-process app list — no walk of the whole process table
        // (the sysinfo refresh this replaces enumerated every pid on the
        // system each poll, a measurable slice of idle CPU).
        let (spotify_running, music_running, safari_running) = media_players_running();

        // If no relevant apps are running, return early with no overhead
        if !spotify_running && !music_running && !safari_running {
            IS_PLAYING.store(false, Ordering::Relaxed);
            WAS_PLAYING.store(false, Ordering::Relaxed);
            return get_last_played_or_default(get_audio_levels());
        }

        // Default to not playing before check
        IS_PLAYING.store(false, Ordering::Relaxed);

        let script = build_now_playing_script(spotify_running, music_running, safari_running);

        if let Ok(stdout) = run_osascript(&script) {
            let parts: Vec<&str> = stdout.split('|').collect();

            if parts.len() >= 8 && parts[0] == "playing" {
                IS_PLAYING.store(true, Ordering::Relaxed);
                // Check if we just started playing (transition from not playing to playing)
                let just_started = !WAS_PLAYING.swap(true, Ordering::Relaxed);

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

                let track_changed = is_track_changed(&title, &artist);

                // Fetch artwork if track changed OR if we just resumed playback
                let artwork = if track_changed || just_started {
                    if *app_id == "music" {
                        get_music_app_artwork()
                    } else if let Some(url) = if *app_id == "safari" {
                        safari_artwork_url(parts[3], parts[6])
                    } else {
                        (!parts[6].is_empty()).then_some(parts[6])
                    } {
                        fetch_artwork_from_url(url).await
                    } else {
                        None
                    }
                } else {
                    get_cached_track().2
                };

                if track_changed || just_started {
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
                    bundle_id: match *app_id {
                        "spotify" => Some("com.spotify.client".into()),
                        "music" => Some("com.apple.Music".into()),
                        "safari" => Some("com.apple.Safari".into()),
                        _ => None,
                    },
                    motion_artwork_url: None,
                };

                save_last_played(&data);
                return data;
            }
        }

        WAS_PLAYING.store(false, Ordering::Relaxed);
        get_last_played_or_default(get_audio_levels())
    }

    #[cfg(target_os = "windows")]
    {
        use std::sync::mpsc;
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        use windows::Storage::Streams::DataReader;

        // Spawn a thread to handle all Windows COM operations synchronously
        // This avoids the !Send issue with COM types across await points
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = (|| -> Option<NowPlayingData> {
                // Use pollster or futures::executor to block on Windows async operations
                let manager = futures_executor::block_on(async {
                    let manager_async =
                        GlobalSystemMediaTransportControlsSessionManager::RequestAsync().ok()?;
                    manager_async.await.ok()
                })?;

                let session = manager.GetCurrentSession().ok()?;

                let properties = futures_executor::block_on(async {
                    let properties_async = session.TryGetMediaPropertiesAsync().ok()?;
                    properties_async.await.ok()
                })?;

                let title = properties.Title().ok().map(|h| h.to_string());
                let artist = properties.Artist().ok().map(|h| h.to_string());
                let album = properties.AlbumTitle().ok().map(|h| h.to_string());

                // Check playback status
                let is_playing = session.GetPlaybackInfo()
                    .ok()
                    .and_then(|info| info.PlaybackStatus().ok())
                    .map(|status| status == windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
                    .unwrap_or(false);

                // Get timeline
                let timeline = session.GetTimelineProperties().ok();
                let duration = timeline
                    .as_ref()
                    .and_then(|t| t.EndTime().ok())
                    .map(|t| t.Duration as f64 / 10_000_000.0);
                let position = timeline
                    .as_ref()
                    .and_then(|t| t.Position().ok())
                    .map(|t| t.Duration as f64 / 10_000_000.0);

                // Artwork - all COM operations done synchronously
                let artwork_base64 = futures_executor::block_on(async {
                    let thumb_ref = properties.Thumbnail().ok()?;
                    let stream_async = thumb_ref.OpenReadAsync().ok()?;
                    let stream = stream_async.await.ok()?;
                    let size = stream.Size().ok()? as usize;
                    let reader = DataReader::CreateDataReader(&stream).ok()?;
                    let load_async = reader.LoadAsync(size as u32).ok()?;
                    load_async.await.ok()?;
                    let mut buffer = vec![0u8; size];
                    reader.ReadBytes(&mut buffer).ok()?;
                    crate::utils::save_temp_file(&buffer, "png")
                });

                IS_PLAYING.store(is_playing, Ordering::Relaxed);

                Some(NowPlayingData {
                    title,
                    artist,
                    album,
                    artwork_base64,
                    duration,
                    elapsed_time: position,
                    is_playing,
                    audio_levels: Some(get_audio_levels_internal()),
                    app_name: Some("System".to_string()),
                    bundle_id: None,
                    motion_artwork_url: None,
                })
            })();

            let _ = tx.send(result);
        });

        // Wait for the result from the blocking thread
        if let Ok(Some(data)) = rx.recv() {
            save_last_played(&data);
            return data;
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
            let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await else {
                return get_last_played_or_default(get_audio_levels());
            };
            let Ok(names) = proxy.list_names().await else {
                return get_last_played_or_default(get_audio_levels());
            };

            for name in names {
                if name.starts_with("org.mpris.MediaPlayer2.") {
                    let Ok(builder) = PlayerProxy::builder(&conn).destination(name.clone()) else {
                        continue;
                    };
                    let Ok(player) = builder.build().await else {
                        continue;
                    };

                    if let Ok(status) = player.playback_status().await {
                        if status == "Playing" {
                            IS_PLAYING.store(true, Ordering::Relaxed);
                            let is_playing = true;

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
                                fetch_artwork_from_url(&url).await
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
                                is_playing,
                                audio_levels: Some(get_audio_levels_internal()),
                                app_name: Some(name.replace("org.mpris.MediaPlayer2.", "")),
                                bundle_id: None,
                                motion_artwork_url: None,
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

#[cfg(target_os = "macos")]
use crate::utils::run_osascript;

/// Get artwork from Music.app using AppleScript to write under the app data dir.
#[cfg(target_os = "macos")]
fn get_music_app_artwork() -> Option<String> {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    let dir = crate::app_data_dir().join("artwork");
    let _ = fs::create_dir_all(&dir);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp_path = dir.join(format!("music-{}-{unique}.data", std::process::id()));
    let temp_path_str = temp_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let _ = fs::remove_file(&temp_path);

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
        temp_path_str, temp_path_str
    );

    match run_osascript(&script) {
        Ok(stdout) if stdout == "success" => {
            if let Ok(data) = fs::read(&temp_path) {
                let _ = fs::remove_file(&temp_path);
                if !data.is_empty() {
                    return crate::utils::encode_bytes_base64(&data);
                }
            }
        }
        Ok(stdout) => log::debug!("Music artwork: {stdout}"),
        Err(err) => log::debug!("Music artwork: {err}"),
    }

    let _ = fs::remove_file(&temp_path);
    None
}

/// Toggle play/pause for the currently playing media.
/// macOS: MediaRemote `kMRATogglePlayPause` (send 2), AppleScript fallback.
pub async fn media_play_pause() -> Result<(), String> {
    note_media_event();
    #[cfg(target_os = "macos")]
    {
        if crate::mediaremote::is_available() {
            return crate::mediaremote::send(crate::mediaremote::MraCommand::TogglePlayPause);
        }

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

        let _ = run_osascript(script)?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        if let Ok(manager_async) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        {
            if let Ok(manager) = manager_async.await {
                if let Ok(session) = manager.GetCurrentSession() {
                    if let Ok(toggle_async) = session.TryTogglePlayPauseAsync() {
                        let _ = toggle_async.await;
                    }
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
            let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await else {
                return Ok(());
            };
            let Ok(names) = proxy.list_names().await else {
                return Ok(());
            };
            for name in names {
                if name.starts_with("org.mpris.MediaPlayer2.") {
                    let Ok(builder) = PlayerProxy::builder(&conn).destination(name) else {
                        continue;
                    };
                    let Ok(player) = builder.build().await else {
                        continue;
                    };
                    let _ = player.play_pause().await;
                }
            }
        }
        Ok(())
    }
}

/// Skip to the next track.
/// macOS: MediaRemote `kMRANextTrack` (send 4), AppleScript fallback.
pub async fn media_next_track() -> Result<(), String> {
    note_media_event();
    #[cfg(target_os = "macos")]
    {
        if crate::mediaremote::is_available() {
            return crate::mediaremote::send(crate::mediaremote::MraCommand::NextTrack);
        }

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

        let _ = run_osascript(script)?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        if let Ok(manager_async) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        {
            if let Ok(manager) = manager_async.await {
                if let Ok(session) = manager.GetCurrentSession() {
                    if let Ok(skip_async) = session.TrySkipNextAsync() {
                        let _ = skip_async.await;
                    }
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
            let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await else {
                return Ok(());
            };
            let Ok(names) = proxy.list_names().await else {
                return Ok(());
            };
            for name in names {
                if name.starts_with("org.mpris.MediaPlayer2.") {
                    let Ok(builder) = PlayerProxy::builder(&conn).destination(name) else {
                        continue;
                    };
                    let Ok(player) = builder.build().await else {
                        continue;
                    };
                    let _ = player.next().await;
                }
            }
        }
        Ok(())
    }
}

/// Go to the previous track.
/// macOS: MediaRemote `kMRAPreviousTrack` (send 5), AppleScript fallback.
pub async fn media_previous_track() -> Result<(), String> {
    note_media_event();
    #[cfg(target_os = "macos")]
    {
        if crate::mediaremote::is_available() {
            return crate::mediaremote::send(crate::mediaremote::MraCommand::PreviousTrack);
        }

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

        let _ = run_osascript(script)?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        if let Ok(manager_async) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        {
            if let Ok(manager) = manager_async.await {
                if let Ok(session) = manager.GetCurrentSession() {
                    if let Ok(skip_async) = session.TrySkipPreviousAsync() {
                        let _ = skip_async.await;
                    }
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
            let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await else {
                return Ok(());
            };
            let Ok(names) = proxy.list_names().await else {
                return Ok(());
            };
            for name in names {
                if name.starts_with("org.mpris.MediaPlayer2.") {
                    let Ok(builder) = PlayerProxy::builder(&conn).destination(name) else {
                        continue;
                    };
                    let Ok(player) = builder.build().await else {
                        continue;
                    };
                    let _ = player.previous().await;
                }
            }
        }
        Ok(())
    }
}

/// Seek to a timeline position in seconds.
/// macOS: MediaRemote `adapter_seek`, AppleScript fallback.
pub async fn media_seek(position: f64) -> Result<(), String> {
    note_media_event();
    #[cfg(target_os = "macos")]
    {
        if crate::mediaremote::is_available() {
            return crate::mediaremote::seek_seconds(position);
        }

        let script = format!(
            r#"
            tell application "System Events"
                set spotifyRunning to (name of processes) contains "Spotify"
                set musicRunning to (name of processes) contains "Music"
                set safariRunning to (name of processes) contains "Safari"
            end tell

            if spotifyRunning then
                tell application "Spotify"
                    set player position to {position}
                    return "spotify"
                end tell
            end if

            if musicRunning then
                tell application "Music"
                    set player position to {position}
                    return "music"
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
                                                var activeMedia = video || audio;
                                                if (activeMedia) {{
                                                    activeMedia.currentTime = {position};
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
            "#
        );
        let _ = run_osascript(&script)?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        if let Ok(manager_async) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        {
            if let Ok(manager) = manager_async.await {
                if let Ok(session) = manager.GetCurrentSession() {
                    let position_ticks = (position * 10_000_000.0) as i64;
                    if let Ok(seek_async) = session.TryChangePlaybackPositionAsync(position_ticks) {
                        let _ = seek_async.await;
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let _ = position;
        Ok(())
    }
}

use std::thread;

/// Setup audio level monitoring using simulated audio visualization
pub fn setup_audio_monitoring() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if AUDIO_LEVELS.get().is_none() {
        let _ = AUDIO_LEVELS.set(std::sync::Mutex::new(vec![0.15; 6]));
    }
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    thread::spawn(move || {
        log::info!("🎭 Starting audio visualization simulation");

        let mut t = 0.0f64;
        let mut prev_levels = vec![0.15; 6];
        let mut beat_phase = 0.0f64;
        let mut energy = 0.5f64;

        // 30fps: smooth bars; every level change repaints the island, so this
        // is a battery/smoothness tradeoff decided in favor of smoothness.
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

            // Simulate beat at ~160 BPM (2.67 beats per second): 2π · 2.67 · dt
            beat_phase += 0.0333 * 2.67 * std::f64::consts::TAU;
            let beat = (beat_phase.sin().max(0.0)).powf(4.0); // Sharp beat pulse

            // Add some randomness for realism (per-band: hash t and the band index)
            let noise = |band: u64| -> f64 {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                ((t * 10000.0) as u64)
                    .wrapping_mul(31)
                    .wrapping_add(band)
                    .hash(&mut hasher);
                (hasher.finish() % 1000) as f64 / 1000.0 - 0.5
            };

            let mut levels = vec![0.0; 6]; // In a loop, this could be reused, but Vec of 6 floats is trivial.
                                           // Keeping as is for simplicity unless specific optimization request for this.

            // Bass (20-150 Hz) - strongest on beat
            levels[0] = energy * (0.4 + beat * 0.5 + noise(0) * 0.1);

            // Low-mid (150-400 Hz) - follows bass with slight delay
            levels[1] = energy * (0.35 + beat * 0.3 + (t * 3.2).sin() * 0.15 + noise(1) * 0.08);

            // Mid (400-1000 Hz) - melodic content
            levels[2] =
                energy * (0.3 + (t * 5.7).sin() * 0.2 + (t * 7.3).cos() * 0.1 + noise(2) * 0.1);

            // High-mid (1000-2500 Hz) - vocals, instruments
            levels[3] =
                energy * (0.28 + (t * 4.1).sin() * 0.18 + (t * 6.8).cos() * 0.12 + noise(3) * 0.08);

            // Presence (2500-6000 Hz) - clarity, attack
            levels[4] = energy * (0.22 + (t * 8.3).sin() * 0.15 + beat * 0.1 + noise(4) * 0.1);

            // Brilliance (6000-20000 Hz) - air, shimmer (generally lower)
            levels[5] =
                energy * (0.18 + (t * 11.2).sin() * 0.1 + (t * 9.7).cos() * 0.08 + noise(5) * 0.06);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safari_artwork_rejects_deceptive_pages_and_private_urls() {
        assert_eq!(
            safari_artwork_url(
                "https://youtube.com.evil.example/watch",
                "http://127.0.0.1/favicon.ico"
            ),
            None
        );
    }

    #[test]
    fn media_app_running_cache_ignores_unrelated_bundles() {
        note_media_app_running("com.apple.finder", true);
        note_media_app_running("com.spotify.client", true);
        note_media_app_running("com.apple.Music", false);
        note_media_app_running("com.apple.Safari", true);
        let packed = MEDIA_APPS.load(Ordering::Relaxed);
        assert_ne!(packed & MEDIA_APPS_PRIMED, 0);
        assert_ne!(packed & MEDIA_APP_SPOTIFY, 0);
        assert_eq!(packed & MEDIA_APP_MUSIC, 0);
        assert_ne!(packed & MEDIA_APP_SAFARI, 0);
        note_media_app_running("com.spotify.client", false);
        let packed = MEDIA_APPS.load(Ordering::Relaxed);
        assert_eq!(packed & MEDIA_APP_SPOTIFY, 0);
    }

    #[test]
    fn safari_artwork_accepts_supported_https_origins() {
        assert_eq!(
            safari_artwork_url(
                "https://music.youtube.com/watch?v=1",
                "https://music.youtube.com/favicon.ico"
            ),
            Some("https://music.youtube.com/favicon.ico")
        );
    }

    static MEDIA_WAIT_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn wait_media_returns_when_event_already_set() {
        let _g = MEDIA_WAIT_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let _ = take_media_event();
        note_media_event();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let start = std::time::Instant::now();
        rt.block_on(wait_media_or_timeout(Duration::from_secs(3)));
        assert!(start.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn wait_media_wakes_on_note() {
        let _g = MEDIA_WAIT_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let _ = take_media_event();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let start = std::time::Instant::now();
        rt.block_on(async {
            let waiter = wait_media_or_timeout(Duration::from_secs(3));
            let poker = async {
                tokio::time::sleep(Duration::from_millis(15)).await;
                note_media_event();
            };
            tokio::join!(waiter, poker);
        });
        assert!(start.elapsed() < Duration::from_millis(500));
    }
}
