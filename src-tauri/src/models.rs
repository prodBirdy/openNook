use serde::Serialize;

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
    /// Name of the app playing the media (Spotify, Music, Safari)
    pub app_name: Option<String>,
}
