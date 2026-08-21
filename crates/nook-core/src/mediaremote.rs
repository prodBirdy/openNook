//! Now Playing and media controls via [mediaremote-adapter].
//!
//! Direct MediaRemote use from a third-party app is broken since macOS 15.4.
//! The adapter loads a helper framework from `/usr/bin/perl` (`com.apple.perl`),
//! which is still entitled to call:
//! - `MRMediaRemoteGetNowPlayingInfo`
//! - `MRMediaRemoteSendCommand`
//!
//! Commands match `MRACommand` in MediaRemoteAdapter.h:
//! `kMRATogglePlayPause = 2`, `kMRANextTrack = 4`, `kMRAPreviousTrack = 5`.
//!
//! [mediaremote-adapter]: https://github.com/ungive/mediaremote-adapter

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// MediaRemote `MRCommand` / adapter `MRACommand` IDs.
#[derive(Debug, Clone, Copy)]
#[repr(i32)]
#[allow(dead_code)]
pub enum MraCommand {
    Play = 0,
    Pause = 1,
    TogglePlayPause = 2,
    Stop = 3,
    NextTrack = 4,
    PreviousTrack = 5,
    ToggleShuffle = 6,
    ToggleRepeat = 7,
    StartForwardSeek = 8,
    EndForwardSeek = 9,
    StartBackwardSeek = 10,
    EndBackwardSeek = 11,
    GoBackFifteenSeconds = 12,
    SkipFifteenSeconds = 13,
}

#[derive(Debug, Clone)]
pub struct AdapterTrack {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_base64: Option<String>,
    pub duration: Option<f64>,
    pub elapsed_time: Option<f64>,
    pub is_playing: bool,
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
}

enum Backend {
    /// `/usr/bin/perl mediaremote-adapter.pl FRAMEWORK COMMAND`
    Adapter {
        perl: PathBuf,
        script: PathBuf,
        framework: PathBuf,
    },
    /// Fixed Homebrew `media-control` path for debug builds.
    #[cfg(debug_assertions)]
    MediaControl { bin: PathBuf },
}

static BACKEND: OnceLock<Option<Backend>> = OnceLock::new();

fn backend() -> Option<&'static Backend> {
    BACKEND.get_or_init(discover).as_ref()
}

pub fn is_available() -> bool {
    backend().is_some()
}

fn discover() -> Option<Backend> {
    if let Some(backend) = from_bundle_resources() {
        log::info!("MediaRemote adapter from app bundle");
        return Some(backend);
    }
    development_backend()
}

#[cfg(debug_assertions)]
fn development_backend() -> Option<Backend> {
    if let Some(backend) = from_env() {
        log::info!("MediaRemote adapter from environment (debug build)");
        return Some(backend);
    }
    if let Some(backend) = from_third_party() {
        log::info!("MediaRemote adapter from workspace (debug build)");
        return Some(backend);
    }
    if let Some(bin) = find_media_control() {
        log::info!("MediaRemote debug fallback at {}", bin.display());
        return Some(Backend::MediaControl { bin });
    }
    log::info!("MediaRemote adapter not found; AppleScript fallback will be used");
    None
}

#[cfg(not(debug_assertions))]
fn development_backend() -> Option<Backend> {
    log::info!("Bundled MediaRemote adapter not found; AppleScript fallback will be used");
    None
}

#[cfg(debug_assertions)]
fn from_env() -> Option<Backend> {
    let script = std::env::var_os("MEDIAREMOTE_ADAPTER_SCRIPT").map(PathBuf::from)?;
    let framework = std::env::var_os("MEDIAREMOTE_ADAPTER_FRAMEWORK").map(PathBuf::from)?;
    adapter_backend(script, framework)
}

fn from_bundle_resources() -> Option<Backend> {
    let exe = std::env::current_exe().ok()?;
    let macos_dir = exe.parent()?;
    let contents = macos_dir.parent()?;
    let resources = contents.join("Resources");
    adapter_backend(
        resources.join("mediaremote-adapter.pl"),
        resources.join("MediaRemoteAdapter.framework"),
    )
}

#[cfg(debug_assertions)]
fn from_third_party() -> Option<Backend> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let candidate = workspace.join("third_party/mediaremote-adapter");
    adapter_backend(
        candidate.join("bin/mediaremote-adapter.pl"),
        candidate.join("build/MediaRemoteAdapter.framework"),
    )
}

fn adapter_backend(script: PathBuf, framework: PathBuf) -> Option<Backend> {
    let script = abs_if_exists(&script)?;
    let framework = abs_if_exists(&framework)?;
    let binary_name = framework
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if !framework.join(&binary_name).is_file() {
        return None;
    }
    let perl = PathBuf::from("/usr/bin/perl");
    if !perl.is_file() {
        return None;
    }
    Some(Backend::Adapter {
        perl,
        script,
        framework,
    })
}

