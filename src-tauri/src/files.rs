use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;
use tauri::{command, AppHandle, Manager};

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
    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
    }
    let path = app_dir.join("file_tray.json");
    let json = serde_json::to_string(&files).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub fn load_file_tray(app_handle: AppHandle) -> Result<Vec<FileTrayItem>, String> {
    let path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("file_tray.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let json = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let files: Vec<FileTrayItem> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
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
