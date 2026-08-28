//! Spotify Web API: OAuth 2.0 PKCE, queue read, jump-to-item.
//!
//! No bundled client secret. The user supplies a developer-app client ID
//! (Settings). Refresh token lives in the Keychain on macOS.

use crate::models::{PlaybackQueue, QueueHidden, QueueItem, QueueJump, QueueSource};
use crate::settings;
use base64::Engine;
use serde::Deserialize;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const REDIRECT_URI: &str = "http://127.0.0.1:43821/callback";
pub const REDIRECT_PORT: u16 = 43821;
pub const AUTH_SCOPES: &str = "user-read-playback-state user-modify-playback-state";
const AUTHORIZE_URL: &str = "https://accounts.spotify.com/authorize";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const API_BASE: &str = "https://api.spotify.com/v1";
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "com.prodBirdy.openNook.spotify";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "refresh-token";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpotifyStatus {
    Disconnected,
    Connecting,
    Connected,
    NeedsClientId,
    PremiumRequired,
    Error(String),
}

#[derive(Debug, Clone)]
struct TokenSet {
    access: String,
    refresh: Option<String>,
    expires_at: Instant,
}

static STATUS: OnceLock<Mutex<SpotifyStatus>> = OnceLock::new();
static TOKENS: OnceLock<Mutex<Option<TokenSet>>> = OnceLock::new();
static CONNECTING: AtomicBool = AtomicBool::new(false);
static PREMIUM_BLOCKED: AtomicBool = AtomicBool::new(false);

fn status_lock() -> &'static Mutex<SpotifyStatus> {
    STATUS.get_or_init(|| Mutex::new(SpotifyStatus::Disconnected))
}

fn tokens_lock() -> &'static Mutex<Option<TokenSet>> {
    TOKENS.get_or_init(|| Mutex::new(None))
}

