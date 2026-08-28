//! Time-synced lyrics: LRC parser, LRCLIB client, and SQLite cache.
//!
//! Network is one HTTPS request per new track (and only when the lyrics
//! toggle is on). Positive hits stay cached forever; misses expire after
//! seven days. Playback position is interpolated by the island — this
//! module does not poll.

use crate::database;
use crate::models::{LyricLine, SyncedLyrics};
use crate::utils::read_response_limited;
use reqwest::Client;
use serde::Deserialize;
use std::sync::OnceLock;
use std::time::Duration;

const LRCLIB_BASE: &str = "https://lrclib.net";
const CLIENT_ID: &str = concat!(
    "openNook/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/prodBirdy/openNook-gpui)"
);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_BODY_BYTES: usize = 256 * 1024;
const NEGATIVE_TTL_SECS: i64 = 7 * 24 * 60 * 60;
const DURATION_TOLERANCE_SECS: f64 = 2.0;
const MIN_LINE_WAIT: Duration = Duration::from_millis(16);

impl SyncedLyrics {
    pub fn has_synced(&self) -> bool {
        !self.lines.is_empty()
    }

    /// Last line whose timestamp is at or before `pos_ms`. `None` before
    /// the first line.
    pub fn active_index(&self, pos_ms: u64) -> Option<usize> {
        active_line_index(&self.lines, pos_ms)
    }

    /// Delay until the next line after `pos_ms`. `None` on the last line
    /// or when there is nothing to highlight.
    pub fn delay_until_next(&self, pos_ms: u64) -> Option<Duration> {
        if self.lines.is_empty() {
            return None;
        }
        let start = match self.lines.binary_search_by_key(&pos_ms, |line| line.time_ms) {
            Ok(mut i) => {
                while i < self.lines.len() && self.lines[i].time_ms <= pos_ms {
                    i += 1;
                }
                i
            }
            Err(i) => i,
        };
        let next_ms = self.lines.get(start)?.time_ms;
        Some(Duration::from_millis(next_ms.saturating_sub(pos_ms)).max(MIN_LINE_WAIT))
    }

    /// Three-line highlight window: previous, current, next.
    pub fn highlight_window(&self, pos_ms: u64) -> [Option<&str>; 3] {
        match self.active_index(pos_ms) {
            None => [None, None, self.lines.first().map(|line| line.text.as_str())],
            Some(i) => [
                i.checked_sub(1)
                    .and_then(|j| self.lines.get(j))
                    .map(|line| line.text.as_str()),
                self.lines.get(i).map(|line| line.text.as_str()),
                self.lines.get(i + 1).map(|line| line.text.as_str()),
            ],
        }
    }
}

/// Last line with `time_ms <= pos_ms`.
pub fn active_line_index(lines: &[LyricLine], pos_ms: u64) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    match lines.binary_search_by_key(&pos_ms, |line| line.time_ms) {
        Ok(i) => Some(i),
        Err(0) => None,
        Err(i) => Some(i - 1),
    }
}

/// Parse LRC into a time-sorted line list. Metadata tags are skipped;
/// `[offset:±ms]` applies to following lines. Enhanced word tags (`<mm:ss>`)
/// are stripped. Duplicate timestamps keep insertion order after the sort.
pub fn parse_lrc(src: &str) -> Vec<LyricLine> {
    let mut offset_ms: i64 = 0;
    let mut lines = Vec::new();
    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(offset) = parse_offset_tag(line) {
            offset_ms = offset;
            continue;
        }
        if is_id_tag(line) {
            continue;
        }
        let (times, rest) = split_timestamps(line);
        if times.is_empty() {
            continue;
        }
        let text = collapse_ws(&strip_word_tags(rest));
        if text.is_empty() {
            continue;
        }
        for time_ms in times {
            let shifted = (time_ms as i64 + offset_ms).max(0) as u64;
            lines.push(LyricLine {
                time_ms: shifted,
                text: text.clone(),
            });
        }
    }
    lines.sort_by(|a, b| a.time_ms.cmp(&b.time_ms));
    lines
}

