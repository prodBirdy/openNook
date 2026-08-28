//! System audio output devices via the CoreAudio HAL.
//!
//! Phase 1 lists and switches the **default output** among devices that already
//! exist as CoreAudio objects: built-in speakers, USB/DisplayPort, connected
//! Bluetooth (AirPods), and an AirPlay route macOS has already made. Starting a
//! system-wide route to a not-yet-connected HomePod or Apple TV is not possible
//! here — those targets never appear in the HAL until the route is active, and
//! MediaRemote routing has been entitlement-blocked since macOS 15.4.
//!
//! Idle cost is zero: `AudioObjectAddPropertyListenerBlock` writes a snapshot
//! and flips a dirty flag. The island tick consumes the flag; there is no
//! sampling loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Caption shown under the picker so the AirPlay limit is not a surprise.
pub const AIRPLAY_INITIATE_NOTE: &str = "HomePod and Apple TV appear only after macOS routes to them. Use Control Center to start AirPlay.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputTransport {
    BuiltIn,
    Bluetooth,
    Usb,
    AirPlay,
    DisplayPort,
    Hdmi,
    Thunderbolt,
    Virtual,
    Unknown,
}

impl OutputTransport {
    /// HAL `kAudioDevicePropertyTransportType` FourCC.
    pub fn from_code(code: u32) -> Self {
        match &code.to_be_bytes() {
            b"bltn" => Self::BuiltIn,
            b"blue" | b"blea" => Self::Bluetooth,
            b"usb " => Self::Usb,
            b"airp" => Self::AirPlay,
            b"dprt" => Self::DisplayPort,
            b"hdmi" => Self::Hdmi,
            b"thun" => Self::Thunderbolt,
            b"virt" | b"grup" => Self::Virtual,
            _ => Self::Unknown,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::AirPlay => "airplay",
            Self::Bluetooth => "bluetooth",
            Self::BuiltIn => "speaker",
            Self::Usb | Self::Thunderbolt => "headphones",
            Self::DisplayPort | Self::Hdmi => "monitor",
            Self::Virtual | Self::Unknown => "volume-2",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "Built-in",
            Self::Bluetooth => "Bluetooth",
            Self::Usb => "USB",
            Self::AirPlay => "AirPlay",
            Self::DisplayPort => "DisplayPort",
            Self::Hdmi => "HDMI",
            Self::Thunderbolt => "Thunderbolt",
            Self::Virtual => "Virtual",
            Self::Unknown => "Output",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDevice {
    pub id: u32,
    pub name: String,
    pub transport: OutputTransport,
    pub is_default: bool,
}

impl OutputDevice {
    pub fn icon(&self) -> &'static str {
        self.transport.icon()
    }
}

/// True after CoreAudio listeners installed successfully.
pub fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

/// Last enumerated output devices. Empty off macOS or before [`start`].
pub fn snapshot() -> Vec<OutputDevice> {
    store().map(|m| lock_mutex(m).clone()).unwrap_or_default()
}

/// Current default output, if the snapshot has one.
pub fn default_output() -> Option<OutputDevice> {
    snapshot().into_iter().find(|d| d.is_default)
}

/// True if a listener rebuilt the snapshot since the last take.
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::Relaxed)
}

/// Re-enumerate now (startup, listener, or the user opening the picker).
pub fn refresh() {
    #[cfg(target_os = "macos")]
    macos::refresh();
}

/// Make `id` the default output (and the default system/alert output).
pub fn set_default_output(id: u32) -> Result<(), String> {
    if id == 0 {
        return Err("invalid output device".into());
    }
    #[cfg(target_os = "macos")]
    {
        return macos::set_default(id);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = id;
        Err("audio output switching is only available on macOS".into())
    }
}

/// Install HAL listeners. Safe to call more than once.
pub fn start() {
    #[cfg(target_os = "macos")]
    macos::start();
}

/// A device is an output if its output-scope stream config has any channels.
pub fn is_output_capable(channels: u32) -> bool {
    channels > 0
}

/// Parse `kAudioDevicePropertyStreamConfiguration` bytes into a channel count.
///
/// `AudioBufferList` is `UInt32 mNumberBuffers` plus pointer-aligned
/// `AudioBuffer` entries (`mNumberChannels`, `mDataByteSize`, `mData`).
pub fn output_channels_from_stream_config(bytes: &[u8]) -> u32 {
    if bytes.len() < 4 {
        return 0;
    }
    let n_buffers = u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let first = stream_config_first_buffer_offset();
    let stride = audio_buffer_size();
    let mut channels = 0u32;
    for i in 0..n_buffers {
        let off = first + i * stride;
        if off + 4 > bytes.len() {
            break;
        }
        channels = channels.saturating_add(u32::from_ne_bytes([
            bytes[off],
            bytes[off + 1],
            bytes[off + 2],
            bytes[off + 3],
        ]));
    }
    channels
}

