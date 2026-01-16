/// Simple base64 encoding using the base64 crate
pub fn base64_encode(data: &[u8]) -> String {
    use base64::prelude::*;
    BASE64_STANDARD.encode(data)
}

/// Save data to a temporary file and return the path
pub fn save_temp_file(data: &[u8], extension: &str) -> Option<String> {
    use std::fs::File;
    use std::io::Write;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let temp_dir = std::env::temp_dir();

    // Hash the data to create a unique filename (deduplication)
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    let hash = hasher.finish();

    let filename = format!("overdone_art_{:x}.{}", hash, extension);
    let path = temp_dir.join(filename);

    // If file exists, return path (cache hit)
    if path.exists() {
        return Some(path.to_string_lossy().to_string());
    }

    if let Ok(mut file) = File::create(&path) {
        if file.write_all(data).is_ok() {
            return Some(path.to_string_lossy().to_string());
        }
    }
    None
}

/// Fetch artwork from a URL (used for Spotify) and save to temp file
pub fn fetch_artwork_from_url(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }

    // Use reqwest blocking client to fetch the image
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()?;

    let response = client.get(url).send().ok()?;

    if response.status().is_success() {
        let bytes = response.bytes().ok()?;
        // Save to temp file instead of returning base64
        save_temp_file(&bytes, "jpg")
    } else {
        None
    }
}

/// Get the system accent color on macOS
#[cfg(target_os = "macos")]
pub fn get_macos_accent_color() -> String {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    // Default to Apple Blue if anything fails
    let default_color = "#007AFF".to_string();

    unsafe {
        // Get NSColor.controlAccentColor
        let color_class = class!(NSColor);
        let accent_color: *mut AnyObject = msg_send![color_class, controlAccentColor];

        if accent_color.is_null() {
            return default_color;
        }

        // Convert to SRGB color space to ensure components are valid
        // colorUsingColorSpace: [NSColorSpace sRGBColorSpace]
        let color_space_class = class!(NSColorSpace);
        let srgb_space: *mut AnyObject = msg_send![color_space_class, sRGBColorSpace];
        let srgb_color: *mut AnyObject = msg_send![accent_color, colorUsingColorSpace: srgb_space];

        if srgb_color.is_null() {
            return default_color;
        }

        // Get RGB components
        type CGFloat = f64;
        let mut r: CGFloat = 0.0;
        let mut g: CGFloat = 0.0;
        let mut b: CGFloat = 0.0;
        let mut a: CGFloat = 0.0;

        // getRed:green:blue:alpha:
        let _: () =
            msg_send![srgb_color, getRed: &mut r, green: &mut g, blue: &mut b, alpha: &mut a];

        // Format as hex string
        return format!(
            "#{:02X}{:02X}{:02X}",
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8
        );
    }
}
