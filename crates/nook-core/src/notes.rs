use crate::database::{get_connection, log_sql};

pub fn save_notes(notes: String) -> Result<(), String> {
    let conn = get_connection().map_err(|e| e.to_string())?;
    let sql = "INSERT OR REPLACE INTO settings (key, value) VALUES ('notes', ?1)";
    log_sql(sql);
    conn.execute(sql, rusqlite::params![notes])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_notes() -> Result<String, String> {
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
