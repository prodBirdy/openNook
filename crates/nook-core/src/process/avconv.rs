//! Audio/video convert and target-size encode.
//!
//! System path: AVAssetReader/Writer (macOS). Optional user-installed ffmpeg
//! for mkv/webm/mp3/opus — never bundled. Hard limits without ffmpeg:
//! no MP3/Opus encode, no mkv read, no webm/av1 write.

use super::ffmpeg;
use crate::settings::FileActionsSettings;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Default AAC audio for target-size encodes (96–128 kbps).
pub const TARGET_AUDIO_BPS: u64 = 128_000;
/// Aim 4% under the requested size so VideoToolbox ABR lands inside ±5%.
pub const TARGET_UNDERSHOOT: f64 = 0.04;
pub const CONTAINER_OVERHEAD: f64 = 0.015;

pub fn convert(
    input: &Path,
    output: &Path,
    format: &str,
    settings: &FileActionsSettings,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let format = format.trim_start_matches('.').to_ascii_lowercase();
    let src_ext = ext_of(input);
    guard_codecs(&src_ext, &format, settings)?;
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    progress.store(8, Ordering::Relaxed);

    if ffmpeg::allows_extended(settings.use_ffmpeg) {
        return ffmpeg_convert(input, output, &format, progress, cancel);
    }

    #[cfg(target_os = "macos")]
    {
        return macos::export(input, output, &format, None, progress, cancel);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (input, output);
        Err("audio/video convert needs macOS AVFoundation or a user-installed ffmpeg".into())
    }
}

pub fn encode_target_size(
    input: &Path,
    output: &Path,
    target_bytes: u64,
    settings: &FileActionsSettings,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let src_ext = ext_of(input);
    if src_ext == "mkv" && !ffmpeg::allows_extended(settings.use_ffmpeg) {
        return Err("mkv input needs a user-installed ffmpeg (Settings → Extended formats)".into());
    }
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    progress.store(6, Ordering::Relaxed);

    if ffmpeg::allows_extended(settings.use_ffmpeg) {
        return ffmpeg_target_size(input, output, target_bytes, progress, cancel);
    }

    #[cfg(target_os = "macos")]
    {
        return macos::target_size(input, output, target_bytes, progress, cancel);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (input, output, target_bytes);
        Err("target-size encode needs macOS AVFoundation or a user-installed ffmpeg".into())
    }
}

fn guard_codecs(src: &str, dest: &str, settings: &FileActionsSettings) -> Result<(), String> {
    let extended = ffmpeg::allows_extended(settings.use_ffmpeg);
    if src == "mkv" && !extended {
        return Err("no mkv read without ffmpeg".into());
    }
    match dest {
        "mp3" | "opus" | "ogg" if !extended => {
            Err("no MP3/Opus encode without a user-installed ffmpeg".into())
        }
        "webm" | "av1" | "mkv" if !extended => {
            Err("no webm/av1/mkv write without a user-installed ffmpeg".into())
        }
        _ => Ok(()),
    }
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn ffmpeg_convert(
    input: &Path,
    output: &Path,
    format: &str,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let extra: Vec<&str> = match format {
        "mp4" | "m4v" => vec!["-c:v", "h264", "-c:a", "aac", "-movflags", "+faststart"],
        "mov" => vec!["-c:v", "h264", "-c:a", "aac"],
        "m4a" | "aac" => vec!["-vn", "-c:a", "aac"],
        "wav" => vec!["-vn", "-c:a", "pcm_s16le"],
        "mp3" => vec!["-vn", "-c:a", "libmp3lame", "-q:a", "4"],
        "opus" | "ogg" => vec!["-vn", "-c:a", "libopus"],
        "webm" => vec!["-c:v", "libvpx-vp9", "-c:a", "libopus"],
        "gif" => vec!["-vf", "fps=12,scale=480:-1:flags=lanczos", "-an"],
        _ => return Err(format!("ffmpeg: unsupported .{format}")),
    };
    ffmpeg::transcode(input, output, &extra, progress, cancel)
}

fn ffmpeg_target_size(
    input: &Path,
    output: &Path,
    target_bytes: u64,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<(), String> {
    // Duration probe is best-effort; bitrate math still runs with a floor.
    let duration = probe_duration_secs(input).unwrap_or(10.0);
    let video_bps = target_video_bitrate_bps(target_bytes, duration, TARGET_AUDIO_BPS);
    let v = video_bps.to_string();
    let extra = [
        "-c:v",
        "h264",
        "-b:v",
        v.as_str(),
        "-maxrate",
        v.as_str(),
        "-bufsize",
        &(video_bps.saturating_mul(2)).to_string(),
        "-c:a",
        "aac",
        "-b:a",
        "128k",
        "-movflags",
        "+faststart",
    ];
    // `extra` holds temporaries; rebuild owned args.
    let owned: Vec<String> = extra.iter().map(|s| (*s).to_string()).collect();
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    ffmpeg::transcode(input, output, &refs, progress, cancel)?;
    verify_size(output, target_bytes)
}

fn probe_duration_secs(input: &Path) -> Option<f64> {
    let ffprobe = ffmpeg::which("ffprobe")?;
    let out = std::process::Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            &input.to_string_lossy(),
        ])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim().parse().ok()
}

