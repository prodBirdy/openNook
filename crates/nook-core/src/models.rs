use serde::{Deserialize, Serialize};

/// Notch and screen information returned to the frontend
#[derive(Debug, Serialize, Deserialize, Clone)]
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
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
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
    /// Name of the app playing the media (Spotify, Music, Safari)
    pub app_name: Option<String>,
    /// Bundle identifier of that app, used to load its icon.
    #[serde(default)]
    pub bundle_id: Option<String>,
}

/// Where an Up Next row came from. Kept off [`NowPlayingData`] so the hot
/// now-playing path stays a single track snapshot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueSource {
    MusicPlaylist,
    Spotify,
}

/// Why the island hides the upcoming list. Music's real Playing Next queue
/// is unreadable; shuffle/radio is the honest fallback.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueHidden {
    Shuffle,
    Radio,
    Idle,
    NeedsSpotifyAuth,
    PremiumRequired,
    AutomationDenied,
}

/// Handle used by [`crate::audio::media_jump_to_queue_item`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum QueueJump {
    /// 1-based `play track i of current playlist` in Music.app.
    MusicTrack { index: u32 },
    /// Sequential `POST /v1/me/player/next` count, plus the track uri so we
    /// can try `PUT /v1/me/player/play` with context+offset first.
    Spotify {
        skip_count: u32,
        uri: String,
    },
}

/// One upcoming row. Artwork stays optional: Spotify uses a 64px URL (lazy),
/// Music rows may ship without art.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueueItem {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artwork_url: Option<String>,
    pub artwork_base64: Option<String>,
    pub source: QueueSource,
    pub jump: QueueJump,
}

/// Separate fetch from now-playing. Empty `items` + no `hidden` means "nothing
/// to show", not an error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PlaybackQueue {
    pub source: Option<QueueSource>,
    pub label: String,
    pub items: Vec<QueueItem>,
    pub hidden: Option<QueueHidden>,
    pub context_uri: Option<String>,
}
