//! Image convert via the `image` crate (png/jpeg/gif/webp/tiff/bmp) plus
//! ImageIO on macOS for HEIC encode/decode. ImageIO can decode webp/avif
//! but cannot encode them — webp encode stays on the Rust crate; avif encode
//! is out of scope.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

const RUST_ENCODE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "tif", "tiff", "bmp"];
const HEIC: &[&str] = &["heic", "heif"];

pub fn can_encode(ext: &str, heic_ok: bool) -> bool {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    RUST_ENCODE.contains(&ext.as_str()) || (heic_ok && HEIC.contains(&ext.as_str()))
}

pub fn convert(
    input: &Path,
    output: &Path,
    format: &str,
    jpeg_quality: u8,
    progress: &AtomicU8,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    progress.store(10, Ordering::Relaxed);
    let format = format.trim_start_matches('.').to_ascii_lowercase();
    let src_ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if HEIC.contains(&src_ext.as_str()) || HEIC.contains(&format.as_str()) {
        #[cfg(target_os = "macos")]
        {
            return imageio::convert(input, output, &format, jpeg_quality, progress);
        }
        #[cfg(not(target_os = "macos"))]
        {
            if HEIC.contains(&src_ext.as_str()) {
                return Err("HEIC needs ImageIO on macOS".into());
            }
            if HEIC.contains(&format.as_str()) {
                return Err("HEIC encode needs ImageIO on macOS".into());
            }
        }
    }

    if format == "avif" {
        return Err("AVIF encode is out of scope (ImageIO decode-only)".into());
    }
    if !can_encode(&format, false) {
        return Err(format!("cannot encode .{format}"));
    }

    progress.store(30, Ordering::Relaxed);
    let img = image::open(input).map_err(|e| format!("decode: {e}"))?;
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    progress.store(70, Ordering::Relaxed);
    write_rust(&img, output, &format, jpeg_quality)?;
    progress.store(100, Ordering::Relaxed);
    Ok(())
}

fn write_rust(
    img: &image::DynamicImage,
    output: &Path,
    format: &str,
    jpeg_quality: u8,
) -> Result<(), String> {
    match format {
        "jpg" | "jpeg" => {
            let file = std::fs::File::create(output).map_err(|e| e.to_string())?;
            let rgb = img.to_rgb8();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(file, jpeg_quality);
            enc.encode(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| format!("jpeg: {e}"))
        }
        "png" => img
            .save_with_format(output, image::ImageFormat::Png)
            .map_err(|e| format!("encode: {e}")),
        "gif" => img
            .save_with_format(output, image::ImageFormat::Gif)
            .map_err(|e| format!("encode: {e}")),
        "webp" => img
            .save_with_format(output, image::ImageFormat::WebP)
            .map_err(|e| format!("encode: {e}")),
        "tif" | "tiff" => img
            .save_with_format(output, image::ImageFormat::Tiff)
            .map_err(|e| format!("encode: {e}")),
        "bmp" => img
            .save_with_format(output, image::ImageFormat::Bmp)
            .map_err(|e| format!("encode: {e}")),
        other => Err(format!("cannot encode .{other}")),
    }
}

