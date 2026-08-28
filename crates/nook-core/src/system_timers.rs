//! Apple Clock (`mobiletimerd`) timer reader and vnode change stream.
//!
//! Primary store: CFPreferences domain `com.apple.mobiletimerd`
//! (`~/Library/Preferences/com.apple.mobiletimerd.plist`). Fallback: the
//! group-container Core Data sqlite. Change detection is a kqueue vnode
//! watch on those two files — no polling.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

/// Seconds between the Apple reference date (2001-01-01 UTC) and the Unix epoch.
pub const APPLE_EPOCH: f64 = 978_307_200.0;

pub const PREFS_DOMAIN: &str = "com.apple.mobiletimerd";
pub const PREFS_KEY_TIMERS: &str = "MTTimers";

/// Clock App Intents deep-link scheme (Clock.app PrivateURLSchemes).
pub const CLOCK_TIMER_SCHEME: &str = "x-apple-clock:timer?id=";

/// Calibrated against MobileTimer.framework `MTTimerState` (iOS / macOS).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i64)]
pub enum MTTimerState {
    #[default]
    Invalid = 0,
    Stopped = 1,
    Running = 2,
    Paused = 3,
    Fired = 4,
    Dismissed = 5,
}

impl MTTimerState {
    pub fn from_raw(value: i64) -> Self {
        match value {
            1 => Self::Stopped,
            2 => Self::Running,
            3 => Self::Paused,
            4 => Self::Fired,
            5 => Self::Dismissed,
            _ => Self::Invalid,
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Paused | Self::Fired)
    }

    pub fn is_running(self) -> bool {
        matches!(self, Self::Running)
    }

    pub fn is_counting(self) -> bool {
        matches!(self, Self::Running | Self::Fired)
    }
}

/// One Clock.app timer, as mirrored by mobiletimerd.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SystemTimer {
    pub id: String,
    pub title: String,
    pub duration: f64,
    pub state: MTTimerState,
    /// Absolute Unix fire date while running.
    pub fire_date: Option<f64>,
    /// Remaining seconds while paused / stopped (from `MTTimerFireTime` interval).
    pub remaining: Option<f64>,
    pub deep_link: String,
}

impl SystemTimer {
    pub fn remaining_secs(&self, now_unix: f64) -> u32 {
        remaining_secs(
            self.state,
            self.fire_date,
            self.remaining,
            self.duration,
            now_unix,
        )
    }

    pub fn total_secs(&self) -> u32 {
        self.duration.max(0.0).ceil() as u32
    }
}

/// Remaining seconds from the stored fire date / interval. Pure; used by tests.
pub fn remaining_secs(
    state: MTTimerState,
    fire_date: Option<f64>,
    remaining: Option<f64>,
    duration: f64,
    now_unix: f64,
) -> u32 {
    match state {
        MTTimerState::Running => fire_date
            .map(|fire| (fire - now_unix).max(0.0).ceil() as u32)
            .or_else(|| remaining.map(|r| r.max(0.0).ceil() as u32))
            .unwrap_or(0),
        MTTimerState::Paused | MTTimerState::Stopped => remaining
            .map(|r| r.max(0.0).ceil() as u32)
            .unwrap_or_else(|| duration.max(0.0).ceil() as u32),
        MTTimerState::Fired => 0,
        MTTimerState::Dismissed | MTTimerState::Invalid => 0,
    }
}

pub fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn apple_to_unix(apple: f64) -> f64 {
    apple + APPLE_EPOCH
}

pub fn unix_to_apple(unix: f64) -> f64 {
    unix - APPLE_EPOCH
}

fn channel() -> &'static watch::Sender<Vec<SystemTimer>> {
    static TX: OnceLock<watch::Sender<Vec<SystemTimer>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, _) = watch::channel(Vec::new());
        tx
    })
}

/// Subscribe to Clock timer snapshots. The sender is process-global; the
/// vnode watcher publishes here and the island consumes `changed()`.
pub fn subscribe() -> watch::Receiver<Vec<SystemTimer>> {
    channel().subscribe()
}

pub fn current() -> Vec<SystemTimer> {
    channel().borrow().clone()
}

