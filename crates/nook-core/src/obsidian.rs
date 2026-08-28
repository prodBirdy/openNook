//! Obsidian vault access without a companion plugin.
//!
//! Discovery reads `obsidian.json`, the note index is a walk of `*.md`
//! (skipping `.obsidian/` and `.trash/`), daily notes use the Moment.js
//! token subset from `daily-notes.json`, and navigation is `obsidian://`.
//! FSEvents (via `notify`) is push-based — no poll loop.

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_FORMAT: &str = "YYYY-MM-DD";
const MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAYS_FULL: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// A vault Obsidian already knows about (`obsidian.json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownVault {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub open: bool,
}

/// Lightweight index row: relative path + mtime, no body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteEntry {
    pub rel_path: String,
    pub title: String,
    pub mtime: SystemTime,
}

/// Daily-note folder / Moment format / optional template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DailyNotesConfig {
    pub folder: String,
    pub format: String,
    pub template: Option<String>,
}

impl Default for DailyNotesConfig {
    fn default() -> Self {
        Self {
            folder: String::new(),
            format: DEFAULT_FORMAT.into(),
            template: None,
        }
    }
}

/// Calendar date used by the Moment.js token translator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    /// 0 = Sunday … 6 = Saturday (Moment.js `ddd` / `dddd`).
    pub weekday: u8,
}

impl CivilDate {
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let weekday = weekday_sun0(year, month, day)?;
        Some(Self {
            year,
            month,
            day,
            weekday,
        })
    }

    pub fn today() -> Self {
        let days = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 86_400;
        civil_from_unix_days(days as i64)
    }
}

/// Keeps the FSEvents / inotify debouncer alive. Dropping it tears the watch down.
pub struct VaultWatch {
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

/// Path to Obsidian's vault registry, if the default location exists on this OS.
pub fn obsidian_config_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|home| home.join("Library/Application Support/obsidian/obsidian.json"))
    }
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir().map(|dir| dir.join("obsidian").join("obsidian.json"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::config_dir().map(|dir| dir.join("obsidian").join("obsidian.json"))
    }
}

/// Vaults Obsidian has registered. Empty when Obsidian is not installed.
pub fn discover_vaults() -> Vec<KnownVault> {
    let Some(path) = obsidian_config_path() else {
        return Vec::new();
    };
    let Ok(json) = fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_obsidian_json(&json)
}

/// Parse an `obsidian.json` document (vault-id → `{path, open}`).
pub fn parse_obsidian_json(json: &str) -> Vec<KnownVault> {
    let parsed: ObsidianJson = match serde_json::from_str(json) {
        Ok(parsed) => parsed,
        Err(_) => return Vec::new(),
    };
    let mut vaults: Vec<KnownVault> = parsed
        .vaults
        .into_iter()
        .filter_map(|(id, entry)| {
            if entry.path.is_empty() {
                return None;
            }
            let path = PathBuf::from(entry.path);
            let name = vault_name(&path);
            Some(KnownVault {
                id,
                path,
                name,
                open: entry.open,
            })
        })
        .collect();
    vaults.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    vaults
}

/// Folder basename; used as the `vault=` URL parameter.
pub fn vault_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// True when `path` sits under the vault's `.obsidian/` or `.trash/` tree.
pub fn is_ignored_path(vault: &Path, path: &Path) -> bool {
    let rel = match path.strip_prefix(vault) {
        Ok(rel) => rel,
        Err(_) => return path == vault,
    };
    rel.components().any(|component| {
        let name = component.as_os_str();
        name == ".obsidian" || name == ".trash"
    })
}

