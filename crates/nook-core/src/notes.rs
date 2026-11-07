use crate::app_data_dir;
use crate::database::{get_connection, log_sql};
use std::path::PathBuf;

pub fn notes_path() -> PathBuf {
    app_data_dir().join("notes.txt")
}

pub fn save_notes(notes: String) -> Result<(), String> {
    let conn = get_connection().map_err(|e| e.to_string())?;
    let sql = "INSERT OR REPLACE INTO settings (key, value) VALUES ('notes', ?1)";
    log_sql(sql);
    conn.execute(sql, rusqlite::params![notes])
        .map_err(|e| e.to_string())?;
    if let Err(err) = std::fs::write(notes_path(), &notes) {
        log::warn!("notes.txt: {err}");
    }
    Ok(())
}

pub fn load_notes() -> Result<String, String> {
    let path = notes_path();
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            return Ok(text);
        }
    }
    let conn = get_connection().map_err(|e| e.to_string())?;
    let sql = "SELECT value FROM settings WHERE key = 'notes'";
    log_sql(sql);
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    match stmt.query_row([], |row| row.get(0)) {
        Ok(n) => Ok(n),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(String::new()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn open_notes_editor() -> Result<(), String> {
    let path = notes_path();
    if !path.exists() {
        let existing = load_notes().unwrap_or_default();
        std::fs::write(&path, existing).map_err(|e| e.to_string())?;
    }
    open::that(&path).map_err(|e| e.to_string())
}
