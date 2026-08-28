//! Cross-app notification shelf.
//!
//! Primary capture is an AXObserver on Notification Center banners
//! (`kAXWindowCreatedNotification`). Optional backfill reads the usernoted
//! SQLite store after an explicit Full Disk Access opt-in, watching db-wal
//! with kqueue — never a timed poll.
//!
//! `UNUserNotificationCenter.getDeliveredNotifications` is own-app only and
//! is not used here.

use crate::database;
use crate::settings::AppSettings;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

pub const SHELF_CAP: usize = 64;
const DEDUP_SECS: i64 = 2;

/// One captured notification, from either backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationEvent {
    pub id: String,
    pub bundle_id: String,
    pub app_name: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    /// Unix seconds.
    pub delivered_at: i64,
    pub unread: bool,
}

impl NotificationEvent {
    pub fn new(
        bundle_id: impl Into<String>,
        app_name: impl Into<String>,
        title: impl Into<String>,
        subtitle: impl Into<String>,
        body: impl Into<String>,
        delivered_at: i64,
    ) -> Self {
        let bundle_id = bundle_id.into();
        let app_name = app_name.into();
        let title = title.into();
        let subtitle = subtitle.into();
        let body = body.into();
        let id = event_id(&bundle_id, &title, &body, delivered_at);
        Self {
            id,
            bundle_id,
            app_name,
            title,
            subtitle,
            body,
            delivered_at,
            unread: true,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty() && self.body.trim().is_empty()
    }
}

/// Capture backend. AX is real-time; the DB reader is FDA-gated backfill.
pub trait NotificationBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn start(&self);
}

/// Bounded newest-first shelf. Pure; the process-wide store wraps this.
#[derive(Debug, Clone)]
pub struct NotificationShelf {
    events: VecDeque<NotificationEvent>,
    cap: usize,
}

impl NotificationShelf {
    pub fn new(cap: usize) -> Self {
        Self {
            events: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn unread_count(&self) -> usize {
        self.events.iter().filter(|e| e.unread).count()
    }

    pub fn iter(&self) -> impl Iterator<Item = &NotificationEvent> {
        self.events.iter()
    }

    pub fn latest(&self) -> Option<&NotificationEvent> {
        self.events.front()
    }

    /// Push newest-first. Drops the oldest past `cap`. Returns false on dedup.
    pub fn push(&mut self, event: NotificationEvent) -> bool {
        if event.is_empty() {
            return false;
        }
        if self.is_duplicate(&event) {
            return false;
        }
        self.events.push_front(event);
        while self.events.len() > self.cap {
            self.events.pop_back();
        }
        true
    }

    pub fn dismiss(&mut self, id: &str) -> bool {
        let before = self.events.len();
        self.events.retain(|e| e.id != id);
        before != self.events.len()
    }

    pub fn mark_read(&mut self, id: &str) -> bool {
        if let Some(event) = self.events.iter_mut().find(|e| e.id == id) {
            if event.unread {
                event.unread = false;
                return true;
            }
        }
        false
    }

    pub fn mark_all_read(&mut self) -> bool {
        let mut changed = false;
        for event in &mut self.events {
            if event.unread {
                event.unread = false;
                changed = true;
            }
        }
        changed
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn snapshot(&self) -> Vec<NotificationEvent> {
        self.events.iter().cloned().collect()
    }

    fn is_duplicate(&self, event: &NotificationEvent) -> bool {
        self.events.iter().any(|existing| {
            existing.bundle_id == event.bundle_id
                && existing.title == event.title
                && existing.body == event.body
                && (existing.delivered_at - event.delivered_at).abs() <= DEDUP_SECS
        })
    }
}

/// Hide notifications from these bundle IDs (or exact app names).
pub fn is_app_allowed(bundle_id: &str, app_name: &str, blocked: &[String]) -> bool {
    if blocked.is_empty() {
        return true;
    }
    !blocked.iter().any(|entry| {
        let entry = entry.trim();
        if entry.is_empty() {
            return false;
        }
        (!bundle_id.is_empty() && bundle_id.eq_ignore_ascii_case(entry))
            || (!app_name.is_empty() && app_name.eq_ignore_ascii_case(entry))
    })
}

pub fn relative_age(delivered_at: i64, now: i64) -> String {
    let delta = now.saturating_sub(delivered_at).max(0);
    if delta < 60 {
        "now".into()
    } else if delta < 3600 {
        format!("{}m", delta / 60)
    } else if delta < 86400 {
        format!("{}h", delta / 3600)
    } else {
        format!("{}d", delta / 86400)
    }
}

/// Decode a usernoted `record.data` blob (bplist / NSKeyedArchiver).
/// Looks for `titl` / `subt` / `body` / `app` / `date` anywhere in the graph.
pub fn decode_usernoted_record(data: &[u8]) -> Option<NotificationEvent> {
    let value = parse_plist(data)?;
    let mut fields = NotifFields::default();
    collect_fields(&value, None, &mut fields);
    fields.into_event()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
    Unavailable,
}

pub fn ax_trusted(prompt: bool) -> bool {
    #[cfg(target_os = "macos")]
    {
        ax::is_trusted(prompt)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = prompt;
        false
    }
}

pub fn fda_status() -> PermissionState {
    #[cfg(target_os = "macos")]
    {
        db::probe()
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionState::Unavailable
    }
}

pub fn usernoted_db_path() -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    {
        db::db_path()
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::path::PathBuf::from("/unavailable/usernoted/db")
    }
}

/// Start backends that the current settings allow. Idempotent; no-op on Linux.
pub fn sync_backends(settings: &AppSettings) {
    if !settings.show_notifications {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        AxObserverBackend.start();
        if settings.notification_fda_opt_in {
            DbReaderBackend.start();
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = settings;
    }
}

pub struct AxObserverBackend;

impl NotificationBackend for AxObserverBackend {
    fn name(&self) -> &'static str {
        "ax_observer"
    }

    fn start(&self) {
        #[cfg(target_os = "macos")]
        ax::start();
    }
}

pub struct DbReaderBackend;

impl NotificationBackend for DbReaderBackend {
    fn name(&self) -> &'static str {
        "db_reader"
    }

    fn start(&self) {
        #[cfg(target_os = "macos")]
        db::start();
    }
}

pub fn subscribe() -> watch::Receiver<u64> {
    gen_tx().subscribe()
}

pub fn snapshot() -> Vec<NotificationEvent> {
    store().lock().map(|s| s.snapshot()).unwrap_or_default()
}

pub fn unread_count() -> usize {
    store().lock().map(|s| s.unread_count()).unwrap_or(0)
}

pub fn known_apps() -> Vec<(String, String)> {
    let mut seen: Vec<(String, String)> = Vec::new();
    if let Ok(shelf) = store().lock() {
        for event in shelf.iter() {
            let key = if event.bundle_id.is_empty() {
                event.app_name.clone()
            } else {
                event.bundle_id.clone()
            };
            if key.is_empty() {
                continue;
            }
            if seen.iter().any(|(id, _)| id == &key) {
                continue;
            }
            let name = if event.app_name.is_empty() {
                key.clone()
            } else {
                event.app_name.clone()
            };
            seen.push((key, name));
        }
    }
    let settings = crate::settings::get_app_settings();
    for blocked in &settings.notification_blocked_apps {
        if blocked.is_empty() {
            continue;
        }
        if !seen.iter().any(|(id, _)| id == blocked) {
            seen.push((blocked.clone(), blocked.clone()));
        }
    }
    seen.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    seen
}

pub fn ingest(event: NotificationEvent) -> bool {
    let settings = crate::settings::get_app_settings();
    if !settings.show_notifications {
        return false;
    }
    if !is_app_allowed(
        &event.bundle_id,
        &event.app_name,
        &settings.notification_blocked_apps,
    ) {
        return false;
    }
    let pushed = store().lock().map(|mut s| s.push(event)).unwrap_or(false);
    if pushed {
        persist_shelf();
        bump();
    }
    pushed
}

pub fn dismiss(id: &str) -> bool {
    let changed = store().lock().map(|mut s| s.dismiss(id)).unwrap_or(false);
    if changed {
        persist_shelf();
        bump();
    }
    changed
}

pub fn mark_read(id: &str) -> bool {
    let changed = store().lock().map(|mut s| s.mark_read(id)).unwrap_or(false);
    if changed {
        persist_shelf();
        bump();
    }
    changed
}

pub fn mark_all_read() -> bool {
    let changed = store()
        .lock()
        .map(|mut s| s.mark_all_read())
        .unwrap_or(false);
    if changed {
        persist_shelf();
        bump();
    }
    changed
}

pub fn clear() {
    if let Ok(mut shelf) = store().lock() {
        if shelf.is_empty() {
            return;
        }
        shelf.clear();
    }
    persist_shelf();
    bump();
}

pub fn load_persisted() {
    let Ok(conn) = database::get_connection() else {
        return;
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, bundle_id, app_name, title, subtitle, body, delivered_at, unread
         FROM notification_shelf
         ORDER BY delivered_at DESC
         LIMIT ?1",
    ) else {
        return;
    };
    let rows = stmt.query_map([SHELF_CAP as i64], |row| {
        Ok(NotificationEvent {
            id: row.get(0)?,
            bundle_id: row.get(1)?,
            app_name: row.get(2)?,
            title: row.get(3)?,
            subtitle: row.get(4)?,
            body: row.get(5)?,
            delivered_at: row.get(6)?,
            unread: row.get::<_, i64>(7)? != 0,
        })
    });
    let Ok(rows) = rows else {
        return;
    };
    if let Ok(mut shelf) = store().lock() {
        shelf.clear();
        for row in rows.flatten() {
            shelf.events.push_back(row);
        }
    }
}

fn persist_shelf() {
    let Ok(events) = store().lock().map(|s| s.snapshot()) else {
        return;
    };
    let Ok(conn) = database::get_connection() else {
        return;
    };
    if conn
        .execute_batch("BEGIN; DELETE FROM notification_shelf;")
        .is_err()
    {
        return;
    }
    for event in &events {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO notification_shelf
             (id, bundle_id, app_name, title, subtitle, body, delivered_at, unread)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                event.id,
                event.bundle_id,
                event.app_name,
                event.title,
                event.subtitle,
                event.body,
                event.delivered_at,
                event.unread as i64
            ],
        );
    }
    let _ = conn.execute_batch("COMMIT;");
}