fn abs_if_exists(path: &Path) -> Option<PathBuf> {
    if path.exists() {
        path.canonicalize()
            .ok()
            .or_else(|| Some(path.to_path_buf()))
    } else {
        None
    }
}

#[cfg(debug_assertions)]
fn find_media_control() -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        "/opt/homebrew/bin/media-control",
        "/usr/local/bin/media-control",
    ];
    for path in CANDIDATES {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn run(args: &[&str]) -> Result<String, String> {
    let backend = backend().ok_or_else(|| "MediaRemote adapter not available".to_string())?;
    let mut cmd = match backend {
        Backend::Adapter {
            perl,
            script,
            framework,
        } => {
            let mut cmd = Command::new(perl);
            cmd.arg(script).arg(framework);
            cmd
        }
        #[cfg(debug_assertions)]
        Backend::MediaControl { bin } => Command::new(bin),
    };
    cmd.args(args);
    let output = cmd
        .output()
        .map_err(|e| format!("failed to run MediaRemote adapter: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        if detail.is_empty() {
            return Err(format!("MediaRemote adapter exited {}", output.status));
        }
        return Err(detail.to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// `adapter_get` / `get --now`. `Ok(None)` means no now-playing item (`null`).
pub fn get_now_playing() -> Result<Option<AdapterTrack>, String> {
    let stdout = run(&["get", "--now"])?;
    parse_get_output(&stdout)
}

/// `adapter_seek`. `position` is seconds; the adapter takes microseconds.
pub fn seek_seconds(position: f64) -> Result<(), String> {
    if !position.is_finite() || position < 0.0 {
        return Err("seek position must be a positive number".into());
    }
    match backend() {
        #[cfg(debug_assertions)]
        Some(Backend::MediaControl { .. }) => {
            let _ = run(&["seek", &format!("{position:.3}")])?;
        }
        Some(Backend::Adapter { .. }) => {
            let micros = (position * 1_000_000.0).round() as i64;
            let _ = run(&["seek", &format!("{micros}")])?;
        }
        None => return Err("MediaRemote adapter not available".into()),
    }
    Ok(())
}

/// `adapter_send` with an `MRACommand` id.
pub fn send(command: MraCommand) -> Result<(), String> {
    match backend() {
        #[cfg(debug_assertions)]
        Some(Backend::MediaControl { .. }) => {
            let _ = run(&[media_control_name(command)])?;
        }
        Some(Backend::Adapter { .. }) => {
            let _ = run(&["send", &format!("{}", command as i32)])?;
        }
        None => return Err("MediaRemote adapter not available".into()),
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn media_control_name(command: MraCommand) -> &'static str {
    match command {
        MraCommand::Play => "play",
        MraCommand::Pause => "pause",
        MraCommand::TogglePlayPause => "toggle-play-pause",
        MraCommand::Stop => "stop",
        MraCommand::NextTrack => "next-track",
        MraCommand::PreviousTrack => "previous-track",
        MraCommand::ToggleShuffle => "toggle-shuffle",
        MraCommand::ToggleRepeat => "toggle-repeat",
        MraCommand::StartForwardSeek => "start-forward-seek",
        MraCommand::EndForwardSeek => "end-forward-seek",
        MraCommand::StartBackwardSeek => "start-backward-seek",
        MraCommand::EndBackwardSeek => "end-backward-seek",
        MraCommand::GoBackFifteenSeconds => "go-back-fifteen-seconds",
        MraCommand::SkipFifteenSeconds => "skip-fifteen-seconds",
    }
}

fn parse_get_output(stdout: &str) -> Result<Option<AdapterTrack>, String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(None);
    }
    let value: Value =
        serde_json::from_str(trimmed).map_err(|e| format!("invalid MediaRemote JSON: {e}"))?;
    if value.is_null() {
        return Ok(None);
    }
    let obj = value
        .as_object()
        .ok_or_else(|| "MediaRemote get did not return an object".to_string())?;
    let title = json_string(obj.get("title"));
    if title.as_ref().is_none_or(|t| t.is_empty()) {
        return Ok(None);
    }
    let bundle = json_string(obj.get("bundleIdentifier"));
    let parent = json_string(obj.get("parentApplicationBundleIdentifier"));
    let bundle_id = parent
        .clone()
        .filter(|p| !p.is_empty())
        .or_else(|| bundle.clone());
    Ok(Some(AdapterTrack {
        title,
        artist: json_string(obj.get("artist")),
        album: json_string(obj.get("album")),
        artwork_base64: json_string(obj.get("artworkData")),
        duration: json_f64(obj.get("duration")),
        elapsed_time: json_f64(obj.get("elapsedTimeNow"))
            .or_else(|| json_f64(obj.get("elapsedTime"))),
        is_playing: json_bool(obj.get("playing")).unwrap_or(false),
        app_name: app_name_from_bundle(bundle.as_deref(), parent.as_deref()),
        bundle_id,
    }))
}

fn json_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Null | Value::String(_) => None,
        other => {
            let s = other.to_string();
            if s.is_empty() || s == "null" {
                None
            } else {
                Some(s.trim_matches('"').to_string())
            }
        }
    }
}

fn json_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.replace(',', ".").parse().ok(),
        _ => None,
    }
}

