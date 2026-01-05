use tauri::{command, AppHandle, Manager};
use std::process::Command;
use serde::{Deserialize, Serialize};
use std::fs;

#[cfg(target_os = "macos")]
use cocoa::base::{id, nil};
#[cfg(target_os = "macos")]
use cocoa::foundation::{NSPoint, NSRect, NSSize};
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl, class};

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
    let app_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
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
    let path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("file_tray.json");
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
pub fn start_drag(app_handle: AppHandle, path: String) -> Result<(), String> {
    println!("start_drag called for: {}", path);

    #[cfg(target_os = "macos")]
    unsafe {
        let window = app_handle.get_webview_window("main").ok_or("Main window not found")?;
        let ns_window = window.ns_window().map_err(|e| e.to_string())? as id;

        let content_view: id = msg_send![ns_window, contentView];
        if content_view == nil {
            return Err("Content view is nil".to_string());
        }

        // Create NSURL
        let path_str = std::ffi::CString::new(path.clone()).unwrap();
        let ns_string: id = msg_send![class!(NSString), stringWithUTF8String:path_str.as_ptr()];
        let file_url: id = msg_send![class!(NSURL), fileURLWithPath:ns_string];

        // Create NSDraggingItem
        let item_alloc: id = msg_send![class!(NSDraggingItem), alloc];
        let item: id = msg_send![item_alloc, initWithPasteboardWriter:file_url];

        // Get mouse location from window (coordinates are relative to window bottom-left)
        let mouse_loc: NSPoint = msg_send![ns_window, mouseLocationOutsideOfEventStream];

        // Create a rect centered at mouse
        let rect = NSRect::new(
            NSPoint::new(mouse_loc.x - 16.0, mouse_loc.y - 16.0),
            NSSize::new(32.0, 32.0)
        );

        // Set dragging frame
        let _: () = msg_send![item, setDraggingFrame:rect contents:nil];

        // Create array of items
        let items: id = msg_send![class!(NSArray), arrayWithObject:item];

        // Get current event
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let event: id = msg_send![app, currentEvent];

        // Begin dragging session
        let _: id = msg_send![content_view, beginDraggingSessionWithItems:items event:event source:content_view];
    }

    Ok(())
}

#[command]
pub fn resolve_path(path: String) -> Result<String, String> {
    fs::canonicalize(&path)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}
