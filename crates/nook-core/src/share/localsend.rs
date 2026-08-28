//! LocalSend protocol v2 send-only client.
//!
//! Discovery: one multicast announce on 224.0.0.167:53317 plus a ~3 s listen
//! for UDP replies and HTTP `/register`. Every socket is closed afterwards.
//! Uploads use prepare-upload + raw-body POST `/upload`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use uuid::Uuid;

pub const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);
pub const DEFAULT_PORT: u16 = 53317;
pub const PROTOCOL_VERSION: &str = "2.1";
pub const DISCOVER_WINDOW: Duration = Duration::from_secs(3);

const API: &str = "/api/localsend/v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub alias: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "deviceModel")]
    pub device_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "deviceType")]
    pub device_type: Option<String>,
    pub fingerprint: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_https")]
    pub protocol: String,
    #[serde(default)]
    pub download: bool,
    #[serde(default)]
    pub announce: bool,
    #[serde(default, skip)]
    pub ip: String,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_https() -> String {
    "https".into()
}

impl DeviceInfo {
    pub fn local(alias: impl Into<String>, fingerprint: impl Into<String>, port: u16) -> Self {
        Self {
            alias: alias.into(),
            version: PROTOCOL_VERSION.into(),
            device_model: Some("openNook".into()),
            device_type: Some("desktop".into()),
            fingerprint: fingerprint.into(),
            port,
            // Send-only announce uses HTTP so peers can POST /register to the
            // short-lived listener. We never advertise a standing HTTPS server.
            protocol: "http".into(),
            download: false,
            announce: true,
            ip: String::new(),
        }
    }

