use tauri::{command, AppHandle, Manager};
use std::process::Command;
use serde::{Deserialize, Serialize};
use std::fs;

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
    #[cfg(target_os = "macos")]
    {
        use objc2::{rc::Retained, ClassType, ProtocolObject};
        use objc2_foundation::{NSURL, NSRect, NSPoint, NSArray};
        use objc2_app_kit::{
            NSView, NSDraggingItem, NSDraggingSource, NSApplication,
            NSWorkspace, NSDraggingSession
        };
        use raw_window_handle::HasWindowHandle;

        println!("start_drag called for: {}", path);

        let window = app_handle.get_webview_window("main").ok_or("Main window not found")?;

        // 1. Get the NSView from the window
        let ns_view = if let Ok(handle) = window.window_handle() {
            if let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() {
                unsafe { Retained::from_raw(appkit_handle.ns_view.as_ptr() as *mut NSView) }
            } else {
                return Err("Not running on macOS AppKit".into());
            }
        } else {
            return Err("Failed to get window handle".into());
        }
        .ok_or("Failed to wrap NSView")?;

        // 2. Create NSURL from path
        let url = unsafe {
            let path_str = objc2_foundation::NSString::from_str(&path);
            NSURL::fileURLWithPath(&path_str)
        };

        // 3. Create NSDraggingItem
        // NSDraggingItem::alloc() returns Allocated<NSDraggingItem>
        let dragging_item = unsafe {
            NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), &url)
        };

        // Set a default image (icon) for the drag
        unsafe {
             let workspace = NSWorkspace::sharedWorkspace();
             let icon = workspace.iconForFile(&objc2_foundation::NSString::from_str(&path));
             let size = icon.size();
             let frame = NSRect::new(
                 NSPoint::new(0.0, 0.0),
                 size
             );
             dragging_item.setDraggingFrame_contents(frame, Some(&icon));
        }

        // 4. Get the current event to initiate the drag
        let current_event = unsafe {
            NSApplication::sharedApplication().currentEvent()
        };

        if let Some(event) = current_event {
            // 5. Begin Dragging Session
             unsafe {
                let items = NSArray::from_slice(&[&dragging_item]);

                // We assume ns_view or the app delegate handles the source protocol.
                // Since we cannot easily implement the protocol in Rust without 'declare' features,
                // and we need to pass a valid object, we will try to pass the view itself cast to the protocol type.
                // This is a gamble: if the view (WKWebView) implements NSDraggingSource, it works.
                // If not, it might crash or throw an exception.

                // Transmute the view to the protocol object type to satisfy the type checker.
                // Note: ProtocolObject<dyn NSDraggingSource> layout is just the object pointer, so this cast is safe memory-wise,
                // but semantic-wise depends on the object.
                let source: &ProtocolObject<dyn NSDraggingSource> = std::mem::transmute(&*ns_view);

                 let _session: Option<Retained<NSDraggingSession>> = ns_view.beginDraggingSessionWithItems_event_source(
                     &items,
                     &event,
                     source
                 );
            }
            Ok(())
        } else {
             Err("No current event found (drag must be initiated by a user action)".into())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}
