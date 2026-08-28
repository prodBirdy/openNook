//! Local file-processing suite: convert, target-size, PDF compress, BG removal, OCR.
//!
//! Jobs are strictly user-triggered. The queue holds progress atomics so the
//! island can paint while a job is live without a resident timer when idle.

pub mod avconv;
pub mod ffmpeg;
pub mod imageconv;
pub mod pdf;
pub mod vision;

use crate::files::{self, FileTrayItem};
use crate::settings::{FileActionsSettings, PdfPreset};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static QUEUE: OnceLock<Mutex<Vec<ProcessJob>>> = OnceLock::new();

fn queue() -> &'static Mutex<Vec<ProcessJob>> {
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Convert,
    TargetSize,
    CompressPdf,
    RemoveBg,
    Ocr,
}

impl JobKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Convert => "Convert",
            Self::TargetSize => "Target size",
            Self::CompressPdf => "Compress PDF",
            Self::RemoveBg => "Remove BG",
            Self::Ocr => "Copy Text",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_live(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }

    /// Legal one-step transitions. Anything else is rejected by [`ProcessJob::set_status`].
    pub fn can_transition(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Running)
                | (Self::Queued, Self::Cancelled)
                | (Self::Running, Self::Done)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobParams {
    Convert { format: String },
    TargetSize { bytes: u64 },
    CompressPdf { preset: PdfPreset },
    RemoveBg,
    Ocr,
}

impl JobParams {
    pub fn kind(&self) -> JobKind {
        match self {
            Self::Convert { .. } => JobKind::Convert,
            Self::TargetSize { .. } => JobKind::TargetSize,
            Self::CompressPdf { .. } => JobKind::CompressPdf,
            Self::RemoveBg => JobKind::RemoveBg,
            Self::Ocr => JobKind::Ocr,
        }
    }
}

pub struct ProcessJob {
    pub id: u64,
    pub kind: JobKind,
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub progress: Arc<AtomicU8>,
    pub status: JobStatus,
    pub message: String,
    pub cancel: Arc<AtomicBool>,
}

impl ProcessJob {
    fn new(kind: JobKind, input: PathBuf) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            kind,
            input,
            output: None,
            progress: Arc::new(AtomicU8::new(0)),
            status: JobStatus::Queued,
            message: String::new(),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_status(&mut self, next: JobStatus) -> bool {
        if !self.status.can_transition(next) {
            return false;
        }
        self.status = next;
        true
    }
}

/// UI-safe snapshot (progress is a plain byte, not an atomic).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobSnapshot {
    pub id: u64,
    pub kind: JobKind,
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub progress: u8,
    pub status: JobStatus,
    pub message: String,
}

