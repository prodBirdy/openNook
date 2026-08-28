//! `opennook://` URL grammar, tray-path checks, and the external-action bus.
//!
//! LaunchServices, the CLI shim, and Finder Services all push
//! [`ExternalAction`]s here. The island drains the queue on the GPUI
//! foreground executor. **Command execution is never a URL action** — browsers
//! can invoke custom schemes, so mapping any of these to a shell would be RCE.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tokio::sync::Notify;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenNookCommand {
    TrayAdd { paths: Vec<String> },
    TrayClear,
    TimerStart { seconds: u32 },
    Expand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalAction {
    TrayAdd(Vec<PathBuf>),
    TrayClear,
    TimerStart { seconds: u32 },
    Expand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlError {
    WrongScheme,
    Forbidden,
    UnknownAction,
    MissingPath,
    MissingSeconds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    Invalid,
    Missing,
    NotFileOrDir,
}

const SCHEME: &str = "opennook:";

/// Actions a custom-scheme URL must never reach. Browsers can invoke
/// `opennook://…` from a web page.
const FORBIDDEN_SEGMENTS: &[&str] = &[
    "shell", "exec", "run", "cmd", "command", "term", "terminal", "sh", "bash", "zsh",
];

pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    i += 3;
                    continue;
                }
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn looks_like_exec(path: &str) -> bool {
    path.split('/')
        .any(|seg| FORBIDDEN_SEGMENTS.contains(&seg.to_ascii_lowercase().as_str()))
}

fn query_params(query: Option<&str>) -> Vec<(String, String)> {
    let Some(query) = query else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(pair), String::new()),
        })
        .collect()
}

/// Parse an `opennook://` URL. Never returns a shell-exec action.
pub fn parse_opennook_url(raw: &str) -> Result<OpenNookCommand, UrlError> {
    let raw = raw.trim();
    let rest = raw.strip_prefix(SCHEME).ok_or(UrlError::WrongScheme)?;
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let rest = rest.trim_start_matches('/');
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return Err(UrlError::UnknownAction);
    }
    if looks_like_exec(path) {
        return Err(UrlError::Forbidden);
    }
    match path {
        "tray/add" => {
            let paths: Vec<String> = query_params(query)
                .into_iter()
                .filter(|(key, _)| key == "path")
                .map(|(_, value)| value)
                .filter(|value| !value.is_empty())
                .collect();
            if paths.is_empty() {
                return Err(UrlError::MissingPath);
            }
            Ok(OpenNookCommand::TrayAdd { paths })
        }
        "tray/clear" => Ok(OpenNookCommand::TrayClear),
        "timer/start" => {
            let seconds = query_params(query)
                .into_iter()
                .find(|(key, _)| key == "seconds")
                .and_then(|(_, value)| value.parse().ok())
                .ok_or(UrlError::MissingSeconds)?;
            Ok(OpenNookCommand::TimerStart { seconds })
        }
        "expand" => Ok(OpenNookCommand::Expand),
        _ => Err(UrlError::UnknownAction),
    }
}

/// Canonicalize `raw` and require a regular file or directory that exists.
/// Used by `tray/add`, the CLI, and Finder Services — not by shell exec.
pub fn validate_tray_path(raw: &str) -> Result<String, PathError> {
    if raw.is_empty() || raw.contains('\0') {
        return Err(PathError::Invalid);
    }
    let path = Path::new(raw);
    let meta = fs_metadata(path)?;
    if !meta.is_file() && !meta.is_dir() {
        return Err(PathError::NotFileOrDir);
    }
    let canon = std::fs::canonicalize(path).map_err(|_| PathError::Missing)?;
    Ok(canon.to_string_lossy().into_owned())
}

fn fs_metadata(path: &Path) -> Result<std::fs::Metadata, PathError> {
    std::fs::metadata(path).map_err(|_| PathError::Missing)
}

pub fn tray_add_url(paths: &[impl AsRef<Path>]) -> String {
    let mut url = String::from("opennook://tray/add");
    let mut first = true;
    for path in paths {
        url.push(if first { '?' } else { '&' });
        first = false;
        url.push_str("path=");
        url.push_str(&percent_encode(&path.as_ref().to_string_lossy()));
    }
    url
}

