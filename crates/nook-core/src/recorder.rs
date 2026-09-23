//! Voice memos: AVAudioEngine capture + on-device SFSpeechRecognizer.
//!
//! Idle cost is zero. The engine, tap, and recognition task exist only between
//! [`start`] and [`stop`]. The island tick reads [`is_live`] (one atomic) and
//! only then a snapshot.

use crate::app_data_dir;
use crate::database::{get_connection, log_sql};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TRANSCRIPT_STITCH_SECS: u64 = 55;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordingItem {
    pub id: i64,
    pub path: String,
    pub created_at: i64,
    pub duration_ms: i64,
    pub transcript: String,
}

/// Live AVAudioPlayer readout. `playing` is false while paused or at end.
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    pub id: i64,
    pub position_s: f64,
    pub duration_s: f64,
    pub playing: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LiveSnapshot {
    pub recording: bool,
    pub started: Option<Instant>,
    pub elapsed_ms: u64,
    pub transcript: String,
    pub level: f32,
    pub transcribing: bool,
    pub error: Option<String>,
    pub playing_id: Option<i64>,
}

static LIVE: AtomicBool = AtomicBool::new(false);
static LEVEL_BITS: AtomicU32 = AtomicU32::new(0);
static PLAYING_ID: AtomicU64 = AtomicU64::new(0);
static STARTED_MS: AtomicU64 = AtomicU64::new(0);
static TRANSCRIBING: AtomicBool = AtomicBool::new(false);

struct Shared {
    transcript: String,
    finalized: String,
    error: Option<String>,
    started: Option<Instant>,
    segment_started: Option<Instant>,
    transcribe: bool,
    path: Option<PathBuf>,
}

fn shared() -> &'static Mutex<Shared> {
    static SLOT: OnceLock<Mutex<Shared>> = OnceLock::new();
    SLOT.get_or_init(|| {
        Mutex::new(Shared {
            transcript: String::new(),
            finalized: String::new(),
            error: None,
            started: None,
            segment_started: None,
            transcribe: true,
            path: None,
        })
    })
}