/// Fetch lyrics for a track. Cache first; one LRCLIB request on miss.
/// `None` is a miss (instrumental / no lyrics / network error).
pub async fn fetch_for_track(
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration: Option<f64>,
) -> Option<SyncedLyrics> {
    if artist.trim().is_empty() && title.trim().is_empty() {
        return None;
    }
    let key = cache_key(artist, title);
    if let Some(cached) = cache_read(&key) {
        return cached;
    }
    match fetch_lrclib(artist, title, album, duration).await {
        Ok(result) => {
            cache_write(&key, result.as_ref());
            result
        }
        Err(err) => {
            log::debug!("lrclib: {err}");
            None
        }
    }
}

fn cache_key(artist: &str, title: &str) -> String {
    format!(
        "{}\u{1f}{}",
        artist.trim().to_lowercase(),
        title.trim().to_lowercase()
    )
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `Some(Some(lyrics))` hit, `Some(None)` valid negative, `None` miss/expired.
fn cache_read(key: &str) -> Option<Option<SyncedLyrics>> {
    let Ok(conn) = database::get_connection() else {
        return None;
    };
    let (payload, hit, fetched_at): (Option<String>, i64, i64) = conn
        .query_row(
            "SELECT payload, hit, fetched_at FROM lyrics WHERE cache_key = ?1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok()?;
    let now = unix_now();
    if hit == 0 {
        if now.saturating_sub(fetched_at) > NEGATIVE_TTL_SECS {
            return None;
        }
        return Some(None);
    }
    let lyrics = payload.and_then(|json| serde_json::from_str(&json).ok())?;
    Some(Some(lyrics))
}

fn cache_write(key: &str, value: Option<&SyncedLyrics>) {
    let Ok(conn) = database::get_connection() else {
        return;
    };
    let (payload, hit) = match value {
        Some(lyrics) => (serde_json::to_string(lyrics).ok(), 1i64),
        None => (None, 0i64),
    };
    if let Err(err) = conn.execute(
        "INSERT OR REPLACE INTO lyrics (cache_key, payload, hit, fetched_at)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![key, payload, hit, unix_now()],
    ) {
        log::debug!("lyrics cache write: {err}");
    }
}

fn http_client() -> Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .user_agent(CLIENT_ID)
                .build()
                .unwrap_or_else(|_| Client::new())
        })
        .clone()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibTrack {
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    instrumental: Option<bool>,
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    synced_lyrics: Option<String>,
}

impl LrclibTrack {
    fn into_lyrics(self) -> Option<SyncedLyrics> {
        let instrumental = self.instrumental.unwrap_or(false);
        let lines = self
            .synced_lyrics
            .as_deref()
            .map(parse_lrc)
            .unwrap_or_default();
        let plain = self
            .plain_lyrics
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if instrumental {
            return Some(SyncedLyrics {
                lines,
                plain,
                instrumental: true,
                source: "lrclib".into(),
            });
        }
        if lines.is_empty() && plain.is_none() {
            return None;
        }
        Some(SyncedLyrics {
            lines,
            plain,
            instrumental: false,
            source: "lrclib".into(),
        })
    }
}

async fn fetch_lrclib(
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration: Option<f64>,
) -> Result<Option<SyncedLyrics>, String> {
    let client = http_client();
    let mut query: Vec<(&str, String)> = vec![
        ("artist_name", artist.to_string()),
        ("track_name", title.to_string()),
    ];
    if let Some(album) = album.filter(|s| !s.is_empty()) {
        query.push(("album_name", album.to_string()));
    }
    if let Some(secs) = duration.filter(|d| *d > 0.0) {
        query.push(("duration", format!("{secs:.0}")));
    }

    let get = client
        .get(format!("{LRCLIB_BASE}/api/get"))
        .header("Lrclib-Client", CLIENT_ID)
        .query(&query)
        .send()
        .await
        .map_err(|err| err.to_string())?;

    if get.status().is_success() {
        let bytes = read_response_limited(get, MAX_BODY_BYTES).await?;
        if let Ok(track) = serde_json::from_slice::<LrclibTrack>(&bytes) {
            if let Some(lyrics) = track.into_lyrics() {
                return Ok(Some(lyrics));
            }
        }
    } else if get.status() != reqwest::StatusCode::NOT_FOUND {
        return Err(format!("lrclib get {}", get.status()));
    }

    let search = client
        .get(format!("{LRCLIB_BASE}/api/search"))
        .header("Lrclib-Client", CLIENT_ID)
        .query(&[("artist_name", artist), ("track_name", title)])
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !search.status().is_success() {
        return Err(format!("lrclib search {}", search.status()));
    }
    let bytes = read_response_limited(search, MAX_BODY_BYTES).await?;
    let tracks: Vec<LrclibTrack> = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
    Ok(pick_search(tracks, duration))
}