/// Walk `*.md` under `vault`, skipping ignored trees. Sorted newest first.
pub fn index_vault(vault: &Path) -> Vec<NoteEntry> {
    if !vault.is_dir() {
        return Vec::new();
    }
    let mut notes = Vec::new();
    for entry in walkdir::WalkDir::new(vault)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            !is_ignored_path(vault, entry.path())
        })
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        if is_ignored_path(vault, path) {
            continue;
        }
        let Ok(rel) = path.strip_prefix(vault) else {
            continue;
        };
        let rel_path = rel.to_string_lossy().replace('\\', "/");
        let title = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| rel_path.clone());
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .unwrap_or(UNIX_EPOCH);
        notes.push(NoteEntry {
            rel_path,
            title,
            mtime,
        });
    }
    notes.sort_by(|a, b| {
        b.mtime
            .cmp(&a.mtime)
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });
    notes
}

/// Stat only the reported paths and patch `index` in place.
pub fn patch_index(index: &mut Vec<NoteEntry>, vault: &Path, paths: &[PathBuf]) {
    for path in paths {
        if is_ignored_path(vault, path) {
            continue;
        }
        let Ok(rel) = path.strip_prefix(vault) else {
            continue;
        };
        let rel_path = rel.to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            if !path.exists() {
                let prefix = if rel_path.ends_with('/') {
                    rel_path.clone()
                } else {
                    format!("{rel_path}/")
                };
                index.retain(|note| {
                    note.rel_path != rel_path && !note.rel_path.starts_with(&prefix)
                });
            }
            continue;
        }
        if !rel_path.ends_with(".md") {
            continue;
        }
        if path.is_file() {
            let title = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| rel_path.clone());
            let mtime = path
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(UNIX_EPOCH);
            if let Some(existing) = index.iter_mut().find(|note| note.rel_path == rel_path) {
                existing.mtime = mtime;
                existing.title = title;
            } else {
                index.push(NoteEntry {
                    rel_path,
                    title,
                    mtime,
                });
            }
        } else {
            index.retain(|note| note.rel_path != rel_path);
        }
    }
    index.sort_by(|a, b| {
        b.mtime
            .cmp(&a.mtime)
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });
}

/// Daily-note settings: core plugin first, Periodic Notes as fallback.
pub fn read_daily_notes_config(vault: &Path) -> DailyNotesConfig {
    let core = vault.join(".obsidian").join("daily-notes.json");
    if let Ok(json) = fs::read_to_string(core) {
        if let Some(config) = parse_daily_notes_json(&json) {
            return config;
        }
    }
    let periodic = vault
        .join(".obsidian")
        .join("plugins")
        .join("periodic-notes")
        .join("data.json");
    if let Ok(json) = fs::read_to_string(periodic) {
        if let Some(config) = parse_periodic_notes_json(&json) {
            return config;
        }
    }
    DailyNotesConfig::default()
}

pub fn parse_daily_notes_json(json: &str) -> Option<DailyNotesConfig> {
    let parsed: DailyNotesFile = serde_json::from_str(json).ok()?;
    Some(DailyNotesConfig {
        folder: parsed.folder.unwrap_or_default(),
        format: nonempty_format(parsed.format),
        template: nonempty_opt(parsed.template),
    })
}

pub fn parse_periodic_notes_json(json: &str) -> Option<DailyNotesConfig> {
    let parsed: PeriodicNotesFile = serde_json::from_str(json).ok()?;
    let daily = parsed.daily?;
    Some(DailyNotesConfig {
        folder: daily.folder.unwrap_or_default(),
        format: nonempty_format(daily.format),
        template: nonempty_opt(daily.template),
    })
}

