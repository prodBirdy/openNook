use crate::app_data_dir;
use rusqlite::{Connection, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static ACTIVE_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Set when the primary Application Support DB could not be opened and the
/// process fell back to a temp-dir copy. The UI can surface this later.
pub static FALLBACK_DB_IN_USE: AtomicBool = AtomicBool::new(false);

pub fn log_sql(sql: &str) {
    log::debug!("SQL: {}", sql);
}

pub fn db_path() -> PathBuf {
    ACTIVE_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| app_data_dir().join("opennook.db"))
}

pub fn init_db() -> Result<()> {
    let primary = app_data_dir().join("opennook.db");
    match try_init(&primary) {
        Ok(()) => Ok(()),
        Err(err) => {
            let fallback = std::env::temp_dir().join("opennook-gpui-fallback.db");
            log::error!("database at {primary:?}: {err}; falling back to {fallback:?}");
            FALLBACK_DB_IN_USE.store(true, Ordering::Relaxed);
            try_init(&fallback)
        }
    }
}

fn try_init(path: &Path) -> Result<()> {
    let conn = Connection::open(path)?;
    migrate(&conn)?;
    restrict_db_mode(path);
    let _ = ACTIVE_PATH.set(path.to_path_buf());
    Ok(())
}

/// History is plain text; keep the file owner-only when the OS allows it.
fn restrict_db_mode(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perm = meta.permissions();
            perm.set_mode(0o600);
            let _ = std::fs::set_permissions(path, perm);
        }
    }
}

/// Ordered migration steps. Index `i` advances `user_version` to `i + 1`.
const MIGRATIONS: &[&[&str]] = &[
    // Version 1 — baseline schema.
    &[
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS widget_state (
            id TEXT PRIMARY KEY,
            enabled BOOLEAN NOT NULL DEFAULT 0,
            config TEXT
        )",
        "CREATE TABLE IF NOT EXISTS file_tray (
            path TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            size INTEGER,
            mime_type TEXT,
            last_modified INTEGER
        )",
        "CREATE TABLE IF NOT EXISTS observe_samples (
            query TEXT NOT NULL,
            at INTEGER NOT NULL,
            value REAL NOT NULL,
            PRIMARY KEY (query, at)
        )",
        "CREATE TABLE IF NOT EXISTS message_watermarks (
            conversation_id TEXT PRIMARY KEY,
            last_rowid INTEGER NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS lyrics (
            cache_key TEXT PRIMARY KEY,
            payload TEXT,
            hit INTEGER NOT NULL DEFAULT 0,
            fetched_at INTEGER NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS motion_artwork (
            cache_key TEXT PRIMARY KEY,
            m3u8 TEXT,
            preview TEXT,
            hit INTEGER NOT NULL DEFAULT 0,
            fetched_at INTEGER NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS recordings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            transcript TEXT NOT NULL DEFAULT ''
        )",
        "CREATE TABLE IF NOT EXISTS notification_shelf (
            id TEXT PRIMARY KEY,
            bundle_id TEXT NOT NULL,
            app_name TEXT NOT NULL,
            title TEXT NOT NULL,
            subtitle TEXT NOT NULL,
            body TEXT NOT NULL,
            delivered_at INTEGER NOT NULL,
            unread INTEGER NOT NULL DEFAULT 1
        )",
    ],
    // Version 2 — drop unused widget_state table.
    &["DROP TABLE IF EXISTS widget_state"],
];

fn migrate(conn: &Connection) -> Result<()> {
    let mut version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (index, steps) in MIGRATIONS.iter().enumerate() {
        let target = (index + 1) as i32;
        if version >= target {
            continue;
        }
        for sql in *steps {
            conn.execute(sql, [])?;
        }
        conn.execute(&format!("PRAGMA user_version = {target}"), [])?;
        version = target;
    }
    Ok(())
}

pub fn get_connection() -> Result<Connection> {
    Connection::open(db_path())
}

pub fn get_setting(key: &str) -> Option<String> {
    let conn = get_connection().ok()?;
    let mut stmt = conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .ok()?;
    stmt.query_row([key], |row| row.get(0)).ok()
}

pub fn set_setting(key: &str, value: &str) -> Result<(), String> {
    let conn = get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