/// Mark the matching id as default; everyone else is not.
pub fn mark_default(devices: &mut [OutputDevice], default_id: u32) {
    for device in devices {
        device.is_default = device.id == default_id && default_id != 0;
    }
}

fn stream_config_first_buffer_offset() -> usize {
    std::mem::size_of::<u32>().next_multiple_of(std::mem::size_of::<usize>())
}

fn audio_buffer_size() -> usize {
    // Two UInt32s + a pointer; matches CoreAudio's AudioBuffer on all LP64/ILP32.
    8 + std::mem::size_of::<*mut ()>()
}

fn store() -> Option<&'static Mutex<Vec<OutputDevice>>> {
    DEVICES.get()
}

fn lock_mutex<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(target_os = "macos")]
fn publish(devices: Vec<OutputDevice>) {
    let slot = DEVICES.get_or_init(|| Mutex::new(Vec::new()));
    *lock_mutex(slot) = devices;
    DIRTY.store(true, Ordering::Relaxed);
}

static DEVICES: OnceLock<Mutex<Vec<OutputDevice>>> = OnceLock::new();
static DIRTY: AtomicBool = AtomicBool::new(false);
static AVAILABLE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
mod macos {
    use super::{
        is_output_capable, mark_default, output_channels_from_stream_config, publish, OutputDevice,
        OutputTransport, AVAILABLE,
    };
    use block2::{Block, RcBlock};
    use std::ffi::c_void;
    use std::sync::{Mutex, Once};