    pub fn origin(&self) -> String {
        let scheme = if self.protocol.eq_ignore_ascii_case("http") {
            "http"
        } else {
            "https"
        };
        format!("{scheme}://{}:{}", self.ip, self.port)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMeta {
    pub id: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    pub size: u64,
    #[serde(rename = "fileType")]
    pub file_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareUploadRequest {
    pub info: DeviceInfo,
    pub files: HashMap<String, FileMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareUploadResponse {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub files: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransferProgress {
    pub file_index: usize,
    pub file_count: usize,
    pub bytes_sent: u64,
    pub bytes_total: u64,
}

impl TransferProgress {
    pub fn fraction(self) -> f32 {
        if self.bytes_total == 0 {
            return 1.0;
        }
        (self.bytes_sent as f32 / self.bytes_total as f32).clamp(0.0, 1.0)
    }
}

pub fn random_fingerprint() -> String {
    Uuid::new_v4().simple().to_string()
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

pub fn normalize_fingerprint(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// SHA-256 of a DER certificate, compared to the announced fingerprint.
pub fn fingerprints_match(announced: &str, cert_der: &[u8]) -> bool {
    let expected = normalize_fingerprint(announced);
    !expected.is_empty() && expected == sha256_hex(cert_der)
}

pub fn mime_for_path(path: &str) -> String {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        Some("svg") => "image/svg+xml",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("aac") => "audio/aac",
        Some("flac") => "audio/flac",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        Some("tar") => "application/x-tar",
        Some("gz") => "application/gzip",
        Some("json") => "application/json",
        Some("txt" | "md") => "text/plain",
        Some("html" | "htm") => "text/html",
        _ => "application/octet-stream",
    }
    .into()
}

pub fn file_meta_for_path(path: &Path) -> Result<FileMeta, String> {
    let meta = std::fs::metadata(path).map_err(|err| err.to_string())?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    Ok(FileMeta {
        id: Uuid::new_v4().to_string(),
        file_name: name,
        size: meta.len(),
        file_type: mime_for_path(&path.to_string_lossy()),
        sha256: None,
    })
}

pub fn parse_announce(bytes: &[u8]) -> Result<DeviceInfo, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|err| err.to_string())?
        .trim_end_matches('\0')
        .trim();
    serde_json::from_str(text).map_err(|err| err.to_string())
}

pub fn encode_announce(info: &DeviceInfo) -> Result<Vec<u8>, String> {
    serde_json::to_vec(info).map_err(|err| err.to_string())
}

pub fn merge_peer(peers: &mut Vec<DeviceInfo>, mut peer: DeviceInfo, self_fp: &str) -> bool {
    if peer.fingerprint.is_empty() || normalize_fingerprint(&peer.fingerprint) == normalize_fingerprint(self_fp)
    {
        return false;
    }
    if peer.ip.is_empty() {
        return false;
    }
    if let Some(existing) = peers
        .iter_mut()
        .find(|known| known.fingerprint == peer.fingerprint)
    {
        if existing.ip.is_empty() {
            existing.ip = peer.ip;
        }
        if existing.alias.is_empty() {
            existing.alias = peer.alias;
        }
        return false;
    }
    if peer.alias.is_empty() {
        peer.alias = peer.ip.clone();
    }
    peers.push(peer);
    true
}

/// Bind UDP + a short-lived HTTP register listener, announce, collect peers.
pub async fn discover_peers(alias: &str, window: Duration) -> Result<Vec<DeviceInfo>, String> {
    let fingerprint = random_fingerprint();
    let udp = bind_discovery_udp().await?;
    let local_port = udp.local_addr().map(|addr| addr.port()).unwrap_or(DEFAULT_PORT);
    let mut us = DeviceInfo::local(alias, fingerprint, local_port);
    us.announce = true;

    let listener = bind_register_listener(local_port).await;
    let payload = encode_announce(&us)?;
    udp.send_to(&payload, (MULTICAST_ADDR, DEFAULT_PORT))
        .await
        .map_err(|err| err.to_string())?;

    let mut peers = Vec::new();
    collect_discovery(&udp, listener.as_ref(), &us, window, &mut peers).await;
    drop(udp);
    drop(listener);
    Ok(peers)
}

async fn bind_discovery_udp() -> Result<UdpSocket, String> {
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DEFAULT_PORT)).await {
        Ok(socket) => socket,
        Err(_) => UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
            .await
            .map_err(|err| err.to_string())?,
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.set_multicast_loop_v4(true);
    socket
        .join_multicast_v4(MULTICAST_ADDR, Ipv4Addr::UNSPECIFIED)
        .map_err(|err| err.to_string())?;
    Ok(socket)
}

async fn bind_register_listener(port: u16) -> Option<TcpListener> {
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .await
        .ok()
        .or(TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await.ok())
}

async fn collect_discovery(
    udp: &UdpSocket,
    listener: Option<&TcpListener>,
    us: &DeviceInfo,
    window: Duration,
    peers: &mut Vec<DeviceInfo>,
) {
    let deadline = tokio::time::Instant::now() + window;
    let mut buf = [0u8; 4096];
    loop {
        let remain = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remain.is_zero() {
            break;
        }
        if let Some(listener) = listener {
            tokio::select! {
                result = udp.recv_from(&mut buf) => {
                    if let Ok((n, from)) = result {
                        ingest_udp(peers, &buf[..n], from, &us.fingerprint);
                    }
                }
                result = listener.accept() => {
                    if let Ok((stream, from)) = result {
                        if let Some(peer) = accept_register(stream, from, us).await {
                            merge_peer(peers, peer, &us.fingerprint);
                        }
                    }
                }
                _ = tokio::time::sleep(remain) => break,
            }
        } else {
            tokio::select! {
                result = udp.recv_from(&mut buf) => {
                    if let Ok((n, from)) = result {
                        ingest_udp(peers, &buf[..n], from, &us.fingerprint);
                    }
                }
                _ = tokio::time::sleep(remain) => break,
            }
        }
    }
}

fn ingest_udp(peers: &mut Vec<DeviceInfo>, bytes: &[u8], from: SocketAddr, self_fp: &str) {
    let Ok(mut peer) = parse_announce(bytes) else {
        return;
    };
    if peer.ip.is_empty() {
        peer.ip = from.ip().to_string();
    }
    if peer.port == 0 {
        peer.port = from.port();
    }
    merge_peer(peers, peer, self_fp);
}

async fn accept_register(
    mut stream: tokio::net::TcpStream,
    from: SocketAddr,
    us: &DeviceInfo,
) -> Option<DeviceInfo> {
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.ok()?;
    let text = std::str::from_utf8(&buf[..n]).ok()?;
    let (headers, body) = text.split_once("\r\n\r\n")?;
    if !headers.contains(&format!("{API}/register")) {
        return None;
    }
    let mut peer: DeviceInfo = serde_json::from_str(body.trim_end_matches('\0').trim()).ok()?;
    if peer.ip.is_empty() {
        peer.ip = from.ip().to_string();
    }
    let reply = serde_json::to_string(us).ok()?;
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
        reply.len()
    );
    let _ = stream.write_all(resp.as_bytes()).await;
    Some(peer)
}

fn https_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|err| err.to_string())
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|err| err.to_string())
}

fn client_for(peer: &DeviceInfo) -> Result<reqwest::Client, String> {
    if peer.protocol.eq_ignore_ascii_case("http") {
        http_client()
    } else {
        https_client()
    }
}

/// Confirm a peer certificate DER matches the announced SHA-256 fingerprint.
pub fn verify_tls_fingerprint(announced: &str, cert_der: Option<&[u8]>) -> Result<(), String> {
    if announced.is_empty() {
        return Ok(());
    }
    let Some(der) = cert_der else {
        return Ok(());
    };
    if fingerprints_match(announced, der) {
        Ok(())
    } else {
        Err("LocalSend certificate fingerprint does not match the announce".into())
    }
}

