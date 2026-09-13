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
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use uuid::Uuid;

pub const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);
pub const DEFAULT_PORT: u16 = 53317;
pub const PROTOCOL_VERSION: &str = "2.1";
pub const DISCOVER_WINDOW: Duration = Duration::from_secs(3);
pub const BUNDLE_ID: &str = "org.localsend.localsendApp";
pub const BUNDLE_IDS: &[&str] = &[BUNDLE_ID, "org.localsend.localsend_app"];

/// True when the LocalSend app is present. The tray drop target is hidden
/// otherwise — sending needs a LocalSend receiver, and this Mac having the
/// app is the signal the user actually uses it.
pub fn app_installed() -> bool {
    if crate::apps::any_installed(BUNDLE_IDS) {
        return true;
    }
    binary_on_path("localsend") || binary_on_path("localsend-cli")
}

fn binary_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
}

const API: &str = "/api/localsend/v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub alias: String,
    pub version: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "deviceModel"
    )]
    pub device_model: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "deviceType"
    )]
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
    if peer.fingerprint.is_empty()
        || normalize_fingerprint(&peer.fingerprint) == normalize_fingerprint(self_fp)
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
///
/// Multicast is best-effort. The protocol's HTTP legacy scan (POST `/register`
/// to every host on each local /24) is what finds receivers when the LocalSend
/// *app* is not installed on this machine — we never need a local LocalSend
/// process, only a peer that is already listening on 53317.
pub async fn discover_peers(alias: &str, window: Duration) -> Result<Vec<DeviceInfo>, String> {
    let fingerprint = random_fingerprint();
    let local_ips = local_ipv4s();
    let udp = bind_discovery_udp(&local_ips).await.ok();
    let local_port = udp
        .as_ref()
        .and_then(|socket| socket.local_addr().ok())
        .map(|addr| addr.port())
        .unwrap_or(DEFAULT_PORT);
    let mut us = DeviceInfo::local(alias, fingerprint, local_port);
    us.announce = true;

    let listener = bind_register_listener(local_port).await;
    let payload = encode_announce(&us)?;
    if let Some(udp) = udp.as_ref() {
        let _ = udp.send_to(&payload, (MULTICAST_ADDR, DEFAULT_PORT)).await;
    }
    announce_on_interfaces(&payload, &local_ips).await;

    let us_scan = us.clone();
    let ips_scan = local_ips.clone();
    let collect_fut = async {
        let mut peers = Vec::new();
        if let Some(udp) = udp.as_ref() {
            collect_discovery(udp, listener.as_ref(), &us, window, &mut peers).await;
        }
        peers
    };
    let (from_udp, from_http) = tokio::join!(collect_fut, http_scan(us_scan, ips_scan, window));
    let mut peers = Vec::new();
    for peer in from_udp.into_iter().chain(from_http) {
        merge_peer(&mut peers, peer, &us.fingerprint);
    }
    peers.retain(|peer| local_ips.iter().all(|ip| peer.ip != ip.to_string()));
    drop(udp);
    drop(listener);
    Ok(peers)
}

async fn bind_discovery_udp(ifaces: &[Ipv4Addr]) -> Result<UdpSocket, String> {
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DEFAULT_PORT)).await {
        Ok(socket) => socket,
        Err(_) => UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
            .await
            .map_err(|err| err.to_string())?,
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.set_multicast_loop_v4(true);
    let mut joined = false;
    for ip in ifaces {
        if socket.join_multicast_v4(MULTICAST_ADDR, *ip).is_ok() {
            joined = true;
        }
    }
    if !joined {
        let _ = socket.join_multicast_v4(MULTICAST_ADDR, Ipv4Addr::UNSPECIFIED);
    }
    Ok(socket)
}

async fn announce_on_interfaces(payload: &[u8], ifaces: &[Ipv4Addr]) {
    for ip in ifaces {
        if let Ok(socket) = UdpSocket::bind((*ip, 0)).await {
            let _ = socket.set_multicast_ttl_v4(4);
            let _ = socket
                .send_to(payload, (MULTICAST_ADDR, DEFAULT_PORT))
                .await;
        }
    }
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

pub fn url_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(byte) =
                    u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
                {
                    out.push(byte);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// RFC1918 + Tailscale CGNAT. HTTP scan never walks a public /24.
pub fn is_private_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 100 && (64..128).contains(&o[1]))
}

