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
    /// Apple Music editorialVideo HLS loop, when the catalog has one.
    #[serde(default)]
    pub motion_artwork_url: Option<String>,
}

/// One timed line from an LRC file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

/// Lyrics for the current track. `lines` is empty when only plain text (or
/// an instrumental) is available. Text is fetched at runtime and never bundled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SyncedLyrics {
    #[serde(default)]
    pub lines: Vec<LyricLine>,
    #[serde(default)]
    pub plain: Option<String>,
    #[serde(default)]
    pub instrumental: bool,
    #[serde(default)]
    pub source: String,
}