pub fn recordings_dir() -> PathBuf {
    let dir = app_data_dir().join("recordings");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Compact `1:05` / `1:05:00` clock from milliseconds.
pub fn format_duration_ms(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn format_elapsed_s(secs: f64) -> String {
    format_duration_ms((secs.max(0.0) * 1000.0).round() as i64)
}

pub fn format_remaining_s(elapsed: f64, duration: f64) -> String {
    format!("-{}", format_elapsed_s(duration - elapsed))
}

pub fn clamp_position(position_s: f64, duration_s: f64) -> f64 {
    if !duration_s.is_finite() || duration_s <= 0.0 {
        return 0.0;
    }
    if !position_s.is_finite() {
        return 0.0;
    }
    position_s.clamp(0.0, duration_s)
}

pub fn clamp_fraction(fraction: f64) -> f64 {
    if !fraction.is_finite() {
        0.0
    } else {
        fraction.clamp(0.0, 1.0)
    }
}

/// True when the player finished the file (paused mid-file is not finished).
pub fn playback_finished(state: &Playback) -> bool {
    !state.playing && state.duration_s > 0.0 && state.position_s + 0.05 >= state.duration_s
}

/// Stitch a finalized 60 s chunk onto the next partial. Overlap (the new
/// segment repeating the previous text) is collapsed.
pub fn stitch_transcripts(finalized: &str, next: &str) -> String {
    let finalized = finalized.trim();
    let next = next.trim();
    if finalized.is_empty() {
        return next.to_string();
    }
    if next.is_empty() {
        return finalized.to_string();
    }
    if next.starts_with(finalized) {
        return next.to_string();
    }
    if finalized.ends_with(next) {
        return finalized.to_string();
    }
    format!("{finalized} {next}")
}

pub fn is_live() -> bool {
    LIVE.load(Ordering::Relaxed)
}

pub fn snapshot() -> LiveSnapshot {
    let recording = is_live();
    let started = shared().lock().ok().and_then(|g| g.started);
    let elapsed_ms = started
        .map(|t| t.elapsed().as_millis() as u64)
        .unwrap_or_else(|| STARTED_MS.load(Ordering::Relaxed));
    let (transcript, error, transcribe) = shared()
        .lock()
        .map(|g| (g.transcript.clone(), g.error.clone(), g.transcribe))
        .unwrap_or_default();
    let playing = PLAYING_ID.load(Ordering::Relaxed);
    LiveSnapshot {
        recording,
        started,
        elapsed_ms,
        transcript,
        level: f32::from_bits(LEVEL_BITS.load(Ordering::Relaxed)),
        transcribing: recording && transcribe && TRANSCRIBING.load(Ordering::Relaxed),
        error,
        playing_id: if playing == 0 {
            None
        } else {
            Some(playing as i64)
        },
    }
}

pub fn list() -> Vec<RecordingItem> {
    let Ok(conn) = get_connection() else {
        return Vec::new();
    };
    list_on(&conn)
}

fn list_on(conn: &rusqlite::Connection) -> Vec<RecordingItem> {
    let sql = "SELECT id, path, created_at, duration_ms, transcript
               FROM recordings ORDER BY created_at DESC";
    log_sql(sql);
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    stmt.query_map([], |row| {
        Ok(RecordingItem {
            id: row.get(0)?,
            path: row.get(1)?,
            created_at: row.get(2)?,
            duration_ms: row.get(3)?,
            transcript: row.get(4)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

pub fn insert(item: &RecordingItem) -> Result<i64, String> {
    let conn = get_connection().map_err(|e| e.to_string())?;
    insert_on(&conn, item)
}

fn insert_on(conn: &rusqlite::Connection, item: &RecordingItem) -> Result<i64, String> {
    let sql = "INSERT INTO recordings (path, created_at, duration_ms, transcript)
               VALUES (?1, ?2, ?3, ?4)";
    log_sql(sql);
    conn.execute(
        sql,
        rusqlite::params![
            item.path,
            item.created_at,
            item.duration_ms,
            item.transcript
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn delete(id: i64) -> Result<(), String> {
    let conn = get_connection().map_err(|e| e.to_string())?;
    let sql = "SELECT path FROM recordings WHERE id = ?1";
    log_sql(sql);
    let path: Option<String> = conn.query_row(sql, [id], |row| row.get(0)).ok();
    let sql = "DELETE FROM recordings WHERE id = ?1";
    log_sql(sql);
    conn.execute(sql, [id]).map_err(|e| e.to_string())?;
    if let Some(path) = path {
        let _ = std::fs::remove_file(path);
    }
    if PLAYING_ID.load(Ordering::Relaxed) == id as u64 {
        stop_playback();
    }
    Ok(())
}

/// Cheap tick hook: restart a long recognition task. No-op when idle.
pub fn pump() {
    if !is_live() {
        return;
    }
    #[cfg(target_os = "macos")]
    macos::maybe_restart_recognition();
}

pub async fn start(transcribe: bool) -> Result<(), String> {
    if is_live() {
        return Ok(());
    }
    if let Ok(mut g) = shared().lock() {
        g.error = None;
        g.transcript.clear();
        g.finalized.clear();
        g.transcribe = transcribe;
    }
    #[cfg(target_os = "macos")]
    {
        macos::start(transcribe).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = transcribe;
        Err("Voice memos need a Mac with a microphone.".into())
    }
}

pub fn stop() -> Result<Option<RecordingItem>, String> {
    if !is_live() {
        return Ok(None);
    }
    #[cfg(target_os = "macos")]
    {
        macos::stop()
    }
    #[cfg(not(target_os = "macos"))]
    {
        LIVE.store(false, Ordering::SeqCst);
        Ok(None)
    }
}

pub fn play(id: i64) -> Result<(), String> {
    let conn = get_connection().map_err(|e| e.to_string())?;
    let sql = "SELECT path FROM recordings WHERE id = ?1";
    log_sql(sql);
    let path: String = conn
        .query_row(sql, [id], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if !std::path::Path::new(&path).exists() {
        return Err("Recording file is gone.".into());
    }
    #[cfg(target_os = "macos")]
    {
        macos::play(&path, id)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        PLAYING_ID.store(id as u64, Ordering::Relaxed);
        Ok(())
    }
}

pub fn stop_playback() {
    PLAYING_ID.store(0, Ordering::Relaxed);
    #[cfg(target_os = "macos")]
    macos::stop_playback();
}

pub fn pause() {
    #[cfg(target_os = "macos")]
    macos::pause();
}

pub fn resume() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::resume()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

pub fn seek(id: i64, position_s: f64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::seek(id, position_s)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (id, position_s);
        Ok(())
    }
}

pub fn seek_fraction(id: i64, fraction: f64) -> Result<(), String> {
    let fraction = clamp_fraction(fraction);
    match playback_state() {
        Some(pb) if pb.id == id && pb.duration_s > 0.0 => {
            seek(id, pb.duration_s * fraction)
        }
        Some(pb) if pb.id == id => Ok(()),
        _ => seek(id, 0.0),
    }
}

pub fn playback_state() -> Option<Playback> {
    let id = PLAYING_ID.load(Ordering::Relaxed);
    if id == 0 {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        macos::playback_state(id as i64)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some(Playback {
            id: id as i64,
            position_s: 0.0,
            duration_s: 0.0,
            playing: true,
        })
    }
}

pub fn open_externally(path: &str) -> Result<(), String> {
    if !Path::new(path).exists() {
        return Err("Recording file is gone.".into());
    }
    let qt = Command::new("/usr/bin/open")
        .args(["-a", "QuickTime Player", path])
        .status();
    match qt {
        Ok(status) if status.success() => Ok(()),
        _ => Command::new("/usr/bin/open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string()),
    }
}

pub fn reveal_in_finder(path: &str) -> Result<(), String> {
    if !Path::new(path).exists() {
        return Err("Recording file is gone.".into());
    }
    Command::new("/usr/bin/open")
        .args(["-R", path])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn permission_hint() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos::permission_hint()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some("Voice memos record on macOS.".into())
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn persist_current(duration_ms: i64) -> Result<Option<RecordingItem>, String> {
    let (path, transcript) = {
        let g = shared().lock().map_err(|e| e.to_string())?;
        let path = g.path.clone().ok_or("no recording path")?;
        let transcript = stitch_transcripts(&g.finalized, &g.transcript);
        (path, transcript)
    };
    let item = RecordingItem {
        id: 0,
        path: path.to_string_lossy().into_owned(),
        created_at: now_unix(),
        duration_ms,
        transcript,
    };
    let id = insert(&item)?;
    Ok(Some(RecordingItem { id, ..item }))
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_foundation::NSString;
    use std::ffi::c_void;
    use std::ptr;
    use tokio::sync::oneshot;

    #[link(name = "AVFAudio", kind = "framework")]
    extern "C" {}
    #[link(name = "Speech", kind = "framework")]
    extern "C" {}
    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    struct SyncObj(Retained<AnyObject>);
    unsafe impl Send for SyncObj {}
    unsafe impl Sync for SyncObj {}

    struct Engine {
        engine: SyncObj,
        input: SyncObj,
        file: Option<SyncObj>,
        request: Option<SyncObj>,
        recognizer: Option<SyncObj>,
        _tap: TapKeep,
        _result: Option<TapKeep>,
    }

    // RcBlock must stay alive for the tap / result handler. RcBlock itself is
    // neither Send nor Sync, but we only hold it to keep the block alive while
    // AVFoundation/Speech invoke it from their own threads.
    // Field kept alive so AVFoundation/Speech blocks are not dropped.
    #[allow(dead_code)]
    struct TapKeep(Box<dyn std::any::Any>);
    unsafe impl Send for TapKeep {}
    unsafe impl Sync for TapKeep {}

    static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);
    static PLAYER: Mutex<Option<SyncObj>> = Mutex::new(None);

    fn class(name: &str) -> Result<&'static AnyClass, String> {
        let cname = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
        AnyClass::get(&cname).ok_or_else(|| format!("missing {name}"))
    }

    fn ns_string(s: &str) -> Retained<NSString> {
        NSString::from_str(s)
    }

    pub async fn start(transcribe: bool) -> Result<(), String> {
        request_mic().await?;
        let speech_ok = if transcribe {
            match request_speech().await {
                Ok(ok) => ok,
                Err(err) => {
                    log::warn!("speech authorization: {err}");
                    false
                }
            }
        } else {
            false
        };
        begin_engine(transcribe && speech_ok)
    }

    pub fn stop() -> Result<Option<RecordingItem>, String> {
        let started = shared().lock().ok().and_then(|g| g.started);
        let duration_ms = started.map(|t| t.elapsed().as_millis() as i64).unwrap_or(0);
        teardown_engine();
        LIVE.store(false, Ordering::SeqCst);
        TRANSCRIBING.store(false, Ordering::Relaxed);
        LEVEL_BITS.store(0, Ordering::Relaxed);
        persist_current(duration_ms)
    }

    pub fn play(path: &str, id: i64) -> Result<(), String> {
        stop_playback();
        let url = file_url(path)?;
        let cls = class("AVAudioPlayer")?;
        let mut err: *mut AnyObject = ptr::null_mut();
        let player: Option<Retained<AnyObject>> = unsafe {
            let alloc: *mut AnyObject = msg_send![cls, alloc];
            let ptr: *mut AnyObject =
                msg_send![alloc, initWithContentsOfURL: &*url, error: &mut err];
            Retained::from_raw(ptr)
        };
        let Some(player) = player else {
            return Err(ns_error(err).unwrap_or_else(|| "AVAudioPlayer failed".into()));
        };
        let ok: bool = unsafe { msg_send![&*player, play] };
        if !ok {
            return Err("Could not play the recording.".into());
        }
        PLAYING_ID.store(id as u64, Ordering::Relaxed);
        *PLAYER.lock().map_err(|e| e.to_string())? = Some(SyncObj(player));
        Ok(())
    }

    pub fn stop_playback() {
        if let Ok(mut g) = PLAYER.lock() {
            if let Some(player) = g.take() {
                let _: () = unsafe { msg_send![&*player.0, stop] };
            }
        }
        PLAYING_ID.store(0, Ordering::Relaxed);
    }

    pub fn pause() {
        if let Ok(g) = PLAYER.lock() {
            if let Some(player) = g.as_ref() {
                let _: () = unsafe { msg_send![&*player.0, pause] };
            }
        }
    }

    pub fn resume() -> Result<(), String> {
        let g = PLAYER.lock().map_err(|e| e.to_string())?;
        let Some(player) = g.as_ref() else {
            return Err("Nothing is playing.".into());
        };
        let ok: bool = unsafe { msg_send![&*player.0, play] };
        if !ok {
            return Err("Could not resume the recording.".into());
        }
        Ok(())
    }

    pub fn seek(id: i64, position_s: f64) -> Result<(), String> {
        if PLAYING_ID.load(Ordering::Relaxed) != id as u64 {
            return Err("That recording is not active.".into());
        }
        let g = PLAYER.lock().map_err(|e| e.to_string())?;
        let Some(player) = g.as_ref() else {
            return Err("Nothing is playing.".into());
        };
        let duration: f64 = unsafe { msg_send![&*player.0, duration] };
        let t = clamp_position(position_s, duration);
        let _: () = unsafe { msg_send![&*player.0, setCurrentTime: t] };
        Ok(())
    }

    pub fn playback_state(id: i64) -> Option<Playback> {
        let g = PLAYER.lock().ok()?;
        let player = g.as_ref()?;
        let position_s: f64 = unsafe { msg_send![&*player.0, currentTime] };
        let duration_s: f64 = unsafe { msg_send![&*player.0, duration] };
        let playing: bool = unsafe { msg_send![&*player.0, isPlaying] };
        Some(Playback {
            id,
            position_s,
            duration_s,
            playing,
        })
    }

    pub fn permission_hint() -> Option<String> {
        match mic_status() {
            3 => None,
            1 | 2 => Some("Microphone is denied. Enable it in System Settings › Privacy.".into()),
            _ => Some("Recording will ask for the microphone.".into()),
        }
    }

    pub fn maybe_restart_recognition() {
        let due = shared()
            .lock()
            .ok()
            .and_then(|g| g.segment_started)
            .is_some_and(|t| t.elapsed() >= Duration::from_secs(TRANSCRIPT_STITCH_SECS));
        if !due {
            return;
        }
        if let Err(err) = restart_recognition() {
            log::warn!("speech restart: {err}");
        }
    }

    fn mic_status() -> isize {
        let Ok(cls) = class("AVCaptureDevice") else {
            return 0;
        };
        let media = ns_string("soun");
        unsafe { msg_send![cls, authorizationStatusForMediaType: &*media] }
    }

    async fn request_mic() -> Result<(), String> {
        match mic_status() {
            3 => return Ok(()),
            1 | 2 => {
                return Err(
                    "Microphone access is denied. Enable it in System Settings › Privacy.".into(),
                )
            }
            _ => {}
        }
        let (tx, rx) = oneshot::channel::<bool>();
        // Scope every ObjC value (class ref, NSString, RcBlock) so none of
        // them lives across the await: they are !Send and would make this
        // future !Send. The runtime copies the completion block.
        {
            let cls = class("AVCaptureDevice")?;
            let media = ns_string("soun");
            let tx = Mutex::new(Some(tx));
            let handler = RcBlock::new(move |granted: Bool| {
                if let Ok(mut slot) = tx.lock() {
                    if let Some(tx) = slot.take() {
                        let _ = tx.send(granted.as_bool());
                    }
                }
            });
            unsafe {
                let _: () = msg_send![
                    cls,
                    requestAccessForMediaType: &*media,
                    completionHandler: &*handler
                ];
            }
        }
        match rx.await {
            Ok(true) => Ok(()),
            Ok(false) => Err("Microphone access was denied.".into()),
            Err(_) => Err("Microphone prompt was cancelled.".into()),
        }
    }

    async fn request_speech() -> Result<bool, String> {
        let status: isize = {
            let cls = class("SFSpeechRecognizer")?;
            unsafe { msg_send![cls, authorizationStatus] }
        };
        match status {
            3 => return Ok(true),
            1 | 2 => return Ok(false),
            _ => {}
        }
        let (tx, rx) = oneshot::channel::<isize>();
        // Scoped for the same !Send reason as request_mic above.
        {
            let cls = class("SFSpeechRecognizer")?;
            let tx = Mutex::new(Some(tx));
            let handler = RcBlock::new(move |status: isize| {
                if let Ok(mut slot) = tx.lock() {
                    if let Some(tx) = slot.take() {
                        let _ = tx.send(status);
                    }
                }
            });
            unsafe {
                let _: () = msg_send![cls, requestAuthorization: &*handler];
            }
        }
        match rx.await {
            Ok(3) => Ok(true),
            Ok(_) => Ok(false),
            Err(_) => Ok(false),
        }
    }

    fn begin_engine(transcribe: bool) -> Result<(), String> {
        let engine_cls = class("AVAudioEngine")?;
        let engine: Retained<AnyObject> = unsafe { msg_send![engine_cls, new] };
        let input: Retained<AnyObject> = unsafe { msg_send![&*engine, inputNode] };
        let format: Retained<AnyObject> = unsafe { msg_send![&*input, outputFormatForBus: 0usize] };

        let path = recordings_dir().join(format!("rec-{}.caf", now_unix()));
        let url = file_url(path.to_str().unwrap_or("rec.caf"))?;
        let settings: Retained<AnyObject> = unsafe { msg_send![&*format, settings] };
        let file_cls = class("AVAudioFile")?;
        let mut err: *mut AnyObject = ptr::null_mut();
        let file: Option<Retained<AnyObject>> = unsafe {
            let alloc: *mut AnyObject = msg_send![file_cls, alloc];
            let ptr: *mut AnyObject = msg_send![
                alloc,
                initForWriting: &*url,
                settings: &*settings,
                error: &mut err
            ];
            Retained::from_raw(ptr)
        };
        let file =
            file.ok_or_else(|| ns_error(err).unwrap_or_else(|| "AVAudioFile failed".into()))?;

        let (request, recognizer, result_keep) = if transcribe {
            match start_recognition() {
                Ok(parts) => {
                    TRANSCRIBING.store(true, Ordering::Relaxed);
                    (Some(parts.0), Some(parts.1), Some(parts.2))
                }
                Err(err) => {
                    log::warn!("on-device speech unavailable ({err}); record-only");
                    TRANSCRIBING.store(false, Ordering::Relaxed);
                    (None, None, None)
                }
            }
        } else {
            TRANSCRIBING.store(false, Ordering::Relaxed);
            (None, None, None)
        };

        let file_for_tap = SyncObj(file.clone());
        let request_for_tap = request.as_ref().map(|r| SyncObj(r.0.clone()));
        let tap = RcBlock::new(move |buffer: *mut AnyObject, _when: *mut c_void| {
            if buffer.is_null() {
                return;
            }
            let buffer = unsafe { &*buffer };
            LEVEL_BITS.store(pcm_rms(buffer).to_bits(), Ordering::Relaxed);
            let mut write_err: *mut AnyObject = ptr::null_mut();
            let _: bool = unsafe {
                msg_send![&*file_for_tap.0, writeFromBuffer: buffer, error: &mut write_err]
            };
            if let Some(req) = request_for_tap.as_ref() {
                let _: () = unsafe { msg_send![&*req.0, appendAudioPCMBuffer: buffer] };
            }
        });

        unsafe {
            let _: () = msg_send![
                &*input,
                installTapOnBus: 0usize,
                bufferSize: 16384u32,
                format: &*format,
                block: &*tap
            ];
            let _: () = msg_send![&*engine, prepare];
            let mut start_err: *mut AnyObject = ptr::null_mut();
            let ok: bool = msg_send![&*engine, startAndReturnError: &mut start_err];
            if !ok {
                return Err(ns_error(start_err).unwrap_or_else(|| "AVAudioEngine failed".into()));
            }
        }

        let now = Instant::now();
        if let Ok(mut g) = shared().lock() {
            g.started = Some(now);
            g.segment_started = if request.is_some() { Some(now) } else { None };
            g.path = Some(path);
            g.transcribe = request.is_some();
            g.transcript.clear();
            g.finalized.clear();
            g.error = None;
        }
        STARTED_MS.store(0, Ordering::Relaxed);
        LIVE.store(true, Ordering::SeqCst);

        let session = Engine {
            engine: SyncObj(engine),
            input: SyncObj(input),
            file: Some(SyncObj(file)),
            request,
            recognizer,
            _tap: TapKeep(Box::new(tap)),
            _result: result_keep,
        };
        *ENGINE.lock().map_err(|e| e.to_string())? = Some(session);
        Ok(())
    }

    fn start_recognition() -> Result<(SyncObj, SyncObj, TapKeep), String> {
        let rec_cls = class("SFSpeechRecognizer")?;
        let recognizer: Retained<AnyObject> = unsafe { msg_send![rec_cls, new] };
        let on_device: bool = unsafe { msg_send![&*recognizer, supportsOnDeviceRecognition] };
        let req_cls = class("SFSpeechAudioBufferRecognitionRequest")?;
        let request: Retained<AnyObject> = unsafe { msg_send![req_cls, new] };
        unsafe {
            let _: () = msg_send![&*request, setShouldReportPartialResults: true];
            if on_device {
                let _: () = msg_send![&*request, setRequiresOnDeviceRecognition: true];
            }
        }
        let handler = RcBlock::new(move |result: *mut AnyObject, error: *mut AnyObject| {
            if !result.is_null() {
                let text = unsafe {
                    let transcription: Retained<AnyObject> = msg_send![&*result, bestTranscription];
                    let formatted: Retained<NSString> = msg_send![&*transcription, formattedString];
                    formatted.to_string()
                };
                if let Ok(mut g) = shared().lock() {
                    g.transcript = stitch_transcripts(&g.finalized, &text);
                }
            }
            if !error.is_null() {
                if let Some(msg) = ns_error(error) {
                    log::warn!("speech: {msg}");
                }
            }
        });
        let _: Retained<AnyObject> = unsafe {
            msg_send![
                &*recognizer,
                recognitionTaskWithRequest: &*request,
                resultHandler: &*handler
            ]
        };
        Ok((
            SyncObj(request),
            SyncObj(recognizer),
            TapKeep(Box::new(handler)),
        ))
    }

    fn restart_recognition() -> Result<(), String> {
        let mut guard = ENGINE.lock().map_err(|e| e.to_string())?;
        let Some(session) = guard.as_mut() else {
            return Ok(());
        };
        if let Some(req) = session.request.take() {
            let _: () = unsafe { msg_send![&*req.0, endAudio] };
        }
        if let Ok(mut g) = shared().lock() {
            g.finalized = g.transcript.clone();
            g.segment_started = Some(Instant::now());
        }
        match start_recognition() {
            Ok((req, rec, keep)) => {
                session.request = Some(req);
                session.recognizer = Some(rec);
                session._result = Some(keep);
                TRANSCRIBING.store(true, Ordering::Relaxed);
            }
            Err(err) => {
                TRANSCRIBING.store(false, Ordering::Relaxed);
                return Err(err);
            }
        }
        Ok(())
    }

    fn teardown_engine() {
        if let Ok(mut g) = ENGINE.lock() {
            if let Some(session) = g.take() {
                if let Some(req) = session.request {
                    let _: () = unsafe { msg_send![&*req.0, endAudio] };
                }
                let _: () = unsafe { msg_send![&*session.input.0, removeTapOnBus: 0usize] };
                let _: () = unsafe { msg_send![&*session.engine.0, stop] };
                drop(session.file);
            }
        }
        if let Ok(g) = shared().lock() {
            if let Some(started) = g.started {
                STARTED_MS.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
            }
        }
    }

    fn file_url(path: &str) -> Result<Retained<AnyObject>, String> {
        let cls = class("NSURL")?;
        let ns = ns_string(path);
        let url: Option<Retained<AnyObject>> = unsafe { msg_send![cls, fileURLWithPath: &*ns] };
        url.ok_or_else(|| "NSURL failed".into())
    }

    fn ns_error(err: *mut AnyObject) -> Option<String> {
        if err.is_null() {
            return None;
        }
        let localized: Retained<NSString> = unsafe { msg_send![&*err, localizedDescription] };
        Some(localized.to_string())
    }

    fn pcm_rms(buffer: &AnyObject) -> f32 {
        unsafe {
            let frames: u32 = msg_send![buffer, frameLength];
            if frames == 0 {
                return 0.0;
            }
            let data: *const *mut f32 = msg_send![buffer, floatChannelData];
            if data.is_null() || (*data).is_null() {
                return 0.0;
            }
            // SAFETY: AVAudioPCMBuffer channel 0 is valid for `frameLength` samples.
            let samples = crate::ffi::slice(*data, frames as usize);
            if samples.is_empty() {
                return 0.0;
            }
            let sum: f32 = samples.iter().map(|s| s * s).sum();
            (sum / frames as f32).sqrt().min(1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn format_duration_pads_and_promotes_hours() {
        assert_eq!(format_duration_ms(0), "0:00");
        assert_eq!(format_duration_ms(5_000), "0:05");
        assert_eq!(format_duration_ms(65_000), "1:05");
        assert_eq!(format_duration_ms(3_600_000), "1:00:00");
        assert_eq!(format_duration_ms(3_661_000), "1:01:01");
    }

    #[test]
    fn remaining_clock_is_signed_and_clamped() {
        assert_eq!(format_remaining_s(12.0, 60.0), "-0:48");
        assert_eq!(format_remaining_s(0.0, 12.0), "-0:12");
        assert_eq!(format_remaining_s(60.0, 60.0), "-0:00");
        assert_eq!(format_remaining_s(80.0, 60.0), "-0:00");
        assert_eq!(format_elapsed_s(12.4), "0:12");
    }

    #[test]
    fn seek_helpers_clamp_fraction_and_position() {
        assert_eq!(clamp_fraction(-0.2), 0.0);
        assert_eq!(clamp_fraction(0.25), 0.25);
        assert_eq!(clamp_fraction(1.4), 1.0);
        assert_eq!(clamp_fraction(f64::NAN), 0.0);
        assert_eq!(clamp_position(-3.0, 60.0), 0.0);
        assert_eq!(clamp_position(12.0, 60.0), 12.0);
        assert_eq!(clamp_position(90.0, 60.0), 60.0);
        assert_eq!(clamp_position(12.0, 0.0), 0.0);
    }

    #[test]
    fn playback_finished_ignores_a_mid_file_pause() {
        let mid = Playback {
            id: 1,
            position_s: 12.0,
            duration_s: 60.0,
            playing: false,
        };
        assert!(!playback_finished(&mid));
        let end = Playback {
            id: 1,
            position_s: 60.0,
            duration_s: 60.0,
            playing: false,
        };
        assert!(playback_finished(&end));
        let live = Playback {
            id: 1,
            position_s: 59.9,
            duration_s: 60.0,
            playing: true,
        };
        assert!(!playback_finished(&live));
    }

    #[test]
    fn stitch_joins_and_collapses_overlap() {
        assert_eq!(stitch_transcripts("", "hello"), "hello");
        assert_eq!(stitch_transcripts("hello", ""), "hello");
        assert_eq!(stitch_transcripts("hello", "world"), "hello world");
        assert_eq!(
            stitch_transcripts("hello world", "hello world today"),
            "hello world today"
        );
        assert_eq!(stitch_transcripts("hello world", "world"), "hello world");
    }

    #[test]
    fn recordings_table_round_trips() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE recordings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                transcript TEXT NOT NULL DEFAULT ''
            )",
            [],
        )
        .unwrap();
        let id = insert_on(
            &conn,
            &RecordingItem {
                id: 0,
                path: "/tmp/rec.caf".into(),
                created_at: 1_700_000_000,
                duration_ms: 1500,
                transcript: "hello".into(),
            },
        )
        .unwrap();
        assert!(id > 0);
        let rows = list_on(&conn);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].transcript, "hello");
        assert_eq!(rows[0].duration_ms, 1500);
    }

    #[test]
    fn idle_snapshot_is_quiet() {
        assert!(!is_live());
        let snap = snapshot();
        assert!(!snap.recording);
        assert_eq!(snap.level, 0.0);
        pump();
        assert!(!is_live());
    }
}
