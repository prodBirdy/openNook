//! Playing Next / Up Next snapshot. Kept off the hot now-playing struct.
//!
//! Music.app: AppleScript `current playlist` window after the current index.
//! This is **not** Music's real Playing Next queue — hide it under shuffle
//! or radio/autoplay. Spotify: Web API via [`crate::spotify`].

use crate::models::{
    NowPlayingData, PlaybackQueue, QueueHidden, QueueItem, QueueJump, QueueSource,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const MUSIC_WINDOW: usize = 10;
static MUSIC_TCC_DENIED: AtomicBool = AtomicBool::new(false);
static ART_READY: AtomicBool = AtomicBool::new(false);
static ART_CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
static ART_INFLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

pub fn music_automation_denied() -> bool {
    MUSIC_TCC_DENIED.load(Ordering::Relaxed)
}

pub fn take_artwork_ready() -> bool {
    ART_READY.swap(false, Ordering::Relaxed)
}

fn art_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn art_inflight() -> &'static Mutex<HashSet<String>> {
    ART_INFLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn cached_artwork(id: &str) -> Option<Option<String>> {
    art_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .cloned()
}

/// Lazy 64px Spotify art. Completes by flipping [`take_artwork_ready`]; no timer.
pub fn request_artwork(id: String, url: String) {
    if url.is_empty() || id.is_empty() {
        return;
    }
    {
        let cache = art_cache().lock().unwrap_or_else(|e| e.into_inner());
        if cache.contains_key(&id) {
            return;
        }
    }
    {
        let mut inflight = art_inflight().lock().unwrap_or_else(|e| e.into_inner());
        if !inflight.insert(id.clone()) {
            return;
        }
    }
    crate::runtime().spawn(async move {
        let art = crate::utils::fetch_artwork_from_url(&url).await;
        art_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, art);
        ART_READY.store(true, Ordering::Relaxed);
        crate::audio::note_media_event();
    });
}

pub fn is_music_app(app_name: Option<&str>, bundle_id: Option<&str>) -> bool {
    let bundle = bundle_id.unwrap_or("");
    let name = app_name.unwrap_or("");
    bundle.eq_ignore_ascii_case("com.apple.Music")
        || name.eq_ignore_ascii_case("Music")
        || name.eq_ignore_ascii_case("iTunes")
}

pub fn is_spotify_app(app_name: Option<&str>, bundle_id: Option<&str>) -> bool {
    let bundle = bundle_id.unwrap_or("");
    let name = app_name.unwrap_or("");
    bundle.eq_ignore_ascii_case("com.spotify.client") || name.eq_ignore_ascii_case("Spotify")
}

pub fn queue_identity(np: &NowPlayingData) -> (Option<String>, Option<String>, Option<String>) {
    (np.title.clone(), np.artist.clone(), np.app_name.clone())
}

/// Upcoming Music tracks after `current_index` (1-based). Empty when shuffle
/// or radio should hide the section.
pub fn music_window(
    current_index: u32,
    tracks: &[(u32, String, String)],
    shuffle: bool,
    radio: bool,
    limit: usize,
) -> Result<Vec<QueueItem>, QueueHidden> {
    if shuffle {
        return Err(QueueHidden::Shuffle);
    }
    if radio {
        return Err(QueueHidden::Radio);
    }
    if current_index == 0 {
        return Err(QueueHidden::Idle);
    }
    let items = tracks
        .iter()
        .filter(|(index, _, _)| *index > current_index)
        .take(limit)
        .map(|(index, title, artist)| QueueItem {
            id: format!("music-{index}"),
            title: title.clone(),
            artist: artist.clone(),
            artwork_url: None,
            artwork_base64: None,
            source: QueueSource::MusicPlaylist,
            jump: QueueJump::MusicTrack { index: *index },
        })
        .collect();
    Ok(items)
}

/// Parse the AppleScript snapshot format:
/// first line `ok|idx|total` / `hide|shuffle` / `hide|radio` / `denied|-1743` / `idle`
/// then `index\ttitle\tartist` rows.
pub fn parse_music_snapshot(stdout: &str) -> PlaybackQueue {
    let mut lines = stdout.lines();
    let header = lines.next().unwrap_or("").trim();
    if header.is_empty() || header == "idle" {
        return hidden(QueueHidden::Idle);
    }
    if header.starts_with("denied") {
        MUSIC_TCC_DENIED.store(true, Ordering::Relaxed);
        return hidden(QueueHidden::AutomationDenied);
    }
    if let Some(rest) = header.strip_prefix("hide|") {
        let reason = match rest {
            "shuffle" => QueueHidden::Shuffle,
            "radio" => QueueHidden::Radio,
            _ => QueueHidden::Idle,
        };
        return hidden(reason);
    }
    if !header.starts_with("ok|") {
        return PlaybackQueue::default();
    }
    let mut parts = header.split('|');
    let _ = parts.next();
    let current_index = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let tracks = lines
        .filter_map(|line| {
            let mut cols = line.splitn(3, '\t');
            let index = cols.next()?.parse().ok()?;
            let title = cols.next()?.to_string();
            let artist = cols.next().unwrap_or("").to_string();
            Some((index, title, artist))
        })
        .collect::<Vec<_>>();
    match music_window(current_index, &tracks, false, false, MUSIC_WINDOW) {
        Ok(items) => {
            MUSIC_TCC_DENIED.store(false, Ordering::Relaxed);
            PlaybackQueue {
                source: Some(QueueSource::MusicPlaylist),
                label: "Up Next in playlist".into(),
                items,
                hidden: None,
                context_uri: None,
            }
        }
        Err(hidden_reason) => hidden(hidden_reason),
    }
}

