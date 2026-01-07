use crate::database::{get_connection, log_sql};
use tauri::{AppHandle, Manager};

/// Save notes to the database (settings table)
#[tauri::command]
pub fn save_notes(app_handle: AppHandle, notes: String) -> Result<(), String> {
    let conn = get_connection(&app_handle).map_err(|e| e.to_string())?;

    let sql = "INSERT OR REPLACE INTO settings (key, value) VALUES ('notes', ?1)";
    log_sql(sql);

    conn.execute(sql, rusqlite::params![notes])
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Load notes from the database
#[tauri::command]
pub fn load_notes(app_handle: AppHandle) -> Result<String, String> {
    let conn = get_connection(&app_handle).map_err(|e| e.to_string())?;

    let sql = "SELECT value FROM settings WHERE key = 'notes'";
    log_sql(sql);

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let notes: Result<String, _> = stmt.query_row([], |row| row.get(0));

    match notes {
        Ok(n) => Ok(n),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(String::new()),
        Err(e) => Err(e.to_string()),
    }
}