/// Translate a Moment.js daily-note format. Unknown tokens stay literal.
/// Empty / unusable formats fall back to `YYYY-MM-DD`.
pub fn format_daily_note(format: &str, date: CivilDate) -> String {
    let pattern = format.trim();
    if pattern.is_empty() {
        return format_daily_note(DEFAULT_FORMAT, date);
    }
    let mut out = String::with_capacity(pattern.len() + 8);
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some((len, text)) = match_token(&pattern[i..], date) {
            out.push_str(&text);
            i += len;
        } else {
            let ch = pattern[i..].chars().next().unwrap_or('?');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    if out.is_empty() {
        return format_daily_note(DEFAULT_FORMAT, date);
    }
    out
}

/// Relative path of today's (or `date`'s) daily note, including `.md`.
pub fn daily_note_rel_path(config: &DailyNotesConfig, date: CivilDate) -> String {
    let formatted = format_daily_note(&config.format, date).replace('\\', "/");
    let file = if formatted.ends_with(".md") {
        formatted
    } else {
        format!("{formatted}.md")
    };
    let folder = config.folder.replace('\\', "/");
    let folder = folder.trim().trim_matches('/');
    if folder.is_empty() {
        file
    } else {
        format!("{folder}/{file}")
    }
}

pub fn read_note(vault: &Path, rel: &str) -> Result<String, String> {
    let path = vault_join(vault, rel)?;
    fs::read_to_string(path).map_err(|err| err.to_string())
}

/// Temp-file + rename in the vault directory so Obsidian reloads cleanly.
pub fn write_note(vault: &Path, rel: &str, body: &str) -> Result<(), String> {
    let dest = vault_join(vault, rel)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let tmp = dest.with_extension("md.tmp-nook");
    fs::write(&tmp, body).map_err(|err| err.to_string())?;
    fs::rename(&tmp, &dest).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        err.to_string()
    })?;
    Ok(())
}

/// Create the daily note from the template (or a `#` heading) if it is missing.
pub fn ensure_daily_note(
    vault: &Path,
    config: &DailyNotesConfig,
    date: CivilDate,
) -> Result<String, String> {
    let rel = daily_note_rel_path(config, date);
    let dest = vault_join(vault, &rel)?;
    if dest.is_file() {
        return Ok(rel);
    }
    let title = dest
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| format_daily_note(DEFAULT_FORMAT, date));
    let body = match config.template.as_deref() {
        Some(template) if !template.trim().is_empty() => {
            let template_rel = template.trim().trim_start_matches('/');
            let template_rel = if template_rel.ends_with(".md") {
                template_rel.to_string()
            } else {
                format!("{template_rel}.md")
            };
            match read_note(vault, &template_rel) {
                Ok(text) => text,
                Err(_) => format!("# {title}\n"),
            }
        }
        _ => format!("# {title}\n"),
    };
    write_note(vault, &rel, &body)?;
    Ok(rel)
}

/// Append `text` as a list item under `heading`, or at EOF when heading is empty.
pub fn append_capture(
    vault: &Path,
    rel: &str,
    heading: Option<&str>,
    text: &str,
) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty capture".into());
    }
    let existing = read_note(vault, rel).unwrap_or_default();
    let next = append_capture_body(&existing, heading, text);
    write_note(vault, rel, &next)
}

/// Ensure today's daily note exists, then append `text`.
pub fn capture_to_daily(
    vault: &Path,
    heading: Option<&str>,
    text: &str,
    use_uri: bool,
) -> Result<String, String> {
    let config = read_daily_notes_config(vault);
    let date = CivilDate::today();
    let rel = daily_note_rel_path(&config, date);
    if use_uri {
        let name = vault_name(vault);
        let file = rel.trim_end_matches(".md");
        open_url(&new_append_url(&name, file, text))?;
        return Ok(rel);
    }
    ensure_daily_note(vault, &config, date)?;
    append_capture(vault, &rel, heading, text)?;
    Ok(rel)
}

pub fn append_capture_body(existing: &str, heading: Option<&str>, text: &str) -> String {
    let line = format!("- {text}");
    let heading = heading.map(str::trim).filter(|h| !h.is_empty());
    let Some(heading) = heading else {
        let mut body = existing.to_string();
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&line);
        body.push('\n');
        return body;
    };
    if let Some(at) = find_heading_insert(existing, heading) {
        let mut body = String::with_capacity(existing.len() + line.len() + 2);
        body.push_str(&existing[..at]);
        if at > 0 && !existing[..at].ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&line);
        body.push('\n');
        body.push_str(&existing[at..]);
        return body;
    }
    let mut body = existing.to_string();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    if !body.is_empty() {
        body.push('\n');
    }
    body.push_str("## ");
    body.push_str(heading);
    body.push('\n');
    body.push_str(&line);
    body.push('\n');
    body
}

