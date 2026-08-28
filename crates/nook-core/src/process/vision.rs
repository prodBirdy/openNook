//! Background removal (Vision) and OCR (Vision + PDFKit).
//!
//! macOS 14+: `VNGenerateForegroundInstanceMaskRequest` (any salient object).
//! macOS 12–13: `VNGeneratePersonSegmentationRequest` — persons only.
//! OCR: `VNRecognizeTextRequest`. PDFs try `PDFDocument.string` first.
//! Screen-region OCR uses `/usr/sbin/screencapture -i` and degrades without
//! Screen Recording TCC.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub fn remove_background(
    input: &Path,
    output: &Path,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    progress.store(8, Ordering::Relaxed);
    #[cfg(target_os = "macos")]
    {
        return macos::remove_background(input, output, progress, cancel);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (input, output);
        Err("background removal needs Vision on macOS".into())
    }
}

pub fn ocr(
    input: &Path,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    progress.store(8, Ordering::Relaxed);
    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "pdf" {
        #[cfg(target_os = "macos")]
        {
            if let Some(text) = macos::pdf_embedded_text(input) {
                if !text.trim().is_empty() {
                    progress.store(100, Ordering::Relaxed);
                    return Ok(text);
                }
            }
            return macos::ocr_pdf_pages(input, progress, cancel);
        }
        #[cfg(not(target_os = "macos"))]
        {
            return Err("PDF OCR needs PDFKit on macOS".into());
        }
    }
    #[cfg(target_os = "macos")]
    {
        macos::ocr_image(input, progress)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = input;
        Err("image OCR needs Vision on macOS".into())
    }
}

/// Interactive region picker. Returns a clear error when Screen Recording TCC
/// is denied or `screencapture` exits non-zero.
pub fn ocr_screen_region(
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = progress;
        return Err("screen OCR needs macOS screencapture".into());
    }
    #[cfg(target_os = "macos")]
    {
        let tmp = std::env::temp_dir().join(format!("nook-ocr-{}.png", std::process::id()));
        let status = std::process::Command::new("/usr/sbin/screencapture")
            .args(["-i", "-x", "-t", "png", &tmp.to_string_lossy()])
            .status()
            .map_err(|e| format!("screencapture: {e}"))?;
        if !status.success() {
            let _ = std::fs::remove_file(&tmp);
            return Err(
                "Screen Recording permission is required for region OCR (System Settings → Privacy)"
                    .into(),
            );
        }
        if !tmp.is_file() {
            return Err("capture cancelled".into());
        }
        progress.store(40, Ordering::Relaxed);
        let result = macos::ocr_image(&tmp, progress);
        let _ = std::fs::remove_file(&tmp);
        result
    }
}

pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return macos::copy_to_clipboard(text);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub fn persons_only() -> bool {
    macos::os_major() < 14
}

