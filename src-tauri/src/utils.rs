/// Simple base64 encoding
pub fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::new();
    let chunks = data.chunks(3);

    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;

        let n = (b0 << 16) | (b1 << 8) | b2;

        result.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        result.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(ALPHABET[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

/// Fetch artwork from a URL (used for Spotify)
#[cfg(target_os = "macos")]
pub fn fetch_artwork_from_url(url: &str) -> Option<String> {
    use std::process::Command;

    if url.is_empty() {
        return None;
    }

    // Use curl to fetch the image and convert to base64
    let output = Command::new("curl")
        .args(["-s", "-L", "--max-time", "2", url])
        .output()
        .ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        // Encode to base64
        let base64 = base64_encode(&output.stdout);
        Some(base64)
    } else {
        None
    }
}