fn pick_search(tracks: Vec<LrclibTrack>, want_duration: Option<f64>) -> Option<SyncedLyrics> {
    let mut best: Option<(i64, SyncedLyrics)> = None;
    for track in tracks {
        let duration = track.duration;
        let Some(lyrics) = track.into_lyrics() else {
            continue;
        };
        let mut score = 0i64;
        if lyrics.has_synced() {
            score += 100;
        } else if lyrics.plain.is_some() {
            score += 20;
        }
        if let (Some(want), Some(got)) = (want_duration, duration) {
            let delta = (want - got).abs();
            if delta <= DURATION_TOLERANCE_SECS {
                score += 50;
            } else {
                score -= delta.min(40.0) as i64;
            }
        }
        match &best {
            Some((best_score, _)) if *best_score >= score => {}
            _ => best = Some((score, lyrics)),
        }
    }
    best.map(|(_, lyrics)| lyrics)
}

fn parse_offset_tag(line: &str) -> Option<i64> {
    let inner = line
        .strip_prefix('[')?
        .strip_suffix(']')?
        .strip_prefix("offset:")?;
    inner.trim().parse().ok()
}

fn is_id_tag(line: &str) -> bool {
    let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return false;
    };
    let name = inner.split_once(':').map(|(k, _)| k).unwrap_or(inner);
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
}

fn split_timestamps(line: &str) -> (Vec<u64>, &str) {
    let mut times = Vec::new();
    let mut rest = line;
    while rest.starts_with('[') {
        let Some(end) = rest.find(']') else {
            break;
        };
        let inner = &rest[1..end];
        let Some(time_ms) = parse_timestamp(inner) else {
            break;
        };
        times.push(time_ms);
        rest = &rest[end + 1..];
    }
    (times, rest)
}

fn parse_timestamp(tag: &str) -> Option<u64> {
    let tag = tag.trim();
    if tag.is_empty() || !tag.chars().next()?.is_ascii_digit() {
        return None;
    }
    let (clock, frac) = match tag.split_once('.') {
        Some((clock, frac)) => (clock, Some(frac)),
        None => (tag, None),
    };
    let segs: Vec<&str> = clock.split(':').collect();
    let (minutes, seconds) = match segs.as_slice() {
        [m, s] => (m.parse::<u64>().ok()?, s.parse::<u64>().ok()?),
        [h, m, s] => {
            let hours = h.parse::<u64>().ok()?;
            let minutes = m.parse::<u64>().ok()?;
            let seconds = s.parse::<u64>().ok()?;
            (hours * 60 + minutes, seconds)
        }
        _ => return None,
    };
    if seconds >= 60 {
        return None;
    }
    let ms = match frac {
        None => 0,
        Some(raw) => {
            let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).take(3).collect();
            if digits.is_empty() {
                return None;
            }
            let n = digits.parse::<u64>().ok()?;
            match digits.len() {
                1 => n * 100,
                2 => n * 10,
                _ => n,
            }
        }
    };
    Some((minutes * 60 + seconds) * 1000 + ms)
}

