//! Apple Music editorialVideo lookup for animated album covers.
//!
//! MediaRemote only yields a static JPEG. Motion art lives on the catalog as
//! `editorialVideo` HLS loops. The path is: iTunes Search (adamId) → AMP
//! `extend=editorialVideo` with an anonymous web JWT scraped from
//! music.apple.com. Everything here fails silent — a miss or a broken token
//! leaves the static artwork in place. Lookups fire only on album change;
//! hits and confirmed misses are cached so each album costs at most one
//! search + one catalog call.

use crate::database;
use crate::utils::read_response_limited;
use reqwest::Client;
use serde::Deserialize;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const ITUNES_SEARCH: &str = "https://itunes.apple.com/search";
const AMP_HOST: &str = "https://amp-api.music.apple.com";
const MUSIC_HOME: &str = "https://music.apple.com";
const ORIGIN: &str = "https://music.apple.com";
const CLIENT_ID: &str = concat!(
    "openNook/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/prodBirdy/openNook)"
);
const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Safari/605.1.15";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_ITUNES_BYTES: usize = 256 * 1024;
const MAX_AMP_BYTES: usize = 512 * 1024;
const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;
const MAX_JS_BYTES: usize = 4 * 1024 * 1024;
const NEGATIVE_TTL_SECS: i64 = 7 * 24 * 60 * 60;
const TOKEN_TTL: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotionArtwork {
    pub m3u8_url: String,
    pub preview_frame: Option<String>,
}

struct TokenCache {
    token: String,
    fetched_at: Instant,
}

fn token_slot() -> &'static Mutex<Option<TokenCache>> {
    static SLOT: OnceLock<Mutex<Option<TokenCache>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn http_client() -> Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .user_agent(BROWSER_UA)
                .build()
                .unwrap_or_else(|_| Client::new())
        })
        .clone()
}

/// Resolve motion art for the current album. `None` on miss, mismatch, or
/// any network/token failure.
pub async fn lookup(artist: &str, album: &str, _title: Option<&str>) -> Option<MotionArtwork> {
    if artist.trim().is_empty() || album.trim().is_empty() {
        return None;
    }
    let key = cache_key(artist, album);
    if let Some(cached) = cache_read(&key) {
        return cached;
    }
    match lookup_uncached(artist, album).await {
        LookupOutcome::Hit(art) => {
            cache_write(&key, Some(&art));
            Some(art)
        }
        LookupOutcome::Miss => {
            cache_write(&key, None);
            None
        }
        LookupOutcome::SoftFail => None,
    }
}

enum LookupOutcome {
    Hit(MotionArtwork),
    Miss,
    SoftFail,
}

