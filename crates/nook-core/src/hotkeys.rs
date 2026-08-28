//! Global snap hotkeys via Carbon `RegisterEventHotKey`.
//!
//! Callback-only: no poll loop, no TCC. The handler consumes the keystroke
//! and calls [`crate::window_snap::snap_frontmost`]. Registration is a
//! one-shot on settings change / launch.

use crate::window_snap::SnapKind;

/// Carbon `controlKey`.
pub const MOD_CONTROL: u32 = 1 << 12;
/// Carbon `optionKey`.
pub const MOD_OPTION: u32 = 1 << 11;
/// Carbon `cmdKey`.
pub const MOD_COMMAND: u32 = 1 << 8;
/// Carbon `shiftKey`.
pub const MOD_SHIFT: u32 = 1 << 9;

/// Virtual key codes (HIToolbox).
pub const KEY_LEFT: u16 = 123;
pub const KEY_RIGHT: u16 = 124;
pub const KEY_DOWN: u16 = 125;
pub const KEY_UP: u16 = 126;
pub const KEY_RETURN: u16 = 36;
pub const KEY_U: u16 = 32;
pub const KEY_I: u16 = 34;
pub const KEY_J: u16 = 38;
pub const KEY_K: u16 = 40;

#[cfg(target_os = "macos")]
const SIGNATURE: u32 = 0x4E4F4F4B; // 'NOOK'

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub key_code: u16,
    pub modifiers: u32,
}

impl Hotkey {
    pub const fn new(key_code: u16, modifiers: u32) -> Self {
        Self {
            key_code,
            modifiers,
        }
    }

    pub fn display(self) -> String {
        let mut parts = String::new();
        if self.modifiers & MOD_CONTROL != 0 {
            parts.push('⌃');
        }
        if self.modifiers & MOD_OPTION != 0 {
            parts.push('⌥');
        }
        if self.modifiers & MOD_SHIFT != 0 {
            parts.push('⇧');
        }
        if self.modifiers & MOD_COMMAND != 0 {
            parts.push('⌘');
        }
        parts.push_str(key_name(self.key_code));
        parts
    }
}

fn key_name(code: u16) -> &'static str {
    match code {
        KEY_LEFT => "←",
        KEY_RIGHT => "→",
        KEY_UP => "↑",
        KEY_DOWN => "↓",
        KEY_RETURN => "↩",
        KEY_U => "U",
        KEY_I => "I",
        KEY_J => "J",
        KEY_K => "K",
        _ => "?",
    }
}

/// Rectangle-style defaults: Control + Option + arrows / U I J K / Return.
pub fn default_bindings() -> [(SnapKind, Hotkey); 9] {
    let mods = MOD_CONTROL | MOD_OPTION;
    [
        (SnapKind::LeftHalf, Hotkey::new(KEY_LEFT, mods)),
        (SnapKind::RightHalf, Hotkey::new(KEY_RIGHT, mods)),
        (SnapKind::TopHalf, Hotkey::new(KEY_UP, mods)),
        (SnapKind::BottomHalf, Hotkey::new(KEY_DOWN, mods)),
        (SnapKind::TopLeft, Hotkey::new(KEY_U, mods)),
        (SnapKind::TopRight, Hotkey::new(KEY_I, mods)),
        (SnapKind::BottomLeft, Hotkey::new(KEY_J, mods)),
        (SnapKind::BottomRight, Hotkey::new(KEY_K, mods)),
        (SnapKind::Maximize, Hotkey::new(KEY_RETURN, mods)),
    ]
}

/// Register (or tear down) Carbon hotkeys from the current settings.
/// Safe to call more than once; must run on the main thread on macOS.
pub fn install() {
    sync();
}

pub fn sync() {
    #[cfg(target_os = "macos")]
    {
        sync_macos();
    }
}

#[cfg(target_os = "macos")]
fn sync_macos() {
    unregister_all();
    if !crate::settings::get_app_settings().window_snap_enabled {
        return;
    }
    ensure_handler();
    for (kind, hotkey) in default_bindings() {
        register(kind, hotkey);
    }
}

#[cfg(target_os = "macos")]
fn register(kind: SnapKind, hotkey: Hotkey) {
    let target = unsafe { GetEventDispatcherTarget() };
    if target.is_null() {
        log::warn!("Carbon event target missing; snap hotkeys not registered");
        return;
    }
    let id = EventHotKeyID {
        signature: SIGNATURE,
        id: kind as u32 + 1,
    };
    let mut href: EventHotKeyRef = std::ptr::null_mut();
    let status = unsafe {
        RegisterEventHotKey(
            u32::from(hotkey.key_code),
            hotkey.modifiers,
            id,
            target,
            0,
            &mut href,
        )
    };
    if status != 0 || href.is_null() {
        log::warn!(
            "RegisterEventHotKey {} failed ({status})",
            kind.label()
        );
        return;
    }
    HOTKEYS.lock().unwrap_or_else(|e| e.into_inner()).push(href);
}