pub fn status() -> SpotifyStatus {
    status_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn set_status(next: SpotifyStatus) {
    *status_lock().lock().unwrap_or_else(|e| e.into_inner()) = next;
}

pub fn is_connected() -> bool {
    matches!(status(), SpotifyStatus::Connected) || tokens_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
        || load_refresh_token().is_some()
}

pub fn premium_required() -> bool {
    PREMIUM_BLOCKED.load(Ordering::Relaxed)
}

pub fn client_id() -> String {
    settings::get_app_settings()
        .spotify_client_id
        .trim()
        .to_string()
}

/// RFC 7636 appendix B vector (used by tests).
pub fn pkce_challenge_s256(verifier: &str) -> String {
    let hash = sha256(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
}

/// SHA-256 without pulling `sha2` 0.11 (edition2024 / rustc 1.85).
fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut data = input.to_vec();
    let bit_len = (input.len() as u64).saturating_mul(8);
    data.push(0x80);
    while (data.len() % 64) != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());
    let mut h = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    for chunk in data.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn generate_verifier() -> String {
    random_b64url(64)
}

pub fn generate_state() -> String {
    random_b64url(24)
}

fn random_b64url(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    fill_random(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

fn fill_random(buf: &mut [u8]) {
    #[cfg(unix)]
    {
        if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
            let _ = file.read_exact(buf);
            return;
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for (i, byte) in buf.iter_mut().enumerate() {
        *byte = ((nanos.wrapping_mul(6364136223846793005) >> ((i % 8) * 8)) & 0xFF) as u8
            ^ (i as u8).wrapping_mul(31);
    }
}

pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn authorize_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> String {
    format!(
        "{AUTHORIZE_URL}?client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge_method=S256&code_challenge={}&state={}",
        percent_encode(client_id),
        percent_encode(redirect_uri),
        percent_encode(AUTH_SCOPES),
        percent_encode(challenge),
        percent_encode(state),
    )
}

pub fn parse_callback_query(query: &str) -> Result<(String, String), String> {
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "code" => code = Some(url_decode(value)),
            "state" => state = Some(url_decode(value)),
            "error" => error = Some(url_decode(value)),
            _ => {}
        }
    }
    if let Some(err) = error {
        return Err(err);
    }
    match (code, state) {
        (Some(code), Some(state)) if !code.is_empty() => Ok((code, state)),
        _ => Err("missing code or state".into()),
    }
}

fn url_decode(value: &str) -> String {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_http_request_target(request: &str) -> Option<String> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let _method = parts.next()?;
    let target = parts.next()?;
    Some(target.to_string())
}

/// How many `POST /next` calls to land on upcoming item `index` (0-based).
pub fn skip_count_for_index(index: usize) -> u32 {
    (index + 1) as u32
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct PlayerState {
    #[serde(default)]
    context: Option<PlayerContext>,
}

#[derive(Debug, Deserialize)]
struct PlayerContext {
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QueueResponse {
    #[serde(default)]
    currently_playing: Option<SpotifyTrack>,
    #[serde(default)]
    queue: Vec<SpotifyTrack>,
}

#[derive(Debug, Deserialize)]
struct SpotifyTrack {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    uri: Option<String>,
    #[serde(default)]
    artists: Vec<SpotifyArtist>,
    #[serde(default)]
    album: Option<SpotifyAlbum>,
}

#[derive(Debug, Deserialize)]
struct SpotifyArtist {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpotifyAlbum {
    #[serde(default)]
    images: Vec<SpotifyImage>,
}

#[derive(Debug, Deserialize)]
struct SpotifyImage {
    url: String,
    #[serde(default)]
    width: Option<u32>,
}

pub fn pick_artwork_url(images: &[(u32, String)]) -> Option<String> {
    if images.is_empty() {
        return None;
    }
    let mut ranked = images.to_vec();
    ranked.sort_by_key(|(w, _)| *w);
    ranked
        .iter()
        .find(|(w, _)| *w >= 64)
        .or(ranked.last())
        .map(|(_, url)| url.clone())
}

pub fn parse_queue_json(body: &str, context_uri: Option<String>) -> Result<PlaybackQueue, String> {
    let parsed: QueueResponse = serde_json::from_str(body).map_err(|err| err.to_string())?;
    let mut items = Vec::new();
    for (index, track) in parsed.queue.into_iter().take(20).enumerate() {
        let title = track.name.unwrap_or_else(|| "Unknown".into());
        let artist = track
            .artists
            .iter()
            .filter_map(|a| a.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let id = track
            .id
            .clone()
            .or_else(|| track.uri.clone())
            .unwrap_or_else(|| format!("spotify-{index}"));
        let uri = track
            .uri
            .clone()
            .unwrap_or_else(|| format!("spotify:track:{id}"));
        let artwork = track.album.as_ref().and_then(|album| {
            let pairs = album
                .images
                .iter()
                .map(|img| (img.width.unwrap_or(0), img.url.clone()))
                .collect::<Vec<_>>();
            pick_artwork_url(&pairs)
        });
        items.push(QueueItem {
            id,
            title,
            artist,
            artwork_url: artwork,
            artwork_base64: None,
            source: QueueSource::Spotify,
            jump: QueueJump::Spotify {
                skip_count: skip_count_for_index(index),
                uri,
            },
        });
    }
    let _ = parsed.currently_playing;
    Ok(PlaybackQueue {
        source: Some(QueueSource::Spotify),
        label: "Playing Next".into(),
        items,
        hidden: None,
        context_uri,
    })
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|err| err.to_string())
}

async fn exchange_token(form: &[(&str, &str)]) -> Result<TokenSet, String> {
    let client = http_client()?;
    let response = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(form)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("token exchange {status}: {body}"));
    }
    let parsed: TokenResponse = response.json().await.map_err(|err| err.to_string())?;
    Ok(TokenSet {
        access: parsed.access_token,
        refresh: parsed.refresh_token,
        expires_at: Instant::now() + Duration::from_secs(parsed.expires_in.saturating_sub(30)),
    })
}

fn store_tokens(tokens: TokenSet) {
    if let Some(refresh) = tokens.refresh.as_deref() {
        store_refresh_token(refresh);
    }
    *tokens_lock().lock().unwrap_or_else(|e| e.into_inner()) = Some(tokens);
    PREMIUM_BLOCKED.store(false, Ordering::Relaxed);
    set_status(SpotifyStatus::Connected);
}

fn clear_tokens() {
    *tokens_lock().lock().unwrap_or_else(|e| e.into_inner()) = None;
    delete_refresh_token();
    PREMIUM_BLOCKED.store(false, Ordering::Relaxed);
    set_status(SpotifyStatus::Disconnected);
}

async fn valid_access_token() -> Result<String, String> {
    {
        let guard = tokens_lock().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tokens) = guard.as_ref() {
            if Instant::now() < tokens.expires_at && !tokens.access.is_empty() {
                return Ok(tokens.access.clone());
            }
        }
    }
    let refresh = load_refresh_token().ok_or_else(|| "not connected".to_string())?;
    let client_id = client_id();
    if client_id.is_empty() {
        set_status(SpotifyStatus::NeedsClientId);
        return Err("Spotify client ID is missing".into());
    }
    let tokens = exchange_token(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh),
        ("client_id", &client_id),
    ])
    .await?;
    let access = tokens.access.clone();
    store_tokens(TokenSet {
        refresh: tokens.refresh.or(Some(refresh)),
        ..tokens
    });
    Ok(access)
}

