use crate::app_data_dir;
use rusqlite::{Connection, Result};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static ACTIVE_PATH: OnceLock<PathBuf> = OnceLock::new();

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

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS widget_state (
            id TEXT PRIMARY KEY,
            enabled BOOLEAN NOT NULL DEFAULT 0,
            config TEXT
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS file_tray (
            path TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            size INTEGER,
            mime_type TEXT,
            last_modified INTEGER
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS observe_samples (
            query TEXT NOT NULL,
            at INTEGER NOT NULL,
            value REAL NOT NULL,
            PRIMARY KEY (query, at)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS clipboard_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            text TEXT,
            image BLOB,
            app_bundle_id TEXT,
            copied_at INTEGER NOT NULL,
            pinned INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS clipboard_items_copied_at
         ON clipboard_items (copied_at)",
        [],
    )?;

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