    type AudioObjectId = u32;
    type OsStatus = i32;
    type CfIndex = isize;

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
    const DEVICES: u32 = u32::from_be_bytes(*b"dev#");
    const DEFAULT_OUTPUT: u32 = u32::from_be_bytes(*b"dOut");
    const DEFAULT_SYSTEM_OUTPUT: u32 = u32::from_be_bytes(*b"sOut");
    const OBJECT_NAME: u32 = u32::from_be_bytes(*b"lnam");
    const TRANSPORT: u32 = u32::from_be_bytes(*b"tran");
    const STREAM_CONFIG: u32 = u32::from_be_bytes(*b"slay");
    const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectHasProperty(object: AudioObjectId, address: *const PropertyAddress) -> u8;
        fn AudioObjectGetPropertyDataSize(
            object: AudioObjectId,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            data_size: *mut u32,
        ) -> OsStatus;
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
        fn AudioObjectAddPropertyListenerBlock(
            object: AudioObjectId,
            address: *const PropertyAddress,
            queue: *mut c_void,
            listener: *mut Block<dyn Fn(u32, *const PropertyAddress)>,
        ) -> OsStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFStringGetLength(s: *const c_void) -> CfIndex;
        fn CFStringGetMaximumSizeForEncoding(len: CfIndex, encoding: u32) -> CfIndex;
        fn CFStringGetCString(s: *const c_void, buf: *mut i8, size: CfIndex, encoding: u32) -> u8;
    }

    static STARTED: Once = Once::new();
    static BLOCKS: Mutex<Vec<RcBlock<dyn Fn(u32, *const PropertyAddress)>>> =
        Mutex::new(Vec::new());

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

    fn set_u32(object: AudioObjectId, address: PropertyAddress, value: u32) -> OsStatus {
        unsafe {
            AudioObjectSetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                std::mem::size_of::<u32>() as u32,
                &value as *const u32 as *const c_void,
            )
        }
    }

    fn property_bytes(object: AudioObjectId, address: PropertyAddress) -> Option<Vec<u8>> {
        if !has_property(object, address) {
            return None;
        }
        let mut size = 0u32;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(object, &address, 0, std::ptr::null(), &mut size)
        };
        if status != 0 || size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                buf.as_mut_ptr() as *mut c_void,
            )
        };
        (status == 0).then_some(buf)
    }

    fn cfstring_to_string(cf: *const c_void) -> Option<String> {
        if cf.is_null() {
            return None;
        }
        let len = unsafe { CFStringGetLength(cf) };
        if len < 0 {
            return None;
        }
        let max = unsafe { CFStringGetMaximumSizeForEncoding(len, CF_STRING_ENCODING_UTF8) };
        if max < 0 {
            return None;
        }
        let mut buf = vec![0i8; (max as usize).saturating_add(1)];
        let ok = unsafe {
            CFStringGetCString(
                cf,
                buf.as_mut_ptr(),
                buf.len() as CfIndex,
                CF_STRING_ENCODING_UTF8,
            )
        };
        if ok == 0 {
            return None;
        }
        let bytes = buf
            .iter()
            .map(|b| *b as u8)
            .take_while(|b| *b != 0)
            .collect();
        String::from_utf8(bytes).ok()
    }

    fn device_name(id: AudioObjectId) -> String {
        let address = addr(OBJECT_NAME, SCOPE_GLOBAL, ELEMENT_MAIN);
        if !has_property(id, address) {
            return format!("Output {id}");
        }
        let mut cf: *const c_void = std::ptr::null();
        let mut size = std::mem::size_of::<*const c_void>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                id,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut cf as *mut *const c_void as *mut c_void,
            )
        };
        if status != 0 || cf.is_null() {
            return format!("Output {id}");
        }
        let name = cfstring_to_string(cf).unwrap_or_else(|| format!("Output {id}"));
        unsafe { CFRelease(cf) };
        if name.trim().is_empty() {
            format!("Output {id}")
        } else {
            name
        }
    }

    fn output_channels(id: AudioObjectId) -> u32 {
        let bytes = property_bytes(id, addr(STREAM_CONFIG, SCOPE_OUTPUT, ELEMENT_MAIN));
        bytes
            .map(|b| output_channels_from_stream_config(&b))
            .unwrap_or(0)
    }

    fn all_device_ids() -> Vec<AudioObjectId> {
        let address = addr(DEVICES, SCOPE_GLOBAL, ELEMENT_MAIN);
        let mut size = 0u32;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &address, 0, std::ptr::null(), &mut size)
        };
        if status != 0 || size == 0 || size as usize % std::mem::size_of::<AudioObjectId>() != 0 {
            return Vec::new();
        }
        let count = size as usize / std::mem::size_of::<AudioObjectId>();
        let mut ids = vec![0u32; count];
        let status = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                ids.as_mut_ptr() as *mut c_void,
            )
        };
        if status != 0 {
            return Vec::new();
        }
        ids.retain(|&id| id != 0);
        ids
    }

    fn default_output_id() -> u32 {
        get_u32(
            SYSTEM_OBJECT,
            addr(DEFAULT_OUTPUT, SCOPE_GLOBAL, ELEMENT_MAIN),
        )
        .unwrap_or(0)
    }

    pub(super) fn refresh() {
        let default_id = default_output_id();
        let mut devices: Vec<OutputDevice> = all_device_ids()
            .into_iter()
            .filter(|&id| is_output_capable(output_channels(id)))
            .map(|id| {
                let transport = get_u32(id, addr(TRANSPORT, SCOPE_GLOBAL, ELEMENT_MAIN))
                    .map(OutputTransport::from_code)
                    .unwrap_or(OutputTransport::Unknown);
                OutputDevice {
                    id,
                    name: device_name(id),
                    transport,
                    is_default: false,
                }
            })
            .collect();
        mark_default(&mut devices, default_id);
        publish(devices);
    }

    pub(super) fn set_default(id: AudioObjectId) -> Result<(), String> {
        if !is_output_capable(output_channels(id)) {
            return Err("device has no output channels".into());
        }
        let status = set_u32(
            SYSTEM_OBJECT,
            addr(DEFAULT_OUTPUT, SCOPE_GLOBAL, ELEMENT_MAIN),
            id,
        );
        if status != 0 {
            return Err(format!("AudioObjectSetPropertyData failed ({status:#x})"));
        }
        let _ = set_u32(
            SYSTEM_OBJECT,
            addr(DEFAULT_SYSTEM_OUTPUT, SCOPE_GLOBAL, ELEMENT_MAIN),
            id,
        );
        refresh();
        Ok(())
    }

    fn listen(object: AudioObjectId, address: PropertyAddress, on_change: fn()) {
        let block = RcBlock::new(move |_n: u32, _addrs: *const PropertyAddress| {
            on_change();
        });
        let status = unsafe {
            let block_ref = &*block;
            let block_ptr = block_ref as *const Block<dyn Fn(u32, *const PropertyAddress)>
                as *mut Block<dyn Fn(u32, *const PropertyAddress)>;
            AudioObjectAddPropertyListenerBlock(object, &address, std::ptr::null_mut(), block_ptr)
        };
        if status == 0 {
            if let Ok(mut guard) = BLOCKS.lock() {
                guard.push(block);
            } else {
                std::mem::forget(block);
            }
        } else {
            log::warn!("CoreAudio output-device listener failed ({status:#x})");
        }
    }

    pub(super) fn start() {
        STARTED.call_once(|| {
            listen(
                SYSTEM_OBJECT,
                addr(DEVICES, SCOPE_GLOBAL, ELEMENT_MAIN),
                refresh,
            );
            listen(
                SYSTEM_OBJECT,
                addr(DEFAULT_OUTPUT, SCOPE_GLOBAL, ELEMENT_MAIN),
                refresh,
            );
            refresh();
            AVAILABLE.store(true, Ordering::Relaxed);
            log::info!("CoreAudio output-device listeners installed");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fourcc(code: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*code)
    }

    fn pack_stream_config(channels: &[u32]) -> Vec<u8> {
        let first = stream_config_first_buffer_offset();
        let stride = audio_buffer_size();
        let mut bytes = vec![0u8; first + channels.len() * stride];
        let n = channels.len() as u32;
        bytes[0..4].copy_from_slice(&n.to_ne_bytes());
        for (i, ch) in channels.iter().enumerate() {
            let off = first + i * stride;
            bytes[off..off + 4].copy_from_slice(&ch.to_ne_bytes());
        }
        bytes
    }

    #[test]
    fn transport_from_code_maps_fourcc() {
        assert_eq!(
            OutputTransport::from_code(fourcc(b"bltn")),
            OutputTransport::BuiltIn
        );
        assert_eq!(
            OutputTransport::from_code(fourcc(b"blue")),
            OutputTransport::Bluetooth
        );
        assert_eq!(
            OutputTransport::from_code(fourcc(b"blea")),
            OutputTransport::Bluetooth
        );
        assert_eq!(
            OutputTransport::from_code(fourcc(b"usb ")),
            OutputTransport::Usb
        );
        assert_eq!(
            OutputTransport::from_code(fourcc(b"airp")),
            OutputTransport::AirPlay
        );
        assert_eq!(
            OutputTransport::from_code(fourcc(b"dprt")),
            OutputTransport::DisplayPort
        );
        assert_eq!(
            OutputTransport::from_code(fourcc(b"hdmi")),
            OutputTransport::Hdmi
        );
        assert_eq!(
            OutputTransport::from_code(fourcc(b"thun")),
            OutputTransport::Thunderbolt
        );
        assert_eq!(
            OutputTransport::from_code(fourcc(b"virt")),
            OutputTransport::Virtual
        );
        assert_eq!(
            OutputTransport::from_code(fourcc(b"grup")),
            OutputTransport::Virtual
        );
        assert_eq!(OutputTransport::from_code(0), OutputTransport::Unknown);
        assert_eq!(
            OutputTransport::from_code(fourcc(b"????")),
            OutputTransport::Unknown
        );
    }

    #[test]
    fn transport_icons_and_labels_are_stable() {
        assert_eq!(OutputTransport::AirPlay.icon(), "airplay");
        assert_eq!(OutputTransport::Bluetooth.icon(), "bluetooth");
        assert_eq!(OutputTransport::BuiltIn.icon(), "speaker");
        assert_eq!(OutputTransport::AirPlay.label(), "AirPlay");
        assert_eq!(OutputTransport::Unknown.label(), "Output");
    }

    #[test]
    fn output_channels_from_stream_config_sums_buffers() {
        assert_eq!(output_channels_from_stream_config(&[]), 0);
        assert_eq!(output_channels_from_stream_config(&[0, 0]), 0);
        assert_eq!(
            output_channels_from_stream_config(&pack_stream_config(&[2])),
            2
        );
        assert_eq!(
            output_channels_from_stream_config(&pack_stream_config(&[2, 2])),
            4
        );
        assert_eq!(
            output_channels_from_stream_config(&pack_stream_config(&[0, 0])),
            0
        );
    }

    #[test]
    fn is_output_capable_needs_a_channel() {
        assert!(!is_output_capable(0));
        assert!(is_output_capable(1));
        assert!(is_output_capable(2));
    }

    #[test]
    fn mark_default_flags_only_the_matching_id() {
        let mut devices = vec![
            OutputDevice {
                id: 10,
                name: "MacBook Speakers".into(),
                transport: OutputTransport::BuiltIn,
                is_default: true,
            },
            OutputDevice {
                id: 20,
                name: "AirPods Pro".into(),
                transport: OutputTransport::Bluetooth,
                is_default: true,
            },
        ];
        mark_default(&mut devices, 20);
        assert!(!devices[0].is_default);
        assert!(devices[1].is_default);
        mark_default(&mut devices, 0);
        assert!(devices.iter().all(|d| !d.is_default));
    }

    #[test]
    fn airplay_note_is_honest_about_initiate_limits() {
        assert!(AIRPLAY_INITIATE_NOTE.contains("Control Center"));
        assert!(AIRPLAY_INITIATE_NOTE.contains("HomePod"));
    }

    #[test]
    fn setters_are_safe_off_macos() {
        assert!(set_default_output(0).is_err());
        #[cfg(not(target_os = "macos"))]
        {
            assert!(!available());
            assert!(snapshot().is_empty());
            assert!(set_default_output(42).is_err());
            assert!(default_output().is_none());
            assert!(!take_dirty());
        }
    }
}