impl From<&ProcessJob> for JobSnapshot {
    fn from(job: &ProcessJob) -> Self {
        Self {
            id: job.id,
            kind: job.kind,
            input: job.input.clone(),
            output: job.output.clone(),
            progress: job.progress.load(Ordering::Relaxed),
            status: job.status,
            message: job.message.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct JobResult {
    pub output: Option<PathBuf>,
    pub hud: String,
    pub insert_tray: bool,
}

pub fn enqueue(kind: JobKind, input: PathBuf) -> JobSnapshot {
    let job = ProcessJob::new(kind, input);
    let snap = JobSnapshot::from(&job);
    if let Ok(mut q) = queue().lock() {
        q.push(job);
        if q.len() > 32 {
            q.retain(|j| j.status.is_live() || j.id == snap.id);
        }
    }
    snap
}

pub fn snapshot_jobs() -> Vec<JobSnapshot> {
    queue()
        .lock()
        .map(|q| q.iter().map(JobSnapshot::from).collect())
        .unwrap_or_default()
}

pub fn live_job() -> Option<JobSnapshot> {
    snapshot_jobs().into_iter().rev().find(|j| j.status.is_live())
}

pub fn any_live() -> bool {
    queue()
        .lock()
        .map(|q| q.iter().any(|j| j.status.is_live()))
        .unwrap_or(false)
}

pub fn cancel(id: u64) -> bool {
    let Ok(mut q) = queue().lock() else {
        return false;
    };
    let Some(job) = q.iter_mut().find(|j| j.id == id) else {
        return false;
    };
    job.cancel.store(true, Ordering::SeqCst);
    if job.status == JobStatus::Queued {
        job.set_status(JobStatus::Cancelled);
    }
    true
}

fn with_job(id: u64, f: impl FnOnce(&mut ProcessJob)) {
    if let Ok(mut q) = queue().lock() {
        if let Some(job) = q.iter_mut().find(|j| j.id == id) {
            f(job);
        }
    }
}

/// Blocking worker. Call from `cx.background_executor`, never the UI tick.
pub fn run_job(id: u64, params: JobParams, settings: &FileActionsSettings) -> Result<JobResult, String> {
    if !settings.enabled {
        return Err("File actions are disabled in Settings".into());
    }
    let input = {
        let Ok(q) = queue().lock() else {
            return Err("job queue unavailable".into());
        };
        let job = q.iter().find(|j| j.id == id).ok_or("job not found")?;
        if job.cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        job.input.clone()
    };
    with_job(id, |job| {
        let _ = job.set_status(JobStatus::Running);
        job.progress.store(5, Ordering::Relaxed);
    });

    let activity = UserActivity::begin();
    let progress = progress_handle(id);
    let cancel = cancel_handle(id);
    let result = dispatch(&input, &params, settings, &progress, &cancel);
    drop(activity);

    match result {
        Ok(done) => {
            with_job(id, |job| {
                job.output = done.output.clone();
                job.message = done.hud.clone();
                job.progress.store(100, Ordering::Relaxed);
                let _ = job.set_status(JobStatus::Done);
            });
            Ok(done)
        }
        Err(err) => {
            let cancelled = cancel.load(Ordering::SeqCst) || err == "cancelled";
            with_job(id, |job| {
                job.message = err.clone();
                let _ = job.set_status(if cancelled {
                    JobStatus::Cancelled
                } else {
                    JobStatus::Failed
                });
            });
            if cancelled {
                cleanup_partial(id);
                Err("cancelled".into())
            } else {
                Err(err)
            }
        }
    }
}

fn progress_handle(id: u64) -> Arc<AtomicU8> {
    queue()
        .lock()
        .ok()
        .and_then(|q| q.iter().find(|j| j.id == id).map(|j| j.progress.clone()))
        .unwrap_or_else(|| Arc::new(AtomicU8::new(0)))
}

fn cancel_handle(id: u64) -> Arc<AtomicBool> {
    queue()
        .lock()
        .ok()
        .and_then(|q| q.iter().find(|j| j.id == id).map(|j| j.cancel.clone()))
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)))
}

fn cleanup_partial(id: u64) {
    let output = queue()
        .lock()
        .ok()
        .and_then(|q| q.iter().find(|j| j.id == id).and_then(|j| j.output.clone()));
    if let Some(path) = output {
        let _ = std::fs::remove_file(path);
    }
}

fn dispatch(
    input: &Path,
    params: &JobParams,
    settings: &FileActionsSettings,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<JobResult, String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    match params {
        JobParams::Convert { format } => {
            let out = output_path(input, format, settings, None)?;
            convert_any(input, &out, format, settings, progress, cancel)?;
            Ok(saved(out))
        }
        JobParams::TargetSize { bytes } => {
            let out = output_path(input, "mp4", settings, Some(*bytes))?;
            avconv::encode_target_size(input, &out, *bytes, settings, progress, cancel)?;
            Ok(saved(out))
        }
        JobParams::CompressPdf { preset } => {
            let out = output_path(input, "pdf", settings, None)?;
            pdf::compress(input, &out, *preset, settings, progress, cancel)?;
            Ok(saved(out))
        }
        JobParams::RemoveBg => {
            let out = output_path(input, "png", settings, Some(0))?;
            vision::remove_background(input, &out, progress, cancel)?;
            Ok(saved(out))
        }
        JobParams::Ocr => {
            let text = vision::ocr(input, progress, cancel)?;
            let chars = text.chars().count();
            vision::copy_to_clipboard(&text)?;
            progress.store(100, Ordering::Relaxed);
            Ok(JobResult {
                output: None,
                hud: format!("Copied {chars} chars"),
                insert_tray: false,
            })
        }
    }
}

