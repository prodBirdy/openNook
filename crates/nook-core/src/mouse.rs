//! Global mouse polling for hover-to-expand, independent of GPUI hit testing.
//!
//! Click-through is owned by the GPUI window (NSWindow ignoresMouseEvents).
//! This module only reports enter/exit against the island's UI bounds.

use crate::notch::get_screen_info;
use crate::settings::get_window_settings;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, RwLock};

#[derive(Debug, Clone, Copy)]
pub struct UiBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEvent {
    Entered,
    Exited,
}

static UI_BOUNDS: std::sync::OnceLock<RwLock<Option<UiBounds>>> = std::sync::OnceLock::new();
static IS_INSIDE: AtomicBool = AtomicBool::new(false);

fn bounds_store() -> &'static RwLock<Option<UiBounds>> {
    UI_BOUNDS.get_or_init(|| RwLock::new(None))
}

pub fn update_ui_bounds(x: f64, y: f64, width: f64, height: f64) {
    if let Ok(mut guard) = bounds_store().write() {
        *guard = Some(UiBounds {
            x,
            y,
            width,
            height,
        });
    }
}

pub fn is_inside() -> bool {
    IS_INSIDE.load(Ordering::Relaxed)
}

pub fn current_mouse_logical() -> (f64, f64) {
    #[cfg(target_os = "macos")]
    {
        use crate::notch::CGPoint;
        use objc2::*;
        unsafe {
            let mouse_loc: CGPoint = msg_send![class!(NSEvent), mouseLocation];
            let (_, screen_height, _, _) = get_screen_info();
            (mouse_loc.x, screen_height - mouse_loc.y)
        }
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut point = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut point);
        }
        (point.x as f64, point.y as f64)
    }
    #[cfg(target_os = "linux")]
    {
        (0.0, 0.0)
    }
}

pub fn hit_test(mouse_x: f64, mouse_y: f64) -> bool {
    let (screen_width, _, notch_height, notch_width) = get_screen_info();
    let settings = get_window_settings();
    let was_inside = IS_INSIDE.load(Ordering::Relaxed);
    let mut padding: f64 = if was_inside { 30.0 } else { 20.0 };
    // Widen the hover so click-through lifts *before* Finder's drag cursor
    // reaches the island; otherwise NSWindow never sees draggingEntered.
    if crate::files::file_drag_active() {
        padding = padding.max(80.0);
    }

    if let Ok(guard) = bounds_store().try_read() {
        if let Some(bounds) = *guard {
            let screen_x = (screen_width - bounds.width) / 2.0;
            return mouse_x >= (screen_x - padding)
                && mouse_x <= (screen_x + bounds.width + padding)
                && mouse_y >= (bounds.y - padding)
                && mouse_y <= (bounds.y + bounds.height + padding);
        }
    }

    let effective_notch_width = if settings.non_notch_mode {
        0.0
    } else {
        notch_width
    };
    let fallback_x_start = (screen_width - effective_notch_width) / 2.0;
    let fallback_x_end = fallback_x_start + effective_notch_width;
    let fallback_y_end = if settings.non_notch_mode {
        1.0
    } else {
        notch_height.max(38.0)
    };

    mouse_x >= (fallback_x_start - padding)
        && mouse_x <= (fallback_x_end + padding)
        && mouse_y >= -padding
        && mouse_y <= (fallback_y_end + padding)
}

/// Spawn a 50 Hz poller. Dropping the receiver stops the UI from seeing events;
/// the thread keeps running for process lifetime (cheap).
pub fn spawn_monitor() -> mpsc::Receiver<MouseEvent> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("nook-mouse".into())
        .spawn(move || loop {
            let (mx, my) = current_mouse_logical();
            let inside = hit_test(mx, my);
            let was = IS_INSIDE.load(Ordering::Relaxed);
            if inside && !was {
                IS_INSIDE.store(true, Ordering::Relaxed);
                if tx.send(MouseEvent::Entered).is_err() {
                    break;
                }
            } else if !inside && was {
                IS_INSIDE.store(false, Ordering::Relaxed);
                if tx.send(MouseEvent::Exited).is_err() {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        })
        .expect("mouse thread");
    rx
}
