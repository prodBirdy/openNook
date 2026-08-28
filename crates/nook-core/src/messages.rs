//! iMessage + WhatsApp replies for the island.
//!
//! iMessage is complete: read-only `chat.db` (bodies from `text` or the
//! `attributedBody` typedstream), event-driven WAL watch, send via Messages
//! AppleScript using the chat GUID. WhatsApp is notify + prefill: the Mac
//! app has no scripting dictionary, so incoming rows come from the usernoted
//! store and Reply opens `whatsapp://send?phone=&text=`.
//!
//! `imessage-database` is GPL-3.0 and cannot be linked from this MIT crate;
//! the typedstream helper below follows the public NXTypedStream layout
//! documented by that ecosystem. Message bodies are never written to
//! `opennook.db` — only ROWID watermarks.

use crate::database;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{Connection, OpenFlags};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, OnceLock};
use std::time::Duration;
use tokio::sync::watch;

/// Debounce window after a WAL / usernoted write burst.
pub const WATCH_DEBOUNCE: Duration = Duration::from_millis(300);

/// System Settings deep link for Full Disk Access (no programmatic prompt).
pub const FDA_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// System Settings deep link for Accessibility (experimental WhatsApp Enter).
pub const ACCESSIBILITY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

const GLOBAL_WATERMARK: &str = "*";
const APPLE_EPOCH: i64 = 978_307_200;
const RECENT_LIMIT: usize = 24;
const WHATSAPP_BUNDLES: &[&str] = &[
    "net.whatsapp.WhatsApp",
    "net.whatsapp.WhatsApp.mac",
    "net.whatsapp.WhatsAppDesktop",
    "net.whatsapp.WhatsAppSMBIO",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdaStatus {
    Granted,
    Denied,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageService {
    IMessage,
    Sms,
    WhatsApp,
}

impl MessageService {
    pub fn label(self) -> &'static str {
        match self {
            Self::IMessage => "iMessage",
            Self::Sms => "SMS",
            Self::WhatsApp => "WhatsApp",
        }
    }
}

/// Parsed Messages chat GUID (`iMessage;-;+49…`, `SMS;-;+1…`, `iMessage;+;chat…`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatGuid {
    pub raw: String,
    pub service: String,
    pub is_group: bool,
    pub identifier: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Conversation {
    pub id: String,
    pub service: MessageService,
    pub title: String,
    pub handle: Option<String>,
    pub snippet: String,
    pub from_me: bool,
    pub unread: bool,
    pub last_rowid: i64,
    pub last_date: f64,
    pub is_group: bool,
    /// Present for iMessage / SMS rows that can be sent via AppleScript.
    pub chat_guid: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IncomingPeek {
    pub conversation_id: String,
    pub sender: String,
    pub snippet: String,
    pub service: MessageService,
    /// Unix seconds (`apple_date_to_unix` for iMessage, wall time for WhatsApp).
    pub last_date: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MessagesSnapshot {
    pub conversations: Vec<Conversation>,
    pub incoming: Option<IncomingPeek>,
    pub fda: FdaStatus,
    pub last_rowid: i64,
}

impl Default for MessagesSnapshot {
    fn default() -> Self {
        Self {
            conversations: Vec::new(),
            incoming: None,
            fda: FdaStatus::Unavailable,
            last_rowid: 0,
        }
    }
}

/// `service;+|-;identifier` — rejects quote / newline injection.
pub fn parse_chat_guid(guid: &str) -> Option<ChatGuid> {
    let guid = guid.trim();
    if guid.is_empty() || guid.len() > 256 {
        return None;
    }
    let mut parts = guid.splitn(3, ';');
    let service = parts.next()?.trim();
    let kind = parts.next()?.trim();
    let identifier = parts.next()?.trim();
    if service.is_empty() || identifier.is_empty() {
        return None;
    }
    if !service.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let is_group = match kind {
        "+" => true,
        "-" => false,
        _ => return None,
    };
    if identifier
        .chars()
        .any(|c| c == '"' || c == '\n' || c == '\r' || c == ';' || c == '\\')
    {
        return None;
    }
    Some(ChatGuid {
        raw: guid.to_string(),
        service: service.to_string(),
        is_group,
        identifier: identifier.to_string(),
    })
}

/// Digits-only E.164 (no leading `+`) for `whatsapp://send?phone=`.
pub fn e164_digits(handle: &str) -> Option<String> {
    let trimmed = handle.trim();
    if trimmed.is_empty() || trimmed.contains('@') {
        return None;
    }
    let rest = trimmed
        .strip_prefix("whatsapp:")
        .or_else(|| trimmed.strip_prefix("tel:"))
        .unwrap_or(trimmed);
    let mut digits = String::new();
    let mut had_plus = false;
    for c in rest.chars() {
        if c == '+' && digits.is_empty() && !had_plus {
            had_plus = true;
        } else if c.is_ascii_digit() {
            digits.push(c);
        } else if matches!(c, ' ' | '-' | '(' | ')' | '.') {
            continue;
        } else if !c.is_ascii_whitespace() {
            return None;
        }
    }
    if let Some(stripped) = digits.strip_prefix("00") {
        digits = stripped.to_string();
    } else if digits.starts_with('0') && !had_plus {
        // National trunk prefix without a country code cannot prefill WhatsApp.
        return None;
    }
    if (7..=15).contains(&digits.len()) {
        Some(digits)
    } else {
        None
    }
}

/// `whatsapp://send?phone=<E164>&text=<prefill>` — prefill only, no auto-send.
pub fn whatsapp_send_url(phone: &str, text: &str) -> Option<String> {
    let digits = e164_digits(phone)?;
    Some(format!(
        "whatsapp://send?phone={digits}&text={}",
        url_encode(text)
    ))
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Quote a string for AppleScript (`\` and `"` escaped; newlines become spaces).
pub fn applescript_literal(s: &str) -> String {
    let flat = s.replace(['\n', '\r'], " ");
    format!(
        "\"{}\"",
        flat.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// Extract the first NSString payload from an `attributedBody` typedstream.
///
/// NXTypedStream integer encoding: a byte in `0..=127` is the length;
/// `0x81` / `0x82` / `0x83` introduce 2 / 4 / 8 little-endian bytes.
pub fn extract_typedstream_text(blob: &[u8]) -> Option<String> {
    if blob.is_empty() {
        return None;
    }
    for marker in [b"NSString".as_slice(), b"NSMutableString".as_slice()] {
        let mut search = blob;
        while let Some(pos) = find_bytes(search, marker) {
            let after = &search[pos + marker.len()..];
            if let Some(text) = typedstream_string_after_marker(after) {
                if looks_like_message_text(&text) {
                    return Some(text);
                }
            }
            search = &search[pos + marker.len()..];
        }
    }
    longest_printable_run(blob)
}

fn typedstream_string_after_marker(after: &[u8]) -> Option<String> {
    // Prefer the `+` (0x2B) C-string type tag that follows the class name.
    let window = &after[..after.len().min(32)];
    if let Some(plus) = window.iter().position(|b| *b == 0x2B) {
        if let Some((len, rest)) = read_nx_int(&after[plus + 1..]) {
            return utf8_lossy_trim(rest, len);
        }
    }
    // Fallback: first plausible NX int + UTF-8 run in the next few bytes.
    for skip in 0..after.len().min(16) {
        if let Some((len, rest)) = read_nx_int(&after[skip..]) {
            if (1..=8 * 1024).contains(&len) {
                if let Some(text) = utf8_lossy_trim(rest, len) {
                    if looks_like_message_text(&text) {
                        return Some(text);
                    }
                }
            }
        }
    }
    None
}

fn read_nx_int(data: &[u8]) -> Option<(usize, &[u8])> {
    let first = *data.first()?;
    match first {
        0x81 => {
            let (bytes, rest) = data.get(1..3)?.split_at(2);
            let n = i16::from_le_bytes([bytes[0], bytes[1]]);
            if n < 0 {
                return None;
            }
            Some((n as usize, rest))
        }
        0x82 => {
            let (bytes, rest) = data.get(1..5)?.split_at(4);
            let n = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if n < 0 {
                return None;
            }
            Some((n as usize, rest))
        }
        0x83 => {
            let (bytes, rest) = data.get(1..9)?.split_at(8);
            let n = i64::from_le_bytes(bytes.try_into().ok()?);
            if n < 0 || n > isize::MAX as i64 {
                return None;
            }
            Some((n as usize, rest))
        }
        0x00..=0x7F => Some((first as usize, &data[1..])),
        _ => None,
    }
}

fn utf8_lossy_trim(rest: &[u8], len: usize) -> Option<String> {
    let bytes = rest.get(..len)?;
    let text = String::from_utf8_lossy(bytes).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn looks_like_message_text(text: &str) -> bool {
    if text.is_empty() || text.len() > 8 * 1024 {
        return false;
    }
    let classish = text.contains("NSString")
        || text.contains("NSAttributedString")
        || text.contains("NSDictionary")
        || text.contains("NSObject")
        || text.contains("streamtyped");
    !classish && text.chars().any(|c| !c.is_ascii_control())
}

fn longest_printable_run(blob: &[u8]) -> Option<String> {
    let mut best: Option<String> = None;
    let mut start = None;
    for (i, &b) in blob.iter().enumerate() {
        let printable = (0x20..=0x7E).contains(&b) || b >= 0xC0;
        if printable {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            consider_run(blob, s, i, &mut best);
        }
    }
    if let Some(s) = start {
        consider_run(blob, s, blob.len(), &mut best);
    }
    best
}

fn consider_run(blob: &[u8], start: usize, end: usize, best: &mut Option<String>) {
    if end.saturating_sub(start) < 3 {
        return;
    }
    if let Ok(text) = std::str::from_utf8(&blob[start..end]) {
        let text = text.trim();
        if looks_like_message_text(text)
            && text.len() > best.as_ref().map(|s| s.len()).unwrap_or(0)
        {
            *best = Some(text.to_string());
        }
    }
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Title + body from a usernoted `record.data` blob (bplist / NSKeyedArchiver).
pub fn parse_usernoted_notification(data: &[u8]) -> Option<(String, String)> {
    if data.starts_with(b"bplist") {
        if let Ok(value) = plist::Value::from_reader(Cursor::new(data)) {
            if let Some(found) = walk_plist_notification(&value) {
                return Some(found);
            }
        }
    }
    if let Some(text) = extract_typedstream_text(data) {
        return Some((text, String::new()));
    }
    None
}

fn walk_plist_notification(value: &plist::Value) -> Option<(String, String)> {
    let mut title = None;
    let mut subtitle = None;
    let mut body = None;
    walk_plist(value, &mut title, &mut subtitle, &mut body);
    let title = title.or(subtitle)?;
    Some((title, body.unwrap_or_default()))
}

fn walk_plist(
    value: &plist::Value,
    title: &mut Option<String>,
    subtitle: &mut Option<String>,
    body: &mut Option<String>,
) {
    match value {
        plist::Value::Dictionary(dict) => {
            for (key, child) in dict {
                let k = key.to_ascii_lowercase();
                if let Some(s) = child.as_string() {
                    let s = s.trim();
                    if s.is_empty() {
                        continue;
                    }
                    match k.as_str() {
                        "titl" | "title" => *title = Some(s.to_string()),
                        "subt" | "subtitle" => *subtitle = Some(s.to_string()),
                        "body" => *body = Some(s.to_string()),
                        _ => {}
                    }
                }
                walk_plist(child, title, subtitle, body);
            }
        }
        plist::Value::Array(items) => {
            for child in items {
                walk_plist(child, title, subtitle, body);
            }
        }
        _ => {}
    }
}

pub fn chat_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join("Library/Messages/chat.db")
}

pub fn usernoted_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join("Library/Group Containers/group.com.apple.usernoted/db2/db")
}

pub fn fda_status() -> FdaStatus {
    fda_status_for(&chat_db_path(), &usernoted_db_path())
}

pub fn fda_status_for(chat_db: &Path, usernoted: &Path) -> FdaStatus {
    let chat_exists = chat_db.exists();
    let note_exists = usernoted.exists();
    if !chat_exists && !note_exists {
        return FdaStatus::Unavailable;
    }
    let chat_ok = !chat_exists || open_readonly(chat_db).is_ok();
    let note_ok = !note_exists || open_readonly(usernoted).is_ok();
    if chat_ok && note_ok {
        FdaStatus::Granted
    } else {
        FdaStatus::Denied
    }
}

pub fn open_fda_settings() -> Result<(), String> {
    open::that(FDA_SETTINGS_URL).map_err(|e| e.to_string())
}

pub fn open_accessibility_settings() -> Result<(), String> {
    open::that(ACCESSIBILITY_SETTINGS_URL).map_err(|e| e.to_string())
}

fn open_readonly(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| e.to_string())
}

pub fn snapshot() -> MessagesSnapshot {
    snapshot_from(Some(&chat_db_path()), Some(&usernoted_db_path()))
}

/// Testable snapshot: each path is optional so fixtures can omit a store.
pub fn snapshot_from(chat_db: Option<&Path>, usernoted: Option<&Path>) -> MessagesSnapshot {
    let chat_path = chat_db.unwrap_or_else(|| Path::new(""));
    let note_path = usernoted.unwrap_or_else(|| Path::new(""));
    let fda = if chat_path.as_os_str().is_empty() && note_path.as_os_str().is_empty() {
        FdaStatus::Unavailable
    } else {
        fda_status_for(chat_path, note_path)
    };

    let mut conversations = Vec::new();
    if let Some(path) = chat_db.filter(|p| p.exists()) {
        match read_imessage_conversations(path) {
            Ok(rows) => conversations.extend(rows),
            Err(err) => log::debug!("chat.db read: {err}"),
        }
    }
    if let Some(path) = usernoted.filter(|p| p.exists()) {
        match read_whatsapp_notifications(path) {
            Ok(rows) => conversations.extend(rows),
            Err(err) => log::debug!("usernoted read: {err}"),
        }
    }

    conversations.sort_by(|a, b| {
        b.last_date
            .partial_cmp(&a.last_date)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if conversations.len() > RECENT_LIMIT {
        conversations.truncate(RECENT_LIMIT);
    }

    let max_rowid = conversations
        .iter()
        .filter(|c| c.service != MessageService::WhatsApp)
        .map(|c| c.last_rowid)
        .max()
        .unwrap_or(0);
    let watermark = seed_global_watermark(max_rowid);
    for row in &mut conversations {
        if row.service == MessageService::WhatsApp {
            let seen = get_watermark(&row.id);
            row.unread = row.last_rowid > seen && !row.from_me;
        } else {
            row.unread = row.last_rowid > watermark && !row.from_me;
        }
    }

    let incoming = incoming_peek(&conversations);
    MessagesSnapshot {
        conversations,
        incoming,
        fda,
        last_rowid: max_rowid,
    }
}

pub fn incoming_peek(conversations: &[Conversation]) -> Option<IncomingPeek> {
    conversations
        .iter()
        .filter(|c| c.unread && !c.from_me && !c.snippet.is_empty())
        .max_by(|a, b| {
            a.last_date
                .partial_cmp(&b.last_date)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.last_rowid.cmp(&b.last_rowid))
        })
        .map(|c| IncomingPeek {
            conversation_id: c.id.clone(),
            sender: c.title.clone(),
            snippet: c.snippet.clone(),
            service: c.service,
            last_date: c.last_date,
        })
}

/// Compact HUD clock: `now`, `2m`, `3h`, `1d`.
pub fn relative_time(unix: f64, now: f64) -> String {
    let secs = (now - unix).max(0.0);
    if secs < 45.0 {
        "now".into()
    } else if secs < 3_600.0 {
        format!("{}m", (secs / 60.0).round() as i64)
    } else if secs < 86_400.0 {
        format!("{}h", (secs / 3_600.0).round() as i64)
    } else {
        format!("{}d", (secs / 86_400.0).round() as i64)
    }
}

/// One or two letters for an initials avatar. Phone numbers fall back to `?`.
pub fn sender_initials(name: &str) -> String {
    let words: Vec<char> = name
        .split_whitespace()
        .filter_map(|word| word.chars().find(|c| c.is_alphabetic()))
        .collect();
    if words.len() >= 2 {
        return format!(
            "{}{}",
            words[0].to_uppercase(),
            words[1].to_uppercase()
        );
    }
    if let Some(ch) = words.first() {
        return ch.to_uppercase().to_string();
    }
    "?".into()
}

/// Stable palette index for a sender name (no Contacts photo).
pub fn sender_avatar_rgb(name: &str) -> u32 {
    const PALETTE: [u32; 8] = [
        0x5AC8FA, 0xFF9F0A, 0xBF5AF2, 0xFF375F, 0x30D158, 0x64D2FF, 0xFFD60A, 0xFF453A,
    ];
    let mut h: u32 = 0;
    for b in name.as_bytes() {
        h = h.wrapping_mul(31).wrapping_add(*b as u32);
    }
    PALETTE[h as usize % PALETTE.len()]
}

/// First launch seeds the watermark at the current max ROWID so history
/// does not dump into the compact peek.
pub fn seed_global_watermark(max_rowid: i64) -> i64 {
    let existing = get_watermark(GLOBAL_WATERMARK);
    if existing <= 0 && max_rowid > 0 {
        set_watermark(GLOBAL_WATERMARK, max_rowid);
        max_rowid
    } else {
        existing
    }
}

pub fn mark_conversation_seen(conversation_id: &str, rowid: i64) {
    if conversation_id.is_empty() || rowid <= 0 {
        return;
    }
    set_watermark(conversation_id, rowid);
    let global = get_watermark(GLOBAL_WATERMARK);
    if rowid > global {
        set_watermark(GLOBAL_WATERMARK, rowid);
    }
}

pub fn send_imessage(chat_guid: &str, text: &str) -> Result<(), String> {
    let guid = parse_chat_guid(chat_guid).ok_or_else(|| "invalid chat GUID".to_string())?;
    let body = text.trim();
    if body.is_empty() {
        return Err("empty message".into());
    }
    let script = format!(
        "tell application \"Messages\" to send {} to chat id {}",
        applescript_literal(body),
        applescript_literal(&guid.raw)
    );
    crate::utils::run_osascript(&script).map(|_| ())
}

pub fn reply_whatsapp(phone: &str, text: &str, auto_send: bool) -> Result<(), String> {
    let url = whatsapp_send_url(phone, text)
        .ok_or_else(|| "WhatsApp prefill needs an international phone number".to_string())?;
    open::that(&url).map_err(|e| e.to_string())?;
    if auto_send {
        if let Err(err) = press_whatsapp_send() {
            log::warn!("whatsapp auto-send skipped: {err}");
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn press_whatsapp_send() -> Result<(), String> {
    if !accessibility_trusted() {
        return Err(
            "Accessibility is off — grant it in System Settings to auto-press Return in WhatsApp"
                .into(),
        );
    }
    press_return_key();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn press_whatsapp_send() -> Result<(), String> {
    Err("WhatsApp auto-send is only available on macOS".into())
}

#[cfg(target_os = "macos")]
fn accessibility_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    unsafe { AXIsProcessTrusted() }
}

#[cfg(target_os = "macos")]
fn press_return_key() {
    type CgEventRef = *mut std::ffi::c_void;
    const KEY_RETURN: u16 = 36;
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateKeyboardEvent(
            source: *const std::ffi::c_void,
            virtualKey: u16,
            keyDown: bool,
        ) -> CgEventRef;
        fn CGEventPost(tap: u32, event: CgEventRef);
        fn CFRelease(cf: CgEventRef);
    }
    unsafe {
        let down = CGEventCreateKeyboardEvent(std::ptr::null(), KEY_RETURN, true);
        let up = CGEventCreateKeyboardEvent(std::ptr::null(), KEY_RETURN, false);
        if !down.is_null() {
            CGEventPost(0, down);
            CFRelease(down);
        }
        if !up.is_null() {
            CGEventPost(0, up);
            CFRelease(up);
        }
    }
}

fn gen_tx() -> &'static watch::Sender<u64> {
    static TX: OnceLock<watch::Sender<u64>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, _rx) = watch::channel(0);
        tx
    })
}

/// Bump the generation so the island re-reads. Used on settings toggle.
pub fn request_refresh() {
    let tx = gen_tx();
    let next = tx.borrow().saturating_add(1);
    let _ = tx.send(next);
}

pub fn subscribe() -> watch::Receiver<u64> {
    start_watchers();
    gen_tx().subscribe()
}

/// FSEvents / kqueue (or inotify on Linux) on the Messages and usernoted
/// directories. Idle cost is two dormant watchers; no poll loop.
pub fn start_watchers() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let tx = gen_tx().clone();
        let (evt_tx, evt_rx) = std::sync::mpsc::channel::<()>();
        let mut watchers = Vec::new();
        for root in watch_roots() {
            let etx = evt_tx.clone();
            match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    if event_is_message_store(&event) {
                        let _ = etx.send(());
                    }
                }
            }) {
                Ok(mut watcher) => {
                    if let Err(err) = watcher.watch(&root, RecursiveMode::NonRecursive) {
                        log::debug!("messages watch {root:?}: {err}");
                    } else {
                        log::info!("messages watching {root:?}");
                        watchers.push(watcher);
                    }
                }
                Err(err) => log::debug!("messages watcher: {err}"),
            }
        }
        let _ = evt_tx; // keep a sender only if watchers exist
        if let Err(err) = std::thread::Builder::new()
            .name("nook-messages-watch".into())
            .spawn(move || debounce_loop(evt_rx, tx))
        {
            log::warn!("messages watch thread: {err}");
        }
        static WATCHERS: OnceLock<Mutex<Vec<RecommendedWatcher>>> = OnceLock::new();
        let _ = WATCHERS.set(Mutex::new(watchers));
    });
}