async fn api_request(
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<(reqwest::StatusCode, String), String> {
    let token = valid_access_token().await?;
    let client = http_client()?;
    let url = format!("{API_BASE}{path}");
    let mut req = client
        .request(method.clone(), &url)
        .bearer_auth(&token)
        .header("Accept", "application/json");
    if let Some(json) = body.clone() {
        req = req.json(&json);
    }
    let response = req.send().await.map_err(|err| err.to_string())?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        *tokens_lock().lock().unwrap_or_else(|e| e.into_inner()) = None;
        let token = valid_access_token().await?;
        let mut retry = client
            .request(method, &url)
            .bearer_auth(token)
            .header("Accept", "application/json");
        if let Some(json) = body {
            retry = retry.json(&json);
        }
        let response = retry.send().await.map_err(|err| err.to_string())?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Ok((status, text));
    }
    Ok((status, text))
}

fn accept_redirect(listener: &TcpListener, expected_state: &str) -> Result<String, String> {
    listener
        .set_nonblocking(true)
        .map_err(|err| err.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(180);
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(pair) => break pair,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("authorization timed out".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => return Err(err.to_string()),
        }
    };
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).map_err(|err| err.to_string())?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let target = parse_http_request_target(&request).ok_or("bad request")?;
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let html = if query.contains("error=") {
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<html><body>Spotify authorization was cancelled. You can close this window.</body></html>"
    } else {
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<html><body>openNook is connected to Spotify. You can close this window.</body></html>"
    };
    let _ = stream.write_all(html.as_bytes());
    let (code, state) = parse_callback_query(query)?;
    if state != expected_state {
        return Err("OAuth state mismatch".into());
    }
    Ok(code)
}

/// Browser PKCE loopback. Safe to call from Settings; no-ops if a flow is live.
pub async fn connect() -> Result<(), String> {
    let client_id = client_id();
    if client_id.is_empty() {
        set_status(SpotifyStatus::NeedsClientId);
        return Err("Add a Spotify client ID in Settings first".into());
    }
    if CONNECTING.swap(true, Ordering::SeqCst) {
        return Err("authorization already in progress".into());
    }
    set_status(SpotifyStatus::Connecting);
    let result = connect_inner(&client_id).await;
    CONNECTING.store(false, Ordering::SeqCst);
    if let Err(err) = &result {
        if !matches!(status(), SpotifyStatus::Connected) {
            set_status(SpotifyStatus::Error(err.clone()));
        }
    }
    result
}

async fn connect_inner(client_id: &str) -> Result<(), String> {
    let verifier = generate_verifier();
    let challenge = pkce_challenge_s256(&verifier);
    let state = generate_state();
    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT)).map_err(|err| {
        format!("could not bind {REDIRECT_URI} ({err}). Close whatever is using port {REDIRECT_PORT} and try again.")
    })?;
    let url = authorize_url(client_id, REDIRECT_URI, &challenge, &state);
    open::that(&url).map_err(|err| format!("open browser: {err}"))?;
    let expected = state.clone();
    let code = tokio::task::spawn_blocking(move || accept_redirect(&listener, &expected))
        .await
        .map_err(|err| err.to_string())??;
    let tokens = exchange_token(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", client_id),
        ("code_verifier", &verifier),
    ])
    .await?;
    store_tokens(tokens);
    Ok(())
}

pub fn disconnect() {
    clear_tokens();
}