pub fn is_usable_lan_ip(ip: Ipv4Addr) -> bool {
    !ip.is_unspecified()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_multicast()
        && !ip.is_broadcast()
}

/// Hosts on each local /24, minus network, broadcast, and this machine.
pub fn scan_targets(local_ips: &[Ipv4Addr]) -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    for ip in local_ips {
        if !is_usable_lan_ip(*ip) || !is_private_v4(*ip) {
            continue;
        }
        let o = ip.octets();
        for host in 1..=254u8 {
            let candidate = Ipv4Addr::new(o[0], o[1], o[2], host);
            if candidate != *ip {
                out.push(candidate);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn preferred_lan_ip(local_ips: &[Ipv4Addr]) -> Option<Ipv4Addr> {
    let usable: Vec<Ipv4Addr> = local_ips
        .iter()
        .copied()
        .filter(|ip| is_usable_lan_ip(*ip) && is_private_v4(*ip))
        .collect();
    usable
        .iter()
        .copied()
        .find(|ip| ip.octets()[0] == 192 && ip.octets()[1] == 168)
        .or_else(|| usable.iter().copied().find(|ip| ip.octets()[0] == 10))
        .or_else(|| usable.iter().copied().find(|ip| ip.octets()[0] == 172))
        .or_else(|| usable.first().copied())
}

pub fn local_ipv4s() -> Vec<Ipv4Addr> {
    #[cfg(unix)]
    {
        unix_ipv4s()
    }
    #[cfg(not(unix))]
    {
        fallback_ipv4s()
    }
}

#[cfg(unix)]
fn unix_ipv4s() -> Vec<Ipv4Addr> {
    use std::ffi::CStr;
    let mut ips = Vec::new();
    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return fallback_ipv4s();
        }
        let mut ptr = ifap;
        while !ptr.is_null() {
            let entry = &*ptr;
            if !entry.ifa_addr.is_null() {
                let family = (*entry.ifa_addr).sa_family as i32;
                if family == libc::AF_INET {
                    let name = if entry.ifa_name.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(entry.ifa_name)
                            .to_string_lossy()
                            .into_owned()
                    };
                    let skip = name.starts_with("lo")
                        || name.starts_with("awdl")
                        || name.starts_with("llw")
                        || name.starts_with("bridge")
                        || name.starts_with("vmenet")
                        || name.starts_with("docker")
                        || name.starts_with("veth");
                    if !skip {
                        let sin = &*(entry.ifa_addr as *const libc::sockaddr_in);
                        let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                        if is_usable_lan_ip(ip) {
                            ips.push(ip);
                        }
                    }
                }
            }
            ptr = entry.ifa_next;
        }
        libc::freeifaddrs(ifap);
    }
    ips.sort();
    ips.dedup();
    if ips.is_empty() {
        fallback_ipv4s()
    } else {
        ips
    }
}

fn fallback_ipv4s() -> Vec<Ipv4Addr> {
    let Ok(socket) = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else {
        return Vec::new();
    };
    if socket.connect((Ipv4Addr::new(1, 1, 1, 1), 53)).is_err() {
        return Vec::new();
    }
    match socket.local_addr() {
        Ok(SocketAddr::V4(addr)) if is_usable_lan_ip(*addr.ip()) => vec![*addr.ip()],
        _ => Vec::new(),
    }
}

fn scan_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .timeout(Duration::from_millis(400))
        .connect_timeout(Duration::from_millis(250))
        .no_proxy()
        .build()
        .map_err(|err| err.to_string())
}