pub fn open_file_url(vault: &str, rel_path: &str) -> String {
    let file = rel_path.replace('\\', "/");
    let file = file.trim_start_matches('/');
    let file = file.strip_suffix(".md").unwrap_or(file);
    format!(
        "obsidian://open?vault={}&file={}",
        encode_component(vault),
        encode_component(file)
    )
}

pub fn open_path_url(abs: &Path) -> String {
    format!(
        "obsidian://open?path={}",
        encode_component(&abs.to_string_lossy())
    )
}

pub fn new_append_url(vault: &str, file: &str, content: &str) -> String {
    let file = file.replace('\\', "/");
    let file = file.trim_start_matches('/');
    let file = file.strip_suffix(".md").unwrap_or(&file);
    format!(
        "obsidian://new?vault={}&file={}&content={}&append=true",
        encode_component(vault),
        encode_component(file),
        encode_component(content)
    )
}

pub fn search_url(vault: &str, query: &str) -> String {
    format!(
        "obsidian://search?vault={}&query={}",
        encode_component(vault),
        encode_component(query)
    )
}

pub fn open_url(url: &str) -> Result<(), String> {
    open::that(url).map_err(|err| err.to_string())
}

/// Recursive FSEvents / inotify watch. Events under `.obsidian/` / `.trash/`
/// are dropped in the callback so workspace.json churn never reaches the UI.
pub fn watch_vault(
    root: PathBuf,
) -> Result<
    (
        VaultWatch,
        tokio::sync::mpsc::UnboundedReceiver<Vec<PathBuf>>,
    ),
    String,
> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let vault = root.clone();
    let mut debouncer = notify_debouncer_mini::new_debouncer(
        Duration::from_secs(1),
        move |res: notify_debouncer_mini::DebounceEventResult| {
            let Ok(events) = res else {
                return;
            };
            let paths: Vec<PathBuf> = events
                .into_iter()
                .map(|event| event.path)
                .filter(|path| !is_ignored_path(&vault, path))
                .collect();
            if !paths.is_empty() {
                let _ = tx.send(paths);
            }
        },
    )
    .map_err(|err| err.to_string())?;
    debouncer
        .watcher()
        .watch(&root, notify::RecursiveMode::Recursive)
        .map_err(|err| err.to_string())?;
    Ok((
        VaultWatch {
            _debouncer: debouncer,
        },
        rx,
    ))
}

#[derive(Deserialize)]
struct ObsidianJson {
    #[serde(default)]
    vaults: HashMap<String, VaultJson>,
}

#[derive(Deserialize)]
struct VaultJson {
    path: String,
    #[serde(default)]
    open: bool,
}

#[derive(Deserialize)]
struct DailyNotesFile {
    format: Option<String>,
    folder: Option<String>,
    template: Option<String>,
}

#[derive(Deserialize)]
struct PeriodicNotesFile {
    daily: Option<DailyNotesFile>,
}

fn nonempty_format(format: Option<String>) -> String {
    format
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_FORMAT.into())
}