#[cfg(not(target_os = "macos"))]
pub fn persons_only() -> bool {
    true
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::CString;

    #[link(name = "Vision", kind = "framework")]
    #[link(name = "CoreImage", kind = "framework")]
    #[link(name = "PDFKit", kind = "framework")]
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    pub fn os_major() -> u64 {
        unsafe {
            let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
            let s: *mut AnyObject = msg_send![info, operatingSystemVersionString];
            let utf8: *const i8 = msg_send![s, UTF8String];
            if utf8.is_null() {
                return 13;
            }
            let text = std::ffi::CStr::from_ptr(utf8).to_string_lossy();
            text.split_whitespace()
                .find_map(|w| w.split('.').next()?.parse().ok())
                .unwrap_or(13)
        }
    }

    fn ns_string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).unwrap_or_else(|_| CString::new("").unwrap());
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }

    fn ns_url(path: &Path) -> *mut AnyObject {
        let s = ns_string(&path.to_string_lossy());
        unsafe { msg_send![class!(NSURL), fileURLWithPath: s] }
    }

    pub fn pdf_embedded_text(input: &Path) -> Option<String> {
        unsafe {
            let url = ns_url(input);
            let doc: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
            let doc: *mut AnyObject = msg_send![doc, initWithURL: url];
            if doc.is_null() {
                return None;
            }
            let s: *mut AnyObject = msg_send![doc, string];
            ns_to_string(s)
        }
    }

    pub fn ocr_pdf_pages(
        input: &Path,
        progress: &AtomicU8,
        cancel: &AtomicBool,
    ) -> Result<String, String> {
        unsafe {
            let url = ns_url(input);
            let doc: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
            let doc: *mut AnyObject = msg_send![doc, initWithURL: url];
            if doc.is_null() {
                return Err("PDFKit could not open the document".into());
            }
            let pages: usize = msg_send![doc, pageCount];
            let mut out = String::new();
            for i in 0..pages {
                if cancel.load(Ordering::SeqCst) {
                    return Err("cancelled".into());
                }
                let page: *mut AnyObject = msg_send![doc, pageAtIndex: i];
                if page.is_null() {
                    continue;
                }
                let size = CGSize {
                    width: 1700.0,
                    height: 2200.0,
                };
                let image: *mut AnyObject = msg_send![page, thumbnailOfSize: size, forBox: 0i64];
                if image.is_null() {
                    continue;
                }
                if let Ok(text) = ocr_nsimage(image) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&text);
                }
                let pct = 20 + ((i + 1) * 70 / pages.max(1)) as u8;
                progress.store(pct.min(99), Ordering::Relaxed);
            }
            progress.store(100, Ordering::Relaxed);
            Ok(out)
        }
    }

    pub fn ocr_image(input: &Path, progress: &AtomicU8) -> Result<String, String> {
        progress.store(30, Ordering::Relaxed);
        unsafe {
            let url = ns_url(input);
            let handler: *mut AnyObject = msg_send![class!(VNImageRequestHandler), alloc];
            let handler: *mut AnyObject = msg_send![
                handler,
                initWithURL: url,
                options: empty_dict()
            ];
            if handler.is_null() {
                return Err("Vision could not open the image".into());
            }
            let text = perform_ocr(handler)?;
            progress.store(100, Ordering::Relaxed);
            Ok(text)
        }
    }

    unsafe fn ocr_nsimage(image: *mut AnyObject) -> Result<String, String> {
        let cg: *mut AnyObject = msg_send![image, CGImage];
        if cg.is_null() {
            return Err("no CGImage".into());
        }
        let handler: *mut AnyObject = msg_send![class!(VNImageRequestHandler), alloc];
        let handler: *mut AnyObject =
            msg_send![handler, initWithCGImage: cg, options: empty_dict()];
        if handler.is_null() {
            return Err("Vision handler".into());
        }
        perform_ocr(handler)
    }

    unsafe fn perform_ocr(handler: *mut AnyObject) -> Result<String, String> {
        let req: *mut AnyObject = msg_send![class!(VNRecognizeTextRequest), alloc];
        let req: *mut AnyObject = msg_send![req, init];
        if req.is_null() {
            return Err("VNRecognizeTextRequest".into());
        }
        // VNRequestTextRecognitionLevelAccurate = 0
        let _: () = msg_send![req, setRecognitionLevel: 0i64];
        let _: () = msg_send![req, setUsesLanguageCorrection: true];
        if os_major() >= 13 {
            let _: () = msg_send![req, setAutomaticallyDetectsLanguage: true];
        }
        let requests: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: req];
        let mut err: *mut AnyObject = std::ptr::null_mut();
        let ok: bool = msg_send![handler, performRequests: requests, error: &mut err];
        if !ok {
            return Err("Vision text request failed".into());
        }
        let results: *mut AnyObject = msg_send![req, results];
        if results.is_null() {
            return Ok(String::new());
        }
        let count: usize = msg_send![results, count];
        let mut text = String::new();
        for i in 0..count {
            let obs: *mut AnyObject = msg_send![results, objectAtIndex: i];
            let cands: *mut AnyObject = msg_send![obs, topCandidates: 1usize];
            if cands.is_null() {
                continue;
            }
            let n: usize = msg_send![cands, count];
            if n == 0 {
                continue;
            }
            let cand: *mut AnyObject = msg_send![cands, objectAtIndex: 0usize];
            let s: *mut AnyObject = msg_send![cand, string];
            if let Some(line) = ns_to_string(s) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&line);
            }
        }
        Ok(text)
    }

    pub fn remove_background(
        input: &Path,
        output: &Path,
        progress: &AtomicU8,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        progress.store(20, Ordering::Relaxed);
        unsafe {
            let url = ns_url(input);
            let handler: *mut AnyObject = msg_send![class!(VNImageRequestHandler), alloc];
            let handler: *mut AnyObject =
                msg_send![handler, initWithURL: url, options: empty_dict()];
            if handler.is_null() {
                return Err("Vision could not open the image".into());
            }
            if os_major() >= 14 {
                lift_subject(handler, output, progress)
            } else {
                person_seg(handler, output, progress)
            }
        }
    }

    unsafe fn lift_subject(
        handler: *mut AnyObject,
        output: &Path,
        progress: &AtomicU8,
    ) -> Result<(), String> {
        let req: *mut AnyObject = msg_send![class!(VNGenerateForegroundInstanceMaskRequest), alloc];
        let req: *mut AnyObject = msg_send![req, init];
        if req.is_null() {
            return person_seg(handler, output, progress);
        }
        let requests: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: req];
        let mut err: *mut AnyObject = std::ptr::null_mut();
        let ok: bool = msg_send![handler, performRequests: requests, error: &mut err];
        if !ok {
            return person_seg(handler, output, progress);
        }
        progress.store(70, Ordering::Relaxed);
        let results: *mut AnyObject = msg_send![req, results];
        let count: usize = if results.is_null() {
            0
        } else {
            msg_send![results, count]
        };
        if count == 0 {
            return Err("no subject to lift".into());
        }
        let obs: *mut AnyObject = msg_send![results, objectAtIndex: 0usize];
        let instances: *mut AnyObject = msg_send![obs, allInstances];
        let mut mask_err: *mut AnyObject = std::ptr::null_mut();
        let buffer: *mut AnyObject = msg_send![
            obs,
            generateMaskedImageOfInstances: instances,
            fromRequestHandler: handler,
            croppedToInstancesExtent: true,
            error: &mut mask_err
        ];
        if buffer.is_null() {
            return Err("mask apply failed".into());
        }
        write_pixel_buffer_png(buffer, output)?;
        progress.store(100, Ordering::Relaxed);
        Ok(())
    }

    unsafe fn person_seg(
        handler: *mut AnyObject,
        output: &Path,
        progress: &AtomicU8,
    ) -> Result<(), String> {
        let req: *mut AnyObject = msg_send![class!(VNGeneratePersonSegmentationRequest), alloc];
        let req: *mut AnyObject = msg_send![req, init];
        if req.is_null() {
            return Err("person segmentation unavailable".into());
        }
        // VNGeneratePersonSegmentationRequestQualityLevelAccurate = 2
        let _: () = msg_send![req, setQualityLevel: 2i64];
        let requests: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: req];
        let mut err: *mut AnyObject = std::ptr::null_mut();
        let ok: bool = msg_send![handler, performRequests: requests, error: &mut err];
        if !ok {
            return Err("person segmentation failed (persons-only on macOS 12–13)".into());
        }
        progress.store(70, Ordering::Relaxed);
        let results: *mut AnyObject = msg_send![req, results];
        let count: usize = if results.is_null() {
            0
        } else {
            msg_send![results, count]
        };
        if count == 0 {
            return Err("no person in the image".into());
        }
        let obs: *mut AnyObject = msg_send![results, objectAtIndex: 0usize];
        let mask: *mut AnyObject = msg_send![obs, pixelBuffer];
        if mask.is_null() {
            return Err("empty person mask".into());
        }
        blend_and_write(handler, mask, output)?;
        progress.store(100, Ordering::Relaxed);
        Ok(())
    }

    unsafe fn blend_and_write(
        handler: *mut AnyObject,
        mask: *mut AnyObject,
        output: &Path,
    ) -> Result<(), String> {
        let src: *mut AnyObject = msg_send![class!(CIImage), imageWithCVPixelBuffer: {
            let buf: *mut AnyObject = msg_send![handler, valueForKey: ns_string("pixelBuffer")];
            buf
        }];
        // Prefer the original file via handler URL.
        let url: *mut AnyObject = msg_send![handler, valueForKey: ns_string("imageURL")];
        let image: *mut AnyObject = if !url.is_null() {
            msg_send![class!(CIImage), imageWithContentsOfURL: url]
        } else {
            src
        };
        if image.is_null() {
            return Err("CoreImage could not load the source".into());
        }
        let mask_ci: *mut AnyObject = msg_send![class!(CIImage), imageWithCVPixelBuffer: mask];
        if mask_ci.is_null() {
            return Err("mask CIImage".into());
        }
        let filter: *mut AnyObject =
            msg_send![class!(CIFilter), filterWithName: ns_string("CIBlendWithMask")];
        if filter.is_null() {
            return Err("CIBlendWithMask".into());
        }
        let _: () = msg_send![filter, setValue: image, forKey: ns_string("inputImage")];
        let _: () = msg_send![filter, setValue: mask_ci, forKey: ns_string("inputMaskImage")];
        let out: *mut AnyObject = msg_send![filter, outputImage];
        if out.is_null() {
            return Err("blend produced no image".into());
        }
        write_ciimage_png(out, output)
    }

    unsafe fn write_pixel_buffer_png(
        buffer: *mut AnyObject,
        output: &Path,
    ) -> Result<(), String> {
        let image: *mut AnyObject = msg_send![class!(CIImage), imageWithCVPixelBuffer: buffer];
        if image.is_null() {
            return Err("CIImage from mask".into());
        }
        write_ciimage_png(image, output)
    }

    unsafe fn write_ciimage_png(image: *mut AnyObject, output: &Path) -> Result<(), String> {
        let ctx: *mut AnyObject = msg_send![class!(CIContext), context];
        if ctx.is_null() {
            return Err("CIContext".into());
        }
        let color: *mut AnyObject =
            msg_send![class!(CGColorSpace), createDeviceRGB];
        let url = ns_url(output);
        let format: i32 = 0x4247_5241; // kCIFormatBGRA8-ish; use PNG representation API
        let mut err: *mut AnyObject = std::ptr::null_mut();
        let ok: bool = msg_send![
            ctx,
            writePNGRepresentationOfImage: image,
            toURL: url,
            format: format,
            colorSpace: color,
            options: empty_dict(),
            error: &mut err
        ];
        if !ok {
            return Err("failed to write PNG".into());
        }
        Ok(())
    }

    pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
        unsafe {
            let pb: *mut AnyObject = msg_send![class!(NSPasteboard), generalPasteboard];
            if pb.is_null() {
                return Err("pasteboard unavailable".into());
            }
            let _: isize = msg_send![pb, clearContents];
            let s = ns_string(text);
            let ty = ns_string("public.utf8-plain-text");
            let ok: bool = msg_send![pb, setString: s, forType: ty];
            if !ok {
                return Err("could not copy text".into());
            }
            Ok(())
        }
    }

    unsafe fn empty_dict() -> *mut AnyObject {
        msg_send![class!(NSDictionary), dictionary]
    }

    fn ns_to_string(s: *mut AnyObject) -> Option<String> {
        if s.is_null() {
            return None;
        }
        unsafe {
            let utf8: *const i8 = msg_send![s, UTF8String];
            if utf8.is_null() {
                return None;
            }
            Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
        }
    }

    #[repr(C)]
    struct CGSize {
        width: f64,
        height: f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_host_degrades_vision() {
        if cfg!(target_os = "macos") {
            return;
        }
        let progress = AtomicU8::new(0);
        let cancel = AtomicBool::new(false);
        assert!(ocr(Path::new("/tmp/x.png"), &progress, &cancel).is_err());
        assert!(ocr(Path::new("/tmp/x.pdf"), &progress, &cancel).is_err());
        assert!(remove_background(
            Path::new("/tmp/x.png"),
            Path::new("/tmp/x-nobg.png"),
            &progress,
            &cancel
        )
        .is_err());
        assert!(ocr_screen_region(&progress, &cancel).is_err());
        assert!(persons_only());
    }

    #[test]
    fn clipboard_is_a_no_op_off_macos() {
        if cfg!(target_os = "macos") {
            return;
        }
        assert!(copy_to_clipboard("hello").is_ok());
    }
}