pub async fn prepare_upload(
    us: &DeviceInfo,
    peer: &DeviceInfo,
    files: &HashMap<String, FileMeta>,
    pin: Option<&str>,
) -> Result<PrepareUploadResponse, String> {
    let client = client_for(peer)?;
    let mut url = format!("{}{API}/prepare-upload", peer.origin());
    if let Some(pin) = pin.filter(|p| !p.is_empty()) {
        url.push_str("?pin=");
        url.push_str(pin);
    }
    let body = PrepareUploadRequest {
        info: DeviceInfo {
            ip: String::new(),
            announce: false,
            ..us.clone()
        },
        files: files.clone(),
    };
    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let status = response.status();
    if status.as_u16() == 204 {
        return Err("receiver accepted no files".into());
    }
    if status.as_u16() == 401 {
        return Err("PIN required or incorrect".into());
    }
    if status.as_u16() == 403 {
        return Err("transfer rejected".into());
    }
    if status.as_u16() == 409 {
        return Err("receiver is busy with another session".into());
    }
    if !status.is_success() {
        return Err(format!("prepare-upload failed ({status})"));
    }
    response
        .json::<PrepareUploadResponse>()
        .await
        .map_err(|err| err.to_string())
}

pub async fn upload_file(
    peer: &DeviceInfo,
    session_id: &str,
    file: &FileMeta,
    token: &str,
    path: &Path,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let total = bytes.len() as u64;
    on_progress(0, total);
    let client = client_for(peer)?;
    let url = format!(
        "{}{API}/upload?sessionId={}&fileId={}&token={}",
        peer.origin(),
        urlencode(session_id),
        urlencode(&file.id),
        urlencode(token)
    );
    let response = client
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, &file.file_type)
        .body(bytes)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("upload failed ({})", response.status()));
    }
    on_progress(total, total);
    Ok(())
}

pub async fn cancel_session(peer: &DeviceInfo, session_id: &str) -> Result<(), String> {
    let client = client_for(peer)?;
    let url = format!(
        "{}{API}/cancel?sessionId={}",
        peer.origin(),
        urlencode(session_id)
    );
    let _ = client.post(&url).send().await;
    Ok(())
}

pub async fn send_files(
    alias: &str,
    peer: &DeviceInfo,
    paths: &[PathBuf],
    pin: Option<&str>,
    mut on_progress: impl FnMut(TransferProgress),
) -> Result<(), String> {
    if paths.is_empty() {
        return Err("no files to send".into());
    }
    let us = DeviceInfo::local(alias, random_fingerprint(), DEFAULT_PORT);
    let mut files = HashMap::new();
    let mut ordered = Vec::new();
    for path in paths {
        let meta = file_meta_for_path(path)?;
        files.insert(meta.id.clone(), meta.clone());
        ordered.push((path.clone(), meta));
    }
    let prepared = prepare_upload(&us, peer, &files, pin).await?;
    let bytes_total: u64 = ordered.iter().map(|(_, meta)| meta.size).sum();
    let mut bytes_sent = 0u64;
    for (index, (path, meta)) in ordered.iter().enumerate() {
        let Some(token) = prepared.files.get(&meta.id) else {
            return Err(format!("receiver skipped {}", meta.file_name));
        };
        let already = bytes_sent;
        upload_file(peer, &prepared.session_id, meta, token, path, |sent, _| {
            on_progress(TransferProgress {
                file_index: index,
                file_count: ordered.len(),
                bytes_sent: already + sent,
                bytes_total,
            });
        })
        .await?;
        bytes_sent += meta.size;
    }
    Ok(())
}

