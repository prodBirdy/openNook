//! System output volume and mute, observed through CoreAudio property listeners.
//!
//! Also owns the shared HUD event bus that [`crate::brightness`] publishes into.
//! Idle cost is zero: nothing runs until CoreAudio (or a brightness callback)
//! fires, and there is no sampling loop.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::watch;

/// How long the island keeps a volume/brightness HUD up after the last change.
pub const HUD_TTL: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudKind {
    Volume,
    Mute,
    Brightness,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudEvent {
    pub kind: HudKind,
    pub value: f32,
    pub seq: u64,
}

impl HudEvent {
    pub const INITIAL: Self = Self {
        kind: HudKind::Volume,
        value: 0.0,
        seq: 0,
    };

    pub fn is_initial(self) -> bool {
        self.seq == 0
    }

    /// Slider fill: mute is drawn empty even when the device still has a level.
    pub fn display_value(self) -> f32 {
        match self.kind {
            HudKind::Mute => 0.0,
            HudKind::Volume | HudKind::Brightness => clamp_unit(self.value),
        }
    }
}

/// Clamp a hardware reading or slider gesture into `0..=1`.
pub fn clamp_unit(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 1.0)
}

/// Mean of the channel scalars that actually exist on a device.
pub fn average_channel_scalars(channels: &[f32]) -> Option<f32> {
    let mut sum = 0.0f32;
    let mut n = 0u32;
    for value in channels {
        if value.is_finite() {
            sum += clamp_unit(*value);
            n += 1;
        }
    }
    (n > 0).then_some(sum / n as f32)
}

/// True after the CoreAudio listeners installed successfully.
pub fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

pub fn volume() -> Option<f32> {
    #[cfg(target_os = "macos")]
    {
        macos::read_volume()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn muted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::read_mute().unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn set_volume(value: f32) {
    let value = clamp_unit(value);
    #[cfg(target_os = "macos")]
    macos::write_volume(value);
    let _ = value;
}

pub fn set_muted(muted: bool) {
    #[cfg(target_os = "macos")]
    macos::write_mute(muted);
    let _ = muted;
}

/// Subscribe to volume, mute, and brightness HUD events.
pub fn subscribe() -> watch::Receiver<HudEvent> {
    bus().subscribe()
}

pub(crate) fn publish(kind: HudKind, value: f32) {
    let value = clamp_unit(value);
    let tx = bus();
    let current = *tx.borrow();
    if current.seq > 0 && current.kind == kind && (current.value - value).abs() < 0.002 {
        return;
    }
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = tx.send(HudEvent { kind, value, seq });
}

/// Install CoreAudio listeners. Safe to call more than once.
pub fn start() {
    #[cfg(target_os = "macos")]
    macos::start();
}

fn bus() -> &'static watch::Sender<HudEvent> {
    TX.get_or_init(|| {
        let (tx, _rx) = watch::channel(HudEvent::INITIAL);
        tx
    })
}

static TX: OnceLock<watch::Sender<HudEvent>> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);
static AVAILABLE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
mod macos {
    use super::{clamp_unit, publish, HudKind, AVAILABLE};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Once;

