/// Simple base64 encoding using the base64 crate
pub fn base64_encode(data: &[u8]) -> String {
    use base64::prelude::*;
    BASE64_STANDARD.encode(data)
}

/// Fetch artwork from a URL (used for Spotify) - async version
pub async fn fetch_artwork_from_url(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }

    // Use async reqwest client to avoid runtime conflicts
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()?;

    let response = client.get(url).send().await.ok()?;

    if response.status().is_success() {
        let bytes = response.bytes().await.ok()?;
        Some(base64_encode(&bytes))
    } else {
        None
    }
}

/// Save binary data to a temp file and return as base64-encoded string
pub fn save_temp_file(data: &[u8], _extension: &str) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    Some(base64_encode(data))
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

/// Get the system accent color on Windows
#[cfg(target_os = "windows")]
pub fn get_windows_accent_color() -> String {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(dwm) = hkcu.open_subkey("SOFTWARE\\Microsoft\\Windows\\DWM") {
        if let Ok(color) = dwm.get_value::<u32, _>("ColorizationColor") {
            // Color is in ARGB format (alpha, red, green, blue)
            let r = (color >> 16) & 0xFF;
            let g = (color >> 8) & 0xFF;
            let b = color & 0xFF;
            return format!("#{:02x}{:02x}{:02x}", r, g, b);
        }
    }
    "#007AFF".to_string()
}
