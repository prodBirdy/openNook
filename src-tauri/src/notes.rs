use std::fs;
use std::path::PathBuf;

/// Get the path to the notes file in the user's home directory
fn get_notes_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".overdone").join("notes.txt")
}

/// Save notes to a file
#[tauri::command]
pub fn save_notes(notes: String) -> Result<(), String> {
    let path = get_notes_path();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    fs::write(&path, notes).map_err(|e| e.to_string())
}

/// Load notes from a file
#[tauri::command]
pub fn load_notes() -> Result<String, String> {
    let path = get_notes_path();

    if path.exists() {
        fs::read_to_string(&path).map_err(|e| e.to_string())
    } else {
        Ok(String::new())
    }
}