fn strip_word_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        match rest[start..].find('>') {
            Some(rel_end) => {
                let inner = &rest[start + 1..start + rel_end];
                if parse_timestamp(inner).is_none() {
                    out.push_str(&rest[start..start + rel_end + 1]);
                }
                rest = &rest[start + rel_end + 1..];
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lrc(src: &str) -> Vec<LyricLine> {
        parse_lrc(src)
    }

    #[test]
    fn parse_standard_lrc_times_and_text() {
        let lines = lrc("[00:12.00] Hello\n[00:15.50] World\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].time_ms, 12_000);
        assert_eq!(lines[0].text, "Hello");
        assert_eq!(lines[1].time_ms, 15_500);
        assert_eq!(lines[1].text, "World");
    }

    #[test]
    fn parse_skips_metadata_and_applies_offset() {
        let lines = lrc("[ti:Song]\n[ar:Artist]\n[offset:+500]\n[00:01.00] Hi\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].time_ms, 1_500);
        assert_eq!(lines[0].text, "Hi");
    }

    #[test]
    fn parse_negative_offset_clamps_at_zero() {
        let lines = lrc("[offset:-2000]\n[00:01.00] Hi\n");
        assert_eq!(lines[0].time_ms, 0);
    }

    #[test]
    fn parse_multiple_timestamps_on_one_line() {
        let lines = lrc("[00:10.00][00:20.00] Chorus\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].time_ms, 10_000);
        assert_eq!(lines[1].time_ms, 20_000);
        assert_eq!(lines[0].text, "Chorus");
        assert_eq!(lines[1].text, "Chorus");
    }

    #[test]
    fn parse_centiseconds_milliseconds_and_hours() {
        assert_eq!(parse_timestamp("01:02.5"), Some(62_500));
        assert_eq!(parse_timestamp("01:02.50"), Some(62_500));
        assert_eq!(parse_timestamp("01:02.500"), Some(62_500));
        assert_eq!(parse_timestamp("1:02"), Some(62_000));
        assert_eq!(parse_timestamp("01:02:03.00"), Some(3_723_000));
        assert!(parse_timestamp("ar:Artist").is_none());
        assert!(parse_timestamp("00:99.00").is_none());
    }

    #[test]
    fn parse_strips_enhanced_word_tags() {
        let lines = lrc("[00:12.00] Hello <00:12.50>world\n");
        assert_eq!(lines[0].text, "Hello world");
    }

    #[test]
    fn parse_windows_line_endings_and_blank_lines() {
        let lines = lrc("[00:01.00] A\r\n\r\n[00:02.00] B\r\n");
        assert_eq!(
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["A", "B"]
        );
    }

    #[test]
    fn parse_sorts_out_of_order_lines() {
        let lines = lrc("[00:20.00] Late\n[00:10.00] Early\n");
        assert_eq!(lines[0].text, "Early");
        assert_eq!(lines[1].text, "Late");
    }

    #[test]
    fn active_line_binary_search() {
        let lines = lrc("[00:00.00] A\n[00:10.00] B\n[00:20.00] C\n");
        assert_eq!(active_line_index(&lines, 0), Some(0));
        assert_eq!(active_line_index(&lines, 9_999), Some(0));
        assert_eq!(active_line_index(&lines, 10_000), Some(1));
        assert_eq!(active_line_index(&lines, 19_999), Some(1));
        assert_eq!(active_line_index(&lines, 20_000), Some(2));
        assert_eq!(active_line_index(&lines, 99_999), Some(2));
        assert_eq!(active_line_index(&[], 0), None);
    }

    #[test]
    fn active_line_none_before_first() {
        let lines = lrc("[00:05.00] A\n[00:10.00] B\n");
        assert_eq!(active_line_index(&lines, 0), None);
        assert_eq!(active_line_index(&lines, 4_999), None);
        assert_eq!(active_line_index(&lines, 5_000), Some(0));
    }

    #[test]
    fn delay_until_next_line() {
        let lyrics = SyncedLyrics {
            lines: lrc("[00:01.00] A\n[00:03.00] B\n"),
            ..SyncedLyrics::default()
        };
        assert_eq!(
            lyrics.delay_until_next(0),
            Some(Duration::from_millis(1_000))
        );
        assert_eq!(
            lyrics.delay_until_next(1_000),
            Some(Duration::from_millis(2_000))
        );
        assert!(lyrics.delay_until_next(3_000).is_none());
        assert!(SyncedLyrics::default().delay_until_next(0).is_none());
    }

    #[test]
    fn highlight_window_tracks_active_line() {
        let lyrics = SyncedLyrics {
            lines: lrc("[00:00.00] A\n[00:10.00] B\n[00:20.00] C\n"),
            ..SyncedLyrics::default()
        };
        assert_eq!(lyrics.highlight_window(0), [None, Some("A"), Some("B")]);
        assert_eq!(
            lyrics.highlight_window(10_000),
            [Some("A"), Some("B"), Some("C")]
        );
        assert_eq!(lyrics.highlight_window(20_000), [Some("B"), Some("C"), None]);
        let late = SyncedLyrics {
            lines: lrc("[00:05.00] A\n"),
            ..SyncedLyrics::default()
        };
        assert_eq!(late.highlight_window(0), [None, None, Some("A")]);
    }

    #[test]
    fn cache_key_is_case_insensitive() {
        assert_eq!(
            cache_key(" Radiohead ", " Karma Police "),
            cache_key("radiohead", "karma police")
        );
    }
}