fn store() -> &'static Mutex<NotificationShelf> {
    STORE.get_or_init(|| Mutex::new(NotificationShelf::new(SHELF_CAP)))
}

fn gen_tx() -> &'static watch::Sender<u64> {
    GEN.get_or_init(|| {
        let (tx, _) = watch::channel(0);
        tx
    })
}

fn bump() {
    let tx = gen_tx();
    let next = tx.borrow().saturating_add(1);
    let _ = tx.send(next);
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn event_id(bundle_id: &str, title: &str, body: &str, delivered_at: i64) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bundle_id.hash(&mut hasher);
    title.hash(&mut hasher);
    body.hash(&mut hasher);
    delivered_at.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn parse_plist(data: &[u8]) -> Option<Plist> {
    bplist::parse(data)
}

#[derive(Default)]
struct NotifFields {
    title: Option<String>,
    subtitle: Option<String>,
    body: Option<String>,
    app: Option<String>,
    date: Option<i64>,
}

impl NotifFields {
    fn into_event(self) -> Option<NotificationEvent> {
        let title = self.title.unwrap_or_default();
        let body = self.body.unwrap_or_default();
        if title.trim().is_empty() && body.trim().is_empty() {
            return None;
        }
        let app = self.app.unwrap_or_default();
        let app_name = display_name_for_bundle(&app);
        Some(NotificationEvent::new(
            app,
            app_name,
            title,
            self.subtitle.unwrap_or_default(),
            body,
            self.date.unwrap_or_else(now_unix),
        ))
    }
}

fn collect_fields(value: &Plist, objects: Option<&[Plist]>, out: &mut NotifFields) {
    let resolved = resolve(value, objects);
    match resolved {
        Plist::Dict(dict) => {
            take_field(dict, objects, "titl", &mut out.title);
            take_field(dict, objects, "title", &mut out.title);
            take_field(dict, objects, "subt", &mut out.subtitle);
            take_field(dict, objects, "subtitle", &mut out.subtitle);
            take_field(dict, objects, "body", &mut out.body);
            take_field(dict, objects, "app", &mut out.app);
            if out.date.is_none() {
                if let Some(raw) = dict.get("date") {
                    out.date = date_to_unix(resolve(raw, objects));
                }
            }
            if let Some(Plist::Array(objs)) = dict.get("$objects") {
                for item in objs {
                    collect_fields(item, Some(objs), out);
                }
                return;
            }
            if let Some(req) = dict.get("req") {
                collect_fields(req, objects, out);
            }
            for (key, child) in dict {
                if key.starts_with('$')
                    || matches!(
                        key.as_str(),
                        "titl" | "title" | "subt" | "subtitle" | "body" | "app" | "date" | "req"
                    )
                {
                    continue;
                }
                collect_fields(child, objects, out);
            }
        }
        Plist::Array(items) => {
            for item in items {
                collect_fields(item, objects, out);
            }
        }
        _ => {}
    }
}

fn take_field(
    dict: &BTreeMap<String, Plist>,
    objects: Option<&[Plist]>,
    key: &str,
    slot: &mut Option<String>,
) {
    if slot.is_some() {
        return;
    }
    if let Some(value) = dict.get(key) {
        *slot = value_string(resolve(value, objects));
    }
}

fn resolve<'a>(value: &'a Plist, objects: Option<&'a [Plist]>) -> &'a Plist {
    if let (Plist::Uid(uid), Some(objects)) = (value, objects) {
        if let Some(resolved) = objects.get(*uid as usize) {
            return resolve(resolved, Some(objects));
        }
    }
    value
}

fn value_string(value: &Plist) -> Option<String> {
    match value {
        Plist::String(s) if !s.is_empty() => Some(s.clone()),
        Plist::Bool(b) => Some(b.to_string()),
        Plist::Integer(i) => Some(i.to_string()),
        Plist::Real(r) => Some(r.to_string()),
        _ => None,
    }
}

fn date_to_unix(value: &Plist) -> Option<i64> {
    match value {
        Plist::Date(secs) => cf_or_unix(*secs as i64),
        Plist::Real(secs) => cf_or_unix(*secs as i64),
        Plist::Integer(i) => cf_or_unix(*i),
        _ => None,
    }
}

fn cf_or_unix(n: i64) -> Option<i64> {
    if n <= 0 {
        return None;
    }
    if n > 1_000_000_000_000 {
        Some(n / 1000)
    } else if n > 1_000_000_000 {
        Some(n)
    } else {
        Some(n + 978_307_200)
    }
}

/// Enough of Apple's plist graph for usernoted `record.data` (bplist00 and XML).
#[derive(Debug, Clone, PartialEq)]
enum Plist {
    Bool(bool),
    Integer(i64),
    Real(f64),
    Date(f64),
    String(String),
    Uid(u64),
    Array(Vec<Plist>),
    Dict(BTreeMap<String, Plist>),
}

mod bplist {
    use super::Plist;
    use std::collections::BTreeMap;

    pub fn parse(data: &[u8]) -> Option<Plist> {
        if data.is_empty() {
            return None;
        }
        if data.starts_with(b"bplist") {
            parse_binary(data)
        } else {
            parse_xml(&String::from_utf8_lossy(data))
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[derive(Clone)]
    enum Encoded {
        Str(String),
        Int(i64),
        Uid(u64),
        Arr(Vec<usize>),
        Dict(Vec<(usize, usize)>),
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn encode_binary(value: &Plist) -> Vec<u8> {
        let mut objects = Vec::new();
        let root = intern(value, &mut objects);
        let ref_size = int_size(objects.len().saturating_sub(1) as u64).max(1);
        let mut body = Vec::new();
        let mut offsets = Vec::new();
        for obj in &objects {
            offsets.push(8 + body.len());
            match obj {
                Encoded::Str(s) => {
                    write_marker(&mut body, 0x50, s.len());
                    body.extend_from_slice(s.as_bytes());
                }
                Encoded::Int(n) => {
                    body.push(0x13);
                    body.extend_from_slice(&n.to_be_bytes());
                }
                Encoded::Uid(n) => {
                    let size = int_size(*n).max(1);
                    body.push(0x80 | (size - 1));
                    write_int(&mut body, *n, size);
                }
                Encoded::Arr(items) => {
                    write_marker(&mut body, 0xA0, items.len());
                    for idx in items {
                        write_int(&mut body, *idx as u64, ref_size);
                    }
                }
                Encoded::Dict(pairs) => {
                    write_marker(&mut body, 0xD0, pairs.len());
                    for (k, _) in pairs {
                        write_int(&mut body, *k as u64, ref_size);
                    }
                    for (_, v) in pairs {
                        write_int(&mut body, *v as u64, ref_size);
                    }
                }
            }
        }
        let offset_table = 8 + body.len();
        let offset_size = 2;
        for off in &offsets {
            write_int(&mut body, *off as u64, offset_size);
        }
        let mut out = b"bplist00".to_vec();
        out.extend_from_slice(&body);
        let mut trailer = [0u8; 32];
        trailer[6] = offset_size;
        trailer[7] = ref_size;
        trailer[8..16].copy_from_slice(&(objects.len() as u64).to_be_bytes());
        trailer[16..24].copy_from_slice(&(root as u64).to_be_bytes());
        trailer[24..32].copy_from_slice(&(offset_table as u64).to_be_bytes());
        out.extend_from_slice(&trailer);
        out
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn intern(value: &Plist, objects: &mut Vec<Encoded>) -> usize {
        match value {
            Plist::String(s) => {
                objects.push(Encoded::Str(s.clone()));
                objects.len() - 1
            }
            Plist::Integer(n) => {
                objects.push(Encoded::Int(*n));
                objects.len() - 1
            }
            Plist::Uid(n) => {
                objects.push(Encoded::Uid(*n));
                objects.len() - 1
            }
            Plist::Bool(b) => intern(&Plist::Integer(i64::from(*b)), objects),
            Plist::Real(n) | Plist::Date(n) => intern(&Plist::Integer(*n as i64), objects),
            Plist::Array(items) => {
                let idxs: Vec<usize> = items.iter().map(|item| intern(item, objects)).collect();
                objects.push(Encoded::Arr(idxs));
                objects.len() - 1
            }
            Plist::Dict(dict) => {
                let pairs: Vec<(usize, usize)> = dict
                    .iter()
                    .map(|(k, v)| {
                        (
                            intern(&Plist::String(k.clone()), objects),
                            intern(v, objects),
                        )
                    })
                    .collect();
                objects.push(Encoded::Dict(pairs));
                objects.len() - 1
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn write_marker(out: &mut Vec<u8>, tagged: u8, len: usize) {
        if len < 15 {
            out.push(tagged | len as u8);
        } else {
            out.push(tagged | 0x0F);
            let size = int_size(len as u64);
            out.push(0x10 | (size - 1));
            write_int(out, len as u64, size);
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn write_int(out: &mut Vec<u8>, value: u64, size: u8) {
        for shift in (0..size).rev() {
            out.push((value >> (shift * 8)) as u8);
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn int_size(value: u64) -> u8 {
        if value <= 0xFF {
            1
        } else if value <= 0xFFFF {
            2
        } else if value <= 0xFFFF_FFFF {
            4
        } else {
            8
        }
    }

    fn parse_binary(data: &[u8]) -> Option<Plist> {
        if data.len() < 40 || !data.starts_with(b"bplist") {
            return None;
        }
        let trailer = &data[data.len() - 32..];
        let offset_size = trailer[6] as usize;
        let ref_size = trailer[7] as usize;
        let num_objects = u64::from_be_bytes(trailer[8..16].try_into().ok()?) as usize;
        let top = u64::from_be_bytes(trailer[16..24].try_into().ok()?) as usize;
        let offset_table = u64::from_be_bytes(trailer[24..32].try_into().ok()?) as usize;
        if offset_size == 0 || ref_size == 0 || num_objects == 0 {
            return None;
        }
        let mut offsets = Vec::with_capacity(num_objects);
        for i in 0..num_objects {
            let start = offset_table + i * offset_size;
            offsets.push(read_int(data, start, offset_size)? as usize);
        }
        read_object(data, offsets[top], &offsets, ref_size)
    }

    fn read_object(
        data: &[u8],
        offset: usize,
        offsets: &[usize],
        ref_size: usize,
    ) -> Option<Plist> {
        let marker = *data.get(offset)?;
        let tagged = marker & 0xF0;
        let mut len = (marker & 0x0F) as usize;
        let mut cur = offset + 1;
        if tagged != 0x00 && tagged != 0x80 && len == 15 && tagged != 0x10 && tagged != 0x20 {
            let (n, next) = read_len(data, cur)?;
            len = n;
            cur = next;
        }
        match tagged {
            0x00 => match marker {
                0x08 => Some(Plist::Bool(false)),
                0x09 => Some(Plist::Bool(true)),
                _ => None,
            },
            0x10 => {
                let size = 1usize << (marker & 0x0F);
                Some(Plist::Integer(read_signed(data, cur, size)?))
            }
            0x20 => {
                let size = 1usize << (marker & 0x0F);
                Some(Plist::Real(read_float(data, cur, size)?))
            }
            0x30 => Some(Plist::Date(read_float(data, cur, 8)?)),
            0x50 => {
                let bytes = data.get(cur..cur + len)?;
                Some(Plist::String(String::from_utf8_lossy(bytes).into_owned()))
            }
            0x60 => {
                let raw = data.get(cur..cur + len * 2)?;
                let units: Vec<u16> = raw
                    .chunks(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                Some(Plist::String(String::from_utf16_lossy(&units)))
            }
            0x80 => {
                let size = (marker & 0x0F) as usize + 1;
                Some(Plist::Uid(read_int(data, cur, size)?))
            }
            0xA0 => {
                let mut items = Vec::with_capacity(len);
                for i in 0..len {
                    let idx = read_int(data, cur + i * ref_size, ref_size)? as usize;
                    items.push(read_object(data, *offsets.get(idx)?, offsets, ref_size)?);
                }
                Some(Plist::Array(items))
            }
            0xD0 => {
                let mut dict = BTreeMap::new();
                for i in 0..len {
                    let key_idx = read_int(data, cur + i * ref_size, ref_size)? as usize;
                    let val_idx = read_int(data, cur + (len + i) * ref_size, ref_size)? as usize;
                    let key = match read_object(data, *offsets.get(key_idx)?, offsets, ref_size)? {
                        Plist::String(s) => s,
                        other => format!("{other:?}"),
                    };
                    dict.insert(
                        key,
                        read_object(data, *offsets.get(val_idx)?, offsets, ref_size)?,
                    );
                }
                Some(Plist::Dict(dict))
            }
            _ => None,
        }
    }

    fn read_len(data: &[u8], offset: usize) -> Option<(usize, usize)> {
        let marker = *data.get(offset)?;
        if marker & 0xF0 != 0x10 {
            return None;
        }
        let size = 1usize << (marker & 0x0F);
        let n = read_int(data, offset + 1, size)? as usize;
        Some((n, offset + 1 + size))
    }

    fn read_int(data: &[u8], offset: usize, size: usize) -> Option<u64> {
        let bytes = data.get(offset..offset + size)?;
        let mut n = 0u64;
        for b in bytes {
            n = (n << 8) | *b as u64;
        }
        Some(n)
    }

    fn read_signed(data: &[u8], offset: usize, size: usize) -> Option<i64> {
        let n = read_int(data, offset, size)?;
        match size {
            1 => Some(n as i8 as i64),
            2 => Some(n as i16 as i64),
            4 => Some(n as i32 as i64),
            _ => Some(n as i64),
        }
    }

    fn read_float(data: &[u8], offset: usize, size: usize) -> Option<f64> {
        match size {
            4 => {
                let bits = read_int(data, offset, 4)? as u32;
                Some(f32::from_bits(bits) as f64)
            }
            8 => {
                let bits = read_int(data, offset, 8)?;
                Some(f64::from_bits(bits))
            }
            _ => None,
        }
    }

    fn parse_xml(xml: &str) -> Option<Plist> {
        let start = xml.find("<plist").or_else(|| xml.find("<dict"))?;
        let mut cur = &xml[start..];
        if let Some(idx) = cur.find("<dict") {
            cur = &cur[idx..];
            return parse_xml_value(&mut cur);
        }
        None
    }

    fn parse_xml_value(cur: &mut &str) -> Option<Plist> {
        skip_ws(cur);
        if cur.starts_with("<dict") {
            *cur = cur.split_once('>')?.1;
            let mut dict = BTreeMap::new();
            loop {
                skip_ws(cur);
                if cur.starts_with("</dict>") {
                    *cur = &cur[7..];
                    return Some(Plist::Dict(dict));
                }
                if !cur.starts_with("<key>") {
                    return None;
                }
                *cur = &cur[5..];
                let (key, rest) = cur.split_once("</key>")?;
                *cur = rest;
                dict.insert(unescape(key), parse_xml_value(cur)?);
            }
        }
        if cur.starts_with("<array") {
            *cur = cur.split_once('>')?.1;
            let mut items = Vec::new();
            loop {
                skip_ws(cur);
                if cur.starts_with("</array>") {
                    *cur = &cur[8..];
                    return Some(Plist::Array(items));
                }
                items.push(parse_xml_value(cur)?);
            }
        }
        if cur.starts_with("<string>") {
            *cur = &cur[8..];
            let (s, rest) = cur.split_once("</string>")?;
            *cur = rest;
            return Some(Plist::String(unescape(s)));
        }
        if cur.starts_with("<integer>") {
            *cur = &cur[9..];
            let (s, rest) = cur.split_once("</integer>")?;
            *cur = rest;
            return Some(Plist::Integer(s.trim().parse().ok()?));
        }
        if cur.starts_with("<real>") {
            *cur = &cur[6..];
            let (s, rest) = cur.split_once("</real>")?;
            *cur = rest;
            return Some(Plist::Real(s.trim().parse().ok()?));
        }
        if cur.starts_with("<date>") {
            *cur = &cur[6..];
            let (_s, rest) = cur.split_once("</date>")?;
            *cur = rest;
            return Some(Plist::Integer(now_fallback()));
        }
        if cur.starts_with("<true") {
            *cur = cur.split_once('>')?.1;
            return Some(Plist::Bool(true));
        }
        if cur.starts_with("<false") {
            *cur = cur.split_once('>')?.1;
            return Some(Plist::Bool(false));
        }
        None
    }

    fn skip_ws(cur: &mut &str) {
        *cur = cur.trim_start();
        while cur.starts_with("<!--") {
            if let Some((_, rest)) = cur.split_once("-->") {
                *cur = rest.trim_start();
            } else {
                break;
            }
        }
    }

    fn unescape(s: &str) -> String {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
    }

    fn now_fallback() -> i64 {
        super::now_unix()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn event_from_banner_texts(texts: Vec<String>) -> Option<NotificationEvent> {
    let texts: Vec<String> = texts
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if texts.is_empty() {
        return None;
    }
    let (app_name, title, subtitle, body) = match texts.len() {
        1 => (
            String::new(),
            texts[0].clone(),
            String::new(),
            String::new(),
        ),
        2 => (
            String::new(),
            texts[0].clone(),
            String::new(),
            texts[1].clone(),
        ),
        _ => (
            texts[0].clone(),
            texts[1].clone(),
            String::new(),
            texts[2..].join("\n"),
        ),
    };
    Some(NotificationEvent::new(
        String::new(),
        app_name,
        title,
        subtitle,
        body,
        now_unix(),
    ))
}

fn display_name_for_bundle(bundle_id: &str) -> String {
    if bundle_id.is_empty() {
        return String::new();
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(name) = macos_app_name(bundle_id) {
            return name;
        }
    }
    bundle_id
        .rsplit('.')
        .next()
        .unwrap_or(bundle_id)
        .to_string()
}

#[cfg(target_os = "macos")]
fn macos_app_name(bundle_id: &str) -> Option<String> {
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::CString;

    unsafe {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return None;
        }
        let cstr = CString::new(bundle_id).ok()?;
        let ns: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: cstr.as_ptr()];
        let url: *mut AnyObject = msg_send![ws, URLForApplicationWithBundleIdentifier: ns];
        if url.is_null() {
            return None;
        }
        let path: *mut AnyObject = msg_send![url, path];
        if path.is_null() {
            return None;
        }
        let name: *mut AnyObject = msg_send![path, lastPathComponent];
        if name.is_null() {
            return None;
        }
        let utf8: *const std::ffi::c_char = msg_send![name, UTF8String];
        if utf8.is_null() {
            return None;
        }
        let raw = std::ffi::CStr::from_ptr(utf8).to_string_lossy();
        Some(raw.trim_end_matches(".app").to_string())
    }
}

static STORE: OnceLock<Mutex<NotificationShelf>> = OnceLock::new();
static GEN: OnceLock<watch::Sender<u64>> = OnceLock::new();

#[cfg(target_os = "macos")]
mod ax {
    use super::*;
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::thread;

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFRunLoopRef = *const c_void;
    type CFRunLoopSourceRef = *const c_void;
    type AXUIElementRef = *const c_void;
    type AXObserverRef = *const c_void;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const AX_SUCCESS: i32 = 0;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: isize, encoding: u32)
            -> bool;
        fn CFStringGetLength(s: CFStringRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(len: isize, encoding: u32) -> isize;
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFArrayGetTypeID() -> usize;
        fn CFArrayGetCount(arr: CFArrayRef) -> isize;
        fn CFArrayGetValueAtIndex(arr: CFArrayRef, idx: isize) -> *const c_void;
        fn CFRelease(cf: CFTypeRef);
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopRun();
        static kCFRunLoopDefaultMode: CFStringRef;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXObserverCreate(
            pid: i32,
            callback: extern "C" fn(AXObserverRef, AXUIElementRef, CFStringRef, *mut c_void),
            out_observer: *mut AXObserverRef,
        ) -> i32;
        fn AXObserverAddNotification(
            observer: AXObserverRef,
            element: AXUIElementRef,
            notification: CFStringRef,
            refcon: *mut c_void,
        ) -> i32;
        fn AXObserverGetRunLoopSource(observer: AXObserverRef) -> CFRunLoopSourceRef;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
    }

    static STARTED: AtomicBool = AtomicBool::new(false);

    pub fn is_trusted(prompt: bool) -> bool {
        unsafe {
            let opts = if prompt {
                prompt_options()
            } else {
                ptr::null()
            };
            AXIsProcessTrustedWithOptions(opts)
        }
    }

    unsafe fn prompt_options() -> CFDictionaryRef {
        let key: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: b"AXTrustedCheckOptionPrompt\0".as_ptr()
        ];
        let yes: *mut AnyObject = msg_send![class!(NSNumber), numberWithBool: true];
        let dict: *mut AnyObject =
            msg_send![class!(NSDictionary), dictionaryWithObject: yes, forKey: key];
        dict as CFDictionaryRef
    }

    pub fn start() {
        if STARTED.swap(true, Ordering::Relaxed) {
            return;
        }
        thread::Builder::new()
            .name("nook-notify-ax".into())
            .spawn(|| unsafe { run_loop() })
            .ok();
    }

    unsafe fn run_loop() {
        // Prompt only from this first start, after the user enabled the widget.
        let _ = is_trusted(true);
        let Some(pid) = notification_center_pid() else {
            log::info!("notification shelf: Notification Center not running; AX observer idle");
            wait_for_notification_center();
            return;
        };
        if !attach(pid) {
            log::warn!("notification shelf: AXObserver attach failed for pid {pid}");
            STARTED.store(false, Ordering::Relaxed);
            return;
        }
        CFRunLoopRun();
    }

    unsafe fn attach(pid: i32) -> bool {
        let mut observer: AXObserverRef = ptr::null();
        if AXObserverCreate(pid, on_ax_event, &mut observer) != AX_SUCCESS || observer.is_null() {
            return false;
        }
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return false;
        }
        let created = cfstr("AXWindowCreated");
        let ax_created = cfstr("AXCreated");
        let added = AXObserverAddNotification(observer, app, created, ptr::null_mut());
        let _ = AXObserverAddNotification(observer, app, ax_created, ptr::null_mut());
        if added != AX_SUCCESS {
            return false;
        }
        let source = AXObserverGetRunLoopSource(observer);
        if source.is_null() {
            return false;
        }
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode);
        log::info!("notification shelf: AXObserver on notificationcenterui pid {pid}");
        true
    }

    extern "C" fn on_ax_event(
        _observer: AXObserverRef,
        element: AXUIElementRef,
        _notification: CFStringRef,
        _refcon: *mut c_void,
    ) {
        let mut texts = Vec::new();
        unsafe {
            scrape(element, &mut texts, 0);
        }
        if let Some(event) = super::event_from_banner_texts(texts) {
            super::ingest(event);
        }
    }

    fn attr_key(name: &'static str) -> CFStringRef {
        use std::collections::HashMap;
        static KEYS: OnceLock<Mutex<HashMap<&'static str, usize>>> = OnceLock::new();
        let keys = KEYS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut guard = keys.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ptr) = guard.get(name) {
            return *ptr as CFStringRef;
        }
        let created = unsafe { cfstr(name) };
        guard.insert(name, created as usize);
        created
    }

    unsafe fn scrape(element: AXUIElementRef, texts: &mut Vec<String>, depth: usize) {
        if element.is_null() || depth > 6 || texts.len() > 12 {
            return;
        }
        if let Some(title) = copy_string_attr(element, "AXTitle") {
            push_text(texts, title);
        }
        let role = copy_string_attr(element, "AXRole").unwrap_or_default();
        if role == "AXStaticText" || role == "AXTextField" {
            if let Some(value) = copy_string_attr(element, "AXValue") {
                push_text(texts, value);
            }
        }
        let children_key = attr_key("AXChildren");
        let mut raw: CFTypeRef = ptr::null();
        if AXUIElementCopyAttributeValue(element, children_key, &mut raw) == AX_SUCCESS
            && !raw.is_null()
            && CFGetTypeID(raw) == CFArrayGetTypeID()
        {
            let count = CFArrayGetCount(raw as CFArrayRef);
            for i in 0..count.min(16) {
                let child = CFArrayGetValueAtIndex(raw as CFArrayRef, i) as AXUIElementRef;
                scrape(child, texts, depth + 1);
            }
        }
        if !raw.is_null() {
            CFRelease(raw);
        }
    }

    unsafe fn copy_string_attr(element: AXUIElementRef, name: &'static str) -> Option<String> {
        let key = attr_key(name);
        let mut raw: CFTypeRef = ptr::null();
        if AXUIElementCopyAttributeValue(element, key, &mut raw) != AX_SUCCESS || raw.is_null() {
            return None;
        }
        let out = if CFGetTypeID(raw) == CFStringGetTypeID() {
            cf_string(raw as CFStringRef)
        } else {
            None
        };
        CFRelease(raw);
        out
    }

    fn push_text(texts: &mut Vec<String>, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() || texts.iter().any(|t| t == &text) {
            return;
        }
        texts.push(text);
    }

    unsafe fn cfstr(name: &str) -> CFStringRef {
        let cstr = CString::new(name).unwrap_or_default();
        CFStringCreateWithCString(ptr::null(), cstr.as_ptr(), K_CF_STRING_ENCODING_UTF8)
    }

    unsafe fn cf_string(s: CFStringRef) -> Option<String> {
        if s.is_null() {
            return None;
        }
        let len = CFStringGetLength(s);
        let cap = CFStringGetMaximumSizeForEncoding(len, K_CF_STRING_ENCODING_UTF8) + 1;
        if cap <= 1 || cap > 8 * 1024 {
            return None;
        }
        let mut buf = vec![0u8; cap as usize];
        if !CFStringGetCString(
            s,
            buf.as_mut_ptr() as *mut c_char,
            cap,
            K_CF_STRING_ENCODING_UTF8,
        ) {
            return None;
        }
        CStr::from_ptr(buf.as_ptr() as *const c_char)
            .to_str()
            .ok()
            .map(|s| s.to_string())
    }

    fn notification_center_pid() -> Option<i32> {
        unsafe {
            let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            if ws.is_null() {
                return None;
            }
            let bid: *mut AnyObject = msg_send![
                class!(NSString),
                stringWithUTF8String: b"com.apple.notificationcenterui\0".as_ptr()
            ];
            let apps: *mut AnyObject = msg_send![ws, runningApplicationsWithBundleIdentifier: bid];
            if apps.is_null() {
                return None;
            }
            let count: usize = msg_send![apps, count];
            if count == 0 {
                return None;
            }
            let app: *mut AnyObject = msg_send![apps, objectAtIndex: 0usize];
            if app.is_null() {
                return None;
            }
            let pid: i32 = msg_send![app, processIdentifier];
            (pid > 0).then_some(pid)
        }
    }

    fn wait_for_notification_center() {
        // Event-driven: NSWorkspace launch notification, then attach + run.
        unsafe {
            let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            if ws.is_null() {
                STARTED.store(false, Ordering::Relaxed);
                return;
            }
            let center: *mut AnyObject = msg_send![ws, notificationCenter];
            let name: *mut AnyObject = msg_send![
                class!(NSString),
                stringWithUTF8String: b"NSWorkspaceDidLaunchApplicationNotification\0".as_ptr()
            ];
            // Block-free: poll-free wait via distributed observation is heavier
            // than we need. Re-check once from the existing island settings
            // sync; leave STARTED false so the next enable retries.
            let _ = (center, name);
            STARTED.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(target_os = "macos")]
mod db {
    use super::*;
    use rusqlite::{Connection, OpenFlags};
    use std::fs::File;
    use std::os::unix::io::AsRawFd;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    const WATERMARK_KEY: &str = "notification_db_rowid";

    static STARTED: AtomicBool = AtomicBool::new(false);

    #[repr(C)]
    struct Kevent {
        ident: usize,
        filter: i16,
        flags: u16,
        fflags: u32,
        data: isize,
        udata: *mut std::ffi::c_void,
    }

    const EVFILT_VNODE: i16 = -4;
    const EV_ADD: u16 = 0x0001;
    const EV_CLEAR: u16 = 0x0020;
    const NOTE_DELETE: u32 = 0x0001;
    const NOTE_WRITE: u32 = 0x0002;
    const NOTE_EXTEND: u32 = 0x0004;
    const NOTE_ATTRIB: u32 = 0x0008;
    const NOTE_RENAME: u32 = 0x0020;

    extern "C" {
        fn kqueue() -> i32;
        fn kevent(
            kq: i32,
            changelist: *const Kevent,
            nchanges: i32,
            eventlist: *mut Kevent,
            nevents: i32,
            timeout: *const std::ffi::c_void,
        ) -> i32;
        fn close(fd: i32) -> i32;
    }

    pub fn db_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library/Group Containers/group.com.apple.usernoted/db2/db")
    }

    pub fn probe() -> PermissionState {
        let path = db_path();
        match File::open(&path) {
            Ok(_) => PermissionState::Granted,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                PermissionState::Denied
            }
            Err(_) => {
                if let Some(parent) = path.parent() {
                    match std::fs::read_dir(parent) {
                        Ok(_) => PermissionState::Denied,
                        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                            PermissionState::Denied
                        }
                        Err(_) => PermissionState::Unavailable,
                    }
                } else {
                    PermissionState::Unavailable
                }
            }
        }
    }

    pub fn start() {
        if probe() != PermissionState::Granted {
            log::info!("notification shelf: usernoted db not readable (Full Disk Access?)");
            return;
        }
        if STARTED.swap(true, Ordering::Relaxed) {
            return;
        }
        let path = db_path();
        thread::Builder::new()
            .name("nook-notify-db".into())
            .spawn(move || watch_loop(path))
            .ok();
    }

    fn watch_loop(path: PathBuf) {
        ingest_new_rows(&path);
        let wal = path.with_file_name("db-wal");
        let db_file = File::open(&path);
        let wal_file = File::open(&wal);
        let mut changes = Vec::new();
        let flags = NOTE_WRITE | NOTE_EXTEND | NOTE_ATTRIB | NOTE_DELETE | NOTE_RENAME;
        if let Ok(ref file) = db_file {
            changes.push(vnode_event(file.as_raw_fd() as usize, flags));
        }
        if let Ok(ref file) = wal_file {
            changes.push(vnode_event(file.as_raw_fd() as usize, flags));
        }
        if changes.is_empty() {
            STARTED.store(false, Ordering::Relaxed);
            return;
        }
        let kq = unsafe { kqueue() };
        if kq < 0 {
            STARTED.store(false, Ordering::Relaxed);
            return;
        }
        let n = unsafe {
            kevent(
                kq,
                changes.as_ptr(),
                changes.len() as i32,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if n < 0 {
            unsafe { close(kq) };
            STARTED.store(false, Ordering::Relaxed);
            return;
        }
        log::info!("notification shelf: kqueue on usernoted db");
        loop {
            let mut ev = Kevent {
                ident: 0,
                filter: 0,
                flags: 0,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            let n = unsafe { kevent(kq, std::ptr::null(), 0, &mut ev, 1, std::ptr::null()) };
            if n <= 0 {
                break;
            }
            ingest_new_rows(&path);
        }
        unsafe { close(kq) };
        STARTED.store(false, Ordering::Relaxed);
    }

    fn vnode_event(ident: usize, fflags: u32) -> Kevent {
        Kevent {
            ident,
            filter: EVFILT_VNODE,
            flags: EV_ADD | EV_CLEAR,
            fflags,
            data: 0,
            udata: std::ptr::null_mut(),
        }
    }

    fn ingest_new_rows(path: &Path) {
        let Ok(conn) = open_ro(path) else {
            return;
        };
        let watermark = load_watermark();
        let rows = match read_records(&conn, watermark) {
            Ok(rows) => rows,
            Err(err) => {
                log::warn!("notification shelf: usernoted query failed ({err})");
                return;
            }
        };
        let mut max_row = watermark;
        for (rowid, blob, bundle) in rows {
            max_row = max_row.max(rowid);
            let Some(mut event) = decode_usernoted_record(&blob) else {
                continue;
            };
            if let Some(bundle) = bundle {
                if event.bundle_id.is_empty() {
                    event.app_name = display_name_for_bundle(&bundle);
                    event.bundle_id = bundle;
                    event.id = event_id(
                        &event.bundle_id,
                        &event.title,
                        &event.body,
                        event.delivered_at,
                    );
                }
            }
            super::ingest(event);
        }
        if max_row > watermark {
            save_watermark(max_row);
        }
    }

    fn open_ro(path: &Path) -> rusqlite::Result<Connection> {
        let uri = format!("file:{}?mode=ro", path.display());
        Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
    }

    fn read_records(
        conn: &Connection,
        watermark: i64,
    ) -> rusqlite::Result<Vec<(i64, Vec<u8>, Option<String>)>> {
        let cols = table_columns(conn, "record")?;
        if cols.is_empty() {
            return Ok(Vec::new());
        }
        let id_col = if cols.iter().any(|c| c == "rec_id") {
            "rec_id"
        } else {
            "rowid"
        };
        let data_col = cols
            .iter()
            .find(|c| *c == "data")
            .ok_or_else(|| rusqlite::Error::InvalidQuery)?;
        let has_app_id = cols.iter().any(|c| c == "app_id");
        let first_run = watermark == 0;
        let sql = if has_app_id {
            format!(
                "SELECT {id_col}, {data_col}, app_id FROM record WHERE {id_col} > ?1
                 ORDER BY {id_col} {} LIMIT 40",
                if first_run { "DESC" } else { "ASC" }
            )
        } else {
            format!(
                "SELECT {id_col}, {data_col} FROM record WHERE {id_col} > ?1
                 ORDER BY {id_col} {} LIMIT 40",
                if first_run { "DESC" } else { "ASC" }
            )
        };
        let after = if first_run { 0 } else { watermark };
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params![after])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let rowid: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let app_id: Option<i64> = if has_app_id { row.get(2).ok() } else { None };
            let bundle = app_id.and_then(|id| lookup_bundle(conn, id));
            out.push((rowid, blob, bundle));
        }
        if first_run {
            out.reverse();
        }
        Ok(out)
    }

    fn lookup_bundle(conn: &Connection, app_id: i64) -> Option<String> {
        let cols = table_columns(conn, "app").ok()?;
        let name = ["identifier", "bundle_id", "app_id"]
            .into_iter()
            .find(|c| cols.iter().any(|col| col == c))?;
        let id_col = if cols.iter().any(|c| c == "app_id") {
            "app_id"
        } else {
            "rowid"
        };
        conn.query_row(
            &format!("SELECT {name} FROM app WHERE {id_col} = ?1"),
            [app_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .filter(|s| !s.is_empty() && s.contains('.'))
    }

    fn table_columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn load_watermark() -> i64 {
        database::get_setting(WATERMARK_KEY)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    fn save_watermark(rowid: i64) {
        let _ = database::set_setting(WATERMARK_KEY, &rowid.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn ev(app: &str, title: &str, body: &str, at: i64) -> NotificationEvent {
        NotificationEvent::new(app, app, title, "", body, at)
    }

    #[test]
    fn ring_buffer_drops_oldest_and_tracks_unread() {
        let mut shelf = NotificationShelf::new(3);
        assert!(shelf.push(ev("a.b", "one", "", 1)));
        assert!(shelf.push(ev("a.b", "two", "", 2)));
        assert!(shelf.push(ev("a.b", "three", "", 3)));
        assert!(shelf.push(ev("a.b", "four", "", 4)));
        let titles: Vec<_> = shelf.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["four", "three", "two"]);
        assert_eq!(shelf.unread_count(), 3);
        assert_eq!(shelf.latest().map(|e| e.title.as_str()), Some("four"));
        let id = shelf.latest().unwrap().id.clone();
        assert!(shelf.mark_read(&id));
        assert_eq!(shelf.unread_count(), 2);
        assert!(shelf.dismiss(&id));
        assert_eq!(shelf.len(), 2);
        shelf.mark_all_read();
        assert_eq!(shelf.unread_count(), 0);
        shelf.clear();
        assert!(shelf.is_empty());
    }

    #[test]
    fn ring_buffer_dedups_near_identical_events() {
        let mut shelf = NotificationShelf::new(8);
        assert!(shelf.push(ev("com.app", "Hi", "body", 100)));
        assert!(!shelf.push(ev("com.app", "Hi", "body", 101)));
        assert!(shelf.push(ev("com.app", "Hi", "body", 200)));
        assert_eq!(shelf.len(), 2);
        assert!(!shelf.push(NotificationEvent::new("x", "x", "", "", "", 1)));
    }

    #[test]
    fn per_app_filter_blocks_bundle_or_name() {
        let blocked = vec!["com.apple.mail".into(), "Slack".into()];
        assert!(!is_app_allowed("com.apple.mail", "Mail", &blocked));
        assert!(!is_app_allowed("", "slack", &blocked));
        assert!(is_app_allowed(
            "com.tinyspeck.slackmacgap",
            "Discord",
            &blocked
        ));
        assert!(is_app_allowed("com.apple.mail", "Mail", &[]));
    }

    #[test]
    fn relative_age_buckets() {
        assert_eq!(relative_age(100, 110), "now");
        assert_eq!(relative_age(0, 180), "3m");
        assert_eq!(relative_age(0, 7200), "2h");
        assert_eq!(relative_age(0, 172_800), "2d");
    }

    fn simple_bplist() -> Vec<u8> {
        let mut dict = BTreeMap::new();
        dict.insert("titl".into(), Plist::String("Hello".into()));
        dict.insert("subt".into(), Plist::String("From Discord".into()));
        dict.insert("body".into(), Plist::String("you were mentioned".into()));
        dict.insert("app".into(), Plist::String("com.hnc.Discord".into()));
        dict.insert("date".into(), Plist::Integer(1_700_000_000));
        let bytes = bplist::encode_binary(&Plist::Dict(dict));
        assert!(bytes.starts_with(b"bplist00"), "fixture is binary plist");
        bytes
    }

    #[test]
    fn decode_simple_bplist_fields() {
        let event = decode_usernoted_record(&simple_bplist()).expect("decoded");
        assert_eq!(event.title, "Hello");
        assert_eq!(event.subtitle, "From Discord");
        assert_eq!(event.body, "you were mentioned");
        assert_eq!(event.bundle_id, "com.hnc.Discord");
        assert_eq!(event.delivered_at, 1_700_000_000);
    }

    fn keyed_archive_bplist() -> Vec<u8> {
        let objects = vec![
            Plist::String("$null".into()),
            {
                let mut req = BTreeMap::new();
                req.insert("req".into(), Plist::Uid(2));
                Plist::Dict(req)
            },
            {
                let mut inner = BTreeMap::new();
                inner.insert("titl".into(), Plist::Uid(3));
                inner.insert("subt".into(), Plist::Uid(4));
                inner.insert("body".into(), Plist::Uid(5));
                inner.insert("app".into(), Plist::Uid(6));
                inner.insert("date".into(), Plist::Uid(7));
                Plist::Dict(inner)
            },
            Plist::String("Ping".into()),
            Plist::String("subtitle".into()),
            Plist::String("the body".into()),
            Plist::String("com.apple.Safari".into()),
            Plist::Integer(1_680_000_000),
        ];
        let mut root = BTreeMap::new();
        root.insert("$archiver".into(), Plist::String("NSKeyedArchiver".into()));
        root.insert("$version".into(), Plist::Integer(100_000));
        root.insert("$objects".into(), Plist::Array(objects));
        let mut top = BTreeMap::new();
        top.insert("root".into(), Plist::Uid(1));
        root.insert("$top".into(), Plist::Dict(top));
        bplist::encode_binary(&Plist::Dict(root))
    }

    #[test]
    fn decode_nskeyedarchiver_bplist() {
        let event = decode_usernoted_record(&keyed_archive_bplist()).expect("decoded");
        assert_eq!(event.title, "Ping");
        assert_eq!(event.subtitle, "subtitle");
        assert_eq!(event.body, "the body");
        assert_eq!(event.bundle_id, "com.apple.Safari");
        assert_eq!(event.delivered_at, 1_680_000_000);
    }

    #[test]
    fn decode_xml_plist_fixture() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>req</key>
    <dict>
        <key>titl</key><string>Calendar</string>
        <key>body</key><string>Meeting in 10 minutes</string>
        <key>app</key><string>com.apple.iCal</string>
        <key>date</key><integer>1700000100</integer>
    </dict>
</dict>
</plist>"#;
        let event = decode_usernoted_record(xml).expect("xml");
        assert_eq!(event.title, "Calendar");
        assert_eq!(event.body, "Meeting in 10 minutes");
        assert_eq!(event.bundle_id, "com.apple.iCal");
    }

    #[test]
    fn decode_rejects_empty_blob() {
        assert!(decode_usernoted_record(b"").is_none());
        assert!(decode_usernoted_record(b"not a plist").is_none());
    }

    #[test]
    fn ax_scrape_maps_static_text_rows() {
        let event = event_from_banner_texts(vec![
            "Discord".into(),
            "New message".into(),
            "hey there".into(),
        ])
        .unwrap();
        assert_eq!(event.app_name, "Discord");
        assert_eq!(event.title, "New message");
        assert_eq!(event.body, "hey there");
    }

    #[test]
    fn backends_advertise_stable_names() {
        assert_eq!(AxObserverBackend.name(), "ax_observer");
        assert_eq!(DbReaderBackend.name(), "db_reader");
    }
}