fn nonempty_opt(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn match_token(rest: &str, date: CivilDate) -> Option<(usize, String)> {
    let month = date.month.saturating_sub(1) as usize;
    let month = month.min(11);
    let weekday = (date.weekday as usize).min(6);
    let candidates: [(&str, String); 10] = [
        ("YYYY", format!("{:04}", date.year)),
        ("MMMM", MONTHS_FULL[month].to_string()),
        ("dddd", WEEKDAYS_FULL[weekday].to_string()),
        ("MMM", MONTHS_SHORT[month].to_string()),
        ("ddd", WEEKDAYS_SHORT[weekday].to_string()),
        ("YY", format!("{:02}", date.year.rem_euclid(100))),
        ("MM", format!("{:02}", date.month)),
        ("DD", format!("{:02}", date.day)),
        ("M", date.month.to_string()),
        ("D", date.day.to_string()),
    ];
    for (token, text) in candidates {
        if rest.starts_with(token) {
            return Some((token.len(), text));
        }
    }
    None
}

fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn vault_join(vault: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.replace('\\', "/");
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() {
        return Err("empty path".into());
    }
    if rel.split('/').any(|part| part == "..") {
        return Err("path escapes vault".into());
    }
    Ok(vault.join(rel))
}

fn find_heading_insert(body: &str, heading: &str) -> Option<usize> {
    let needle = heading.trim();
    let mut pos = 0;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim();
        let title = trimmed
            .strip_prefix("###### ")
            .or_else(|| trimmed.strip_prefix("##### "))
            .or_else(|| trimmed.strip_prefix("#### "))
            .or_else(|| trimmed.strip_prefix("### "))
            .or_else(|| trimmed.strip_prefix("## "))
            .or_else(|| trimmed.strip_prefix("# "));
        if title == Some(needle) {
            let after = pos + line.len();
            return Some(heading_section_end(body, after));
        }
        pos += line.len();
    }
    None
}

fn heading_section_end(body: &str, start: usize) -> usize {
    let rest = &body[start..];
    let mut offset = 0;
    let mut end = rest.len();
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim();
        if is_markdown_heading(trimmed) {
            end = offset;
            break;
        }
        offset += line.len();
    }
    start + rest[..end].trim_end().len()
}

fn is_markdown_heading(trimmed: &str) -> bool {
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && trimmed.chars().nth(hashes) == Some(' ')
}

fn weekday_sun0(year: i32, month: u8, day: u8) -> Option<u8> {
    if !(1..=12).contains(&month) {
        return None;
    }
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = year;
    if month < 3 {
        y -= 1;
    }
    Some(((y + y / 4 - y / 100 + y / 400 + T[month as usize - 1] + i32::from(day)) % 7) as u8)
}