/// Publish `timers` if they differ from the last snapshot.
pub fn publish(timers: Vec<SystemTimer>) {
    let tx = channel();
    if *tx.borrow() != timers {
        let _ = tx.send(timers);
    }
}

/// Start the vnode watcher once. Safe to call from [`crate::init`].
pub fn start_watcher() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        publish(read_system_timers());
        #[cfg(target_os = "macos")]
        macos::spawn_vnode_watch();
    });
}

/// Read Clock timers: CFPreferences/plist first, group-container sqlite second.
pub fn read_system_timers() -> Vec<SystemTimer> {
    let from_plist = read_plist_timers();
    if !from_plist.is_empty() {
        return collapse_timers(from_plist);
    }
    collapse_timers(read_sqlite_timers())
}

fn read_plist_timers() -> Vec<SystemTimer> {
    #[cfg(target_os = "macos")]
    macos::synchronize_prefs();
    let Some(path) = prefs_plist_path() else {
        return Vec::new();
    };
    match plist::Value::from_file(&path) {
        Ok(value) => parse_mt_timers(&value),
        Err(err) => {
            if path.exists() {
                log::debug!("mobiletimerd plist unreadable ({err})");
            }
            Vec::new()
        }
    }
}

fn read_sqlite_timers() -> Vec<SystemTimer> {
    let Some(path) = sqlite_path() else {
        return Vec::new();
    };
    if !path.exists() {
        return Vec::new();
    }
    read_sqlite_file(&path)
}

pub fn prefs_plist_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join("Library/Preferences")
            .join(format!("{PREFS_DOMAIN}.plist"))
    })
}

pub fn sqlite_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join("Library/Group Containers/group.com.apple.mobiletimerd/local.sqlite")
    })
}

pub fn sqlite_wal_path() -> Option<PathBuf> {
    sqlite_path().map(|p| {
        let mut wal = p.into_os_string();
        wal.push("-wal");
        PathBuf::from(wal)
    })
}

