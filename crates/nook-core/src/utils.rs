/// Encode binary data as a base64 string (album artwork).
pub fn encode_bytes_base64(data: &[u8]) -> Option<String> {
    use base64::prelude::*;
    if data.is_empty() {
        return None;
    }
    Some(BASE64_STANDARD.encode(data))
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
        encode_bytes_base64(&bytes)
    } else {
        None
    }
}

/// Run `/usr/bin/osascript`. Shared by media controls and iMessage send.
pub fn run_osascript(script: &str) -> Result<String, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = script;
        return Err("osascript is only available on macOS".into());
    }
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| e.to_string())?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            log::warn!("osascript failed ({}): {stderr}", output.status);
            if stderr.contains("-1743") || stderr.to_lowercase().contains("not allowed") {
                log::warn!(
                    "Automation permission denied. Grant access in System Settings → Privacy & Security → Automation."
                );
            }
            return Err(format!("osascript failed: {}", output.status));
        }
        if !stderr.is_empty() {
            log::debug!("osascript stderr: {stderr}");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
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