fn hidden(reason: QueueHidden) -> PlaybackQueue {
    PlaybackQueue {
        source: Some(QueueSource::MusicPlaylist),
        label: "Up Next in playlist".into(),
        items: Vec::new(),
        hidden: Some(reason),
        context_uri: None,
    }
}

#[cfg(target_os = "macos")]
const MUSIC_QUEUE_SCRIPT: &str = r#"
tell application "Music"
    try
        if player state is stopped then return "idle"
        if shuffle enabled then return "hide|shuffle"
        set trackClass to (class of current track as text)
        if trackClass contains "URL" or trackClass contains "radio" then return "hide|radio"
        set plClass to (class of current playlist as text)
        if plClass contains "radio" then return "hide|radio"
        set idx to index of current track
        set total to count of tracks of current playlist
        set firstIdx to idx + 1
        set lastIdx to idx + 10
        if lastIdx > total then set lastIdx to total
        set out to "ok|" & idx & "|" & total
        if firstIdx > total then return out
        repeat with i from firstIdx to lastIdx
            set t to track i of current playlist
            set tName to name of t
            set tArtist to ""
            try
                set tArtist to artist of t
            end try
            set out to out & return & i & tab & tName & tab & tArtist
        end repeat
        return out
    on error errMsg number errNum
        if errNum is -1743 then return "denied|-1743"
        return "error|" & errNum & "|" & errMsg
    end try
end tell
"#;

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<String, String> {
    use std::process::Command;
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| e.to_string())?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("-1743") || stderr.to_lowercase().contains("not allowed") {
        MUSIC_TCC_DENIED.store(true, Ordering::Relaxed);
        return Err("automation denied".into());
    }
    if !output.status.success() {
        return Err(format!("osascript failed: {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn fetch_music_queue() -> PlaybackQueue {
    #[cfg(target_os = "macos")]
    {
        match run_osascript(MUSIC_QUEUE_SCRIPT) {
            Ok(stdout) => parse_music_snapshot(&stdout),
            Err(_) if music_automation_denied() => hidden(QueueHidden::AutomationDenied),
            Err(err) => {
                log::debug!("music queue: {err}");
                PlaybackQueue::default()
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        PlaybackQueue::default()
    }
}

pub async fn fetch_playback_queue(np: &NowPlayingData) -> PlaybackQueue {
    if is_spotify_app(np.app_name.as_deref(), np.bundle_id.as_deref()) {
        match crate::spotify::fetch_queue().await {
            Ok(queue) => queue,
            Err(err) => {
                log::debug!("spotify queue: {err}");
                PlaybackQueue {
                    source: Some(QueueSource::Spotify),
                    label: "Playing Next".into(),
                    ..PlaybackQueue::default()
                }
            }
        }
    } else if is_music_app(np.app_name.as_deref(), np.bundle_id.as_deref()) {
        fetch_music_queue().await
    } else {
        PlaybackQueue::default()
    }
}

pub async fn jump_to_item(item: &QueueItem, context_uri: Option<&str>) -> Result<(), String> {
    match &item.jump {
        QueueJump::MusicTrack { index } => jump_music(*index),
        QueueJump::Spotify { skip_count, uri } => {
            crate::spotify::jump_to(*skip_count, uri, context_uri).await
        }
    }
}

fn jump_music(index: u32) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            r#"tell application "Music" to play track {index} of current playlist"#
        );
        run_osascript(&script).map(|_| ())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = index;
        Err("Music queue jump is only available on macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windowing_skips_current_and_caps() {
        let tracks = (1u32..=20)
            .map(|i| (i, format!("t{i}"), format!("a{i}")))
            .collect::<Vec<_>>();
        let items = music_window(7, &tracks, false, false, 10).unwrap();
        assert_eq!(items.len(), 10);
        assert_eq!(items[0].title, "t8");
        assert_eq!(
            items[0].jump,
            QueueJump::MusicTrack { index: 8 }
        );
        assert_eq!(items[9].title, "t17");
    }

    #[test]
    fn shuffle_and_radio_hide() {
        let tracks = vec![(2, "x".into(), "y".into())];
        assert_eq!(
            music_window(1, &tracks, true, false, 10).unwrap_err(),
            QueueHidden::Shuffle
        );
        assert_eq!(
            music_window(1, &tracks, false, true, 10).unwrap_err(),
            QueueHidden::Radio
        );
    }

    #[test]
    fn snapshot_ok_and_denied() {
        let text = "ok|3|6\n4\tFour\tA\n5\tFive\tB\n";
        let queue = parse_music_snapshot(text);
        assert_eq!(queue.label, "Up Next in playlist");
        assert_eq!(queue.items.len(), 2);
        assert_eq!(queue.items[0].title, "Four");
        assert_eq!(queue.hidden, None);

        let denied = parse_music_snapshot("denied|-1743");
        assert_eq!(denied.hidden, Some(QueueHidden::AutomationDenied));
        assert!(music_automation_denied());

        let shuffle = parse_music_snapshot("hide|shuffle");
        assert_eq!(shuffle.hidden, Some(QueueHidden::Shuffle));
        assert!(shuffle.items.is_empty());
    }

    #[test]
    fn app_detection() {
        assert!(is_music_app(Some("Music"), None));
        assert!(is_music_app(None, Some("com.apple.Music")));
        assert!(is_spotify_app(Some("Spotify"), None));
        assert!(!is_spotify_app(Some("Safari"), Some("com.apple.Safari")));
    }
}