fn json_bool(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => Some(n.as_i64().unwrap_or(0) != 0),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn app_name_from_bundle(bundle: Option<&str>, parent: Option<&str>) -> Option<String> {
    let id = parent.filter(|p| !p.is_empty()).or(bundle)?;
    Some(
        match id {
            "com.spotify.client" => "Spotify",
            "com.apple.Music" | "com.apple.iTunes" => "Music",
            "com.apple.Safari"
            | "com.apple.Safari.WebApp"
            | "com.apple.WebKit.GPU"
            | "com.apple.WebKit.Networking" => "Safari",
            "com.google.Chrome" | "com.google.Chrome.canary" => "Chrome",
            "com.brave.Browser" => "Brave",
            "company.thebrowser.Browser" | "com.operasoftware.Opera" => "Browser",
            "com.microsoft.edgemac" => "Edge",
            "org.mozilla.firefox" => "Firefox",
            "com.apple.TV" => "TV",
            "com.colliderli.iina" => "IINA",
            "org.videolan.vlc" => "VLC",
            "com.apple.Music.MacAppStore" => "Music",
            "com.tidal.desktop" => "TIDAL",
            "com.apple.podcasts" => "Podcasts",
            other => return Some(humanize_bundle(other)),
        }
        .to_string(),
    )
}

fn humanize_bundle(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id);
    if last.is_empty() {
        return id.to_string();
    }
    let mut chars = last.chars();
    match chars.next() {
        Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
        None => last.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_null() {
        assert!(parse_get_output("null").unwrap().is_none());
        assert!(parse_get_output("").unwrap().is_none());
    }

    #[test]
    fn parse_track() {
        let json = r#"{
            "bundleIdentifier": "com.spotify.client",
            "playing": true,
            "title": "Helpless",
            "artist": "The Staves",
            "album": "Dead & Born & Grown",
            "duration": 214.2,
            "elapsedTime": 12,
            "elapsedTimeNow": 13.5,
            "artworkData": "abc"
        }"#;
        let track = parse_get_output(json).unwrap().unwrap();
        assert_eq!(track.title.as_deref(), Some("Helpless"));
        assert_eq!(track.artist.as_deref(), Some("The Staves"));
        assert_eq!(track.album.as_deref(), Some("Dead & Born & Grown"));
        assert_eq!(track.duration, Some(214.2));
        assert_eq!(track.elapsed_time, Some(13.5));
        assert!(track.is_playing);
        assert_eq!(track.app_name.as_deref(), Some("Spotify"));
        assert_eq!(track.bundle_id.as_deref(), Some("com.spotify.client"));
        assert_eq!(track.artwork_base64.as_deref(), Some("abc"));
    }

    #[test]
    fn parse_missing_title_is_idle() {
        let json = r#"{"playing": true, "bundleIdentifier": "com.spotify.client"}"#;
        assert!(parse_get_output(json).unwrap().is_none());
    }

    #[test]
    fn parent_bundle_wins_for_webkit() {
        assert_eq!(
            app_name_from_bundle(Some("com.apple.WebKit.GPU"), Some("com.apple.Safari")).as_deref(),
            Some("Safari")
        );
    }

    #[test]
    fn command_ids_match_adapter() {
        assert_eq!(MraCommand::TogglePlayPause as i32, 2);
        assert_eq!(MraCommand::NextTrack as i32, 4);
        assert_eq!(MraCommand::PreviousTrack as i32, 5);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn release_build_has_no_ambient_backend_fallback() {
        assert!(development_backend().is_none());
    }

    #[test]
    fn adapter_get_succeeds_when_framework_is_present() {
        let compiled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../third_party/mediaremote-adapter/build/MediaRemoteAdapter.framework");
        if !compiled.exists() && !is_available() {
            return;
        }
        assert!(
            is_available(),
            "MediaRemoteAdapter.framework is present but was not discovered"
        );
        get_now_playing().expect("adapter get --now should run");
    }
}
