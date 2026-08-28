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

use serde::Deserialize;
use serde_json::{Map, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

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

fn adapter_supports_stream() -> bool {
    matches!(backend(), Some(Backend::Adapter { .. }))
}

struct StreamState {
    merged: Value,
    track: Option<AdapterTrack>,
    elapsed_base: Option<f64>,
    elapsed_at: Instant,
    playing: bool,
    primed: bool,
}

impl StreamState {
    fn fresh() -> Self {
        Self {
            merged: Value::Object(Map::new()),
            track: None,
            elapsed_base: None,
            elapsed_at: Instant::now(),
            playing: false,
            primed: false,
        }
    }
}

fn stream_state() -> &'static Mutex<StreamState> {
    static STATE: OnceLock<Mutex<StreamState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(StreamState::fresh()))
}

static STREAM_CHANGED: AtomicBool = AtomicBool::new(false);
static STREAM_ALIVE: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
struct StreamEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    diff: bool,
    #[serde(default)]
    payload: Value,
}

/// Spawn the long-lived adapter `stream --diff` child (once). No-op for the
/// debug `media-control` backend and when the adapter is missing.
pub fn ensure_stream() {
    if !adapter_supports_stream() {
        return;
    }
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("nook-mr-stream".into())
            .spawn(stream_supervisor);
    });
}

/// Latest snapshot from the live stream, with `elapsed_time` interpolated
/// from the last event while playing. `None` if the stream has not produced
/// a snapshot yet (caller should one-shot `get --now`).
pub fn latest_now_playing() -> Option<Option<AdapterTrack>> {
    let Ok(state) = stream_state().lock() else {
        return None;
    };
    if !state.primed {
        return None;
    }
    Some(interpolated_track(&state))
}

pub fn take_stream_changed() -> bool {
    STREAM_CHANGED.swap(false, Ordering::Relaxed)
}

pub fn stream_is_live() -> bool {
    STREAM_ALIVE.load(Ordering::Relaxed)
}

fn interpolated_track(state: &StreamState) -> Option<AdapterTrack> {
    let mut track = state.track.clone()?;
    if state.playing {
        if let Some(base) = state.elapsed_base.or(track.elapsed_time) {
            let mut elapsed = base + state.elapsed_at.elapsed().as_secs_f64();
            if let Some(duration) = track.duration {
                elapsed = elapsed.min(duration.max(0.0));
            }
            track.elapsed_time = Some(elapsed);
        }
    }
    Some(track)
}

fn merge_diff(target: &mut Value, diff: Value) {
    let Value::Object(diff_map) = diff else {
        if !diff.is_null() {
            *target = diff;
        }
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    let Some(map) = target.as_object_mut() else {
        return;
    };
    for (k, v) in diff_map {
        if v.is_null() {
            map.remove(&k);
        } else {
            map.insert(k, v);
        }
    }
}

fn apply_payload(diff: bool, payload: Value) {
    let mut state = stream_state().lock().unwrap_or_else(|e| e.into_inner());
    if diff {
        merge_diff(&mut state.merged, payload);
    } else {
        state.merged = match payload {
            Value::Object(map) => Value::Object(map),
            Value::Null => Value::Object(Map::new()),
            other => other,
        };
    }
    let json = serde_json::to_string(&state.merged).unwrap_or_else(|_| "{}".into());
    let track = parse_get_output(&json).ok().flatten();
    state.playing = track.as_ref().map(|t| t.is_playing).unwrap_or(false);
    state.elapsed_base = track.as_ref().and_then(|t| t.elapsed_time);
    state.elapsed_at = Instant::now();
    state.track = track;
    state.primed = true;
    STREAM_CHANGED.store(true, Ordering::Relaxed);
    drop(state);
    crate::audio::note_media_event();
}

fn apply_stream_event(line: &str) -> bool {
    let ev: StreamEvent = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(err) => {
            log::debug!("MediaRemote stream JSON: {err}");
            return false;
        }
    };
    if ev.kind != "data" {
        return false;
    }
    apply_payload(ev.diff, ev.payload);
    true
}

fn apply_primer_stdout(stdout: &str) {
    let trimmed = stdout.trim();
    let payload = if trimmed.is_empty() || trimmed == "null" {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(trimmed).unwrap_or_else(|_| Value::Object(Map::new()))
    };
    apply_payload(false, payload);
}

fn stream_supervisor() {
    if let Ok(out) = run(&["get", "--now"]) {
        apply_primer_stdout(&out);
    }
    let mut backoff = Duration::from_millis(200);
    loop {
        match spawn_stream_child() {
            Ok(mut child) => {
                STREAM_ALIVE.store(true, Ordering::Relaxed);
                backoff = Duration::from_millis(200);
                let _ = read_stream(&mut child);
                STREAM_ALIVE.store(false, Ordering::Relaxed);
                let _ = child.kill();
                let _ = child.wait();
            }
            Err(err) => {
                log::debug!("MediaRemote stream spawn failed: {err}");
            }
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(8));
    }
}