#[cfg(target_os = "macos")]
fn unregister_all() {
    let mut refs = HOTKEYS.lock().unwrap_or_else(|e| e.into_inner());
    for href in refs.drain(..) {
        if !href.is_null() {
            unsafe {
                let _ = UnregisterEventHotKey(href);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn ensure_handler() {
    HANDLER.call_once(|| {
        let target = unsafe { GetEventDispatcherTarget() };
        if target.is_null() {
            log::error!("Carbon event target missing; cannot install snap handler");
            return;
        }
        let spec = EventTypeSpec {
            event_class: EVENT_CLASS_KEYBOARD,
            event_kind: EVENT_HOT_KEY_PRESSED,
        };
        let mut href: *mut std::ffi::c_void = std::ptr::null_mut();
        let status = unsafe {
            InstallEventHandler(
                target,
                hotkey_handler,
                1,
                &spec,
                std::ptr::null_mut(),
                &mut href,
            )
        };
        if status != 0 {
            log::error!("InstallEventHandler for snap hotkeys failed ({status})");
        }
    });
}

#[cfg(target_os = "macos")]
extern "C" fn hotkey_handler(
    _next: *mut std::ffi::c_void,
    event: *mut std::ffi::c_void,
    _user: *mut std::ffi::c_void,
) -> i32 {
    let mut id = EventHotKeyID {
        signature: 0,
        id: 0,
    };
    let status = unsafe {
        GetEventParameter(
            event,
            EVENT_PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOT_KEY_ID,
            std::ptr::null_mut(),
            std::mem::size_of::<EventHotKeyID>() as u32,
            std::ptr::null_mut(),
            &mut id as *mut EventHotKeyID as *mut std::ffi::c_void,
        )
    };
    if status == 0 && id.signature == SIGNATURE {
        if let Some(kind) = SnapKind::from_u8(id.id.saturating_sub(1) as u8) {
            if let Err(err) = crate::window_snap::snap_frontmost(kind) {
                log::debug!("snap {} skipped: {err:?}", kind.label());
            }
        }
    }
    0
}

#[cfg(target_os = "macos")]
const EVENT_CLASS_KEYBOARD: u32 = 0x6B65_7962; // 'keyb'
#[cfg(target_os = "macos")]
const EVENT_HOT_KEY_PRESSED: u32 = 5;
#[cfg(target_os = "macos")]
const EVENT_PARAM_DIRECT_OBJECT: u32 = 0x2D2D_2D2D; // '----'
#[cfg(target_os = "macos")]
const TYPE_EVENT_HOT_KEY_ID: u32 = 0x686B_6964; // 'hkid'

#[cfg(target_os = "macos")]
type EventHotKeyRef = *mut std::ffi::c_void;

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[cfg(target_os = "macos")]
#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetEventDispatcherTarget() -> *mut std::ffi::c_void;
    fn InstallEventHandler(
        target: *mut std::ffi::c_void,
        handler: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void) -> i32,
        num_types: u32,
        list: *const EventTypeSpec,
        user_data: *mut std::ffi::c_void,
        out_ref: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        hot_key_id: EventHotKeyID,
        target: *mut std::ffi::c_void,
        options: u32,
        out_ref: *mut EventHotKeyRef,
    ) -> i32;
    fn UnregisterEventHotKey(hot_key: EventHotKeyRef) -> i32;
    fn GetEventParameter(
        event: *mut std::ffi::c_void,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        buffer_size: u32,
        actual_size: *mut u32,
        buffer: *mut std::ffi::c_void,
    ) -> i32;
}

#[cfg(target_os = "macos")]
static HANDLER: std::sync::Once = std::sync::Once::new();
#[cfg(target_os = "macos")]
static HOTKEYS: std::sync::Mutex<Vec<EventHotKeyRef>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn default_bindings_are_unique() {
        let bindings = default_bindings();
        assert_eq!(bindings.len(), SnapKind::ALL.len());
        let mut keys = HashSet::new();
        let mut kinds = HashSet::new();
        for (kind, hotkey) in bindings {
            assert!(kinds.insert(kind), "duplicate snap kind {kind:?}");
            assert!(
                keys.insert((hotkey.key_code, hotkey.modifiers)),
                "duplicate hotkey {:?}",
                hotkey.display()
            );
            assert!(hotkey.modifiers & MOD_CONTROL != 0);
            assert!(hotkey.modifiers & MOD_OPTION != 0);
        }
    }

    #[test]
    fn display_uses_mac_glyphs() {
        let left = Hotkey::new(KEY_LEFT, MOD_CONTROL | MOD_OPTION);
        assert_eq!(left.display(), "⌃⌥←");
        let max = Hotkey::new(KEY_RETURN, MOD_CONTROL | MOD_OPTION);
        assert_eq!(max.display(), "⌃⌥↩");
        let corner = Hotkey::new(KEY_U, MOD_CONTROL | MOD_OPTION);
        assert_eq!(corner.display(), "⌃⌥U");
    }
}