/// Parse an `MTTimers` array (or a domain dict that contains one).
pub fn parse_mt_timers(value: &plist::Value) -> Vec<SystemTimer> {
    match value {
        plist::Value::Array(items) => items.iter().filter_map(parse_mt_timer).collect(),
        plist::Value::Dictionary(dict) => {
            if let Some(timers) = dict.get(PREFS_KEY_TIMERS) {
                return parse_mt_timers(timers);
            }
            if let Some(data) = dict.get(PREFS_KEY_TIMERS).and_then(plist::Value::as_data) {
                if let Some(decoded) = decode_keyed_archive(data) {
                    return parse_mt_timers(&decoded);
                }
            }
            // A single timer dict, or an NSKeyedArchiver envelope.
            if dict.contains_key("$archiver") {
                if let Some(root) = resolve_keyed_root(dict) {
                    return parse_mt_timers(&root);
                }
            }
            if dict.contains_key("MTTimerID") || dict.contains_key("MTTimerState") {
                return parse_mt_timer(value).into_iter().collect();
            }
            Vec::new()
        }
        plist::Value::Data(bytes) => decode_keyed_archive(bytes)
            .map(|inner| parse_mt_timers(&inner))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn parse_mt_timer(value: &plist::Value) -> Option<SystemTimer> {
    let dict = value.as_dictionary()?;
    let id = dict_string(dict, "MTTimerID")
        .or_else(|| dict_string(dict, "timerID"))
        .filter(|s| !s.is_empty())?;
    let title = dict_string(dict, "MTTimerTitle")
        .or_else(|| dict_string(dict, "title"))
        .unwrap_or_default();
    let duration = dict_f64(dict, "MTTimerDuration")
        .or_else(|| dict_f64(dict, "duration"))
        .unwrap_or(0.0);
    let state = MTTimerState::from_raw(
        dict_i64(dict, "MTTimerState")
            .or_else(|| dict_i64(dict, "state"))
            .unwrap_or(0),
    );
    let dismissed = dict.get("MTTimerDismissedDate").and_then(plist_date_unix);
    if dismissed.is_some() && !state.is_active() {
        return None;
    }
    let fire = decode_fire_time(
        dict.get("MTTimerFireTime")
            .or_else(|| dict.get("MTTimerFireDate"))
            .or_else(|| dict.get("fireTime")),
    );
    let fired = dict.get("MTTimerFiredDate").and_then(plist_date_unix);
    let fire_date = match state {
        MTTimerState::Running => fire.unix.or(fired),
        MTTimerState::Fired => fired.or(fire.unix),
        _ => fire.unix,
    };
    let remaining = match state {
        MTTimerState::Running => None,
        _ => fire.interval.or_else(|| {
            fire_date.map(|fd| (fd - unix_now()).max(0.0)).or(Some(duration))
        }),
    };
    Some(SystemTimer {
        deep_link: format!("{CLOCK_TIMER_SCHEME}{id}"),
        id,
        title,
        duration,
        state,
        fire_date,
        remaining,
    })
}

/// Decoded `MTTimerFireTime`: absolute Unix date and/or remaining interval.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FireTime {
    pub unix: Option<f64>,
    pub interval: Option<f64>,
}

pub fn decode_fire_time(value: Option<&plist::Value>) -> FireTime {
    let Some(value) = value else {
        return FireTime::default();
    };
    match value {
        plist::Value::Date(date) => FireTime {
            unix: plist_date_unix(value).or_else(|| Some(system_time_unix(date.to_owned().into()))),
            interval: None,
        },
        plist::Value::Real(n) => classify_number(*n),
        plist::Value::Integer(n) => n.as_signed().map(|i| classify_number(i as f64)).unwrap_or_default(),
        plist::Value::Data(bytes) => decode_keyed_archive(bytes)
            .map(|inner| decode_fire_time(Some(&inner)))
            .unwrap_or_default(),
        plist::Value::Dictionary(dict) => decode_fire_dict(dict),
        _ => FireTime::default(),
    }
}

fn decode_fire_dict(dict: &plist::Dictionary) -> FireTime {
    if dict.contains_key("$archiver") {
        if let Some(root) = resolve_keyed_root(dict) {
            return decode_fire_time(Some(&root));
        }
    }
    if let Some(time) = dict_f64(dict, "NS.time") {
        return FireTime {
            unix: Some(apple_to_unix(time)),
            interval: None,
        };
    }
    for key in [
        "MTTimerTimeInterval",
        "timeInterval",
        "interval",
        "remaining",
        "NS.number",
        "seconds",
        "value",
    ] {
        if let Some(n) = dict_f64(dict, key) {
            return classify_number(n);
        }
    }
    if let Some(class) = keyed_class_name(dict) {
        if class.contains("Interval") {
            if let Some(n) = dict.values().find_map(value_f64) {
                return FireTime {
                    unix: None,
                    interval: Some(n.max(0.0)),
                };
            }
        }
        if class.contains("Date") {
            if let Some(n) = dict.values().find_map(value_f64) {
                return classify_number(n);
            }
        }
    }
    FireTime::default()
}

/// A raw number is an interval when it looks like a countdown, otherwise a date.
pub fn classify_number(n: f64) -> FireTime {
    if !n.is_finite() || n == 0.0 {
        return FireTime::default();
    }
    // Remaining / duration: Clock timers are hours, not years.
    if (0.0..86_400.0 * 7.0).contains(&n) {
        return FireTime {
            unix: None,
            interval: Some(n),
        };
    }
    // NSDate `NS.time` (Apple reference) after ~2004 sits around 1e8+.
    if (1.0e8..1.0e10).contains(&n) && n < APPLE_EPOCH {
        return FireTime {
            unix: Some(apple_to_unix(n)),
            interval: None,
        };
    }
    if n > 1.0e12 {
        return FireTime {
            unix: Some(n / 1000.0),
            interval: None,
        };
    }
    if n > 1.0e9 {
        return FireTime {
            unix: Some(n),
            interval: None,
        };
    }
    FireTime {
        unix: None,
        interval: Some(n.max(0.0)),
    }
}

fn decode_keyed_archive(bytes: &[u8]) -> Option<plist::Value> {
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes)).ok()?;
    match &value {
        plist::Value::Dictionary(dict) if dict.contains_key("$archiver") => resolve_keyed_root(dict),
        _ => Some(value),
    }
}