    type AudioObjectId = u32;
    type OsStatus = i32;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const SYSTEM_OBJECT: AudioObjectId = 1;
    const ELEMENT_MAIN: u32 = 0;
    const SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
    const SCOPE_OUTPUT: u32 = u32::from_be_bytes(*b"outp");
    const DEFAULT_OUTPUT: u32 = u32::from_be_bytes(*b"dOut");
    const VIRTUAL_MAIN_VOLUME: u32 = u32::from_be_bytes(*b"vmvc");
    const VOLUME_SCALAR: u32 = u32::from_be_bytes(*b"volm");
    const MUTE: u32 = u32::from_be_bytes(*b"mute");

    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectHasProperty(object: AudioObjectId, address: *const PropertyAddress) -> u8;
        fn AudioObjectGetPropertyData(
            object: AudioObjectId,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            data_size: *mut u32,
            data: *mut c_void,
        ) -> OsStatus;
        fn AudioObjectSetPropertyData(
            object: AudioObjectId,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            data_size: u32,
            data: *const c_void,
        ) -> OsStatus;
        fn AudioObjectAddPropertyListener(
            object: AudioObjectId,
            address: *const PropertyAddress,
            listener: unsafe extern "C" fn(
                AudioObjectId,
                u32,
                *const PropertyAddress,
                *mut c_void,
            ) -> OsStatus,
            client: *mut c_void,
        ) -> OsStatus;
    }

    static STARTED: Once = Once::new();
    static DEVICE: AtomicU32 = AtomicU32::new(0);
    static LAST_VOLUME: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    static LAST_MUTE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    fn addr(selector: u32, scope: u32, element: u32) -> PropertyAddress {
        PropertyAddress {
            selector,
            scope,
            element,
        }
    }

    fn has_property(object: AudioObjectId, address: PropertyAddress) -> bool {
        unsafe { AudioObjectHasProperty(object, &address) != 0 }
    }

    fn get_f32(object: AudioObjectId, address: PropertyAddress) -> Option<f32> {
        if !has_property(object, address) {
            return None;
        }
        let mut value = 0.0f32;
        let mut size = std::mem::size_of::<f32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut value as *mut f32 as *mut c_void,
            )
        };
        (status == 0 && value.is_finite()).then_some(value)
    }

    fn get_u32(object: AudioObjectId, address: PropertyAddress) -> Option<u32> {
        if !has_property(object, address) {
            return None;
        }
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut value as *mut u32 as *mut c_void,
            )
        };
        (status == 0).then_some(value)
    }

    fn set_f32(object: AudioObjectId, address: PropertyAddress, value: f32) -> bool {
        if !has_property(object, address) {
            return false;
        }
        let status = unsafe {
            AudioObjectSetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                std::mem::size_of::<f32>() as u32,
                &value as *const f32 as *const c_void,
            )
        };
        status == 0
    }

    fn set_u32(object: AudioObjectId, address: PropertyAddress, value: u32) -> bool {
        if !has_property(object, address) {
            return false;
        }
        let status = unsafe {
            AudioObjectSetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                std::mem::size_of::<u32>() as u32,
                &value as *const u32 as *const c_void,
            )
        };
        status == 0
    }

    fn default_output() -> Option<AudioObjectId> {
        let id = get_u32(
            SYSTEM_OBJECT,
            addr(DEFAULT_OUTPUT, SCOPE_GLOBAL, ELEMENT_MAIN),
        )?;
        (id != 0).then_some(id)
    }

    fn volume_on(device: AudioObjectId) -> Option<f32> {
        let virtual_main = addr(VIRTUAL_MAIN_VOLUME, SCOPE_OUTPUT, ELEMENT_MAIN);
        if let Some(value) = get_f32(device, virtual_main) {
            return Some(clamp_unit(value));
        }
        let master = addr(VOLUME_SCALAR, SCOPE_OUTPUT, ELEMENT_MAIN);
        if let Some(value) = get_f32(device, master) {
            return Some(clamp_unit(value));
        }
        let mut channels = Vec::new();
        for element in 1u32..=8 {
            if let Some(value) = get_f32(device, addr(VOLUME_SCALAR, SCOPE_OUTPUT, element)) {
                channels.push(value);
            }
        }
        super::average_channel_scalars(&channels)
    }

    fn mute_on(device: AudioObjectId) -> Option<bool> {
        get_u32(device, addr(MUTE, SCOPE_OUTPUT, ELEMENT_MAIN)).map(|v| v != 0)
    }

    pub(super) fn read_volume() -> Option<f32> {
        volume_on(default_output()?)
    }

    pub(super) fn read_mute() -> Option<bool> {
        mute_on(default_output()?)
    }

    pub(super) fn write_volume(value: f32) {
        let Some(device) = default_output() else {
            return;
        };
        let virtual_main = addr(VIRTUAL_MAIN_VOLUME, SCOPE_OUTPUT, ELEMENT_MAIN);
        if set_f32(device, virtual_main, value) {
            let _ = set_u32(device, addr(MUTE, SCOPE_OUTPUT, ELEMENT_MAIN), 0);
            return;
        }
        let master = addr(VOLUME_SCALAR, SCOPE_OUTPUT, ELEMENT_MAIN);
        let mut wrote = set_f32(device, master, value);
        for element in 1u32..=8 {
            wrote |= set_f32(device, addr(VOLUME_SCALAR, SCOPE_OUTPUT, element), value);
        }
        if wrote {
            let _ = set_u32(device, addr(MUTE, SCOPE_OUTPUT, ELEMENT_MAIN), 0);
        }
    }

    pub(super) fn write_mute(muted: bool) {
        let Some(device) = default_output() else {
            return;
        };
        let _ = set_u32(
            device,
            addr(MUTE, SCOPE_OUTPUT, ELEMENT_MAIN),
            u32::from(muted),
        );
    }

    /// CoreAudio HAL thread. `client` is a leaked `fn()` from [`listen`].
    unsafe extern "C" fn on_property(
        _object: AudioObjectId,
        _n: u32,
        _addrs: *const PropertyAddress,
        client: *mut c_void,
    ) -> OsStatus {
        if !client.is_null() {
            // SAFETY: `client` is `Box::leak` of a `fn()`; listeners are
            // registered once and never removed or sent across threads.
            // The enclosing `unsafe fn` body is already an unsafe block
            // (edition 2021).
            let callback = *(client as *const fn());
            callback();
        }
        0
    }

    fn listen(object: AudioObjectId, address: PropertyAddress, on_change: fn()) {
        // Process-lifetime: keep the callback pointer alive for CoreAudio.
        let client = Box::leak(Box::new(on_change)) as *mut fn() as *mut c_void;
        let status =
            unsafe { AudioObjectAddPropertyListener(object, &address, on_property, client) };
        if status != 0 {
            log::warn!("CoreAudio listener failed ({status:#x})");
        }
    }

    fn publish_device_state() {
        let Some(device) = default_output() else {
            return;
        };
        let muted = mute_on(device).unwrap_or(false);
        let volume = volume_on(device).unwrap_or(0.0);
        let bits = volume.to_bits();
        let mute_changed = LAST_MUTE.swap(muted, Ordering::Relaxed) != muted;
        let vol_changed = LAST_VOLUME.swap(bits, Ordering::Relaxed) != bits;
        if mute_changed && muted {
            publish(HudKind::Mute, volume);
        } else if mute_changed || vol_changed {
            publish(
                if muted {
                    HudKind::Mute
                } else {
                    HudKind::Volume
                },
                volume,
            );
        }
    }

    fn attach_device() {
        let Some(device) = default_output() else {
            AVAILABLE.store(false, Ordering::Relaxed);
            return;
        };
        let previous = DEVICE.swap(device, Ordering::Relaxed);
        if previous == device && AVAILABLE.load(Ordering::Relaxed) {
            return;
        }
        listen(
            device,
            addr(VIRTUAL_MAIN_VOLUME, SCOPE_OUTPUT, ELEMENT_MAIN),
            publish_device_state,
        );
        listen(
            device,
            addr(VOLUME_SCALAR, SCOPE_OUTPUT, ELEMENT_MAIN),
            publish_device_state,
        );
        for element in 1u32..=2 {
            listen(
                device,
                addr(VOLUME_SCALAR, SCOPE_OUTPUT, element),
                publish_device_state,
            );
        }
        listen(
            device,
            addr(MUTE, SCOPE_OUTPUT, ELEMENT_MAIN),
            publish_device_state,
        );
        AVAILABLE.store(true, Ordering::Relaxed);
        if let Some(volume) = volume_on(device) {
            LAST_VOLUME.store(volume.to_bits(), Ordering::Relaxed);
        }
        LAST_MUTE.store(mute_on(device).unwrap_or(false), Ordering::Relaxed);
    }

    fn on_default_device() {
        attach_device();
        publish_device_state();
    }

    pub(super) fn start() {
        STARTED.call_once(|| {
            let _ = super::bus();
            listen(
                SYSTEM_OBJECT,
                addr(DEFAULT_OUTPUT, SCOPE_GLOBAL, ELEMENT_MAIN),
                on_default_device,
            );
            attach_device();
            if AVAILABLE.load(Ordering::Relaxed) {
                log::info!("CoreAudio volume HUD listeners installed");
            } else {
                log::warn!("no default output device; volume HUD disabled");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_unit_rejects_nan_and_inf() {
        assert_eq!(clamp_unit(f32::NAN), 0.0);
        assert_eq!(clamp_unit(f32::INFINITY), 0.0);
        assert_eq!(clamp_unit(-2.0), 0.0);
        assert_eq!(clamp_unit(0.25), 0.25);
    }

    #[test]
    fn average_channel_scalars_skips_empty_and_non_finite() {
        assert_eq!(average_channel_scalars(&[]), None);
        assert_eq!(average_channel_scalars(&[f32::NAN]), None);
        assert_eq!(average_channel_scalars(&[0.2, 0.4]), Some(0.3));
        assert_eq!(average_channel_scalars(&[1.5, -1.0]), Some(0.5));
    }

    #[test]
    fn hud_event_display_value_empties_mute() {
        let mute = HudEvent {
            kind: HudKind::Mute,
            value: 0.8,
            seq: 1,
        };
        assert_eq!(mute.display_value(), 0.0);
        let volume = HudEvent {
            kind: HudKind::Volume,
            value: 1.4,
            seq: 2,
        };
        assert_eq!(volume.display_value(), 1.0);
        assert!(HudEvent::INITIAL.is_initial());
        assert!(!volume.is_initial());
    }

    #[test]
    fn publish_skips_duplicate_values_and_bumps_seq() {
        let mut rx = subscribe();
        publish(HudKind::Volume, 0.4);
        assert!(rx.has_changed().unwrap());
        let first = *rx.borrow_and_update();
        assert_eq!(first.kind, HudKind::Volume);
        assert!((first.value - 0.4).abs() < f32::EPSILON);
        assert!(first.seq >= 1);

        publish(HudKind::Volume, 0.4);
        assert!(!rx.has_changed().unwrap());

        publish(HudKind::Mute, 0.4);
        let second = *rx.borrow_and_update();
        assert_eq!(second.kind, HudKind::Mute);
        assert!(second.seq > first.seq);
    }

    #[test]
    fn hud_ttl_is_brief() {
        assert_eq!(HUD_TTL, Duration::from_millis(1500));
    }

    #[test]
    fn volume_setters_are_safe_off_macos() {
        set_volume(0.5);
        set_muted(true);
        #[cfg(not(target_os = "macos"))]
        {
            assert!(!available());
            assert_eq!(volume(), None);
            assert!(!muted());
        }
    }
}
