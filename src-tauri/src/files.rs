use crate::database::{get_connection, log_sql};
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;
use tauri::{command, AppHandle};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileTrayItem {
    pub name: String,
    pub size: u64,
    pub path: String,
    #[serde(rename = "type")]
    pub mime_type: String,
    #[serde(rename = "lastModified")]
    pub last_modified: u64,
}

#[command]
pub fn save_file_tray(app_handle: AppHandle, files: Vec<FileTrayItem>) -> Result<(), String> {
    let conn = get_connection(&app_handle).map_err(|e| e.to_string())?;

    conn.execute_batch("BEGIN TRANSACTION;")
        .map_err(|e| e.to_string())?;

    // Clear existing (simpler than syncing for now)
    conn.execute("DELETE FROM file_tray", [])
        .map_err(|e| e.to_string())?;

    for file in files {
        let sql = "INSERT INTO file_tray (path, name, size, mime_type, last_modified) VALUES (?1, ?2, ?3, ?4, ?5)";
        log_sql(&format!("{} [{}]", sql, file.path));

        conn.execute(
            sql,
            rusqlite::params![
                file.path,
                file.name,
                file.size,
                file.mime_type,
                file.last_modified
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    conn.execute_batch("COMMIT;").map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub fn load_file_tray(app_handle: AppHandle) -> Result<Vec<FileTrayItem>, String> {
    let conn = get_connection(&app_handle).map_err(|e| e.to_string())?;

    let sql = "SELECT path, name, size, mime_type, last_modified FROM file_tray";
    log_sql(sql);

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(FileTrayItem {
                path: row.get(0)?,
                name: row.get(1)?,
                size: row.get(2)?,
                mime_type: row.get(3)?,
                last_modified: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut files = Vec::new();
    for row in rows {
        files.push(row.map_err(|e| e.to_string())?);
    }

    Ok(files)
}

#[command]
pub fn open_file(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    Command::new("open")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub fn reveal_file(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    Command::new("open")
        .args(&["-R", &path])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub fn on_file_drop(path: String) {
    println!("File dropped: {}", path);
}

#[command]
pub fn resolve_path(path: String) -> Result<String, String> {
    fs::canonicalize(&path)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

#[command]
pub fn save_drag_icon(_app_handle: AppHandle, icon_data: Vec<u8>) -> Result<String, String> {
    use std::io::Write;
    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join("temp_drag_icon.png");

    let mut file = fs::File::create(&file_path).map_err(|e| e.to_string())?;
    file.write_all(&icon_data).map_err(|e| e.to_string())?;

    Ok(file_path.to_string_lossy().into_owned())
}
