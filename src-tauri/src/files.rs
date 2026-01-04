use tauri::command;
use std::process::Command;

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