pub fn tray_clear_url() -> String {
    "opennook://tray/clear".into()
}

pub fn timer_start_url(seconds: u32) -> String {
    format!("opennook://timer/start?seconds={seconds}")
}

pub fn expand_url() -> String {
    "opennook://expand".into()
}

/// Resolve and validate raw path strings from a parsed `tray/add`.
pub fn validated_paths(raw: &[String]) -> Vec<PathBuf> {
    raw.iter()
        .filter_map(|path| validate_tray_path(path).ok().map(PathBuf::from))
        .collect()
}

fn queue() -> &'static Mutex<Vec<ExternalAction>> {
    static QUEUE: OnceLock<Mutex<Vec<ExternalAction>>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

fn wake() -> &'static Notify {
    static WAKE: OnceLock<Notify> = OnceLock::new();
    WAKE.get_or_init(Notify::new)
}

pub fn push_action(action: ExternalAction) {
    if let Ok(mut guard) = queue().lock() {
        guard.push(action);
    }
    wake().notify_waiters();
}

/// Parse LaunchServices URLs and enqueue only side-effect-safe actions.
/// `file://` (and bare paths) become tray adds after [`validate_tray_path`].
/// Anything that looks like shell execution is dropped.
pub fn ingest_open_urls(urls: &[String]) {
    for url in urls {
        if let Some(path) = file_url_path(url) {
            if let Ok(valid) = validate_tray_path(&path) {
                push_action(ExternalAction::TrayAdd(vec![PathBuf::from(valid)]));
            }
            continue;
        }
        match parse_opennook_url(url) {
            Ok(OpenNookCommand::TrayAdd { paths }) => {
                let valid = validated_paths(&paths);
                if !valid.is_empty() {
                    push_action(ExternalAction::TrayAdd(valid));
                }
            }
            Ok(OpenNookCommand::TrayClear) => push_action(ExternalAction::TrayClear),
            Ok(OpenNookCommand::TimerStart { seconds }) => {
                push_action(ExternalAction::TimerStart { seconds });
            }
            Ok(OpenNookCommand::Expand) => push_action(ExternalAction::Expand),
            Err(UrlError::Forbidden) => {
                log::warn!("ignored forbidden opennook URL (no shell mapping)");
            }
            Err(err) => log::debug!("ignored opennook URL ({err:?}): {url}"),
        }
    }
}

fn file_url_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let path = percent_decode(rest);
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// Block until an external action is available. Event-driven (tokio Notify).
pub async fn recv_action() -> ExternalAction {
    loop {
        if let Some(action) = pop_action() {
            return action;
        }
        wake().notified().await;
    }
}

fn pop_action() -> Option<ExternalAction> {
    queue().lock().ok()?.pop()
}