pub fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&hex_encode(&[byte]).to_ascii_uppercase());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANNOUNCE: &str = r#"{
        "alias": "Nice Orange",
        "version": "2.0",
        "deviceModel": "Samsung",
        "deviceType": "mobile",
        "fingerprint": "abcDEF0123",
        "port": 53317,
        "protocol": "https",
        "download": true,
        "announce": true
    }"#;

    #[test]
    fn parse_announce_reads_protocol_fields() {
        let device = parse_announce(ANNOUNCE.as_bytes()).unwrap();
        assert_eq!(device.alias, "Nice Orange");
        assert_eq!(device.device_type.as_deref(), Some("mobile"));
        assert_eq!(device.port, 53317);
        assert_eq!(device.protocol, "https");
        assert!(device.announce);
        assert!(device.download);
    }

    #[test]
    fn parse_announce_tolerates_trailing_nulls() {
        let mut bytes = ANNOUNCE.as_bytes().to_vec();
        bytes.push(0);
        bytes.push(0);
        assert_eq!(parse_announce(&bytes).unwrap().alias, "Nice Orange");
    }

    #[test]
    fn encode_announce_round_trips_send_identity() {
        let us = DeviceInfo::local("Desk", "fp-1", 53317);
        let parsed = parse_announce(&encode_announce(&us).unwrap()).unwrap();
        assert_eq!(parsed.alias, "Desk");
        assert_eq!(parsed.version, PROTOCOL_VERSION);
        assert_eq!(parsed.protocol, "http");
        assert_eq!(parsed.device_type.as_deref(), Some("desktop"));
        assert!(parsed.announce);
        assert!(!parsed.download);
    }

    #[test]
    fn fingerprint_pin_accepts_hex_and_colon_forms() {
        let der = b"certificate-der";
        let hex = sha256_hex(der);
        assert!(fingerprints_match(&hex, der));
        let colon = hex
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(":");
        assert!(fingerprints_match(&colon, der));
        assert!(!fingerprints_match("deadbeef", der));
        assert_eq!(normalize_fingerprint("AB:cd"), "abcd");
        assert!(verify_tls_fingerprint(&hex, Some(der)).is_ok());
        assert!(verify_tls_fingerprint("deadbeef", Some(der)).is_err());
        assert!(verify_tls_fingerprint(&hex, None).is_ok());
    }

    #[test]
    fn mime_for_common_and_unknown_paths() {
        assert_eq!(mime_for_path("a.PNG"), "image/png");
        assert_eq!(mime_for_path("clip.mp4"), "video/mp4");
        assert_eq!(mime_for_path("doc.pdf"), "application/pdf");
        assert_eq!(mime_for_path("noext"), "application/octet-stream");
    }

    #[test]
    fn file_meta_reads_size_and_name() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("nook-ls-{}.txt", Uuid::new_v4()));
        std::fs::write(&path, b"hello").unwrap();
        let meta = file_meta_for_path(&path).unwrap();
        assert_eq!(meta.size, 5);
        assert_eq!(meta.file_type, "text/plain");
        assert_eq!(meta.file_name, path.file_name().unwrap().to_string_lossy());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn merge_peer_skips_self_and_dedups() {
        let mut peers = Vec::new();
        assert!(!merge_peer(
            &mut peers,
            DeviceInfo::local("me", "SELF", 53317),
            "SELF"
        ));
        let mut other = DeviceInfo::local("Phone", "peer-1", 53317);
        other.ip = "192.168.1.8".into();
        other.announce = false;
        assert!(merge_peer(&mut peers, other.clone(), "SELF"));
        assert!(!merge_peer(&mut peers, other, "SELF"));
        assert_eq!(peers.len(), 1);
    }

    #[test]
    fn prepare_upload_request_shape() {
        let us = DeviceInfo::local("Desk", "fp", 53317);
        let mut files = HashMap::new();
        files.insert(
            "id-1".into(),
            FileMeta {
                id: "id-1".into(),
                file_name: "shot.png".into(),
                size: 12,
                file_type: "image/png".into(),
                sha256: None,
            },
        );
        let json = serde_json::to_value(PrepareUploadRequest {
            info: us,
            files,
        })
        .unwrap();
        assert_eq!(json["info"]["alias"], "Desk");
        assert_eq!(json["files"]["id-1"]["fileName"], "shot.png");
        assert_eq!(json["files"]["id-1"]["size"], 12);
    }

    #[test]
    fn prepare_upload_response_parses_tokens() {
        let parsed: PrepareUploadResponse = serde_json::from_str(
            r#"{"sessionId":"sess","files":{"id-1":"tok-1","id-2":"tok-2"}}"#,
        )
        .unwrap();
        assert_eq!(parsed.session_id, "sess");
        assert_eq!(parsed.files.get("id-1").map(String::as_str), Some("tok-1"));
    }

    #[test]
    fn urlencode_leaves_unreserved_and_escapes_the_rest() {
        assert_eq!(urlencode("abc-_.~"), "abc-_.~");
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
    }

    #[test]
    fn origin_uses_announced_scheme_and_port() {
        let mut peer = DeviceInfo::local("Phone", "fp", 53317);
        peer.ip = "10.0.0.4".into();
        peer.protocol = "https".into();
        assert_eq!(peer.origin(), "https://10.0.0.4:53317");
        peer.protocol = "http".into();
        assert_eq!(peer.origin(), "http://10.0.0.4:53317");
    }

    #[test]
    fn progress_fraction_clamps() {
        assert_eq!(
            TransferProgress {
                file_index: 0,
                file_count: 1,
                bytes_sent: 0,
                bytes_total: 0
            }
            .fraction(),
            1.0
        );
        assert!((TransferProgress {
            file_index: 0,
            file_count: 2,
            bytes_sent: 25,
            bytes_total: 100
        }
        .fraction()
            - 0.25)
            .abs()
            < f32::EPSILON);
    }
}