pub async fn fetch_queue() -> Result<PlaybackQueue, String> {
    if client_id().is_empty() && load_refresh_token().is_none() {
        return Ok(PlaybackQueue {
            source: Some(QueueSource::Spotify),
            hidden: Some(QueueHidden::NeedsSpotifyAuth),
            label: "Playing Next".into(),
            ..PlaybackQueue::default()
        });
    }
    if load_refresh_token().is_none()
        && tokens_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
    {
        return Ok(PlaybackQueue {
            source: Some(QueueSource::Spotify),
            hidden: Some(QueueHidden::NeedsSpotifyAuth),
            label: "Playing Next".into(),
            ..PlaybackQueue::default()
        });
    }
    let context_uri = match api_request(reqwest::Method::GET, "/me/player", None).await {
        Ok((status, body)) if status.is_success() => serde_json::from_str::<PlayerState>(&body)
            .ok()
            .and_then(|p| p.context.and_then(|c| c.uri)),
        Ok((status, _)) if status.as_u16() == 204 => None,
        Ok((status, _)) if status.as_u16() == 403 => {
            PREMIUM_BLOCKED.store(true, Ordering::Relaxed);
            set_status(SpotifyStatus::PremiumRequired);
            return Ok(PlaybackQueue {
                source: Some(QueueSource::Spotify),
                hidden: Some(QueueHidden::PremiumRequired),
                label: "Playing Next".into(),
                ..PlaybackQueue::default()
            });
        }
        Ok((status, body)) => {
            log::debug!("spotify player {status}: {body}");
            None
        }
        Err(err) => {
            log::debug!("spotify player: {err}");
            None
        }
    };
    let (status, body) = api_request(reqwest::Method::GET, "/me/player/queue", None).await?;
    if status.as_u16() == 403 {
        PREMIUM_BLOCKED.store(true, Ordering::Relaxed);
        set_status(SpotifyStatus::PremiumRequired);
        return Ok(PlaybackQueue {
            source: Some(QueueSource::Spotify),
            hidden: Some(QueueHidden::PremiumRequired),
            label: "Playing Next".into(),
            ..PlaybackQueue::default()
        });
    }
    if !status.is_success() {
        return Err(format!("queue {status}: {body}"));
    }
    PREMIUM_BLOCKED.store(false, Ordering::Relaxed);
    set_status(SpotifyStatus::Connected);
    parse_queue_json(&body, context_uri)
}

pub async fn jump_to(skip_count: u32, uri: &str, context_uri: Option<&str>) -> Result<(), String> {
    if let Some(context) = context_uri.filter(|s| !s.is_empty()) {
        if play_context_offset(context, uri).await.is_ok() {
            return Ok(());
        }
    }
    skip_n(skip_count).await
}

async fn play_context_offset(context_uri: &str, uri: &str) -> Result<(), String> {
    let body = serde_json::json!({
        "context_uri": context_uri,
        "offset": { "uri": uri },
        "position_ms": 0
    });
    let (status, text) =
        api_request(reqwest::Method::PUT, "/me/player/play", Some(body)).await?;
    if status.as_u16() == 403 {
        PREMIUM_BLOCKED.store(true, Ordering::Relaxed);
        set_status(SpotifyStatus::PremiumRequired);
        return Err("Spotify Premium is required to jump in the queue".into());
    }
    if status.is_success() || status.as_u16() == 204 {
        Ok(())
    } else {
        Err(format!("play {status}: {text}"))
    }
}

async fn skip_n(count: u32) -> Result<(), String> {
    for _ in 0..count {
        let (status, text) = api_request(reqwest::Method::POST, "/me/player/next", None).await?;
        if status.as_u16() == 403 {
            PREMIUM_BLOCKED.store(true, Ordering::Relaxed);
            set_status(SpotifyStatus::PremiumRequired);
            return Err("Spotify Premium is required to skip tracks".into());
        }
        if !status.is_success() && status.as_u16() != 204 {
            return Err(format!("next {status}: {text}"));
        }
    }
    Ok(())
}

fn load_refresh_token() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .filter(|s| !s.is_empty())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let path = crate::app_data_dir().join("spotify-refresh");
        std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