#[cfg(target_os = "macos")]
mod imageio {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    #[link(name = "ImageIO", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const std::ffi::c_void);
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const std::ffi::c_void,
            buffer: *const u8,
            size: isize,
            is_directory: u8,
        ) -> *mut std::ffi::c_void;
        fn CFStringCreateWithCString(
            alloc: *const std::ffi::c_void,
            c_str: *const i8,
            encoding: u32,
        ) -> *mut std::ffi::c_void;
        fn CGImageSourceCreateWithURL(
            url: *const std::ffi::c_void,
            options: *const std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CGImageSourceCreateImageAtIndex(
            source: *mut std::ffi::c_void,
            index: usize,
            options: *const std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CGImageDestinationCreateWithURL(
            url: *const std::ffi::c_void,
            ty: *const std::ffi::c_void,
            count: usize,
            options: *const std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CGImageDestinationAddImage(
            dest: *mut std::ffi::c_void,
            image: *mut std::ffi::c_void,
            properties: *const std::ffi::c_void,
        );
        fn CGImageDestinationFinalize(dest: *mut std::ffi::c_void) -> u8;
    }

    const UTF8: u32 = 0x0800_0100;

    fn cf_url(path: &Path) -> Result<*mut std::ffi::c_void, String> {
        let bytes = path.as_os_str().as_bytes();
        let url = unsafe {
            CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                bytes.as_ptr(),
                bytes.len() as isize,
                0,
            )
        };
        if url.is_null() {
            Err(format!("ImageIO URL: {}", path.display()))
        } else {
            Ok(url)
        }
    }

    fn uti_for(format: &str) -> &'static str {
        match format {
            "heic" | "heif" => "public.heic",
            "jpg" | "jpeg" => "public.jpeg",
            "png" => "public.png",
            "gif" => "com.compuserve.gif",
            "tif" | "tiff" => "public.tiff",
            "bmp" => "com.microsoft.bmp",
            _ => "public.png",
        }
    }

    pub fn convert(
        input: &Path,
        output: &Path,
        format: &str,
        _jpeg_quality: u8,
        progress: &AtomicU8,
    ) -> Result<(), String> {
        progress.store(20, Ordering::Relaxed);
        let src = cf_url(input)?;
        let dst = cf_url(output)?;
        let uti = CString::new(uti_for(format)).map_err(|e| e.to_string())?;
        let ty = unsafe { CFStringCreateWithCString(std::ptr::null(), uti.as_ptr(), UTF8) };
        if ty.is_null() {
            unsafe {
                CFRelease(src);
                CFRelease(dst);
            }
            return Err("ImageIO UTI".into());
        }
        let source = unsafe { CGImageSourceCreateWithURL(src, std::ptr::null()) };
        if source.is_null() {
            unsafe {
                CFRelease(src);
                CFRelease(dst);
                CFRelease(ty);
            }
            return Err("ImageIO could not open the file".into());
        }
        progress.store(50, Ordering::Relaxed);
        let image = unsafe { CGImageSourceCreateImageAtIndex(source, 0, std::ptr::null()) };
        if image.is_null() {
            unsafe {
                CFRelease(source);
                CFRelease(src);
                CFRelease(dst);
                CFRelease(ty);
            }
            return Err("ImageIO could not decode the image".into());
        }
        let dest = unsafe { CGImageDestinationCreateWithURL(dst, ty, 1, std::ptr::null()) };
        if dest.is_null() {
            unsafe {
                CFRelease(image);
                CFRelease(source);
                CFRelease(src);
                CFRelease(dst);
                CFRelease(ty);
            }
            return Err(format!("ImageIO cannot encode .{format}"));
        }
        progress.store(80, Ordering::Relaxed);
        unsafe {
            CGImageDestinationAddImage(dest, image, std::ptr::null());
            let ok = CGImageDestinationFinalize(dest);
            CFRelease(dest);
            CFRelease(image);
            CFRelease(source);
            CFRelease(src);
            CFRelease(dst);
            CFRelease(ty);
            if ok == 0 {
                return Err("ImageIO finalize failed".into());
            }
        }
        progress.store(100, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_formats_encode_without_heic() {
        for ext in ["png", "jpeg", "jpg", "webp", "gif", "tiff", "bmp"] {
            assert!(can_encode(ext, false), "{ext}");
        }
        assert!(!can_encode("heic", false));
        assert!(can_encode("heic", true));
        assert!(!can_encode("avif", true));
        assert!(!can_encode("mkv", true));
    }

    #[test]
    fn convert_png_to_jpeg_round_trip() {
        let dir = std::env::temp_dir().join(format!("nook-img-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("in.png");
        let dst = dir.join("out.jpg");
        let img = image::RgbImage::from_pixel(8, 8, image::Rgb([20, 80, 160]));
        image::DynamicImage::ImageRgb8(img)
            .save(&src)
            .expect("write png");
        let progress = AtomicU8::new(0);
        let cancel = AtomicBool::new(false);
        convert(&src, &dst, "jpeg", 80, &progress, &cancel).expect("convert");
        assert!(dst.is_file());
        assert_eq!(progress.load(Ordering::Relaxed), 100);
        let decoded = image::open(&dst).expect("jpeg opens");
        assert_eq!(decoded.width(), 8);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn avif_encode_is_rejected() {
        let progress = AtomicU8::new(0);
        let cancel = AtomicBool::new(false);
        let err = convert(
            Path::new("/tmp/x.png"),
            Path::new("/tmp/x.avif"),
            "avif",
            80,
            &progress,
            &cancel,
        )
        .unwrap_err();
        assert!(err.to_ascii_lowercase().contains("avif"));
    }
}