fn resolve_keyed_root(dict: &plist::Dictionary) -> Option<plist::Value> {
    let objects = dict.get("$objects")?.as_array()?;
    let top = dict.get("$top")?.as_dictionary()?;
    let uid = top
        .get("root")
        .or_else(|| top.get("NS.objects"))
        .or_else(|| top.values().next())?;
    resolve_uid(objects, uid)
}

fn resolve_uid(objects: &[plist::Value], value: &plist::Value) -> Option<plist::Value> {
    let idx = match value {
        plist::Value::Uid(uid) => uid.get() as usize,
        plist::Value::Integer(i) => i.as_unsigned()? as usize,
        other => return Some(other.clone()),
    };
    objects.get(idx).cloned()
}

fn keyed_class_name(dict: &plist::Dictionary) -> Option<String> {
    dict.get("$classname")
        .and_then(plist::Value::as_string)
        .map(|s| s.to_string())
        .or_else(|| {
            dict.get("$classes")
                .and_then(plist::Value::as_array)
                .and_then(|a| a.first())
                .and_then(plist::Value::as_string)
                .map(|s| s.to_string())
        })
}

fn dict_string(dict: &plist::Dictionary, key: &str) -> Option<String> {
    dict.get(key).and_then(|v| match v {
        plist::Value::String(s) => Some(s.clone()),
        plist::Value::Data(bytes) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    })
}

fn dict_f64(dict: &plist::Dictionary, key: &str) -> Option<f64> {
    dict.get(key).and_then(value_f64)
}

fn dict_i64(dict: &plist::Dictionary, key: &str) -> Option<i64> {
    dict.get(key).and_then(|v| match v {
        plist::Value::Integer(i) => i.as_signed(),
        plist::Value::Real(n) => Some(*n as i64),
        plist::Value::String(s) => s.parse().ok(),
        _ => None,
    })
}

fn value_f64(value: &plist::Value) -> Option<f64> {
    match value {
        plist::Value::Real(n) => Some(*n),
        plist::Value::Integer(i) => i.as_signed().map(|n| n as f64),
        plist::Value::String(s) => s.parse().ok(),
        plist::Value::Date(_) => plist_date_unix(value),
        _ => None,
    }
}

fn plist_date_unix(value: &plist::Value) -> Option<f64> {
    match value {
        plist::Value::Date(date) => Some(system_time_unix((*date).into())),
        _ => None,
    }
}

fn system_time_unix(time: SystemTime) -> f64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .or_else(|_| {
            UNIX_EPOCH
                .duration_since(time)
                .map(|d| -(d.as_secs_f64()))
        })
        .unwrap_or(0.0)
}

