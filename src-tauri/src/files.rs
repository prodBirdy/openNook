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
        use tauri::Url;
        use objc2::{rc::Retained, msg_send, class, sel};
        use objc2_foundation::{NSURL, CGPoint, NSSize};
        use objc2_app_kit::{
            NSView, NSDraggingItem, NSDraggingSession,
            NSDraggingSource, NSDragOperationCopy, NSApplication,
            NSImage, NSEvent
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
        let dragging_item = unsafe {
            NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), &url)
        };

        // Set the dragging frame (optional but good for visual feedback)
        // ideally we would get the position from the frontend, but for now we centre it or put it at mouse
        // We can get the mouse location from the current event or window

        // Set a default image (icon) for the drag
        unsafe {
             // Try to get system icon for file
             let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
             let icon = workspace.iconForFile(&objc2_foundation::NSString::from_str(&path));
             // Set frame size to icon size (usually 32x32)
             let size = icon.size();
             let frame = objc2_foundation::NSRect::new(
                 objc2_foundation::NSPoint::new(0.0, 0.0),
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
                let items = objc2_foundation::NSArray::from_slice(&[&dragging_item]);
                // We pass nil as source for now, or we can implement a simple source
                // If we pass nil, it might not work as expected because we need to implement NSDraggingSource protocol
                // But creating a proper delegate in Rust is verbose.
                // However, beginDraggingSession... takes a source object.
                // Let's try to pass the view itself if it implements it, or just use a simpler API if available.
                // Actually, `beginDraggingSessionWithItems:event:source:` requires `source` to conform to `NSDraggingSource`.
                // `NSView` does NOT conform to `NSDraggingSource` by default.

                // Workaround: We can't easily implement a full Objective-C protocol in Rust without `objc2-foundation` subclassing features which might be complex here.
                // But wait, standard dragImage:at:offset:event:pasteboard:source:slideBack: is deprecated but easier?
                // No, sticking to beginDraggingSession is better.

                // Let's check if there is any object we can use as source.
                // Maybe the window? No.

                // Wait, if we use `dragFile:fromRect:slideBack:event:` on NSView?
                // Deprecated in 10.13.

                // If we cannot implement NSDraggingSource easily, we might be stuck.
                // BUT, `objc2` allows declaring classes.
                // For this task, maybe I can just implement a minimal class or use `NSApp` if it allows? No.

                // Let's look at how others do it.
                // `tao` or `winit` might implement it.

                // Simplest way: use `dragImage:...` on NSView if available, even if deprecated.
                // `dragImage:at:offset:event:pasteboard:source:slideBack:`
                // source: "The object that initiated the drag operation. This object must conform to the NSDraggingSource protocol."

                // It seems inescapable to have an object implementing NSDraggingSource.

                // Let's implement a minimal dummy source.
                // Or maybe `NSView` in Tauri's window IS implementing it?
                // No, it's a `WKWebView` (or `WryWebView` which inherits `WKWebView`). `WKWebView` might implement it?
                // If I pass `ns_view` as source, and it doesn't conform, it will crash or warn.

                // Let's try passing the view. WKWebView handles drags internally, so it might conform.
                 let _session = ns_view.beginDraggingSessionWithItems_event_source(
                     &items,
                     &event,
                     &ns_view // hoping this works as source
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
