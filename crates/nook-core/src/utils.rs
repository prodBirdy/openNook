/// Simple base64 encoding using the base64 crate
pub fn base64_encode(data: &[u8]) -> String {
    use base64::prelude::*;
    BASE64_STANDARD.encode(data)
}

pub async fn read_response_limited(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    use futures_util::StreamExt;

    if response
        .content_length()
        .is_some_and(|len| len > max_bytes as u64)
    {
        return Err(format!("response body exceeds {max_bytes} bytes"));
    }
    let mut body =
        Vec::with_capacity(response.content_length().unwrap_or(0).min(max_bytes as u64) as usize);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| err.to_string())?;
        if chunk.len() > max_bytes.saturating_sub(body.len()) {
            return Err(format!("response body exceeds {max_bytes} bytes"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Fetch artwork from a URL (used for Spotify) - async version
pub async fn fetch_artwork_from_url(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }

    // Use async reqwest client to avoid runtime conflicts
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;

    let response = client.get(url).send().await.ok()?;

    if response.status().is_success()
        && response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("image/"))
    {
        let bytes = read_response_limited(response, 5 * 1024 * 1024)
            .await
            .ok()?;
        Some(base64_encode(&bytes))
    } else {
        None
    }
}

/// Encode binary data as a base64 string (used for album artwork).
pub fn encode_bytes_base64(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    Some(base64_encode(data))
}

/// Back-compat alias used by the Windows Now Playing path.
pub fn save_temp_file(data: &[u8], _extension: &str) -> Option<String> {
    encode_bytes_base64(data)
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
        format!(
            "#{:02X}{:02X}{:02X}",
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[tokio::test]
    async fn response_body_aborts_at_the_byte_cap() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\nto\r\n2\r\nol\r\n0\r\n\r\n",
            );
        });
        let response = reqwest::get(format!("http://{addr}")).await.unwrap();
        assert!(read_response_limited(response, 3).await.is_err());
    }

    #[tokio::test]
    async fn normal_artwork_body_remains_functional() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 3\r\nConnection: close\r\n\r\npng",
            );
        });
        assert_eq!(
            fetch_artwork_from_url(&format!("http://{addr}"))
                .await
                .as_deref(),
            Some("cG5n")
        );
    }
}