fn convert_any(
    input: &Path,
    output: &Path,
    format: &str,
    settings: &FileActionsSettings,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mime = files::mime_from_path(&input.to_string_lossy());
    match mime.as_str() {
        "image" => imageconv::convert(input, output, format, settings.jpeg_quality, progress, cancel),
        "video" | "audio" => avconv::convert(input, output, format, settings, progress, cancel),
        "pdf" if format.eq_ignore_ascii_case("pdf") => {
            pdf::compress(input, output, settings.pdf_preset, settings, progress, cancel)
        }
        other => Err(format!("cannot convert {other} to {format}")),
    }
}

fn saved(out: PathBuf) -> JobResult {
    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    JobResult {
        output: Some(out),
        hud: format!("Saved {name}"),
        insert_tray: true,
    }
}

/// Alongside the source, or the configured folder. Never overwrites: `-1`, `-2`, …
pub fn output_path(
    input: &Path,
    ext: &str,
    settings: &FileActionsSettings,
    size_hint: Option<u64>,
) -> Result<PathBuf, String> {
    let dir = match settings.output_folder.as_deref() {
        Some(folder) if !folder.is_empty() => PathBuf::from(folder),
        _ => input
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("output folder: {e}"))?;
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "nook".into());
    let suffix = match size_hint {
        Some(0) => "-nobg".into(),
        Some(bytes) if bytes >= 1024 * 1024 && bytes % (1024 * 1024) == 0 => {
            format!("-{}MB", bytes / (1024 * 1024))
        }
        Some(bytes) => format!("-{}", files::format_size(bytes as i64).replace(' ', "")),
        None => String::new(),
    };
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let mut candidate = dir.join(format!("{stem}{suffix}.{ext}"));
    if candidate == input {
        candidate = dir.join(format!("{stem}-converted.{ext}"));
    }
    let mut n = 1u32;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}{suffix}-{n}.{ext}"));
        n += 1;
        if n > 999 {
            return Err("too many existing outputs".into());
        }
    }
    Ok(candidate)
}

pub fn insert_output(path: &Path) -> Result<FileTrayItem, String> {
    files::add_dropped_path(&path.to_string_lossy())
}