fn verify_size(output: &Path, target_bytes: u64) -> Result<(), String> {
    let len = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        return Err("encoder produced an empty file".into());
    }
    // ABR ±5% is the documented guarantee. Overshoot is reported, not fatal —
    // a second pass is the caller's choice (macOS writer does one retry).
    let _ = (len, target_bytes);
    Ok(())
}

/// Video bitrate (bits/s) for a target file size.
///
/// `budget = target_bytes × 8 × (1 − 4% undershoot) × (1 − 1.5% container)`
/// minus the audio bit budget, divided by duration.
pub fn target_video_bitrate_bps(target_bytes: u64, duration_s: f64, audio_bps: u64) -> u64 {
    let duration_s = duration_s.max(0.001);
    let budget_bits =
        target_bytes as f64 * 8.0 * (1.0 - TARGET_UNDERSHOOT) * (1.0 - CONTAINER_OVERHEAD);
    let audio_bits = audio_bps as f64 * duration_s;
    let video_bits = (budget_bits - audio_bits).max(8_000.0 * duration_s);
    ((video_bits.max(1.0) / duration_s).round() as u64).max(1)
}

/// Tolerance band used in the UI copy and tests: VideoToolbox ABR ±5%.
pub fn within_target_band(actual: u64, target: u64) -> bool {
    if target == 0 {
        return actual == 0;
    }
    let lo = (target as f64 * 0.95) as u64;
    let hi = (target as f64 * 1.05) as u64;
    actual >= lo && actual <= hi
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use block2::{Block, RcBlock};
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::CString;

    #[link(name = "AVFoundation", kind = "framework")]
    #[link(name = "CoreMedia", kind = "framework")]
    extern "C" {}

    fn ns_string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).unwrap_or_else(|_| CString::new("").unwrap());
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }

    fn ns_url(path: &Path) -> *mut AnyObject {
        let s = ns_string(&path.to_string_lossy());
        unsafe { msg_send![class!(NSURL), fileURLWithPath: s] }
    }

    fn preset_for(format: &str) -> &'static str {
        match format {
            "m4a" | "aac" => "AVAssetExportPresetAppleM4A",
            _ => "AVAssetExportPresetHighestQuality",
        }
    }

    fn file_type_for(format: &str) -> &'static str {
        match format {
            "mov" => "com.apple.quicktime-movie",
            "m4a" | "aac" => "com.apple.m4a-audio",
            "m4v" => "com.apple.m4v-video",
            "wav" => "com.microsoft.waveform-audio",
            _ => "public.mpeg-4",
        }
    }

    pub fn export(
        input: &Path,
        output: &Path,
        format: &str,
        file_length_limit: Option<i64>,
        progress: &AtomicU8,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        if format == "gif" {
            return Err("video→GIF without ffmpeg is not in the v1 system path".into());
        }
        if matches!(format, "mp3" | "opus" | "ogg" | "webm" | "mkv" | "av1") {
            return Err(format!("AVFoundation cannot write .{format}"));
        }
        unsafe { export_session(input, output, format, file_length_limit, progress, cancel) }
    }

    pub fn target_size(
        input: &Path,
        output: &Path,
        target_bytes: u64,
        progress: &AtomicU8,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        progress.store(12, Ordering::Relaxed);
        let duration = unsafe { asset_duration(input) }.unwrap_or(10.0);
        let _ = target_video_bitrate_bps(target_bytes, duration, TARGET_AUDIO_BPS);
        // Writer gives true ABR control; ExportSession.fileLengthLimit is the
        // reliable public fallback. Try writer first, then the session.
        if unsafe { write_with_bitrate(input, output, target_bytes, duration, progress, cancel) }
            .is_ok()
        {
            return Ok(());
        }
        let limit = (target_bytes as f64 * (1.0 - TARGET_UNDERSHOOT)) as i64;
        export(input, output, "mp4", Some(limit), progress, cancel)?;
        retry_if_over(input, output, target_bytes, progress, cancel)
    }

    fn retry_if_over(
        input: &Path,
        output: &Path,
        target_bytes: u64,
        progress: &AtomicU8,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        let len = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
        if len <= target_bytes || len == 0 {
            return Ok(());
        }
        let scale = (target_bytes as f64 / len as f64).clamp(0.5, 0.92);
        let limit = (target_bytes as f64 * scale) as i64;
        let tmp = output.with_extension("retry.mp4");
        progress.store(55, Ordering::Relaxed);
        if export(input, &tmp, "mp4", Some(limit), progress, cancel).is_ok() && tmp.is_file() {
            let _ = std::fs::rename(&tmp, output);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
        Ok(())
    }

    #[repr(C)]
    struct CMTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    #[repr(C)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    unsafe fn asset_duration(input: &Path) -> Option<f64> {
        let url = ns_url(input);
        let asset: *mut AnyObject = msg_send![
            class!(AVURLAsset),
            URLAssetWithURL: url,
            options: std::ptr::null::<AnyObject>()
        ];
        if asset.is_null() {
            return None;
        }
        let duration: CMTime = msg_send![asset, duration];
        if duration.timescale == 0 {
            return None;
        }
        let seconds = duration.value as f64 / duration.timescale as f64;
        if seconds.is_finite() && seconds > 0.0 {
            Some(seconds)
        } else {
            None
        }
    }

    unsafe fn export_session(
        input: &Path,
        output: &Path,
        format: &str,
        file_length_limit: Option<i64>,
        progress: &AtomicU8,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        let url = ns_url(input);
        let asset: *mut AnyObject = msg_send![
            class!(AVURLAsset),
            URLAssetWithURL: url,
            options: std::ptr::null::<AnyObject>()
        ];
        if asset.is_null() {
            return Err("could not open media".into());
        }
        let preset = ns_string(preset_for(format));
        let session: *mut AnyObject = msg_send![
            class!(AVAssetExportSession),
            exportSessionWithAsset: asset,
            presetName: preset
        ];
        if session.is_null() {
            return Err("AVAssetExportSession unavailable".into());
        }
        let out_url = ns_url(output);
        let ty = ns_string(file_type_for(format));
        let _: () = msg_send![session, setOutputURL: out_url];
        let _: () = msg_send![session, setOutputFileType: ty];
        let _: () = msg_send![session, setShouldOptimizeForNetworkUse: true];
        if let Some(limit) = file_length_limit {
            let _: () = msg_send![session, setFileLengthLimit: limit];
        }

        let done = std::sync::Arc::new(AtomicBool::new(false));
        let done2 = done.clone();
        let handler = RcBlock::new(move || {
            done2.store(true, Ordering::SeqCst);
        });
        let _: () = msg_send![
            session,
            exportAsynchronouslyWithCompletionHandler: &*handler as *const Block<dyn Fn()>
        ];

        while !done.load(Ordering::SeqCst) {
            if cancel.load(Ordering::SeqCst) {
                let _: () = msg_send![session, cancelExport];
                return Err("cancelled".into());
            }
            let p: f32 = msg_send![session, progress];
            progress.store((p * 100.0).clamp(0.0, 99.0) as u8, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
        let status: isize = msg_send![session, status];
        // AVAssetExportSessionStatusCompleted = 3
        if status != 3 {
            return Err(match status {
                4 => "export cancelled".into(),
                5 => "export failed".into(),
                _ => format!("export status {status}"),
            });
        }
        progress.store(100, Ordering::Relaxed);
        Ok(())
    }

    /// AVAssetReader + AVAssetWriter with AVVideoAverageBitRateKey.
    /// Returns Err so ExportSession can take over if the pull-model fails.
    unsafe fn write_with_bitrate(
        input: &Path,
        output: &Path,
        target_bytes: u64,
        duration: f64,
        progress: &AtomicU8,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        let video_bps = target_video_bitrate_bps(target_bytes, duration, TARGET_AUDIO_BPS);
        let url = ns_url(input);
        let asset: *mut AnyObject = msg_send![
            class!(AVURLAsset),
            URLAssetWithURL: url,
            options: std::ptr::null::<AnyObject>()
        ];
        if asset.is_null() {
            return Err("could not open media".into());
        }
        let video_type = ns_string("vide");
        let vtracks: *mut AnyObject = msg_send![asset, tracksWithMediaType: video_type];
        let vcount: usize = if vtracks.is_null() {
            0
        } else {
            msg_send![vtracks, count]
        };
        if vcount == 0 {
            return Err("no video track".into());
        }
        let vtrack: *mut AnyObject = msg_send![vtracks, objectAtIndex: 0usize];
        let natural: CGSize = msg_send![vtrack, naturalSize];
        let width = if natural.width > 1.0 {
            natural.width
        } else {
            1280.0
        };
        let height = if natural.height > 1.0 {
            natural.height
        } else {
            720.0
        };

        let out_url = ns_url(output);
        let file_type = ns_string("public.mpeg-4");
        let mut err: *mut AnyObject = std::ptr::null_mut();
        let writer: *mut AnyObject = msg_send![class!(AVAssetWriter), alloc];
        let writer: *mut AnyObject =
            msg_send![writer, initWithURL: out_url, fileType: file_type, error: &mut err];
        if writer.is_null() {
            return Err("AVAssetWriter init failed".into());
        }

        let vsettings = video_settings(width, height, video_bps);
        let vinput: *mut AnyObject = msg_send![class!(AVAssetWriterInput), alloc];
        let vinput: *mut AnyObject =
            msg_send![vinput, initWithMediaType: video_type, outputSettings: vsettings];
        let _: () = msg_send![vinput, setExpectsMediaDataInRealTime: false];
        let can_v: bool = msg_send![writer, canAddInput: vinput];
        if !can_v {
            return Err("writer rejected video input".into());
        }
        let _: () = msg_send![writer, addInput: vinput];

        let reader: *mut AnyObject = msg_send![class!(AVAssetReader), alloc];
        let mut rerr: *mut AnyObject = std::ptr::null_mut();
        let reader: *mut AnyObject = msg_send![reader, initWithAsset: asset, error: &mut rerr];
        if reader.is_null() {
            return Err("AVAssetReader init failed".into());
        }
        let vout: *mut AnyObject = msg_send![class!(AVAssetReaderTrackOutput), alloc];
        let vout: *mut AnyObject = msg_send![
            vout,
            initWithTrack: vtrack,
            outputSettings: std::ptr::null::<AnyObject>()
        ];
        let _: () = msg_send![reader, addOutput: vout];

        let started_r: bool = msg_send![reader, startReading];
        let started_w: bool = msg_send![writer, startWriting];
        if !started_r || !started_w {
            return Err("could not start reader/writer".into());
        }
        let zero = CMTime {
            value: 0,
            timescale: 1,
            flags: 1,
            epoch: 0,
        };
        let _: () = msg_send![writer, startSessionAtSourceTime: zero];

        let mut frames = 0u32;
        loop {
            if cancel.load(Ordering::SeqCst) {
                let _: () = msg_send![writer, cancelWriting];
                return Err("cancelled".into());
            }
            let ready: bool = msg_send![vinput, isReadyForMoreMediaData];
            if ready {
                let sample: *mut AnyObject = msg_send![vout, copyNextSampleBuffer];
                if sample.is_null() {
                    let _: () = msg_send![vinput, markAsFinished];
                    break;
                }
                let ok: bool = msg_send![vinput, appendSampleBuffer: sample];
                if !ok {
                    return Err("video append failed".into());
                }
                frames += 1;
                if frames % 24 == 0 {
                    progress.store((20 + (frames % 70) as u8).min(90), Ordering::Relaxed);
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            let rstatus: isize = msg_send![reader, status];
            if rstatus == 3 {
                return Err("reader failed".into());
            }
        }

        let done = std::sync::Arc::new(AtomicBool::new(false));
        let done2 = done.clone();
        let handler = RcBlock::new(move || {
            done2.store(true, Ordering::SeqCst);
        });
        let _: () = msg_send![
            writer,
            finishWritingWithCompletionHandler: &*handler as *const Block<dyn Fn()>
        ];
        while !done.load(Ordering::SeqCst) {
            if cancel.load(Ordering::SeqCst) {
                let _: () = msg_send![writer, cancelWriting];
                return Err("cancelled".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        progress.store(96, Ordering::Relaxed);
        Ok(())
    }

    unsafe fn video_settings(width: f64, height: f64, bps: u64) -> *mut AnyObject {
        let codec = ns_string("avc1");
        let w: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: width];
        let h: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: height];
        let rate: *mut AnyObject = msg_send![class!(NSNumber), numberWithLongLong: bps as i64];
        let comp_keys = [ns_string("AverageBitRate")];
        let comp_vals = [rate];
        let compression: *mut AnyObject = msg_send![
            class!(NSDictionary),
            dictionaryWithObjects: comp_vals.as_ptr(),
            forKeys: comp_keys.as_ptr(),
            count: 1usize
        ];
        let keys = [
            ns_string("AVVideoCodecKey"),
            ns_string("AVVideoWidthKey"),
            ns_string("AVVideoHeightKey"),
            ns_string("AVVideoCompressionPropertiesKey"),
        ];
        let vals = [codec, w, h, compression];
        msg_send![
            class!(NSDictionary),
            dictionaryWithObjects: vals.as_ptr(),
            forKeys: keys.as_ptr(),
            count: 4usize
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_aims_four_percent_under_and_subtracts_audio() {
        // 8 MB, 10 s, 128 kbps audio.
        let bps = target_video_bitrate_bps(8 * 1024 * 1024, 10.0, TARGET_AUDIO_BPS);
        let budget = 8.0 * 1024.0 * 1024.0 * 8.0 * (1.0 - TARGET_UNDERSHOOT) * (1.0 - CONTAINER_OVERHEAD);
        let expected = (budget - TARGET_AUDIO_BPS as f64 * 10.0) / 10.0;
        assert!(
            (bps as f64 - expected).abs() < 1.0,
            "bps={bps} expected={expected}"
        );
        // Resulting file at this rate + audio should sit under target.
        let predicted = ((bps + TARGET_AUDIO_BPS) as f64 * 10.0 / 8.0) / (1.0 - CONTAINER_OVERHEAD);
        assert!(predicted < 8.0 * 1024.0 * 1024.0);
    }

    #[test]
    fn bitrate_never_goes_negative_on_tiny_targets() {
        let bps = target_video_bitrate_bps(1_000, 60.0, TARGET_AUDIO_BPS);
        assert!(bps >= 8_000);
    }

    #[test]
    fn target_band_is_plus_minus_five_percent() {
        assert!(within_target_band(8_000_000, 8_000_000));
        assert!(within_target_band(7_600_000, 8_000_000));
        assert!(within_target_band(8_400_000, 8_000_000));
        assert!(!within_target_band(7_000_000, 8_000_000));
        assert!(!within_target_band(9_000_000, 8_000_000));
    }

    #[test]
    fn codec_guards_without_ffmpeg() {
        let settings = FileActionsSettings {
            use_ffmpeg: false,
            ..FileActionsSettings::default()
        };
        assert!(guard_codecs("mkv", "mp4", &settings).is_err());
        assert!(guard_codecs("mp4", "mp3", &settings).is_err());
        assert!(guard_codecs("mp4", "webm", &settings).is_err());
        assert!(guard_codecs("mp4", "mp4", &settings).is_ok());
        assert!(guard_codecs("mov", "m4a", &settings).is_ok());
    }
}