/// mobiletimerd hoards every timer ever created. Keep one row per id, preferring
/// the live state (running > paused > fired > rest).
pub fn collapse_timers(timers: Vec<SystemTimer>) -> Vec<SystemTimer> {
    let mut best: HashMap<String, SystemTimer> = HashMap::new();
    for timer in timers {
        if timer.id.is_empty() || !timer.state.is_active() {
            continue;
        }
        best.entry(timer.id.clone())
            .and_modify(|existing| {
                if state_rank(timer.state) >= state_rank(existing.state) {
                    *existing = timer.clone();
                }
            })
            .or_insert(timer);
    }
    let mut out: Vec<SystemTimer> = best.into_values().collect();
    out.sort_by(|a, b| {
        state_rank(b.state)
            .cmp(&state_rank(a.state))
            .then_with(|| a.fire_date.partial_cmp(&b.fire_date).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

fn state_rank(state: MTTimerState) -> u8 {
    match state {
        MTTimerState::Running => 4,
        MTTimerState::Paused => 3,
        MTTimerState::Fired => 2,
        MTTimerState::Stopped => 1,
        _ => 0,
    }
}

fn read_sqlite_file(path: &Path) -> Vec<SystemTimer> {
    let uri = format!("file:{}?mode=ro", path.display());
    let conn = match rusqlite::Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    ) {
        Ok(conn) => conn,
        Err(err) => {
            log::debug!("mobiletimerd sqlite skipped ({err})");
            return Vec::new();
        }
    };
    let columns = sqlite_columns(&conn);
    if columns.is_empty() {
        return Vec::new();
    }
    let sql = format!(
        "SELECT {} FROM ZMTCDTIMER",
        columns.join(", ")
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(err) => {
            log::debug!("mobiletimerd sqlite query skipped ({err})");
            return Vec::new();
        }
    };
    let names: Vec<String> = stmt.column_names().into_iter().map(|s| s.to_string()).collect();
    let mut rows = match stmt.query([]) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    while let Ok(Some(row)) = rows.next() {
        if let Some(timer) = timer_from_sqlite_row(row, &names) {
            out.push(timer);
        }
    }
    out
}

fn sqlite_columns(conn: &rusqlite::Connection) -> Vec<&'static str> {
    let info = conn.prepare("PRAGMA table_info(ZMTCDTIMER)");
    let Ok(mut stmt) = info else {
        return Vec::new();
    };
    let rows = stmt.query_map([], |row| row.get::<_, String>(1));
    let Ok(rows) = rows else {
        return Vec::new();
    };
    let present: Vec<String> = rows.filter_map(|r| r.ok()).collect();
    const CANDIDATES: &[&str] = &[
        "ZTIMERURL",
        "ZTITLE",
        "ZDURATION",
        "ZSTATE",
        "ZFIREDDATE",
        "ZDISMISSEDDATE",
        "ZFIRETIME",
        "ZTIMERID",
        "ZUUID",
    ];
    CANDIDATES
        .iter()
        .copied()
        .filter(|name| present.iter().any(|p| p.eq_ignore_ascii_case(name)))
        .collect()
}

fn timer_from_sqlite_row(row: &rusqlite::Row<'_>, names: &[String]) -> Option<SystemTimer> {
    let get_str = |name: &str| -> Option<String> {
        let idx = names.iter().position(|n| n.eq_ignore_ascii_case(name))?;
        row.get::<_, Option<String>>(idx).ok().flatten()
    };
    let get_f64 = |name: &str| -> Option<f64> {
        let idx = names.iter().position(|n| n.eq_ignore_ascii_case(name))?;
        row.get::<_, Option<f64>>(idx)
            .ok()
            .flatten()
            .or_else(|| row.get::<_, Option<i64>>(idx).ok().flatten().map(|n| n as f64))
    };
    let get_i64 = |name: &str| -> Option<i64> {
        let idx = names.iter().position(|n| n.eq_ignore_ascii_case(name))?;
        row.get::<_, Option<i64>>(idx)
            .ok()
            .flatten()
            .or_else(|| row.get::<_, Option<f64>>(idx).ok().flatten().map(|n| n as i64))
    };
    let get_blob = |name: &str| -> Option<Vec<u8>> {
        let idx = names.iter().position(|n| n.eq_ignore_ascii_case(name))?;
        row.get::<_, Option<Vec<u8>>>(idx).ok().flatten()
    };

    let url = get_str("ZTIMERURL").unwrap_or_default();
    let id = url_timer_id(&url)
        .or_else(|| get_str("ZTIMERID"))
        .or_else(|| get_str("ZUUID"))
        .filter(|s| !s.is_empty())?;
    let state = MTTimerState::from_raw(get_i64("ZSTATE").unwrap_or(0));
    if get_f64("ZDISMISSEDDATE").is_some() && !state.is_active() {
        return None;
    }
    let duration = get_f64("ZDURATION").unwrap_or(0.0);
    let fire = get_blob("ZFIRETIME")
        .and_then(|bytes| Some(decode_fire_time(Some(&plist::Value::Data(bytes)))))
        .unwrap_or_default();
    let fired = get_f64("ZFIREDDATE").map(normalize_store_date);
    let fire_date = match state {
        MTTimerState::Running => fire.unix.or(fired),
        MTTimerState::Fired => fired.or(fire.unix),
        _ => fire.unix,
    };
    Some(SystemTimer {
        title: get_str("ZTITLE").unwrap_or_default(),
        duration,
        state,
        fire_date,
        remaining: match state {
            MTTimerState::Running => None,
            _ => fire.interval,
        },
        deep_link: if url.starts_with("x-apple-clock:") {
            url
        } else {
            format!("{CLOCK_TIMER_SCHEME}{id}")
        },
        id,
    })
}

fn url_timer_id(url: &str) -> Option<String> {
    let idx = url.find("id=")?;
    let rest = &url[idx + 3..];
    let end = rest.find('&').unwrap_or(rest.len());
    let id = rest[..end].trim();
    (!id.is_empty()).then(|| id.to_string())
}

fn normalize_store_date(n: f64) -> f64 {
    if n > 1.0e12 {
        n / 1000.0
    } else if n < APPLE_EPOCH && n > 1.0e7 {
        apple_to_unix(n)
    } else {
        n
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::ffi::CString;
    use std::os::fd::RawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;
    use std::thread;
    use std::time::Duration;

    pub fn synchronize_prefs() {
        unsafe {
            let name = CFStringCreateWithCString(
                std::ptr::null(),
                c"com.apple.mobiletimerd".as_ptr(),
                kCFStringEncodingUTF8,
            );
            if !name.is_null() {
                let _ = CFPreferencesAppSynchronize(name);
                CFRelease(name);
            }
        }
    }

    pub fn spawn_vnode_watch() {
        thread::Builder::new()
            .name("nook-clock-timers".into())
            .spawn(watch_loop)
            .ok();
    }

    fn watch_loop() {
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            log::warn!("kqueue for Clock timers failed");
            return;
        }
        let mut watched: Vec<Watched> = Vec::new();
        loop {
            refresh_watches(kq, &mut watched);
            let mut ev = libc::kevent {
                ident: 0,
                filter: 0,
                flags: 0,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            let timeout = libc::timespec {
                tv_sec: 30,
                tv_nsec: 0,
            };
            let n = unsafe { libc::kevent(kq, std::ptr::null(), 0, &mut ev, 1, &timeout) };
            if n < 0 {
                thread::sleep(Duration::from_millis(250));
                continue;
            }
            if n > 0 {
                // cfprefsd writes via rename; give the new file a moment.
                thread::sleep(Duration::from_millis(40));
                refresh_watches(kq, &mut watched);
            }
            publish(read_system_timers());
        }
    }

    struct Watched {
        path: PathBuf,
        file: Option<std::fs::File>,
        #[allow(dead_code)]
        ident: usize,
    }

    impl Drop for Watched {
        fn drop(&mut self) {
            self.file.take();
        }
    }

    fn watch_targets() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(plist) = prefs_plist_path() {
            if let Some(parent) = plist.parent() {
                paths.push(parent.to_path_buf());
            }
            paths.push(plist);
        }
        if let Some(db) = sqlite_path() {
            paths.push(db);
        }
        if let Some(wal) = sqlite_wal_path() {
            paths.push(wal);
        }
        paths
    }

    fn refresh_watches(kq: RawFd, watched: &mut Vec<Watched>) {
        let targets = watch_targets();
        watched.retain(|w| targets.iter().any(|p| p == &w.path));
        for path in targets {
            if watched.iter().any(|w| w.path == path) {
                // Re-open if the vnode vanished (atomic rename).
                if let Some(slot) = watched.iter_mut().find(|w| w.path == path) {
                    if slot.file.is_none() || !path.exists() {
                        if let Some(next) = open_watch(kq, &path) {
                            *slot = next;
                        }
                    }
                }
                continue;
            }
            if let Some(item) = open_watch(kq, &path) {
                watched.push(item);
            }
        }
    }

    fn open_watch(kq: RawFd, path: &Path) -> Option<Watched> {
        if !path.exists() {
            return None;
        }
        let file = std::fs::File::open(path).ok()?;
        let fd = file.as_raw_fd();
        let flags = (libc::NOTE_DELETE
            | libc::NOTE_WRITE
            | libc::NOTE_EXTEND
            | libc::NOTE_RENAME
            | libc::NOTE_REVOKE
            | libc::NOTE_ATTRIB) as u32;
        let ev = libc::kevent {
            ident: fd as usize,
            filter: libc::EVFILT_VNODE,
            flags: (libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR) as u16,
            fflags: flags,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        let rc = unsafe { libc::kevent(kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
        if rc < 0 {
            // Directory may not accept every NOTE_*; still keep the fd if add failed
            // only because the file disappeared mid-open.
            log::debug!("vnode watch add failed for {}", path.display());
        }
        let _ = CString::new(path.as_os_str().as_bytes());
        Some(Watched {
            path: path.to_path_buf(),
            file: Some(file),
            ident: fd as usize, // kqueue ident; kept so the File outlives the watch
        })
    }

    const kCFStringEncodingUTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFPreferencesAppSynchronize(applicationID: *const std::ffi::c_void) -> u8;
        fn CFStringCreateWithCString(
            alloc: *const std::ffi::c_void,
            cStr: *const i8,
            encoding: u32,
        ) -> *mut std::ffi::c_void;
        fn CFRelease(cf: *const std::ffi::c_void);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer(
        id: &str,
        state: MTTimerState,
        duration: f64,
        fire_date: Option<f64>,
        remaining: Option<f64>,
    ) -> SystemTimer {
        SystemTimer {
            id: id.into(),
            title: id.into(),
            duration,
            state,
            fire_date,
            remaining,
            deep_link: format!("{CLOCK_TIMER_SCHEME}{id}"),
        }
    }

    #[test]
    fn mt_timer_state_maps_mobiletimer_enum() {
        assert_eq!(MTTimerState::from_raw(0), MTTimerState::Invalid);
        assert_eq!(MTTimerState::from_raw(1), MTTimerState::Stopped);
        assert_eq!(MTTimerState::from_raw(2), MTTimerState::Running);
        assert_eq!(MTTimerState::from_raw(3), MTTimerState::Paused);
        assert_eq!(MTTimerState::from_raw(4), MTTimerState::Fired);
        assert_eq!(MTTimerState::from_raw(5), MTTimerState::Dismissed);
        assert_eq!(MTTimerState::from_raw(99), MTTimerState::Invalid);
        assert!(MTTimerState::Running.is_active());
        assert!(MTTimerState::Paused.is_active());
        assert!(MTTimerState::Fired.is_active());
        assert!(!MTTimerState::Stopped.is_active());
        assert!(!MTTimerState::Dismissed.is_active());
        assert!(MTTimerState::Running.is_running());
        assert!(MTTimerState::Running.is_counting());
        assert!(!MTTimerState::Paused.is_counting());
    }

    #[test]
    fn remaining_from_absolute_fire_date() {
        let now = 1_700_000_000.0;
        assert_eq!(
            remaining_secs(MTTimerState::Running, Some(now + 90.2), None, 300.0, now),
            91
        );
        assert_eq!(
            remaining_secs(MTTimerState::Running, Some(now - 5.0), None, 300.0, now),
            0
        );
        assert_eq!(
            remaining_secs(MTTimerState::Fired, Some(now - 1.0), None, 300.0, now),
            0
        );
    }

    #[test]
    fn remaining_from_paused_interval() {
        let now = 1_700_000_000.0;
        assert_eq!(
            remaining_secs(MTTimerState::Paused, None, Some(45.1), 300.0, now),
            46
        );
        assert_eq!(
            remaining_secs(MTTimerState::Stopped, None, None, 120.4, now),
            121
        );
        assert_eq!(
            remaining_secs(MTTimerState::Dismissed, None, Some(10.0), 300.0, now),
            0
        );
    }

    #[test]
    fn apple_epoch_round_trip() {
        let unix = 1_700_000_000.0;
        assert!((apple_to_unix(unix_to_apple(unix)) - unix).abs() < f64::EPSILON);
        assert!((apple_to_unix(0.0) - APPLE_EPOCH).abs() < f64::EPSILON);
    }

    #[test]
    fn classify_number_splits_interval_and_date() {
        let interval = classify_number(300.0);
        assert_eq!(interval.interval, Some(300.0));
        assert!(interval.unix.is_none());

        let apple = classify_number(700_000_000.0);
        assert!(apple.unix.is_some());
        assert!((apple.unix.unwrap() - apple_to_unix(700_000_000.0)).abs() < 0.001);

        let unix = classify_number(1_700_000_000.0);
        assert_eq!(unix.unix, Some(1_700_000_000.0));
    }

    #[test]
    fn decode_ns_time_fire_date() {
        let mut dict = plist::Dictionary::new();
        dict.insert("NS.time".into(), plist::Value::Real(700_000_000.0));
        let fire = decode_fire_time(Some(&plist::Value::Dictionary(dict)));
        assert!((fire.unix.unwrap() - apple_to_unix(700_000_000.0)).abs() < 0.001);
    }

    #[test]
    fn decode_interval_keyed_object() {
        let mut dict = plist::Dictionary::new();
        dict.insert("$classname".into(), plist::Value::String("MTTimerTimeInterval".into()));
        dict.insert("timeInterval".into(), plist::Value::Real(90.0));
        let fire = decode_fire_time(Some(&plist::Value::Dictionary(dict)));
        assert_eq!(fire.interval, Some(90.0));
    }

    #[test]
    fn parse_mt_timers_array_and_filter_dismissed() {
        let xml = r#"
        <dict>
            <key>MTTimers</key>
            <array>
                <dict>
                    <key>MTTimerID</key><string>AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA</string>
                    <key>MTTimerTitle</key><string>Pasta</string>
                    <key>MTTimerDuration</key><real>600</real>
                    <key>MTTimerState</key><integer>2</integer>
                    <key>MTTimerFireTime</key><real>1700000120</real>
                </dict>
                <dict>
                    <key>MTTimerID</key><string>BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB</string>
                    <key>MTTimerTitle</key><string>Old</string>
                    <key>MTTimerDuration</key><real>60</real>
                    <key>MTTimerState</key><integer>5</integer>
                    <key>MTTimerDismissedDate</key><real>1700000000</real>
                </dict>
                <dict>
                    <key>MTTimerID</key><string>CCCCCCCC-CCCC-CCCC-CCCC-CCCCCCCCCCCC</string>
                    <key>MTTimerTitle</key><string>Hold</string>
                    <key>MTTimerDuration</key><real>120</real>
                    <key>MTTimerState</key><integer>3</integer>
                    <key>MTTimerFireTime</key><real>44</real>
                </dict>
            </array>
        </dict>
        "#;
        let value = plist::Value::from_reader_xml(std::io::Cursor::new(xml)).unwrap();
        let parsed = collapse_timers(parse_mt_timers(&value));
        assert_eq!(parsed.len(), 2);
        let pasta = parsed.iter().find(|t| t.title == "Pasta").unwrap();
        assert_eq!(pasta.state, MTTimerState::Running);
        assert_eq!(pasta.fire_date, Some(1_700_000_120.0));
        assert_eq!(
            pasta.deep_link,
            "x-apple-clock:timer?id=AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA"
        );
        assert_eq!(pasta.remaining_secs(1_700_000_000.0), 120);
        let hold = parsed.iter().find(|t| t.title == "Hold").unwrap();
        assert_eq!(hold.state, MTTimerState::Paused);
        assert_eq!(hold.remaining_secs(1_700_000_000.0), 44);
    }

    #[test]
    fn collapse_prefers_running_duplicate() {
        let timers = vec![
            timer("same", MTTimerState::Dismissed, 60.0, None, None),
            timer("same", MTTimerState::Paused, 60.0, None, Some(10.0)),
            timer("same", MTTimerState::Running, 60.0, Some(9_999.0), None),
        ];
        let out = collapse_timers(timers);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].state, MTTimerState::Running);
        assert_eq!(out[0].fire_date, Some(9_999.0));
    }

    #[test]
    fn url_timer_id_extracts_uuid() {
        assert_eq!(
            url_timer_id("x-apple-clock:timer?id=ABC-123&x=1").as_deref(),
            Some("ABC-123")
        );
        assert!(url_timer_id("clock-timer://").is_none());
    }
}
