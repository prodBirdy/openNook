use crate::database::{get_connection, log_sql};
use serde::{Deserialize, Serialize};
use std::fs;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileTrayItem {
    pub name: String,
    pub size: i64,
    pub path: String,
    #[serde(rename = "type")]
    pub mime_type: String,
    #[serde(rename = "lastModified")]
    pub last_modified: i64,
}

pub fn save_file_tray(files: Vec<FileTrayItem>) -> Result<(), String> {
    let conn = get_connection().map_err(|e| e.to_string())?;
    conn.execute_batch("BEGIN TRANSACTION;")
        .map_err(|e| e.to_string())?;
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

pub fn load_file_tray() -> Result<Vec<FileTrayItem>, String> {
    let conn = get_connection().map_err(|e| e.to_string())?;
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

pub fn open_file(path: String) -> Result<(), String> {
    open::that(&path).map_err(|e| e.to_string())
}

#[allow(unused_variables)]
pub fn reveal_file(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            open::that(parent).map_err(|e| e.to_string())?;
        } else {
            open::that(&path).map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

pub fn resolve_path(path: String) -> Result<String, String> {
    fs::canonicalize(&path)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

/// Drag-pasteboard changeCount last seen with no mouse button held, or
/// [`i64::MIN`] before we have ever observed an idle cursor.
#[cfg(target_os = "macos")]
static DRAG_PB_IDLE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(i64::MIN);

/// True while the user is dragging files (Finder / another app) on macOS.
/// Used to punch a hole in click-through so the island can receive the drop.
///
/// The drag pasteboard is *not* emptied when a drag ends — it keeps the last
/// drag's types indefinitely — so "the board has types" reports a drag forever
/// after the first one, which pins the island's hover padding open and leaves
/// the full-width overlay swallowing every click. Gate on the left button
/// actually being held, and on the board having changed since it was last seen
/// idle, so a leftover board plus an unrelated left-drag doesn't count either.
pub fn file_drag_active() -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::*;
        use std::sync::atomic::Ordering;

        unsafe {
            let name: *mut AnyObject = msg_send![
                class!(NSString),
                stringWithUTF8String: c"Apple CFPasteboard drag".as_ptr()
            ];
            let pb: *mut AnyObject = msg_send![class!(NSPasteboard), pasteboardWithName: name];
            if pb.is_null() {
                return false;
            }
            let change_count: i64 = msg_send![pb, changeCount];

            // No button down means nothing can be in flight; re-baseline so the
            // next press only counts once something new lands on the board.
            let buttons: usize = msg_send![class!(NSEvent), pressedMouseButtons];
            if buttons & 1 == 0 {
                DRAG_PB_IDLE.store(change_count, Ordering::Relaxed);
                return false;
            }
            // Launched mid-press: no baseline to compare against, stay quiet.
            let idle = DRAG_PB_IDLE.load(Ordering::Relaxed);
            if idle == i64::MIN || change_count == idle {
                return false;
            }

            let types: *mut AnyObject = msg_send![pb, types];
            if types.is_null() {
                return false;
            }
            // Any type on the drag pasteboard means a drag is in progress.
            // Finder often exposes promised-file / dyn.* UTIs that do not
            // contain the substring "file", so a name filter misses the drag
            // and click-through stays on until the cursor is already inside
            // the overlay — at which point AppKit never sends draggingEntered.
            let count: usize = msg_send![types, count];
            count > 0
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn add_dropped_path(path: &str) -> Result<FileTrayItem, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let mime = mime_from_path(path);
    let last_modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    Ok(FileTrayItem {
        name,
        size: meta.len() as i64,
        path: path.to_string(),
        mime_type: mime,
        last_modified,
    })
}

pub(crate) fn mime_from_path(path: &str) -> String {
    match std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "heic") => "image".into(),
        Some("mp4" | "mov" | "mkv") => "video".into(),
        Some("mp3" | "wav" | "aac" | "flac") => "audio".into(),
        Some("pdf") => "pdf".into(),
        Some("zip" | "tar" | "gz") => "archive".into(),
        _ => "file".into(),
    }
}

pub fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_from_common_extensions() {
        assert_eq!(mime_from_path("a.PNG"), "image");
        assert_eq!(mime_from_path("clip.mp4"), "video");
        assert_eq!(mime_from_path("doc.pdf"), "pdf");
        assert_eq!(mime_from_path("noext"), "file");
    }

    #[test]
    fn format_size_buckets() {
        assert_eq!(format_size(12), "12 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(1_572_864), "1.5 MB");
    }
}