fn spawn_stream_child() -> Result<std::process::Child, String> {
    let Some(Backend::Adapter {
        perl,
        script,
        framework,
    }) = backend()
    else {
        return Err("MediaRemote stream needs the adapter backend".into());
    };
    Command::new(perl)
        .arg(script)
        .arg(framework)
        .arg("stream")
        .arg("--diff")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn MediaRemote stream: {e}"))
}

fn read_stream(child: &mut std::process::Child) -> bool {
    let Some(stdout) = child.stdout.take() else {
        return false;
    };
    let pid = child.id();
    let first = std::sync::Arc::new(AtomicBool::new(false));
    let first_flag = first.clone();
    let _ = std::thread::Builder::new()
        .name("nook-mr-watchdog".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            if !first_flag.load(Ordering::Relaxed) {
                let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
            }
        });
    let reader = BufReader::new(stdout);
    let mut saw_line = false;
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        if !saw_line {
            saw_line = true;
            first.store(true, Ordering::Relaxed);
        }
        apply_stream_event(&line);
    }
    saw_line
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
/// Prefers the live `stream` cell when the supervisor has primed it so a
/// poll never forks perl. One-shot `get --now` remains the startup primer
/// and the debug `media-control` path.
pub fn get_now_playing() -> Result<Option<AdapterTrack>, String> {
    if let Some(cached) = latest_now_playing() {
        return Ok(cached);
    }
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

    static STREAM_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset_stream() {
        let mut state = stream_state().lock().unwrap_or_else(|e| e.into_inner());
        *state = StreamState::fresh();
        STREAM_CHANGED.store(false, Ordering::Relaxed);
    }

    #[test]
    fn stream_full_replace_then_diff_merge() {
        let _guard = STREAM_TEST.lock().unwrap_or_else(|e| e.into_inner());
        reset_stream();
        assert!(apply_stream_event(
            r#"{"type":"data","diff":false,"payload":{
                "title":"Helpless","artist":"The Staves","playing":true,
                "duration":214.2,"elapsedTime":12,
                "bundleIdentifier":"com.spotify.client"
            }}"#
        ));
        let track = latest_now_playing().unwrap().unwrap();
        assert_eq!(track.title.as_deref(), Some("Helpless"));
        let elapsed = track.elapsed_time.unwrap();
        assert!(elapsed >= 12.0 && elapsed < 12.2, "elapsed={elapsed}");
        assert!(track.is_playing);

        assert!(apply_stream_event(
            r#"{"type":"data","diff":true,"payload":{"elapsedTime":40,"playing":false}}"#
        ));
        let track = latest_now_playing().unwrap().unwrap();
        assert_eq!(track.title.as_deref(), Some("Helpless"));
        assert_eq!(track.elapsed_time, Some(40.0));
        assert!(!track.is_playing);
        assert!(take_stream_changed());
    }

    #[test]
    fn stream_empty_payload_is_idle() {
        let _guard = STREAM_TEST.lock().unwrap_or_else(|e| e.into_inner());
        reset_stream();
        apply_stream_event(
            r#"{"type":"data","diff":false,"payload":{"title":"X","playing":true,"bundleIdentifier":"com.spotify.client"}}"#,
        );
        apply_stream_event(r#"{"type":"data","diff":false,"payload":{}}"#);
        assert!(latest_now_playing().unwrap().is_none());
    }

    #[test]
    fn stream_diff_null_removes_key() {
        let _guard = STREAM_TEST.lock().unwrap_or_else(|e| e.into_inner());
        reset_stream();
        apply_stream_event(
            r#"{"type":"data","diff":false,"payload":{"title":"A","artist":"B","playing":true,"bundleIdentifier":"com.spotify.client"}}"#,
        );
        apply_stream_event(r#"{"type":"data","diff":true,"payload":{"title":null}}"#);
        assert!(latest_now_playing().unwrap().is_none());
    }

    #[test]
    fn elapsed_interpolates_while_playing() {
        let _guard = STREAM_TEST.lock().unwrap_or_else(|e| e.into_inner());
        reset_stream();
        apply_stream_event(
            r#"{"type":"data","diff":false,"payload":{"title":"A","playing":true,"elapsedTime":10,"duration":100,"bundleIdentifier":"com.spotify.client"}}"#,
        );
        std::thread::sleep(Duration::from_millis(40));
        let elapsed = latest_now_playing()
            .unwrap()
            .unwrap()
            .elapsed_time
            .unwrap();
        assert!(elapsed >= 10.03, "elapsed={elapsed}");
        assert!(elapsed < 11.0, "elapsed={elapsed}");
    }

    #[test]
    fn elapsed_does_not_advance_when_paused() {
        let _guard = STREAM_TEST.lock().unwrap_or_else(|e| e.into_inner());
        reset_stream();
        apply_stream_event(
            r#"{"type":"data","diff":false,"payload":{"title":"A","playing":false,"elapsedTime":10,"duration":100,"bundleIdentifier":"com.spotify.client"}}"#,
        );
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(
            latest_now_playing().unwrap().unwrap().elapsed_time,
            Some(10.0)
        );
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