/// Enqueue already-validated file paths from Finder Services.
pub fn ingest_service_paths(paths: Vec<PathBuf>) {
    let valid: Vec<PathBuf> = paths
        .into_iter()
        .filter_map(|path| validate_tray_path(&path.to_string_lossy()).ok().map(PathBuf::from))
        .collect();
    if !valid.is_empty() {
        push_action(ExternalAction::TrayAdd(valid));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_item(name: &str, dir: bool) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("nook-wp13-{name}-{stamp}"));
        if dir {
            fs::create_dir_all(&path).unwrap();
        } else {
            fs::write(&path, b"ok").unwrap();
        }
        path
    }

    #[test]
    fn parse_tray_add_repeats_path() {
        let cmd = parse_opennook_url(
            "opennook://tray/add?path=%2Ftmp%2Fa.txt&path=%2FUsers%2Fme%2Fb%20c",
        )
        .unwrap();
        assert_eq!(
            cmd,
            OpenNookCommand::TrayAdd {
                paths: vec!["/tmp/a.txt".into(), "/Users/me/b c".into()]
            }
        );
    }

    #[test]
    fn parse_accepts_slash_variants() {
        assert_eq!(
            parse_opennook_url("opennook:tray/clear").unwrap(),
            OpenNookCommand::TrayClear
        );
        assert_eq!(
            parse_opennook_url("opennook:///expand").unwrap(),
            OpenNookCommand::Expand
        );
        assert_eq!(
            parse_opennook_url("opennook://timer/start?seconds=300").unwrap(),
            OpenNookCommand::TimerStart { seconds: 300 }
        );
    }

    #[test]
    fn parse_rejects_wrong_scheme_and_unknown() {
        assert_eq!(
            parse_opennook_url("https://example.com/tray/add?path=/tmp"),
            Err(UrlError::WrongScheme)
        );
        assert_eq!(
            parse_opennook_url("opennook://settings"),
            Err(UrlError::UnknownAction)
        );
        assert_eq!(
            parse_opennook_url("opennook://tray/add"),
            Err(UrlError::MissingPath)
        );
        assert_eq!(
            parse_opennook_url("opennook://timer/start"),
            Err(UrlError::MissingSeconds)
        );
    }

    #[test]
    fn parse_never_maps_urls_to_shell_exec() {
        for url in [
            "opennook://shell/exec?cmd=rm%20-rf%20/",
            "opennook://exec?cmd=id",
            "opennook://run/ls",
            "opennook://cmd/whoami",
            "opennook://command/id",
            "opennook://term/run?cmd=ls",
            "opennook://terminal?cmd=ls",
            "opennook://sh?c=id",
            "opennook://bash?c=id",
            "opennook://zsh?c=id",
            "opennook://tray/add/shell?path=/tmp",
        ] {
            assert_eq!(
                parse_opennook_url(url),
                Err(UrlError::Forbidden),
                "{url}"
            );
        }
    }

    #[test]
    fn builders_round_trip() {
        let url = tray_add_url(&[Path::new("/tmp/hello world.txt")]);
        assert_eq!(
            parse_opennook_url(&url).unwrap(),
            OpenNookCommand::TrayAdd {
                paths: vec!["/tmp/hello world.txt".into()]
            }
        );
        assert_eq!(
            parse_opennook_url(&tray_clear_url()).unwrap(),
            OpenNookCommand::TrayClear
        );
        assert_eq!(
            parse_opennook_url(&timer_start_url(12)).unwrap(),
            OpenNookCommand::TimerStart { seconds: 12 }
        );
        assert_eq!(
            parse_opennook_url(&expand_url()).unwrap(),
            OpenNookCommand::Expand
        );
    }

    #[test]
    fn validate_accepts_file_and_dir() {
        let file = temp_item("file", false);
        let dir = temp_item("dir", true);
        let file_out = validate_tray_path(&file.to_string_lossy()).unwrap();
        let dir_out = validate_tray_path(&dir.to_string_lossy()).unwrap();
        assert!(Path::new(&file_out).is_file());
        assert!(Path::new(&dir_out).is_dir());
        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn validate_rejects_missing_empty_and_nul() {
        assert_eq!(
            validate_tray_path("/no/such/nook-wp13-missing-path"),
            Err(PathError::Missing)
        );
        assert_eq!(validate_tray_path(""), Err(PathError::Invalid));
        assert_eq!(validate_tray_path("a\0b"), Err(PathError::Invalid));
    }

    #[test]
    fn validate_resolves_relative_and_dotdot() {
        let dir = temp_item("rel", true);
        let file = dir.join("inner.txt");
        fs::write(&file, b"x").unwrap();
        let sneaky = dir.join("sub");
        fs::create_dir_all(&sneaky).unwrap();
        let via_dotdot = sneaky.join("..").join("inner.txt");
        let canon = validate_tray_path(&via_dotdot.to_string_lossy()).unwrap();
        assert_eq!(
            Path::new(&canon),
            fs::canonicalize(&file).unwrap().as_path()
        );
        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_url_becomes_tray_path_candidate() {
        assert_eq!(
            file_url_path("file:///tmp/hello%20world"),
            Some("/tmp/hello world".into())
        );
        assert_eq!(file_url_path("opennook://expand"), None);
    }
}