fn civil_from_unix_days(days: i64) -> CivilDate {
    // Howard Hinnant, days_from_civil inverse. Unix epoch is 1970-01-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u8;
    let year = (y + i64::from(month <= 2)) as i32;
    let weekday = ((days + 4).rem_euclid(7)) as u8;
    CivilDate {
        year,
        month,
        day,
        weekday,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn date(y: i32, m: u8, d: u8) -> CivilDate {
        CivilDate::new(y, m, d).expect("valid date")
    }

    #[test]
    fn daily_note_format_tokens_and_nested_folders() {
        let d = date(2026, 8, 28);
        assert_eq!(d.weekday, 5, "2026-08-28 is Friday");
        assert_eq!(format_daily_note("YYYY-MM-DD", d), "2026-08-28");
        assert_eq!(format_daily_note("YY/M/D", d), "26/8/28");
        assert_eq!(
            format_daily_note("YYYY/MM-MMMM/YYYY-MM-DD", d),
            "2026/08-August/2026-08-28"
        );
        assert_eq!(format_daily_note("ddd D MMM YYYY", d), "Fri 28 Aug 2026");
        assert_eq!(format_daily_note("dddd", d), "Friday");
        assert_eq!(format_daily_note("", d), "2026-08-28");
        assert_eq!(format_daily_note("  ", d), "2026-08-28");
    }

    #[test]
    fn daily_note_rel_path_joins_folder_and_extension() {
        let d = date(2026, 1, 5);
        let config = DailyNotesConfig {
            folder: "Daily/".into(),
            format: "YYYY-MM-DD".into(),
            template: None,
        };
        assert_eq!(daily_note_rel_path(&config, d), "Daily/2026-01-05.md");
        let nested = DailyNotesConfig {
            folder: String::new(),
            format: "YYYY/MM-MMMM/YYYY-MM-DD".into(),
            template: None,
        };
        assert_eq!(
            daily_note_rel_path(&nested, date(2026, 8, 28)),
            "2026/08-August/2026-08-28.md"
        );
    }

    #[test]
    fn url_builders_percent_encode() {
        assert_eq!(
            open_file_url("My Vault", "Daily/2026-08-28.md"),
            "obsidian://open?vault=My%20Vault&file=Daily/2026-08-28"
        );
        assert_eq!(
            open_path_url(Path::new("/Users/me/Notes/Hi there.md")),
            "obsidian://open?path=/Users/me/Notes/Hi%20there.md"
        );
        assert_eq!(
            new_append_url("Vault", "inbox.md", "hello world"),
            "obsidian://new?vault=Vault&file=inbox&content=hello%20world&append=true"
        );
        assert_eq!(
            search_url("Vault", "tag:#work"),
            "obsidian://search?vault=Vault&query=tag%3A%23work"
        );
    }

    #[test]
    fn parse_obsidian_json_sorts_by_name() {
        let json = r#"{
            "vaults": {
                "b": {"path": "/tmp/Zebra", "open": false},
                "a": {"path": "/tmp/Alpha", "open": true}
            }
        }"#;
        let vaults = parse_obsidian_json(json);
        assert_eq!(vaults.len(), 2);
        assert_eq!(vaults[0].name, "Alpha");
        assert!(vaults[0].open);
        assert_eq!(vaults[1].name, "Zebra");
    }

    #[test]
    fn ignored_paths_filter_obsidian_and_trash() {
        let vault = Path::new("/tmp/vault");
        assert!(is_ignored_path(
            vault,
            Path::new("/tmp/vault/.obsidian/workspace.json")
        ));
        assert!(is_ignored_path(
            vault,
            Path::new("/tmp/vault/.trash/gone.md")
        ));
        assert!(!is_ignored_path(
            vault,
            Path::new("/tmp/vault/Daily/today.md")
        ));
        assert!(!is_ignored_path(vault, vault));
    }

    #[test]
    fn append_capture_body_heading_and_eof() {
        let eof = append_capture_body("# Today\n", None, "hello");
        assert_eq!(eof, "# Today\n- hello\n");

        let under = append_capture_body(
            "# Today\n\n## Inbox\n- old\n\n## Later\n",
            Some("Inbox"),
            "new",
        );
        assert!(under.contains("## Inbox\n- old\n- new\n"), "{under}");
        assert!(under.contains("## Later\n"), "{under}");

        let created = append_capture_body("# Today\n", Some("Inbox"), "first");
        assert_eq!(created, "# Today\n\n## Inbox\n- first\n");
    }

    #[test]
    fn index_skips_hidden_trees_and_reads_md() {
        let root = std::env::temp_dir().join(format!("nook-obsidian-idx-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Daily")).unwrap();
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join(".trash")).unwrap();
        fs::write(root.join("Daily/hello.md"), "# hi\n").unwrap();
        fs::write(root.join(".obsidian/workspace.json"), "{}").unwrap();
        fs::write(root.join(".trash/gone.md"), "nope").unwrap();
        fs::write(root.join("readme.txt"), "skip").unwrap();

        let notes = index_vault(&root);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].rel_path, "Daily/hello.md");
        assert_eq!(notes[0].title, "hello");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_and_capture_round_trip() {
        let root = std::env::temp_dir().join(format!("nook-obsidian-rw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write_note(&root, "Daily/note.md", "# Note\n").unwrap();
        append_capture(&root, "Daily/note.md", Some("Inbox"), "captured").unwrap();
        let body = read_note(&root, "Daily/note.md").unwrap();
        assert!(body.contains("# Note"));
        assert!(body.contains("## Inbox"));
        assert!(body.contains("- captured"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unix_epoch_civil_date_is_thursday() {
        let epoch = civil_from_unix_days(0);
        assert_eq!(
            (epoch.year, epoch.month, epoch.day, epoch.weekday),
            (1970, 1, 1, 4)
        );
        let today = CivilDate::today();
        assert!((1..=12).contains(&today.month));
        assert!((1..=31).contains(&today.day));
        assert!(today.weekday <= 6);
    }
}