/// Screen-region OCR via `/usr/sbin/screencapture -i`. Degrades without Screen Recording TCC.
pub fn ocr_screen_region(
    settings: &FileActionsSettings,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<JobResult, String> {
    if !settings.enabled {
        return Err("File actions are disabled in Settings".into());
    }
    let text = vision::ocr_screen_region(progress, cancel)?;
    let chars = text.chars().count();
    vision::copy_to_clipboard(&text)?;
    Ok(JobResult {
        output: None,
        hud: format!("Copied {chars} chars"),
        insert_tray: false,
    })
}

/// Keep App Nap from throttling a long encode. No-op off macOS.
struct UserActivity {
    #[cfg(target_os = "macos")]
    token: Option<*mut objc2::runtime::AnyObject>,
}

impl UserActivity {
    fn begin() -> Self {
        #[cfg(target_os = "macos")]
        {
            use objc2::runtime::AnyObject;
            use objc2::*;
            // NSActivityUserInitiated = 0x00FFFFFF
            let token: *mut AnyObject = unsafe {
                let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
                let reason: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: c"nook file-process".as_ptr()];
                msg_send![info, beginActivityWithOptions: 0x00FFFFFFu64, reason: reason]
            };
            Self {
                token: if token.is_null() { None } else { Some(token) },
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {}
        }
    }
}

impl Drop for UserActivity {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        {
            use objc2::runtime::AnyObject;
            use objc2::*;
            if let Some(token) = self.token {
                unsafe {
                    let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
                    let _: () = msg_send![info, endActivity: token];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_settings() -> FileActionsSettings {
        FileActionsSettings::default()
    }

    #[test]
    fn status_machine_allows_only_forward_edges() {
        assert!(JobStatus::Queued.can_transition(JobStatus::Running));
        assert!(JobStatus::Queued.can_transition(JobStatus::Cancelled));
        assert!(JobStatus::Running.can_transition(JobStatus::Done));
        assert!(JobStatus::Running.can_transition(JobStatus::Failed));
        assert!(JobStatus::Running.can_transition(JobStatus::Cancelled));
        assert!(!JobStatus::Queued.can_transition(JobStatus::Done));
        assert!(!JobStatus::Done.can_transition(JobStatus::Running));
        assert!(!JobStatus::Failed.can_transition(JobStatus::Queued));
        assert!(!JobStatus::Cancelled.can_transition(JobStatus::Running));
        assert!(!JobStatus::Done.can_transition(JobStatus::Failed));
    }

    #[test]
    fn enqueue_run_and_cancel_follow_the_machine() {
        let snap = enqueue(JobKind::Ocr, PathBuf::from("/tmp/shot.png"));
        assert_eq!(snap.status, JobStatus::Queued);
        assert_eq!(snap.kind, JobKind::Ocr);
        assert!(snap.status.is_live());

        with_job(snap.id, |job| {
            assert!(job.set_status(JobStatus::Running));
            assert!(!job.set_status(JobStatus::Queued));
            assert!(job.set_status(JobStatus::Cancelled));
            assert!(!job.set_status(JobStatus::Done));
        });
        let after = snapshot_jobs()
            .into_iter()
            .find(|j| j.id == snap.id)
            .unwrap();
        assert_eq!(after.status, JobStatus::Cancelled);
        assert!(after.status.is_terminal());
    }

    #[test]
    fn cancel_queued_job_marks_cancelled() {
        let snap = enqueue(JobKind::Convert, PathBuf::from("/tmp/a.png"));
        assert!(cancel(snap.id));
        let after = snapshot_jobs()
            .into_iter()
            .find(|j| j.id == snap.id)
            .unwrap();
        assert_eq!(after.status, JobStatus::Cancelled);
    }

    #[test]
    fn output_path_avoids_overwrite_and_honours_folder() {
        let dir = std::env::temp_dir().join(format!("nook-process-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let input = dir.join("clip.mov");
        let _ = std::fs::write(&input, b"x");
        let mut settings = empty_settings();
        settings.output_folder = Some(dir.to_string_lossy().into_owned());

        let first = output_path(&input, "mp4", &settings, Some(8 * 1024 * 1024)).unwrap();
        assert_eq!(first.file_name().unwrap(), "clip-8MB.mp4");
        let _ = std::fs::write(&first, b"y");
        let second = output_path(&input, "mp4", &settings, Some(8 * 1024 * 1024)).unwrap();
        assert_eq!(second.file_name().unwrap(), "clip-8MB-1.mp4");

        let alongside = output_path(&input, "png", &empty_settings(), Some(0)).unwrap();
        assert_eq!(alongside.parent(), input.parent());
        assert!(alongside
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("-nobg"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn job_params_map_to_kind() {
        assert_eq!(
            JobParams::Convert {
                format: "png".into()
            }
            .kind(),
            JobKind::Convert
        );
        assert_eq!(
            JobParams::TargetSize { bytes: 8 }.kind(),
            JobKind::TargetSize
        );
        assert_eq!(
            JobParams::CompressPdf {
                preset: PdfPreset::Screen
            }
            .kind(),
            JobKind::CompressPdf
        );
        assert_eq!(JobParams::RemoveBg.kind(), JobKind::RemoveBg);
        assert_eq!(JobParams::Ocr.kind(), JobKind::Ocr);
    }

    #[test]
    fn disabled_settings_reject_work() {
        let mut settings = empty_settings();
        settings.enabled = false;
        let snap = enqueue(JobKind::Ocr, PathBuf::from("/tmp/x.png"));
        let err = run_job(snap.id, JobParams::Ocr, &settings).unwrap_err();
        assert!(err.contains("disabled"));
    }
}