fn watch_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = chat_db_path().parent() {
        if dir.is_dir() {
            roots.push(dir.to_path_buf());
        }
    }
    if let Some(dir) = usernoted_db_path().parent() {
        if dir.is_dir() {
            roots.push(dir.to_path_buf());
        }
    }
    roots
}

fn event_is_message_store(event: &notify::Event) -> bool {
    event.paths.iter().any(|path| {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        name.starts_with("chat.db") || name == "db" || name.starts_with("db-")
    })
}

fn debounce_loop(rx: std::sync::mpsc::Receiver<()>, tx: watch::Sender<u64>) {
    loop {
        if rx.recv().is_err() {
            break;
        }
        loop {
            match rx.recv_timeout(WATCH_DEBOUNCE) {
                Ok(()) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        let next = tx.borrow().saturating_add(1);
        let _ = tx.send(next);
    }
}

fn read_imessage_conversations(path: &Path) -> Result<Vec<Conversation>, String> {
    let conn = open_readonly(path)?;
    let sql = "
        SELECT
            c.guid,
            c.display_name,
            c.chat_identifier,
            c.style,
            m.ROWID,
            m.text,
            m.attributedBody,
            m.is_from_me,
            m.date,
            m.is_read,
            h.id
        FROM chat c
        JOIN (
            SELECT chat_id, MAX(message_id) AS message_id
            FROM chat_message_join
            GROUP BY chat_id
        ) latest ON latest.chat_id = c.ROWID
        JOIN message m ON m.ROWID = latest.message_id
        LEFT JOIN handle h ON h.ROWID = m.handle_id
        ORDER BY m.date DESC
        LIMIT 40
    ";
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let guid: String = row.get(0)?;
            let display_name: Option<String> = row.get(1)?;
            let chat_identifier: Option<String> = row.get(2)?;
            let style: i64 = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
            let rowid: i64 = row.get(4)?;
            let text: Option<String> = row.get(5)?;
            let attributed: Option<Vec<u8>> = row.get(6)?;
            let from_me: i64 = row.get::<_, Option<i64>>(7)?.unwrap_or(0);
            let date: i64 = row.get::<_, Option<i64>>(8)?.unwrap_or(0);
            let handle: Option<String> = row.get(10)?;
            Ok(imessage_row(
                guid,
                display_name,
                chat_identifier,
                style,
                rowid,
                text,
                attributed,
                from_me != 0,
                date,
                handle,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        if let Ok(conv) = row {
            out.push(conv);
        }
    }
    Ok(out)
}

fn imessage_row(
    guid: String,
    display_name: Option<String>,
    chat_identifier: Option<String>,
    style: i64,
    rowid: i64,
    text: Option<String>,
    attributed: Option<Vec<u8>>,
    from_me: bool,
    date: i64,
    handle: Option<String>,
) -> Conversation {
    let parsed = parse_chat_guid(&guid);
    let is_group = parsed
        .as_ref()
        .map(|g| g.is_group)
        .unwrap_or(style == 43);
    let service = match parsed
        .as_ref()
        .map(|g| g.service.to_ascii_lowercase())
        .as_deref()
    {
        Some("sms") | Some("rcs") => MessageService::Sms,
        _ => MessageService::IMessage,
    };
    let handle = handle
        .filter(|s| !s.is_empty())
        .or_else(|| {
            parsed
                .as_ref()
                .filter(|g| !g.is_group)
                .map(|g| g.identifier.clone())
        })
        .or(chat_identifier.clone())
        .filter(|s| !s.is_empty());
    let title = display_name
        .filter(|s| !s.trim().is_empty())
        .or_else(|| handle.clone())
        .or_else(|| chat_identifier.clone())
        .unwrap_or_else(|| "Message".into());
    let snippet = text
        .and_then(|s| {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
        .or_else(|| attributed.as_deref().and_then(extract_typedstream_text))
        .unwrap_or_default();
    Conversation {
        id: guid.clone(),
        service,
        title,
        handle,
        snippet,
        from_me,
        unread: false,
        last_rowid: rowid,
        last_date: apple_date_to_unix(date),
        is_group,
        chat_guid: parsed.map(|g| g.raw),
    }
}

fn apple_date_to_unix(date: i64) -> f64 {
    if date == 0 {
        return 0.0;
    }
    let seconds = if date.abs() > 1_000_000_000_000 {
        date as f64 / 1_000_000_000.0
    } else if date.abs() > 10_000_000_000 {
        date as f64 / 1_000_000.0
    } else {
        date as f64
    };
    APPLE_EPOCH as f64 + seconds
}

fn read_whatsapp_notifications(path: &Path) -> Result<Vec<Conversation>, String> {
    let conn = open_readonly(path)?;
    let tables = table_names(&conn)?;
    if !tables.iter().any(|t| t == "record") {
        return Ok(Vec::new());
    }
    let app_col = if tables.iter().any(|t| t == "app") {
        Some(app_identifier_column(&conn).unwrap_or_else(|| "identifier".into()))
    } else {
        None
    };
    let data_col = first_existing_column(&conn, "record", &["data", "payload"])
        .ok_or_else(|| "usernoted record has no data column".to_string())?;
    let date_col = first_existing_column(&conn, "record", &["delivered_date", "date", "timestamp"]);
    let app_id_col = first_existing_column(&conn, "record", &["app_id", "app"]);

    let sql = match (&app_col, &app_id_col, &date_col) {
        (Some(ident), Some(app_id), Some(date)) => format!(
            "SELECT r.rowid, a.{ident}, r.{date}, r.{data_col}
             FROM record r
             JOIN app a ON a.app_id = r.{app_id}
             ORDER BY r.rowid DESC
             LIMIT 40"
        ),
        (_, Some(app_id), Some(date)) => format!(
            "SELECT r.rowid, r.{app_id}, r.{date}, r.{data_col}
             FROM record r
             ORDER BY r.rowid DESC
             LIMIT 40"
        ),
        (_, _, Some(date)) => format!(
            "SELECT r.rowid, NULL, r.{date}, r.{data_col}
             FROM record r
             ORDER BY r.rowid DESC
             LIMIT 40"
        ),
        _ => format!(
            "SELECT r.rowid, NULL, 0, r.{data_col}
             FROM record r
             ORDER BY r.rowid DESC
             LIMIT 40"
        ),
    };

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let rowid: i64 = row.get(0)?;
            let app: Option<String> = row.get(1)?;
            let date: f64 = match row.get::<_, Option<f64>>(2) {
                Ok(v) => v.unwrap_or(0.0),
                Err(_) => row
                    .get::<_, Option<i64>>(2)
                    .ok()
                    .flatten()
                    .map(|n| n as f64)
                    .unwrap_or(0.0),
            };
            let data: Option<Vec<u8>> = row.get(3)?;
            Ok((rowid, app, date, data))
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        let Ok((rowid, app, date, data)) = row else {
            continue;
        };
        let bundle = app.unwrap_or_default();
        if !is_whatsapp_bundle(&bundle) {
            continue;
        }
        let (title, body) = data
            .as_deref()
            .and_then(parse_usernoted_notification)
            .unwrap_or_else(|| ("WhatsApp".into(), String::new()));
        let handle = e164_digits(&title)
            .or_else(|| e164_digits(&body))
            .map(|d| format!("+{d}"));
        let id = format!("wa:{title}:{rowid}");
        out.push(Conversation {
            id,
            service: MessageService::WhatsApp,
            title,
            handle,
            snippet: body,
            from_me: false,
            unread: false,
            last_rowid: rowid,
            last_date: if date > 1_000_000_000_000.0 {
                date / 1_000_000_000.0
            } else if date > 10_000_000_000.0 {
                date / 1_000.0
            } else {
                date
            },
            is_group: false,
            chat_guid: None,
        });
    }
    Ok(out)
}

fn is_whatsapp_bundle(bundle: &str) -> bool {
    let lower = bundle.to_ascii_lowercase();
    WHATSAPP_BUNDLES
        .iter()
        .any(|id| lower == id.to_ascii_lowercase())
        || lower.contains("whatsapp")
}

fn table_names(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

fn first_existing_column(conn: &Connection, table: &str, names: &[&str]) -> Option<String> {
    let cols = column_names(conn, table).ok()?;
    names
        .iter()
        .find(|n| cols.iter().any(|c| c.eq_ignore_ascii_case(n)))
        .map(|s| (*s).to_string())
}

fn column_names(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

fn app_identifier_column(conn: &Connection) -> Option<String> {
    first_existing_column(conn, "app", &["identifier", "bundle_id", "app", "name"])
}

fn get_watermark(conversation_id: &str) -> i64 {
    let Ok(conn) = database::get_connection() else {
        return 0;
    };
    conn.query_row(
        "SELECT last_rowid FROM message_watermarks WHERE conversation_id = ?1",
        [conversation_id],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

fn set_watermark(conversation_id: &str, rowid: i64) {
    let Ok(conn) = database::get_connection() else {
        return;
    };
    if let Err(err) = conn.execute(
        "INSERT INTO message_watermarks (conversation_id, last_rowid) VALUES (?1, ?2)
         ON CONFLICT(conversation_id) DO UPDATE SET last_rowid = excluded.last_rowid",
        rusqlite::params![conversation_id, rowid],
    ) {
        log::debug!("message watermark: {err}");
    }
}

fn encode_nx_int(n: usize) -> Vec<u8> {
    if n <= 127 {
        vec![n as u8]
    } else if n <= i16::MAX as usize {
        let mut v = vec![0x81];
        v.extend_from_slice(&(n as i16).to_le_bytes());
        v
    } else {
        let mut v = vec![0x82];
        v.extend_from_slice(&(n as i32).to_le_bytes());
        v
    }
}

/// Build a minimal `streamtyped` blob for tests.
pub fn typedstream_for_text(text: &str) -> Vec<u8> {
    let mut blob = b"\x04\x0bstreamtyped".to_vec();
    blob.extend_from_slice(b"NSString");
    blob.push(0x2B);
    blob.extend(encode_nx_int(text.len()));
    blob.extend_from_slice(text.as_bytes());
    blob
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "nook-wp23-{name}-{}-{nanos}.db",
            std::process::id()
        ))
    }

    #[test]
    fn parse_chat_guid_accepts_imessage_sms_and_groups() {
        let one = parse_chat_guid("iMessage;-;+491701234567").unwrap();
        assert_eq!(one.service, "iMessage");
        assert!(!one.is_group);
        assert_eq!(one.identifier, "+491701234567");

        let sms = parse_chat_guid("SMS;-;+15551234567").unwrap();
        assert_eq!(sms.service, "SMS");
        assert!(!sms.is_group);

        let group = parse_chat_guid("iMessage;+;chat838492").unwrap();
        assert!(group.is_group);
        assert_eq!(group.identifier, "chat838492");

        let email = parse_chat_guid("iMessage;-;ada@example.com").unwrap();
        assert_eq!(email.identifier, "ada@example.com");
    }

    #[test]
    fn parse_chat_guid_rejects_injection() {
        assert!(parse_chat_guid("").is_none());
        assert!(parse_chat_guid("iMessage;+49").is_none());
        assert!(parse_chat_guid("iMessage;-;+49\"; beep").is_none());
        assert!(parse_chat_guid("iMessage;-;+49\nfoo").is_none());
        assert!(parse_chat_guid("not a guid").is_none());
        assert!(parse_chat_guid("iMessage;x;+1").is_none());
    }

    #[test]
    fn e164_digits_normalizes_handles() {
        assert_eq!(e164_digits("+49 170 1234567").as_deref(), Some("491701234567"));
        assert_eq!(e164_digits("whatsapp:+15551234567").as_deref(), Some("15551234567"));
        assert_eq!(e164_digits("0015551234567").as_deref(), Some("15551234567"));
        assert_eq!(e164_digits("tel:+14155552671").as_deref(), Some("14155552671"));
        assert!(e164_digits("ada@example.com").is_none());
        assert!(e164_digits("01701234567").is_none());
        assert!(e164_digits("chat838492").is_none());
        assert!(e164_digits("").is_none());
    }

    #[test]
    fn whatsapp_send_url_prefills_without_autosend() {
        let url = whatsapp_send_url("+49 170 1234567", "hello there").unwrap();
        assert_eq!(
            url,
            "whatsapp://send?phone=491701234567&text=hello%20there"
        );
        let escaped = whatsapp_send_url("+15551234567", "ok&go=1").unwrap();
        assert!(escaped.contains("text=ok%26go%3D1"));
        assert!(whatsapp_send_url("not-a-phone", "hi").is_none());
    }

    #[test]
    fn applescript_literal_escapes_quotes_and_newlines() {
        assert_eq!(applescript_literal(r#"hi "there""#), r#""hi \"there\"""#);
        assert_eq!(applescript_literal("line1\nline2"), r#""line1 line2""#);
        assert_eq!(applescript_literal(r#"a\b"#), r#""a\\b""#);
    }

    #[test]
    fn extract_typedstream_text_reads_nx_lengths() {
        let short = typedstream_for_text("hello island");
        assert_eq!(
            extract_typedstream_text(&short).as_deref(),
            Some("hello island")
        );

        let long_body = "x".repeat(200);
        let long = typedstream_for_text(&long_body);
        assert!(long.contains(&0x81), "200-byte payload uses 0x81 length");
        assert_eq!(extract_typedstream_text(&long).as_deref(), Some(long_body.as_str()));

        assert!(extract_typedstream_text(&[]).is_none());
        assert_eq!(
            extract_typedstream_text(b"xxxxNSString garbage").as_deref(),
            None
        );
    }

    #[test]
    fn incoming_peek_picks_newest_unread_not_from_me() {
        let rows = vec![
            Conversation {
                id: "a".into(),
                service: MessageService::IMessage,
                title: "Ada".into(),
                handle: None,
                snippet: "old".into(),
                from_me: false,
                unread: true,
                last_rowid: 1,
                last_date: 10.0,
                is_group: false,
                chat_guid: None,
            },
            Conversation {
                id: "b".into(),
                service: MessageService::WhatsApp,
                title: "Bea".into(),
                handle: None,
                snippet: "new".into(),
                from_me: false,
                unread: true,
                last_rowid: 2,
                last_date: 20.0,
                is_group: false,
                chat_guid: None,
            },
            Conversation {
                id: "c".into(),
                service: MessageService::IMessage,
                title: "Me".into(),
                handle: None,
                snippet: "mine".into(),
                from_me: true,
                unread: true,
                last_rowid: 3,
                last_date: 30.0,
                is_group: false,
                chat_guid: None,
            },
        ];
        let peek = incoming_peek(&rows).unwrap();
        assert_eq!(peek.sender, "Bea");
        assert_eq!(peek.snippet, "new");
        assert_eq!(peek.service, MessageService::WhatsApp);
        assert_eq!(peek.last_date, 20.0);
    }

    #[test]
    fn relative_time_buckets() {
        assert_eq!(relative_time(100.0, 110.0), "now");
        assert_eq!(relative_time(100.0, 220.0), "2m");
        assert_eq!(relative_time(100.0, 7_300.0), "2h");
        assert_eq!(relative_time(100.0, 180_000.0), "2d");
    }

    #[test]
    fn sender_initials_from_name_or_question() {
        assert_eq!(sender_initials("Ada Lovelace"), "AL");
        assert_eq!(sender_initials("Ada"), "A");
        assert_eq!(sender_initials("+49 151 0000"), "?");
        assert_eq!(sender_initials(""), "?");
        assert_eq!(sender_avatar_rgb("Ada"), sender_avatar_rgb("Ada"));
        let palette: Vec<u32> = ["Ada", "Bea", "Cam", "Dee", "Eve", "Fay"]
            .iter()
            .map(|n| sender_avatar_rgb(n))
            .collect();
        assert!(
            palette.windows(2).any(|w| w[0] != w[1]),
            "initials avatars should not all share one fill"
        );
    }

    #[test]
    fn snapshot_reads_fixture_chat_db_and_usernoted() {
        let chat = temp_path("chat");
        let notes = temp_path("noted");
        write_chat_fixture(&chat);
        write_usernoted_fixture(&notes);

        let snap = snapshot_from(Some(&chat), Some(&notes));
        assert_eq!(snap.fda, FdaStatus::Granted);
        assert!(
            snap.conversations
                .iter()
                .any(|c| c.title == "Ada" && c.snippet == "typed hello" && c.chat_guid.is_some())
        );
        assert!(
            snap.conversations
                .iter()
                .any(|c| c.service == MessageService::WhatsApp && c.title == "Bea")
        );

        let _ = fs::remove_file(&chat);
        let _ = fs::remove_file(&notes);
    }

    #[test]
    fn fda_unavailable_when_paths_missing() {
        let missing = std::env::temp_dir().join("nook-wp23-does-not-exist-chat.db");
        let _ = fs::remove_file(&missing);
        assert_eq!(
            fda_status_for(&missing, &missing),
            FdaStatus::Unavailable
        );
    }

    #[test]
    fn parse_usernoted_notification_reads_bplist_keys() {
        let mut req = plist::Dictionary::new();
        req.insert("titl".into(), plist::Value::String("Bea".into()));
        req.insert("body".into(), plist::Value::String("on my way".into()));
        let mut root = plist::Dictionary::new();
        root.insert("req".into(), plist::Value::Dictionary(req));
        let mut buf = Vec::new();
        plist::to_writer_binary(&mut buf, &plist::Value::Dictionary(root)).unwrap();
        assert_eq!(
            parse_usernoted_notification(&buf),
            Some(("Bea".into(), "on my way".into()))
        );
    }

    #[test]
    fn notify_watch_sees_a_write() {
        let dir = std::env::temp_dir().join(format!(
            "nook-wp23-watch-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chat.db-wal");
        fs::write(&path, b"init").unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    if event_is_message_store(&event) {
                        let _ = tx.send(());
                    }
                }
            })
            .unwrap();
        watcher
            .watch(&dir, RecursiveMode::NonRecursive)
            .unwrap();
        fs::write(&path, b"changed").unwrap();
        rx.recv_timeout(Duration::from_secs(3))
            .expect("watcher should wake on WAL write");
        let _ = fs::remove_dir_all(&dir);
    }

    fn write_chat_fixture(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE handle (ROWID INTEGER PRIMARY KEY, id TEXT);
            CREATE TABLE chat (
                ROWID INTEGER PRIMARY KEY,
                guid TEXT,
                display_name TEXT,
                chat_identifier TEXT,
                style INTEGER
            );
            CREATE TABLE message (
                ROWID INTEGER PRIMARY KEY,
                text TEXT,
                attributedBody BLOB,
                is_from_me INTEGER,
                date INTEGER,
                is_read INTEGER,
                handle_id INTEGER
            );
            CREATE TABLE chat_message_join (chat_id INTEGER, message_id INTEGER);
            INSERT INTO handle VALUES (1, '+491701234567');
            INSERT INTO chat VALUES (1, 'iMessage;-;+491701234567', 'Ada', '+491701234567', 45);
            INSERT INTO message VALUES (10, NULL, X'', 0, 1000, 0, 1);
            INSERT INTO chat_message_join VALUES (1, 10);
            ",
        )
        .unwrap();
        let blob = typedstream_for_text("typed hello");
        conn.execute(
            "UPDATE message SET attributedBody = ?1 WHERE ROWID = 10",
            [blob],
        )
        .unwrap();
    }

    fn write_usernoted_fixture(path: &Path) {
        let mut req = plist::Dictionary::new();
        req.insert("titl".into(), plist::Value::String("Bea".into()));
        req.insert("body".into(), plist::Value::String("ping".into()));
        let mut root = plist::Dictionary::new();
        root.insert("req".into(), plist::Value::Dictionary(req));
        let mut buf = Vec::new();
        plist::to_writer_binary(&mut buf, &plist::Value::Dictionary(root)).unwrap();

        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE app (app_id INTEGER PRIMARY KEY, identifier TEXT);
            CREATE TABLE record (
                rec_id INTEGER PRIMARY KEY,
                app_id INTEGER,
                delivered_date REAL,
                data BLOB
            );
            INSERT INTO app VALUES (1, 'net.whatsapp.WhatsApp');
            ",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO record (rec_id, app_id, delivered_date, data) VALUES (5, 1, 1700000000, ?1)",
            [buf],
        )
        .unwrap();
    }
}