async fn probe_peer(client: &reqwest::Client, us: &DeviceInfo, ip: Ipv4Addr) -> Option<DeviceInfo> {
    let body = DeviceInfo {
        ip: String::new(),
        announce: false,
        ..us.clone()
    };
    for https in [true, false] {
        let scheme = if https { "https" } else { "http" };
        let url = format!("{scheme}://{ip}:{DEFAULT_PORT}{API}/register");
        let Ok(response) = client.post(&url).json(&body).send().await else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(mut peer) = response.json::<DeviceInfo>().await else {
            continue;
        };
        if peer.ip.is_empty() {
            peer.ip = ip.to_string();
        }
        if peer.port == 0 {
            peer.port = DEFAULT_PORT;
        }
        peer.protocol = scheme.into();
        return Some(peer);
    }
    None
}

async fn http_scan(us: DeviceInfo, local_ips: Vec<Ipv4Addr>, window: Duration) -> Vec<DeviceInfo> {
    let targets = scan_targets(&local_ips);
    if targets.is_empty() {
        return Vec::new();
    }
    let Ok(client) = scan_client() else {
        return Vec::new();
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sem = Arc::new(tokio::sync::Semaphore::new(48));
    for ip in targets {
        let sem = sem.clone();
        let client = client.clone();
        let us = us.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let Ok(_permit) = sem.acquire_owned().await else {
                return;
            };
            if let Some(peer) = probe_peer(&client, &us, ip).await {
                let _ = tx.send(peer);
            }
        });
    }
    drop(tx);
    let deadline = tokio::time::Instant::now() + window;
    let mut peers = Vec::new();
    loop {
        let remain = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remain.is_zero() {
            break;
        }
        match tokio::time::timeout(remain, rx.recv()).await {
            Ok(Some(peer)) => {
                merge_peer(&mut peers, peer, &us.fingerprint);
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    peers
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
        let json = serde_json::to_value(PrepareUploadRequest { info: us, files }).unwrap();
        assert_eq!(json["info"]["alias"], "Desk");
        assert_eq!(json["files"]["id-1"]["fileName"], "shot.png");
        assert_eq!(json["files"]["id-1"]["size"], 12);
    }

    #[test]
    fn prepare_upload_response_parses_tokens() {
        let parsed: PrepareUploadResponse =
            serde_json::from_str(r#"{"sessionId":"sess","files":{"id-1":"tok-1","id-2":"tok-2"}}"#)
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
        assert!(
            (TransferProgress {
                file_index: 0,
                file_count: 2,
                bytes_sent: 25,
                bytes_total: 100
            }
            .fraction()
                - 0.25)
                .abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn scan_targets_walks_slash24_minus_self() {
        let self_ip = Ipv4Addr::new(192, 168, 1, 20);
        let targets = scan_targets(&[self_ip]);
        assert_eq!(targets.len(), 253);
        assert!(!targets.contains(&self_ip));
        assert!(targets.contains(&Ipv4Addr::new(192, 168, 1, 1)));
        assert!(targets.contains(&Ipv4Addr::new(192, 168, 1, 254)));
        assert!(!targets.contains(&Ipv4Addr::new(192, 168, 1, 0)));
        assert!(!targets.contains(&Ipv4Addr::new(192, 168, 1, 255)));
    }

    #[test]
    fn scan_targets_skips_loopback_and_public() {
        assert!(scan_targets(&[Ipv4Addr::LOCALHOST]).is_empty());
        assert!(scan_targets(&[Ipv4Addr::new(8, 8, 8, 8)]).is_empty());
        assert!(scan_targets(&[Ipv4Addr::new(169, 254, 1, 1)]).is_empty());
    }

    #[test]
    fn preferred_lan_ip_picks_wifi_over_cgnat() {
        let ips = [
            Ipv4Addr::new(100, 64, 0, 2),
            Ipv4Addr::new(192, 168, 4, 18),
            Ipv4Addr::LOCALHOST,
        ];
        assert_eq!(preferred_lan_ip(&ips), Some(Ipv4Addr::new(192, 168, 4, 18)));
    }

    #[test]
    fn url_decode_reverses_urlencode() {
        assert_eq!(url_decode(&urlencode("a b/c")), "a b/c");
        assert_eq!(url_decode("hello%20world"), "hello world");
    }

    #[test]
    fn official_bundle_id_is_listed() {
        assert_eq!(BUNDLE_ID, "org.localsend.localsendApp");
        assert!(BUNDLE_IDS.contains(&BUNDLE_ID));
    }
}
