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
    let _ = ACTIVE_PATH.set(path.to_path_buf());
    Ok(())
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
