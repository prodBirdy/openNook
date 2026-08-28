//! Optional presence wrapper for a **user-installed** ffmpeg on PATH.
//!
//! v1 does not download, bundle, or link ffmpeg. The Settings toggle only
//! lights up when [`on_path`] is true. GPL builds (evermeet.cx statics, x264)
//! are out of scope — we never ship a binary.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::OnceLock;

static PRESENT: OnceLock<bool> = OnceLock::new();

/// One-shot PATH probe. No idle loop; call from a user action or Settings.
pub fn on_path() -> bool {
    *PRESENT.get_or_init(detect)
}

fn detect() -> bool {
    which("ffmpeg").is_some()
}

pub fn which(bin: &str) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(bin);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // Common user-install prefixes when PATH is thin (launchd GUI apps).
    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"] {
        let candidate = PathBuf::from(dir).join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Hard limits this wrapper will not attempt even when ffmpeg is present.
pub fn refuses(format: &str) -> Option<&'static str> {
    match format.to_ascii_lowercase().as_str() {
        // Encode-side refusals without an LGPL-configured build we control.
        // User ffmpeg *may* have these, but we still refuse mkv *read* and
        // webm/av1 *write* unless the Settings toggle is on — the caller
        // checks [`allows_extended`].
        _ => None,
    }
}

/// Extended formats (mkv in, webm/av1/mp3/opus out) require the toggle *and* PATH.
pub fn allows_extended(use_ffmpeg: bool) -> bool {
    use_ffmpeg && on_path()
}

pub fn run(
    args: &[String],
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let bin = which("ffmpeg").ok_or("ffmpeg is not installed")?;
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    progress.store(progress.load(Ordering::Relaxed).max(8), Ordering::Relaxed);
    let mut child = Command::new(&bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg spawn: {e}"))?;
    // User-triggered subprocess; wait, then exit. No resident daemon.
    if cancel.load(Ordering::SeqCst) {
        let _ = child.kill();
        return Err("cancelled".into());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail = err.lines().rev().take(4).collect::<Vec<_>>().join("; ");
        return Err(if tail.is_empty() {
            format!("ffmpeg exited {}", out.status)
        } else {
            tail
        });
    }
    progress.store(90, Ordering::Relaxed);
    Ok(())
}

/// `-y -i <in> … <out>` helper used by avconv when extended formats are on.
pub fn transcode(
    input: &std::path::Path,
    output: &std::path::Path,
    extra: &[&str],
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut args = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_string()));
    args.push(output.to_string_lossy().into_owned());
    run(&args, progress, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_requires_toggle_and_binary() {
        if on_path() {
            assert!(allows_extended(true));
            assert!(!allows_extended(false));
        } else {
            assert!(!allows_extended(true));
            assert!(!allows_extended(false));
        }
    }

    #[test]
    fn refuses_is_reserved_and_empty_for_now() {
        assert!(refuses("mp4").is_none());
        assert!(refuses("png").is_none());
    }
}