fn store_refresh_token(token: &str) {
    #[cfg(target_os = "macos")]
    {
        if let Err(err) = security_framework::passwords::set_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
            token.as_bytes(),
        ) {
            log::warn!("spotify keychain store: {err}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let path = crate::app_data_dir().join("spotify-refresh");
        if let Err(err) = std::fs::write(path, token) {
            log::warn!("spotify token store: {err}");
        }
    }
}

fn delete_refresh_token() {
    #[cfg(target_os = "macos")]
    {
        let _ = security_framework::passwords::delete_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let path = crate::app_data_dir().join("spotify-refresh");
        let _ = std::fs::remove_file(path);
    }
}

/// Prime in-memory status from a persisted refresh token (no network).
pub fn hydrate_status() {
    if load_refresh_token().is_some() {
        if client_id().is_empty() {
            set_status(SpotifyStatus::NeedsClientId);
        } else {
            set_status(SpotifyStatus::Connected);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_rfc7636_appendix_b() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge_s256(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorize_url_includes_pkce_and_scopes() {
        let url = authorize_url(
            "cid",
            REDIRECT_URI,
            "challenge",
            "state-1",
        );
        assert!(url.starts_with(AUTHORIZE_URL));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("code_challenge=challenge"));
        assert!(url.contains("state=state-1"));
        assert!(url.contains("user-read-playback-state"));
        assert!(url.contains("user-modify-playback-state"));
        assert!(url.contains(&percent_encode(REDIRECT_URI)));
    }

    #[test]
    fn percent_encode_leaves_unreserved() {
        assert_eq!(percent_encode("Abc-_.~9"), "Abc-_.~9");
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("x&y"), "x%26y");
    }

    #[test]
    fn callback_query_reads_code_and_rejects_error() {
        let (code, state) = parse_callback_query("code=abc%2Fdef&state=s1").unwrap();
        assert_eq!(code, "abc/def");
        assert_eq!(state, "s1");
        assert_eq!(
            parse_callback_query("error=access_denied&state=s1").unwrap_err(),
            "access_denied"
        );
    }

    #[test]
    fn http_target_extracts_path() {
        let req = "GET /callback?code=1&state=2 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(
            parse_http_request_target(req).as_deref(),
            Some("/callback?code=1&state=2")
        );
    }

    #[test]
    fn skip_count_is_one_based() {
        assert_eq!(skip_count_for_index(0), 1);
        assert_eq!(skip_count_for_index(4), 5);
    }

    #[test]
    fn artwork_prefers_64px_or_smallest_above() {
        let images = vec![
            (640, "big".into()),
            (300, "med".into()),
            (64, "small".into()),
        ];
        assert_eq!(pick_artwork_url(&images).as_deref(), Some("small"));
        assert_eq!(
            pick_artwork_url(&[(32, "tiny".into())]).as_deref(),
            Some("tiny")
        );
        assert_eq!(pick_artwork_url(&[]), None);
    }

    #[test]
    fn queue_json_maps_upcoming_tracks() {
        let body = r#"{
            "currently_playing": {"id":"now","name":"Now","uri":"spotify:track:now","artists":[{"name":"A"}]},
            "queue": [
                {"id":"n1","name":"Next","uri":"spotify:track:n1","artists":[{"name":"B"}],
                 "album":{"images":[{"url":"https://i.scdn.co/image/x","width":64}]}},
                {"id":"n2","name":"Later","uri":"spotify:track:n2","artists":[{"name":"C"},{"name":"D"}]}
            ]
        }"#;
        let queue = parse_queue_json(body, Some("spotify:playlist:p".into())).unwrap();
        assert_eq!(queue.source, Some(QueueSource::Spotify));
        assert_eq!(queue.label, "Playing Next");
        assert_eq!(queue.items.len(), 2);
        assert_eq!(queue.items[0].title, "Next");
        assert_eq!(queue.items[0].artist, "B");
        assert_eq!(
            queue.items[0].artwork_url.as_deref(),
            Some("https://i.scdn.co/image/x")
        );
        assert_eq!(
            queue.items[0].jump,
            QueueJump::Spotify {
                skip_count: 1,
                uri: "spotify:track:n1".into()
            }
        );
        assert_eq!(
            queue.items[1].jump,
            QueueJump::Spotify {
                skip_count: 2,
                uri: "spotify:track:n2".into()
            }
        );
        assert_eq!(queue.items[1].artist, "C, D");
        assert_eq!(queue.context_uri.as_deref(), Some("spotify:playlist:p"));
    }

    #[test]
    fn generated_verifier_is_pkce_safe_length() {
        let verifier = generate_verifier();
        assert!(verifier.len() >= 43 && verifier.len() <= 128);
        assert!(verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        let state = generate_state();
        assert!(state.len() >= 16);
    }
}