async fn lookup_uncached(artist: &str, album: &str) -> LookupOutcome {
    let Some(hit) = search_album(artist, album).await else {
        return LookupOutcome::SoftFail;
    };
    let Some(hit) = hit else {
        return LookupOutcome::Miss;
    };
    match fetch_editorial_video(&hit).await {
        Ok(Some(art)) => LookupOutcome::Hit(art),
        Ok(None) => LookupOutcome::Miss,
        Err(err) => {
            log::debug!("motion art catalog: {err}");
            LookupOutcome::SoftFail
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AlbumHit {
    collection_id: u64,
    storefront: String,
}

async fn search_album(artist: &str, album: &str) -> Option<Option<AlbumHit>> {
    let term = format!("{} {}", artist.trim(), album.trim());
    let response = http_client()
        .get(ITUNES_SEARCH)
        .header(reqwest::header::USER_AGENT, CLIENT_ID)
        .query(&[
            ("term", term.as_str()),
            ("entity", "album"),
            ("media", "music"),
            ("limit", "8"),
        ])
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = read_response_limited(response, MAX_ITUNES_BYTES)
        .await
        .ok()?;
    let parsed: ItunesSearch = serde_json::from_slice(&bytes).ok()?;
    Some(pick_album_hit(artist, album, &parsed))
}

#[derive(Debug, Deserialize)]
struct ItunesSearch {
    #[serde(default)]
    results: Vec<ItunesAlbum>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItunesAlbum {
    collection_id: Option<u64>,
    artist_name: Option<String>,
    collection_name: Option<String>,
    collection_view_url: Option<String>,
}

fn pick_album_hit(artist: &str, album: &str, search: &ItunesSearch) -> Option<AlbumHit> {
    search.results.iter().find_map(|row| {
        let id = row.collection_id?;
        let got_artist = row.artist_name.as_deref().unwrap_or("");
        let got_album = row.collection_name.as_deref().unwrap_or("");
        if !album_names_match(artist, album, got_artist, got_album) {
            return None;
        }
        Some(AlbumHit {
            collection_id: id,
            storefront: storefront_from_url(row.collection_view_url.as_deref())
                .unwrap_or_else(|| "us".into()),
        })
    })
}

async fn fetch_editorial_video(hit: &AlbumHit) -> Result<Option<MotionArtwork>, String> {
    let mut token = match cached_or_fresh_token().await {
        Some(token) => token,
        None => return Err("amp token unavailable".into()),
    };
    let url = format!(
        "{AMP_HOST}/v1/catalog/{}/albums/{}?extend=editorialVideo",
        hit.storefront, hit.collection_id
    );
    let mut response = amp_get(&url, &token).await?;
    if response.status().as_u16() == 401 {
        invalidate_token();
        token = cached_or_fresh_token()
            .await
            .ok_or_else(|| "amp token refresh failed".to_string())?;
        response = amp_get(&url, &token).await?;
    }
    if !response.status().is_success() {
        return Err(format!("amp status {}", response.status()));
    }
    let bytes = read_response_limited(response, MAX_AMP_BYTES).await?;
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
    Ok(parse_editorial_video(&json))
}

async fn amp_get(url: &str, token: &str) -> Result<reqwest::Response, String> {
    http_client()
        .get(url)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(reqwest::header::ORIGIN, ORIGIN)
        .header(reqwest::header::REFERER, format!("{ORIGIN}/"))
        .send()
        .await
        .map_err(|err| err.to_string())
}

async fn cached_or_fresh_token() -> Option<String> {
    if let Some(token) = cached_token() {
        return Some(token);
    }
    let token = scrape_amp_token().await?;
    if let Ok(mut slot) = token_slot().lock() {
        *slot = Some(TokenCache {
            token: token.clone(),
            fetched_at: Instant::now(),
        });
    }
    Some(token)
}

fn cached_token() -> Option<String> {
    let slot = token_slot().lock().ok()?;
    let cached = slot.as_ref()?;
    if cached.fetched_at.elapsed() > TOKEN_TTL {
        return None;
    }
    if cached.token.is_empty() {
        return None;
    }
    Some(cached.token.clone())
}

fn invalidate_token() {
    if let Ok(mut slot) = token_slot().lock() {
        *slot = None;
    }
}

async fn scrape_amp_token() -> Option<String> {
    let html = fetch_text(MUSIC_HOME, MAX_HTML_BYTES).await?;
    let js_path = extract_index_js(&html)?;
    let js_url = if js_path.starts_with("http") {
        js_path
    } else {
        format!("{ORIGIN}{js_path}")
    };
    let js = fetch_text(&js_url, MAX_JS_BYTES).await?;
    extract_jwt(&js)
}

async fn fetch_text(url: &str, max_bytes: usize) -> Option<String> {
    let response = http_client().get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = read_response_limited(response, max_bytes).await.ok()?;
    String::from_utf8(bytes).ok()
}

pub(crate) fn cache_key(artist: &str, album: &str) -> String {
    format!(
        "{}\u{1f}{}",
        artist.trim().to_lowercase(),
        album.trim().to_lowercase()
    )
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `Some(Some(art))` hit, `Some(None)` valid negative, `None` miss/expired.
fn cache_read(key: &str) -> Option<Option<MotionArtwork>> {
    let Ok(conn) = database::get_connection() else {
        return None;
    };
    let (m3u8, preview, hit, fetched_at): (Option<String>, Option<String>, i64, i64) = conn
        .query_row(
            "SELECT m3u8, preview, hit, fetched_at FROM motion_artwork WHERE cache_key = ?1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .ok()?;
    let now = unix_now();
    if hit == 0 {
        if now.saturating_sub(fetched_at) > NEGATIVE_TTL_SECS {
            return None;
        }
        return Some(None);
    }
    let m3u8 = m3u8.filter(|url| !url.is_empty())?;
    Some(Some(MotionArtwork {
        m3u8_url: m3u8,
        preview_frame: preview.filter(|url| !url.is_empty()),
    }))
}

fn cache_write(key: &str, value: Option<&MotionArtwork>) {
    let Ok(conn) = database::get_connection() else {
        return;
    };
    let (m3u8, preview, hit) = match value {
        Some(art) => (Some(art.m3u8_url.as_str()), art.preview_frame.as_deref(), 1i64),
        None => (None, None, 0i64),
    };
    if let Err(err) = conn.execute(
        "INSERT OR REPLACE INTO motion_artwork (cache_key, m3u8, preview, hit, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![key, m3u8, preview, hit, unix_now()],
    ) {
        log::debug!("motion art cache write: {err}");
    }
}

pub(crate) fn normalize_name(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(crate) fn names_match(want: &str, got: &str) -> bool {
    let want = normalize_name(want);
    let got = normalize_name(got);
    if want.is_empty() || got.is_empty() {
        return false;
    }
    want == got || want.contains(&got) || got.contains(&want)
}

pub(crate) fn album_names_match(
    want_artist: &str,
    want_album: &str,
    got_artist: &str,
    got_album: &str,
) -> bool {
    names_match(want_artist, got_artist) && names_match(want_album, got_album)
}

pub(crate) fn storefront_from_url(url: Option<&str>) -> Option<String> {
    let url = url?;
    let after = url.split("music.apple.com/").nth(1)?;
    let code = after.split('/').next()?.to_ascii_lowercase();
    if code.len() == 2 && code.bytes().all(|b| b.is_ascii_lowercase()) {
        Some(code)
    } else {
        None
    }
}

pub(crate) fn extract_index_js(html: &str) -> Option<String> {
    let start = html.find("/assets/index")?;
    let rest = &html[start..];
    let end = rest.find(".js")? + 3;
    let path = &rest[..end];
    if path.len() < 16 || path.len() > 200 || path.contains(['<', '>', '"', '\'']) {
        return None;
    }
    Some(path.to_string())
}

pub(crate) fn extract_jwt(haystack: &str) -> Option<String> {
    let bytes = haystack.as_bytes();
    let mut i = 0;
    while i + 20 < bytes.len() {
        if bytes[i] == b'e' && bytes[i + 1] == b'y' && bytes[i + 2] == b'J' {
            let start = i;
            let mut dots = 0;
            let mut j = i;
            while j < bytes.len() {
                let c = bytes[j];
                if c == b'.' {
                    dots += 1;
                    j += 1;
                    continue;
                }
                if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' {
                    j += 1;
                    continue;
                }
                break;
            }
            if dots == 2 && j.saturating_sub(start) > 40 {
                return Some(haystack[start..j].to_string());
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    None
}

pub(crate) fn parse_editorial_video(json: &serde_json::Value) -> Option<MotionArtwork> {
    let ev = json
        .get("data")?
        .as_array()?
        .first()?
        .get("attributes")?
        .get("editorialVideo")?;
    for key in [
        "motionSquareVideo1x1",
        "motionDetailSquare",
        "motionDetailTall",
    ] {
        let node = match ev.get(key) {
            Some(node) => node,
            None => continue,
        };
        let video = match node.get("video").and_then(|v| v.as_str()) {
            Some(video) if video.contains(".m3u8") => video.to_string(),
            _ => continue,
        };
        let preview = node
            .get("previewFrame")
            .and_then(|frame| frame.get("url").and_then(|v| v.as_str()).or(frame.as_str()))
            .map(str::to_string);
        return Some(MotionArtwork {
            m3u8_url: video,
            preview_frame: preview,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_punctuation_and_case() {
        assert_eq!(normalize_name("Taylor Swift"), "taylor swift");
        assert_eq!(normalize_name("  Folklore (Deluxe) "), "folklore deluxe");
        assert_eq!(normalize_name("A$AP Rocky"), "a ap rocky");
    }

    #[test]
    fn album_match_requires_both_fields() {
        assert!(album_names_match(
            "Taylor Swift",
            "Folklore",
            "Taylor Swift",
            "folklore (deluxe edition)"
        ));
        assert!(!album_names_match(
            "Taylor Swift",
            "Folklore",
            "Taylor Swift",
            "Lover"
        ));
        assert!(!album_names_match(
            "Taylor Swift",
            "Folklore",
            "Bon Iver",
            "Folklore"
        ));
        assert!(!album_names_match("", "Folklore", "Taylor Swift", "Folklore"));
    }

    #[test]
    fn storefront_reads_collection_url() {
        assert_eq!(
            storefront_from_url(Some(
                "https://music.apple.com/gb/album/folklore/1524801260"
            ))
            .as_deref(),
            Some("gb")
        );
        assert_eq!(
            storefront_from_url(Some(
                "https://music.apple.com/us/album/something/1?uo=4"
            ))
            .as_deref(),
            Some("us")
        );
        assert_eq!(storefront_from_url(Some("https://example.com/x")), None);
    }

    #[test]
    fn jwt_is_pulled_from_a_js_bundle_snippet() {
        let js = r#"const t="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0In0.abc_def-0123456789","other""#;
        let token = extract_jwt(js).expect("jwt");
        assert!(token.starts_with("eyJ"));
        assert_eq!(token.matches('.').count(), 2);
        assert!(extract_jwt("no token here").is_none());
        assert!(extract_jwt("eyJshort").is_none());
    }

    #[test]
    fn index_js_path_is_pulled_from_html() {
        let html = r#"<script crossorigin src="/assets/index-a1b2c3d4.js"></script>"#;
        assert_eq!(
            extract_index_js(html).as_deref(),
            Some("/assets/index-a1b2c3d4.js")
        );
        assert!(extract_index_js("<html></html>").is_none());
    }

    #[test]
    fn editorial_video_prefers_square_hls() {
        let json = serde_json::json!({
            "data": [{
                "attributes": {
                    "editorialVideo": {
                        "motionDetailTall": { "video": "https://x/tall.m3u8" },
                        "motionSquareVideo1x1": {
                            "video": "https://x/square.m3u8",
                            "previewFrame": { "url": "https://x/frame.jpg" }
                        }
                    }
                }
            }]
        });
        let art = parse_editorial_video(&json).expect("video");
        assert_eq!(art.m3u8_url, "https://x/square.m3u8");
        assert_eq!(art.preview_frame.as_deref(), Some("https://x/frame.jpg"));
    }

    #[test]
    fn editorial_video_missing_is_none() {
        let json = serde_json::json!({ "data": [{ "attributes": {} }] });
        assert!(parse_editorial_video(&json).is_none());
        let empty = serde_json::json!({ "data": [] });
        assert!(parse_editorial_video(&empty).is_none());
    }

    #[test]
    fn itunes_pick_rejects_fuzzy_wrong_album() {
        let search = ItunesSearch {
            results: vec![ItunesAlbum {
                collection_id: Some(99),
                artist_name: Some("Taylor Swift".into()),
                collection_name: Some("Lover".into()),
                collection_view_url: Some("https://music.apple.com/us/album/lover/1".into()),
            }],
        };
        assert!(pick_album_hit("Taylor Swift", "Folklore", &search).is_none());
    }

    #[test]
    fn itunes_pick_accepts_verified_match() {
        let search = ItunesSearch {
            results: vec![ItunesAlbum {
                collection_id: Some(1524801260),
                artist_name: Some("Taylor Swift".into()),
                collection_name: Some("folklore".into()),
                collection_view_url: Some(
                    "https://music.apple.com/us/album/folklore/1524801260".into(),
                ),
            }],
        };
        let hit = pick_album_hit("Taylor Swift", "Folklore", &search).unwrap();
        assert_eq!(hit.collection_id, 1524801260);
        assert_eq!(hit.storefront, "us");
    }

    #[test]
    fn cache_key_is_stable_and_case_insensitive() {
        assert_eq!(
            cache_key("Taylor Swift", "Folklore"),
            cache_key("taylor swift", " folklore ")
        );
        assert_ne!(
            cache_key("Taylor Swift", "Folklore"),
            cache_key("Taylor Swift", "Lover")
        );
    }

    #[test]
    fn lookup_without_artist_or_album_is_none() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(rt.block_on(lookup("", "Folklore", None)).is_none());
        assert!(rt.block_on(lookup("Taylor Swift", "", None)).is_none());
    }
}
