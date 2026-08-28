//! PDF compress via PDFKit (macOS 13+ JPEG/optimize), QuartzFilter fallback,
//! then rasterize-as-last-resort. Never Ghostscript (AGPL) or qpdf.

use crate::settings::{FileActionsSettings, PdfPreset};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub fn compress(
    input: &Path,
    output: &Path,
    preset: PdfPreset,
    settings: &FileActionsSettings,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    let _ = settings;
    progress.store(10, Ordering::Relaxed);

    #[cfg(target_os = "macos")]
    {
        return macos::compress(input, output, preset, progress, cancel);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (input, output, preset);
        Err("PDF compress needs PDFKit on macOS".into())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use crate::notch::CGSize;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ffi::CString;

    #[link(name = "PDFKit", kind = "framework")]
    #[link(name = "Quartz", kind = "framework")]
    extern "C" {}

    fn ns_string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).unwrap_or_else(|_| CString::new("").unwrap());
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }

    fn ns_url(path: &Path) -> *mut AnyObject {
        let s = ns_string(&path.to_string_lossy());
        unsafe { msg_send![class!(NSURL), fileURLWithPath: s] }
    }

    fn os_major() -> u64 {
        unsafe {
            let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
            let ver: *mut AnyObject = msg_send![info, operatingSystemVersion];
            let _ = ver;
            // operatingSystemVersion is a struct; read via NSOperatingSystemVersion fields
            // through processInfo.operatingSystemVersionString as a fallback parse.
            let s: *mut AnyObject = msg_send![info, operatingSystemVersionString];
            let utf8: *const i8 = msg_send![s, UTF8String];
            if utf8.is_null() {
                return 13;
            }
            let text = std::ffi::CStr::from_ptr(utf8).to_string_lossy();
            // "Version 14.6.1 (Build …)"
            text.split_whitespace()
                .find_map(|w| w.split('.').next()?.parse().ok())
                .unwrap_or(13)
        }
    }

    pub fn compress(
        input: &Path,
        output: &Path,
        preset: PdfPreset,
        progress: &AtomicU8,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        match preset {
            PdfPreset::Raster => rasterize(input, output, progress, cancel),
            PdfPreset::Screen | PdfPreset::Print => {
                if os_major() >= 13 {
                    if write_with_options(input, output, preset, progress).is_ok() {
                        return Ok(());
                    }
                }
                progress.store(40, Ordering::Relaxed);
                if quartz_filter(input, output, progress).is_ok() {
                    return Ok(());
                }
                progress.store(60, Ordering::Relaxed);
                rasterize(input, output, progress, cancel)
            }
        }
    }

    fn write_with_options(
        input: &Path,
        output: &Path,
        preset: PdfPreset,
        progress: &AtomicU8,
    ) -> Result<(), String> {
        unsafe {
            let url = ns_url(input);
            let doc: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
            let doc: *mut AnyObject = msg_send![doc, initWithURL: url];
            if doc.is_null() {
                return Err("PDFKit could not open the document".into());
            }
            progress.store(35, Ordering::Relaxed);
            let jpeg = ns_string("PDFDocumentSaveImagesAsJPEGOption");
            let optimize = ns_string("PDFDocumentOptimizeImagesForScreenOption");
            let yes: *mut AnyObject = msg_send![class!(NSNumber), numberWithBool: true];
            let keys = [jpeg, optimize];
            let vals = [yes, yes];
            let opts: *mut AnyObject = msg_send![
                class!(NSDictionary),
                dictionaryWithObjects: vals.as_ptr(),
                forKeys: keys.as_ptr(),
                count: if matches!(preset, PdfPreset::Screen) { 2usize } else { 1usize }
            ];
            let out = ns_url(output);
            let ok: bool = msg_send![doc, writeToURL: out, withOptions: opts];
            if !ok {
                return Err("PDFKit write failed".into());
            }
            progress.store(100, Ordering::Relaxed);
            Ok(())
        }
    }

    fn quartz_filter(
        input: &Path,
        output: &Path,
        progress: &AtomicU8,
    ) -> Result<(), String> {
        unsafe {
            let filter_path = ns_string("/System/Library/Filters/Reduce File Size.qfilter");
            let filter_url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: filter_path];
            let filter: *mut AnyObject =
                msg_send![class!(QuartzFilter), quartzFilterWithURL: filter_url];
            if filter.is_null() {
                return Err("QuartzFilter unavailable".into());
            }
            let url = ns_url(input);
            let doc: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
            let doc: *mut AnyObject = msg_send![doc, initWithURL: url];
            if doc.is_null() {
                return Err("PDFKit could not open the document".into());
            }
            let key = ns_string("QuartzFilter");
            let keys = [key];
            let vals = [filter];
            let opts: *mut AnyObject = msg_send![
                class!(NSDictionary),
                dictionaryWithObjects: vals.as_ptr(),
                forKeys: keys.as_ptr(),
                count: 1usize
            ];
            let out = ns_url(output);
            let ok: bool = msg_send![doc, writeToURL: out, withOptions: opts];
            if !ok {
                return Err("QuartzFilter write failed".into());
            }
            progress.store(100, Ordering::Relaxed);
            Ok(())
        }
    }

    /// Last resort: render pages to JPEG and rebuild. Loses selectable text.
    fn rasterize(
        input: &Path,
        output: &Path,
        progress: &AtomicU8,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        unsafe {
            let url = ns_url(input);
            let doc: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
            let doc: *mut AnyObject = msg_send![doc, initWithURL: url];
            if doc.is_null() {
                return Err("PDFKit could not open the document".into());
            }
            let pages: usize = msg_send![doc, pageCount];
            if pages == 0 {
                return Err("empty PDF".into());
            }
            let out_doc: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
            let out_doc: *mut AnyObject = msg_send![out_doc, init];
            if out_doc.is_null() {
                return Err("PDFKit alloc failed".into());
            }
            for i in 0..pages {
                if cancel.load(Ordering::SeqCst) {
                    return Err("cancelled".into());
                }
                let page: *mut AnyObject = msg_send![doc, pageAtIndex: i];
                if page.is_null() {
                    continue;
                }
                // thumbnailOfSize: at ~120 dpi-ish; 612×792 letter @ 150 dpi ≈ 1275×1650
                let size = CGSize {
                    width: 1275.0,
                    height: 1650.0,
                };
                let image: *mut AnyObject = msg_send![page, thumbnailOfSize: size, forBox: 0i64];
                if image.is_null() {
                    continue;
                }
                let new_page: *mut AnyObject = msg_send![class!(PDFPage), alloc];
                let new_page: *mut AnyObject = msg_send![new_page, initWithImage: image];
                if !new_page.is_null() {
                    let _: () = msg_send![out_doc, insertPage: new_page, atIndex: i];
                }
                let pct = 20 + ((i + 1) * 70 / pages.max(1)) as u8;
                progress.store(pct.min(95), Ordering::Relaxed);
            }
            let out = ns_url(output);
            let ok: bool = msg_send![out_doc, writeToURL: out];
            if !ok {
                return Err("rasterized PDF write failed".into());
            }
            progress.store(100, Ordering::Relaxed);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_host_degrades_without_pdfkit() {
        if cfg!(target_os = "macos") {
            return;
        }
        let progress = AtomicU8::new(0);
        let cancel = AtomicBool::new(false);
        let err = compress(
            Path::new("/tmp/doc.pdf"),
            Path::new("/tmp/doc-out.pdf"),
            PdfPreset::Screen,
            &FileActionsSettings::default(),
            &progress,
            &cancel,
        )
        .unwrap_err();
        assert!(err.to_ascii_lowercase().contains("pdfkit") || err.contains("macOS"));
    }
}
