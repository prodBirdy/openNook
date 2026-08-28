//! Per-app volume mixer.
//!
//! Listing apps that are producing sound is prompt-free. Changing a slider
//! creates a Core Audio process tap (macOS 14.4+), which is a capture object
//! and triggers the system-audio-recording consent prompt. With every gain at
//! unity there are no taps, no aggregate device, and no IOProc.
//!
//! The tap path uses public HAL + `CATapDescription` APIs from objc2-core-audio
//! 0.3.2 (objc2 0.6). Nothing private is required. Helper grouping optionally
//! calls `responsibility_get_pid_responsible_for_pid` via `dlsym` and falls
//! back to bundle-id heuristics when that symbol is absent.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Explanatory copy shown before the first slider move (and in Settings).
pub const TCC_PREPROMPT: &str = "Per-app volume uses macOS system-audio recording. openNook does not save or send your audio — it only scales it in real time so each app can have its own level. macOS will ask for permission the first time you move a slider, and Control Center shows a recording indicator while any slider is below 100%.";

pub const UNITY: f32 = 1.0;
pub const GAIN_MIN: f32 = 0.0;
pub const GAIN_MAX: f32 = 1.5;
const UNITY_EPS: f32 = 0.001;
const MUTE_EPS: f32 = 0.001;

const GAINS_KEY: &str = "mixer_gains";
const ACK_KEY: &str = "mixer_capture_ack";

/// One row in the mixer card: a playing app (helpers already grouped).
#[derive(Clone, Debug, PartialEq)]
pub struct MixerApp {
    pub bundle_id: String,
    pub name: String,
    pub pids: Vec<i32>,
    pub object_ids: Vec<u32>,
    pub gain: f32,
    pub muted: bool,
    pub level: f32,
}

/// Privacy / pipeline state shown in Settings and the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureStatus {
    Unavailable,
    Idle,
    NeedsConsent,
    Denied,
    Active,
}

/// Persisted / in-memory gain table keyed by canonical bundle id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GainMap {
    gains: BTreeMap<String, f32>,
    restore: BTreeMap<String, f32>,
}

impl GainMap {
    pub fn get(&self, id: &str) -> f32 {
        self.gains
            .get(id)
            .copied()
            .filter(|g| g.is_finite())
            .unwrap_or(UNITY)
    }

    pub fn set(&mut self, id: &str, gain: f32) {
        let id = id.trim();
        if id.is_empty() {
            return;
        }
        let gain = sanitize_gain(gain);
        if is_unity(gain) {
            self.gains.remove(id);
            self.restore.remove(id);
            return;
        }
        if gain > MUTE_EPS {
            self.restore.insert(id.to_string(), gain);
        }
        self.gains.insert(id.to_string(), gain);
    }

    pub fn is_muted(&self, id: &str) -> bool {
        self.get(id) <= MUTE_EPS
    }

    pub fn toggle_mute(&mut self, id: &str) {
        if self.is_muted(id) {
            let restore = self.restore.get(id).copied().unwrap_or(UNITY);
            self.set(id, if restore <= MUTE_EPS { UNITY } else { restore });
        } else {
            let current = self.get(id);
            if current > MUTE_EPS {
                self.restore.insert(id.to_string(), current);
            }
            self.gains.insert(id.to_string(), 0.0);
        }
    }

    pub fn reset(&mut self, id: &str) {
        self.gains.remove(id);
        self.restore.remove(id);
    }

    pub fn reset_all(&mut self) {
        self.gains.clear();
        self.restore.clear();
    }

    pub fn has_active(&self) -> bool {
        self.gains.values().any(|g| !is_unity(*g))
    }

    pub fn active_ids(&self) -> Vec<String> {
        self.gains
            .iter()
            .filter(|(_, g)| !is_unity(**g))
            .map(|(k, _)| k.clone())
            .collect()
    }

    pub fn to_persist(&self) -> BTreeMap<String, f32> {
        self.gains
            .iter()
            .filter(|(_, g)| !is_unity(**g))
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    pub fn from_persist(map: BTreeMap<String, f32>) -> Self {
        let mut this = Self::default();
        for (key, gain) in map {
            this.set(&key, gain);
        }
        this
    }
}

/// A Core Audio process object as enumerated from the HAL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawProcess {
    pub pid: i32,
    pub object_id: u32,
    pub bundle_id: String,
    pub name: String,
    pub running_output: bool,
    pub responsible_bundle: Option<String>,
}

pub fn is_unity(gain: f32) -> bool {
    (gain - UNITY).abs() < UNITY_EPS
}

pub fn sanitize_gain(gain: f32) -> f32 {
    if gain.is_finite() {
        gain.clamp(GAIN_MIN, GAIN_MAX)
    } else {
        UNITY
    }
}

/// Realtime-safe sample scale. Boosts (`gain > 1`) go through tanh.
pub fn scale_sample(sample: f32, gain: f32) -> f32 {
    if !sample.is_finite() {
        return 0.0;
    }
    let g = sanitize_gain(gain);
    let x = sample * g;
    if g <= UNITY {
        x.clamp(-1.0, 1.0)
    } else {
        x.tanh()
    }
}

const HELPER_TOKENS: &[&str] = &[
    "helper",
    "gpu",
    "webcontent",
    "networking",
    "renderer",
    "plugin",
    "gpuhelper",
    "rendererhelper",
    "pluginhelper",
    "broker",
];

/// Last bundle-id component looks like a helper / GPU / renderer process.
pub fn is_helper_bundle(bundle_id: &str) -> bool {
    let last = bundle_id.rsplit('.').next().unwrap_or("");
    let lower = last.to_ascii_lowercase();
    HELPER_TOKENS.contains(&lower.as_str())
        || lower.contains("helper")
        || bundle_id.contains(".helper")
        || bundle_id.contains(".Helper")
}

fn strip_helper_suffix(bundle_id: &str) -> String {
    let mut parts: Vec<&str> = bundle_id.split('.').collect();
    while parts.len() > 2 {
        let last = parts.last().copied().unwrap_or("").to_ascii_lowercase();
        if HELPER_TOKENS.contains(&last.as_str()) || last.contains("helper") {
            parts.pop();
        } else {
            break;
        }
    }
    parts.join(".")
}

/// Map a process bundle id onto the app the user thinks is playing.
pub fn canonical_bundle_id(bundle_id: &str) -> String {
    match bundle_id {
        "com.apple.WebKit.GPU"
        | "com.apple.WebKit.WebContent"
        | "com.apple.WebKit.Networking"
        | "com.apple.WebKit.GPU.Development"
        | "com.apple.WebKit.WebContent.Development" => "com.apple.Safari".into(),
        other => strip_helper_suffix(other),
    }
}

/// Group a helper under its responsible parent when we know one.
pub fn group_key(bundle_id: &str, responsible_bundle: Option<&str>) -> String {
    if let Some(resp) = responsible_bundle {
        if is_helper_bundle(bundle_id) && !is_helper_bundle(resp) {
            return canonical_bundle_id(resp);
        }
    }
    canonical_bundle_id(bundle_id)
}

fn title_case_token(token: &str) -> String {
    let mut chars = token.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn humanize_bundle(bundle_id: &str) -> String {
    let mut parts: Vec<&str> = bundle_id.split('.').collect();
    while let Some(last) = parts.last().copied() {
        if last.eq_ignore_ascii_case("client")
            || last.eq_ignore_ascii_case("mac")
            || last.eq_ignore_ascii_case("macos")
            || last.eq_ignore_ascii_case("desktop")
        {
            parts.pop();
        } else {
            break;
        }
    }
    title_case_token(parts.last().copied().unwrap_or(bundle_id))
}

pub fn display_name_for_bundle(bundle_id: &str) -> String {
    match bundle_id {
        "com.apple.Safari" => "Safari".into(),
        "com.apple.Music" => "Music".into(),
        "com.apple.TV" => "TV".into(),
        "com.apple.quicktimeplayer" => "QuickTime".into(),
        "com.apple.FaceTime" => "FaceTime".into(),
        "com.spotify.client" => "Spotify".into(),
        "com.google.Chrome" => "Chrome".into(),
        "com.google.Chrome.canary" => "Chrome Canary".into(),
        "com.microsoft.edgemac" => "Edge".into(),
        "org.mozilla.firefox" => "Firefox".into(),
        "org.videolan.vlc" => "VLC".into(),
        "com.hnc.Discord" => "Discord".into(),
        "com.tinyspeck.slackmacgap" => "Slack".into(),
        "us.zoom.xos" => "Zoom".into(),
        other => humanize_bundle(other),
    }
}

/// Collapse helper processes that share a canonical bundle into one row.
pub fn group_playing_apps(procs: &[RawProcess], gains: &GainMap) -> Vec<MixerApp> {
    let mut groups: BTreeMap<String, MixerApp> = BTreeMap::new();
    for proc in procs.iter().filter(|proc| proc.running_output) {
        let key = group_key(&proc.bundle_id, proc.responsible_bundle.as_deref());
        if key.is_empty() {
            continue;
        }
        let entry = groups.entry(key.clone()).or_insert_with(|| MixerApp {
            name: if !proc.name.is_empty() && !is_helper_bundle(&proc.bundle_id) {
                proc.name.clone()
            } else {
                display_name_for_bundle(&key)
            },
            bundle_id: key.clone(),
            pids: Vec::new(),
            object_ids: Vec::new(),
            gain: gains.get(&key),
            muted: gains.is_muted(&key),
            level: 0.0,
        });
        if !entry.pids.contains(&proc.pid) {
            entry.pids.push(proc.pid);
        }
        if proc.object_id != 0 && !entry.object_ids.contains(&proc.object_id) {
            entry.object_ids.push(proc.object_id);
        }
        if !proc.name.is_empty() && !is_helper_bundle(&proc.bundle_id) {
            entry.name = proc.name.clone();
        }
        entry.gain = gains.get(&key);
        entry.muted = gains.is_muted(&key);
    }
    let mut apps: Vec<_> = groups.into_values().collect();
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.bundle_id.cmp(&b.bundle_id))
    });
    apps
}

pub fn version_at_least(
    major: i64,
    minor: i64,
    patch: i64,
    need_maj: i64,
    need_min: i64,
    need_pat: i64,
) -> bool {
    (major, minor, patch) >= (need_maj, need_min, need_pat)
}

pub fn capture_status_label(status: CaptureStatus) -> &'static str {
    match status {
        CaptureStatus::Unavailable => "Requires macOS 14.4 or later",
        CaptureStatus::Idle => "Idle — no recording while every slider is at 100%",
        CaptureStatus::NeedsConsent => "Permission asked on the first slider move",
        CaptureStatus::Denied => {
            "Denied — enable Screen & System Audio Recording in Privacy & Security"
        }
        CaptureStatus::Active => "Recording indicator on — a slider is below 100%",
    }
}

struct MixerState {
    gains: GainMap,
    apps: Vec<MixerApp>,
    ack: bool,
    denied: bool,
    card_visible: bool,
    enabled: bool,
    #[cfg(target_os = "macos")]
    inner: macos::Engine,
}

impl Default for MixerState {
    fn default() -> Self {
        Self {
            gains: GainMap::default(),
            apps: Vec::new(),
            ack: false,
            denied: false,
            card_visible: false,
            enabled: true,
            #[cfg(target_os = "macos")]
            inner: macos::Engine::default(),
        }
    }
}

static STATE: OnceLock<Mutex<MixerState>> = OnceLock::new();
static GEN: AtomicU64 = AtomicU64::new(1);
static DIRTY: AtomicBool = AtomicBool::new(false);

fn lock_state() -> MutexGuard<'static, MixerState> {
    STATE
        .get_or_init(|| Mutex::new(MixerState::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn bump() {
    GEN.fetch_add(1, Ordering::Relaxed);
}

fn persist_gains(map: &GainMap) {
    match serde_json::to_string(&map.to_persist()) {
        Ok(json) => {
            if let Err(err) = crate::database::set_setting(GAINS_KEY, &json) {
                log::debug!("mixer persist gains: {err}");
            }
        }
        Err(err) => log::debug!("mixer serialize gains: {err}"),
    }
}

fn persist_ack(ack: bool) {
    if let Err(err) = crate::database::set_setting(ACK_KEY, if ack { "1" } else { "0" }) {
        log::debug!("mixer persist ack: {err}");
    }
}

fn load_persisted(state: &mut MixerState) {
    if let Some(json) = crate::database::get_setting(GAINS_KEY) {
        if let Ok(map) = serde_json::from_str::<BTreeMap<String, f32>>(&json) {
            state.gains = GainMap::from_persist(map);
        }
    }
    if let Some(flag) = crate::database::get_setting(ACK_KEY) {
        state.ack = flag == "1" || flag.eq_ignore_ascii_case("true");
    }
}

/// Load saved gains and, on macOS 14.4+, clean leftover private aggregates.
pub fn init() {
    let mut state = lock_state();
    load_persisted(&mut state);
    #[cfg(target_os = "macos")]
    {
        macos::cleanup_orphans();
        if is_available() && state.enabled && state.gains.has_active() {
            state.inner.ensure_watchers();
            DIRTY.store(true, Ordering::Relaxed);
        }
    }
    bump();
}

pub fn generation() -> u64 {
    GEN.load(Ordering::Relaxed)
}

pub fn is_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::process_taps_supported()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn snapshot() -> Vec<MixerApp> {
    lock_state().apps.clone()
}

pub fn capture_acknowledged() -> bool {
    lock_state().ack
}

pub fn acknowledge_capture() {
    let mut state = lock_state();
    if state.ack {
        return;
    }
    state.ack = true;
    persist_ack(true);
    bump();
}

pub fn capture_status() -> CaptureStatus {
    if !is_available() {
        return CaptureStatus::Unavailable;
    }
    let state = lock_state();
    if state.denied {
        return CaptureStatus::Denied;
    }
    if state.gains.has_active() && state.ack {
        return CaptureStatus::Active;
    }
    if !state.ack {
        return CaptureStatus::NeedsConsent;
    }
    CaptureStatus::Idle
}

pub fn set_card_visible(visible: bool) {
    let mut state = lock_state();
    if state.card_visible == visible {
        return;
    }
    state.card_visible = visible;
    DIRTY.store(true, Ordering::Relaxed);
}

pub fn set_enabled(enabled: bool) {
    let mut state = lock_state();
    if state.enabled == enabled {
        return;
    }
    state.enabled = enabled;
    if !enabled {
        state.card_visible = false;
        #[cfg(target_os = "macos")]
        {
            state.inner.teardown_pipeline();
            state.inner.stop_watchers();
        }
    } else {
        DIRTY.store(true, Ordering::Relaxed);
    }
    bump();
}

pub fn set_gain(bundle_id: &str, gain: f32) {
    let mut state = lock_state();
    if !state.ack && !is_unity(sanitize_gain(gain)) {
        return;
    }
    let key = canonical_bundle_id(bundle_id);
    state.gains.set(&key, gain);
    persist_gains(&state.gains);
    refresh_app_gains(&mut state);
    DIRTY.store(true, Ordering::Relaxed);
    bump();
}

pub fn toggle_mute(bundle_id: &str) {
    let mut state = lock_state();
    if !state.ack {
        return;
    }
    let key = canonical_bundle_id(bundle_id);
    state.gains.toggle_mute(&key);
    persist_gains(&state.gains);
    refresh_app_gains(&mut state);
    DIRTY.store(true, Ordering::Relaxed);
    bump();
}

pub fn reset_all() {
    let mut state = lock_state();
    state.gains.reset_all();
    persist_gains(&state.gains);
    refresh_app_gains(&mut state);
    #[cfg(target_os = "macos")]
    {
        state.inner.teardown_pipeline();
        if !state.card_visible {
            state.inner.stop_watchers();
        }
    }
    DIRTY.store(true, Ordering::Relaxed);
    bump();
}

pub fn mark_denied() {
    let mut state = lock_state();
    if !state.denied {
        state.denied = true;
        bump();
    }
}

fn refresh_app_gains(state: &mut MixerState) {
    for app in &mut state.apps {
        app.gain = state.gains.get(&app.bundle_id);
        app.muted = state.gains.is_muted(&app.bundle_id);
    }
}

/// Apply event-driven HAL updates. Cheap when nothing is dirty.
pub fn pump() {
    if !DIRTY.swap(false, Ordering::AcqRel) {
        return;
    }
    let mut state = lock_state();
    if !is_available() || !state.enabled {
        state.apps.clear();
        #[cfg(target_os = "macos")]
        {
            state.inner.teardown_pipeline();
            state.inner.stop_watchers();
        }
        return;
    }
    let want_watchers = state.card_visible || state.gains.has_active();
    #[cfg(target_os = "macos")]
    {
        if want_watchers {
            state.inner.ensure_watchers();
            let procs = state.inner.refresh_processes();
            state.apps = group_playing_apps(&procs, &state.gains);
            let running_keys: Vec<String> =
                state.apps.iter().map(|a| a.bundle_id.clone()).collect();
            if state.ack {
                let MixerState {
                    inner,
                    gains,
                    apps,
                    denied,
                    ..
                } = &mut *state;
                if let Err(err) = inner.sync_pipeline(gains, apps) {
                    log::warn!("mixer pipeline: {err}");
                    if err.contains("denied") || err.contains("permission") || err.contains("-54") {
                        *denied = true;
                    }
                }
            }
            if !state.gains.has_active()
                && !running_keys.iter().any(|id| !is_unity(state.gains.get(id)))
            {
                state.inner.teardown_pipeline();
            }
            if !state.card_visible && !state.gains.has_active() {
                state.inner.stop_watchers();
            }
        } else {
            state.apps.clear();
            state.inner.teardown_pipeline();
            state.inner.stop_watchers();
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = want_watchers;
        state.apps.clear();
    }
    bump();
}

/// Copy IOProc peaks into the UI snapshot. Call only while the card is open.
pub fn copy_levels(apps: &mut [MixerApp]) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::copy_levels(apps)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = apps;
        false
    }
}

#[cfg(target_os = "macos")]
mod macos {
    //! Core Audio process-tap engine (macOS 14.4+ public APIs only).
    //!
    //! Aligned with objc2 0.6 / objc2-core-audio 0.3.2:
    //! `kAudioObjectSystemObject` is `c_int`; HAL functions take `NonNull` and
    //! `extern "C-unwind"` listeners. `CATapDescription::alloc` needs `AnyThread`.
    //! No private entitlements. `responsibility_get_pid_responsible_for_pid` is
    //! resolved with `dlsym` and ignored when missing.

    use super::*;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, AnyThread, Encode, Encoding};
    use objc2_core_audio::{
        kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceNameKey,
        kAudioAggregateDeviceSubDeviceListKey, kAudioAggregateDeviceTapAutoStartKey,
        kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey,
        kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyDeviceUID,
        kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices,
        kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
        kAudioProcessPropertyBundleID, kAudioProcessPropertyIsRunningOutput,
        kAudioProcessPropertyPID, kAudioSubDeviceUIDKey, kAudioSubTapDriftCompensationKey,
        kAudioSubTapUIDKey, kAudioTapPropertyUID, AudioDeviceCreateIOProcID,
        AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop,
        AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
        AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap,
        AudioObjectAddPropertyListener, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
        AudioObjectID, AudioObjectPropertyAddress, AudioObjectRemovePropertyListener,
        AudioObjectSetPropertyData, CATapDescription, CATapMuteBehavior,
    };
    use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};
    use objc2_core_foundation::{CFDictionary, CFRetained, CFString};
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUUID};
    use std::ffi::{c_void, CStr};
    use std::ptr::NonNull;
    use std::sync::atomic::AtomicU32;

    /// `kAudioObjectSystemObject` is `c_int` in objc2-core-audio 0.3.2.
    const SYSTEM_OBJECT: AudioObjectID = kAudioObjectSystemObject as AudioObjectID;

    /// Same layout as Foundation's `NSOperatingSystemVersion` (`NSInteger` triple).
    #[repr(C)]
    struct NsVer {
        major: isize,
        minor: isize,
        patch: isize,
    }

    unsafe impl Encode for NsVer {
        const ENCODING: Encoding = Encoding::Struct(
            "NSOperatingSystemVersion",
            &[isize::ENCODING, isize::ENCODING, isize::ENCODING],
        );
    }

    const MAX_TAPS: usize = 16;
    const AGG_PREFIX: &str = "openNook Mixer";
    const BUFFER_FRAMES: u32 = 512;

    static TAP_GAINS: [AtomicU32; MAX_TAPS] = [const { AtomicU32::new(0) }; MAX_TAPS];
    static TAP_PEAKS: [AtomicU32; MAX_TAPS] = [const { AtomicU32::new(0) }; MAX_TAPS];
    static TAP_COUNT: AtomicU32 = AtomicU32::new(0);
    static TAP_KEYS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    pub struct Engine {
        watching: bool,
        pipeline: Option<Pipeline>,
        processes: Vec<RawProcess>,
        running_listeners: Vec<AudioObjectID>,
    }

    impl Default for Engine {
        fn default() -> Self {
            Self {
                watching: false,
                pipeline: None,
                processes: Vec::new(),
                running_listeners: Vec::new(),
            }
        }
    }

    struct Pipeline {
        taps: Vec<AudioObjectID>,
        agg: AudioObjectID,
        proc_id: AudioDeviceIOProcID,
        started: bool,
    }

    impl Drop for Pipeline {
        fn drop(&mut self) {
            unsafe { teardown(self) }
        }
    }

    pub fn process_taps_supported() -> bool {
        unsafe {
            let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
            if info.is_null() {
                return false;
            }
            let ver = NsVer {
                major: 14,
                minor: 4,
                patch: 0,
            };
            let ok: bool = msg_send![info, isOperatingSystemAtLeastVersion: ver];
            ok
        }
    }

    pub fn cleanup_orphans() {
        let devices = property_ids(SYSTEM_OBJECT, kAudioHardwarePropertyDevices);
        for id in devices {
            if let Some(name) = read_cf_string(id, kAudioObjectPropertyName) {
                if name.starts_with(AGG_PREFIX) {
                    log::info!("mixer: destroying orphaned aggregate {name}");
                    unsafe {
                        AudioHardwareDestroyAggregateDevice(id);
                    }
                }
            }
        }
    }

    impl Engine {
        pub fn ensure_watchers(&mut self) {
            if self.watching || !process_taps_supported() {
                return;
            }
            add_listener(SYSTEM_OBJECT, kAudioHardwarePropertyProcessObjectList);
            add_listener(SYSTEM_OBJECT, kAudioHardwarePropertyDefaultOutputDevice);
            self.watching = true;
        }

        pub fn stop_watchers(&mut self) {
            if !self.watching {
                return;
            }
            remove_listener(SYSTEM_OBJECT, kAudioHardwarePropertyProcessObjectList);
            remove_listener(SYSTEM_OBJECT, kAudioHardwarePropertyDefaultOutputDevice);
            for id in self.running_listeners.drain(..) {
                remove_listener(id, kAudioProcessPropertyIsRunningOutput);
            }
            self.watching = false;
        }

        pub fn refresh_processes(&mut self) -> Vec<RawProcess> {
            let ids = property_ids(SYSTEM_OBJECT, kAudioHardwarePropertyProcessObjectList);
            let mut next_listeners = Vec::new();
            let mut procs = Vec::new();
            for object_id in ids {
                if !self.running_listeners.contains(&object_id) {
                    add_listener(object_id, kAudioProcessPropertyIsRunningOutput);
                }
                next_listeners.push(object_id);
                let pid = read_i32(object_id, kAudioProcessPropertyPID).unwrap_or(0);
                let bundle =
                    read_cf_string(object_id, kAudioProcessPropertyBundleID).unwrap_or_default();
                let running =
                    read_u32(object_id, kAudioProcessPropertyIsRunningOutput).unwrap_or(0) != 0;
                let (name, responsible) = identity_for_pid(pid, &bundle);
                procs.push(RawProcess {
                    pid,
                    object_id,
                    bundle_id: bundle,
                    name,
                    running_output: running,
                    responsible_bundle: responsible,
                });
            }
            for old in &self.running_listeners {
                if !next_listeners.contains(old) {
                    remove_listener(*old, kAudioProcessPropertyIsRunningOutput);
                }
            }
            self.running_listeners = next_listeners;
            self.processes = procs.clone();
            procs
        }

        pub fn teardown_pipeline(&mut self) {
            self.pipeline = None;
            TAP_COUNT.store(0, Ordering::Relaxed);
            if let Ok(mut keys) = TAP_KEYS.lock() {
                keys.clear();
            }
        }

        pub fn sync_pipeline(&mut self, gains: &GainMap, apps: &[MixerApp]) -> Result<(), String> {
            let needed: Vec<&MixerApp> = apps
                .iter()
                .filter(|app| !is_unity(gains.get(&app.bundle_id)) && !app.object_ids.is_empty())
                .collect();
            if needed.is_empty() {
                self.teardown_pipeline();
                return Ok(());
            }
            if needed.len() > MAX_TAPS {
                return Err("too many tapped apps".into());
            }
            // Rebuild whenever the set of tapped apps or the default device changes.
            self.teardown_pipeline();
            self.pipeline = Some(build_pipeline(&needed, gains)?);
            Ok(())
        }
    }

    fn build_pipeline(apps: &[&MixerApp], gains: &GainMap) -> Result<Pipeline, String> {
        let default_out = default_output_device().ok_or("no default output")?;
        let device_uid = read_cf_string(default_out, kAudioDevicePropertyDeviceUID)
            .ok_or("default output has no uid")?;

        let mut taps = Vec::new();
        let mut tap_uids = Vec::new();
        let mut keys = Vec::new();
        for (i, app) in apps.iter().enumerate() {
            let tap = create_tap(app).map_err(|err| {
                if err.contains("-54") || err.to_lowercase().contains("denied") {
                    format!("permission denied: {err}")
                } else {
                    err
                }
            })?;
            let uid = read_cf_string(tap, kAudioTapPropertyUID).unwrap_or_default();
            if uid.is_empty() {
                unsafe {
                    AudioHardwareDestroyProcessTap(tap);
                }
                return Err("tap has no uid".into());
            }
            TAP_GAINS[i].store(gains.get(&app.bundle_id).to_bits(), Ordering::Relaxed);
            TAP_PEAKS[i].store(0, Ordering::Relaxed);
            keys.push(app.bundle_id.clone());
            taps.push(tap);
            tap_uids.push(uid);
        }
        TAP_COUNT.store(taps.len() as u32, Ordering::Relaxed);
        if let Ok(mut slot) = TAP_KEYS.lock() {
            *slot = keys;
        }

        let dict = aggregate_description(&device_uid, &tap_uids);
        let cf_dict: &CFDictionary =
            unsafe { &*(objc2::rc::Retained::as_ptr(&dict) as *const CFDictionary) };
        let mut agg: AudioObjectID = 0;
        let status =
            unsafe { AudioHardwareCreateAggregateDevice(cf_dict, NonNull::from(&mut agg)) };
        if status != 0 || agg == 0 {
            for tap in taps {
                unsafe {
                    AudioHardwareDestroyProcessTap(tap);
                }
            }
            return Err(format!("create aggregate failed ({status})"));
        }
        set_buffer_frames(agg, BUFFER_FRAMES);

        let mut proc_id: AudioDeviceIOProcID = None;
        let status = unsafe {
            AudioDeviceCreateIOProcID(
                agg,
                Some(io_proc),
                std::ptr::null_mut(),
                NonNull::from(&mut proc_id),
            )
        };
        if status != 0 || proc_id.is_none() {
            unsafe {
                AudioHardwareDestroyAggregateDevice(agg);
            }
            for tap in taps {
                unsafe {
                    AudioHardwareDestroyProcessTap(tap);
                }
            }
            return Err(format!("create IOProc failed ({status})"));
        }
        let status = unsafe { AudioDeviceStart(agg, proc_id) };
        if status != 0 {
            unsafe {
                AudioDeviceDestroyIOProcID(agg, proc_id);
                AudioHardwareDestroyAggregateDevice(agg);
            }
            for tap in taps {
                unsafe {
                    AudioHardwareDestroyProcessTap(tap);
                }
            }
            return Err(format!("start IOProc failed ({status})"));
        }

        Ok(Pipeline {
            taps,
            agg,
            proc_id,
            started: true,
        })
    }

    fn create_tap(app: &MixerApp) -> Result<AudioObjectID, String> {
        let nums: Vec<objc2::rc::Retained<NSNumber>> = app
            .object_ids
            .iter()
            .map(|id| NSNumber::new_u32(*id))
            .collect();
        let refs: Vec<&NSNumber> = nums.iter().map(|n| n.as_ref()).collect();
        let array = NSArray::from_slice(&refs);
        let desc = unsafe {
            CATapDescription::initStereoMixdownOfProcesses(CATapDescription::alloc(), &array)
        };
        unsafe {
            desc.setName(&NSString::from_str(&format!(
                "{AGG_PREFIX} {}",
                app.bundle_id
            )));
            desc.setPrivate(true);
            // CATapMutedWhenTapped — silence the process while the tap is live.
            desc.setMuteBehavior(CATapMuteBehavior(2));
        }
        let mut tap: AudioObjectID = 0;
        let status = unsafe { AudioHardwareCreateProcessTap(Some(&desc), &mut tap) };
        if status != 0 || tap == 0 {
            return Err(format!("create tap failed ({status})"));
        }
        Ok(tap)
    }

    fn aggregate_description(
        device_uid: &str,
        tap_uids: &[String],
    ) -> objc2::rc::Retained<NSDictionary<NSString, AnyObject>> {
        let device = NSString::from_str(device_uid);
        let sub = NSDictionary::<NSString, AnyObject>::from_slices(
            &[&*ns(kAudioSubDeviceUIDKey)],
            &[&device],
        );
        let sub_list = NSArray::from_retained_slice(&[sub]);

        let drift = NSNumber::new_i32(1);
        let mut tap_dicts = Vec::new();
        for uid in tap_uids {
            let uid_ns = NSString::from_str(uid);
            let dict = NSDictionary::<NSString, AnyObject>::from_slices(
                &[
                    &*ns(kAudioSubTapUIDKey),
                    &*ns(kAudioSubTapDriftCompensationKey),
                ],
                &[&uid_ns, &drift],
            );
            tap_dicts.push(dict);
        }
        let tap_list = NSArray::from_retained_slice(&tap_dicts);

        let name = NSString::from_str(&format!("{AGG_PREFIX} {}", random_uid()));
        let uid = NSString::from_str(&random_uid());
        let yes = NSNumber::new_i32(1);
        NSDictionary::<NSString, AnyObject>::from_slices(
            &[
                &*ns(kAudioAggregateDeviceNameKey),
                &*ns(kAudioAggregateDeviceUIDKey),
                &*ns(kAudioAggregateDeviceIsPrivateKey),
                &*ns(kAudioAggregateDeviceTapAutoStartKey),
                &*ns(kAudioAggregateDeviceSubDeviceListKey),
                &*ns(kAudioAggregateDeviceTapListKey),
            ],
            &[&name, &uid, &yes, &yes, &sub_list, &tap_list],
        )
    }

    fn ns(key: &CStr) -> objc2::rc::Retained<NSString> {
        NSString::from_str(key.to_str().unwrap_or_default())
    }

    fn random_uid() -> String {
        NSUUID::new().UUIDString().to_string()
    }

    fn default_output_device() -> Option<AudioObjectID> {
        read_u32(SYSTEM_OBJECT, kAudioHardwarePropertyDefaultOutputDevice)
    }

    fn set_buffer_frames(device: AudioObjectID, frames: u32) {
        let address = address(kAudioDevicePropertyBufferFrameSize);
        let mut frames = frames;
        let size = std::mem::size_of::<u32>() as u32;
        unsafe {
            AudioObjectSetPropertyData(
                device,
                NonNull::from(&address),
                0,
                std::ptr::null(),
                size,
                NonNull::from(&mut frames).cast::<c_void>(),
            );
        }
    }

    unsafe fn teardown(pipeline: &mut Pipeline) {
        if pipeline.started {
            AudioDeviceStop(pipeline.agg, pipeline.proc_id);
            pipeline.started = false;
        }
        if pipeline.proc_id.is_some() {
            AudioDeviceDestroyIOProcID(pipeline.agg, pipeline.proc_id);
            pipeline.proc_id = None;
        }
        if pipeline.agg != 0 {
            AudioHardwareDestroyAggregateDevice(pipeline.agg);
            pipeline.agg = 0;
        }
        for tap in pipeline.taps.drain(..) {
            if tap != 0 {
                AudioHardwareDestroyProcessTap(tap);
            }
        }
    }

    unsafe extern "C-unwind" fn io_proc(
        _device: AudioObjectID,
        _now: NonNull<AudioTimeStamp>,
        in_data: NonNull<AudioBufferList>,
        _in_time: NonNull<AudioTimeStamp>,
        out_data: NonNull<AudioBufferList>,
        _out_time: NonNull<AudioTimeStamp>,
        _client: *mut c_void,
    ) -> i32 {
        let _ = std::panic::catch_unwind(|| mix_buffers(in_data, out_data));
        0
    }

    fn mix_buffers(in_data: NonNull<AudioBufferList>, out_data: NonNull<AudioBufferList>) {
        unsafe {
            let output = out_data.as_ref();
            let out_bufs = std::slice::from_raw_parts_mut(
                output.mBuffers.as_ptr() as *mut objc2_core_audio_types::AudioBuffer,
                output.mNumberBuffers as usize,
            );
            for buf in out_bufs.iter_mut() {
                if buf.mData.is_null() {
                    continue;
                }
                std::ptr::write_bytes(buf.mData, 0, buf.mDataByteSize as usize);
            }
            let input = in_data.as_ref();
            let in_bufs =
                std::slice::from_raw_parts(input.mBuffers.as_ptr(), input.mNumberBuffers as usize);
            let n = TAP_COUNT.load(Ordering::Relaxed) as usize;
            for (i, buf) in in_bufs.iter().enumerate() {
                if buf.mData.is_null() {
                    continue;
                }
                let gain = f32::from_bits(
                    TAP_GAINS[i.min(n.saturating_sub(1).min(MAX_TAPS - 1))].load(Ordering::Relaxed),
                );
                let count = buf.mDataByteSize as usize / std::mem::size_of::<f32>();
                let samples = std::slice::from_raw_parts(buf.mData as *const f32, count);
                let mut peak = 0.0f32;
                if let Some(out) = out_bufs.first_mut() {
                    if !out.mData.is_null() {
                        let out_count = out.mDataByteSize as usize / std::mem::size_of::<f32>();
                        let dest = std::slice::from_raw_parts_mut(out.mData as *mut f32, out_count);
                        let n = count.min(out_count);
                        for j in 0..n {
                            let s = scale_sample(samples[j], gain);
                            let a = s.abs();
                            if a > peak {
                                peak = a;
                            }
                            dest[j] = (dest[j] + s).clamp(-1.0, 1.0);
                        }
                    }
                }
                if i < MAX_TAPS {
                    TAP_PEAKS[i].store(peak.to_bits(), Ordering::Relaxed);
                }
            }
        }
    }

    pub fn copy_levels(apps: &mut [MixerApp]) -> bool {
        let keys = TAP_KEYS.lock().unwrap_or_else(|e| e.into_inner());
        let mut changed = false;
        for app in apps.iter_mut() {
            let peak = keys
                .iter()
                .position(|k| k == &app.bundle_id)
                .and_then(|i| TAP_PEAKS.get(i))
                .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
                .unwrap_or(0.0);
            let next = (app.level * 0.72 + peak * 0.28).clamp(0.0, 1.0);
            if (next - app.level).abs() > 0.012 {
                app.level = next;
                changed = true;
            }
        }
        changed
    }

    fn identity_for_pid(pid: i32, bundle: &str) -> (String, Option<String>) {
        let name = running_app_name(pid).unwrap_or_else(|| display_name_for_bundle(bundle));
        let responsible = responsible_pid(pid)
            .filter(|r| *r != pid)
            .and_then(running_app_bundle);
        (name, responsible)
    }

    fn running_app_name(pid: i32) -> Option<String> {
        unsafe {
            let app: *mut AnyObject = msg_send![
                class!(NSRunningApplication),
                runningApplicationWithProcessIdentifier: pid
            ];
            if app.is_null() {
                return None;
            }
            let name: *mut AnyObject = msg_send![app, localizedName];
            nsstring_to_rust(name)
        }
    }

    fn running_app_bundle(pid: i32) -> Option<String> {
        unsafe {
            let app: *mut AnyObject = msg_send![
                class!(NSRunningApplication),
                runningApplicationWithProcessIdentifier: pid
            ];
            if app.is_null() {
                return None;
            }
            let ident: *mut AnyObject = msg_send![app, bundleIdentifier];
            nsstring_to_rust(ident)
        }
    }

    fn nsstring_to_rust(s: *mut AnyObject) -> Option<String> {
        if s.is_null() {
            return None;
        }
        unsafe {
            let ptr: *const i8 = msg_send![s, UTF8String];
            if ptr.is_null() {
                return None;
            }
            CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_string())
        }
    }

    fn responsible_pid(pid: i32) -> Option<i32> {
        unsafe {
            extern "C" {
                fn dlsym(handle: *mut c_void, name: *const i8) -> *mut c_void;
            }
            const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
            let sym = dlsym(
                RTLD_DEFAULT,
                b"responsibility_get_pid_responsible_for_pid\0".as_ptr() as *const i8,
            );
            if sym.is_null() {
                return None;
            }
            let f: unsafe extern "C" fn(i32) -> i32 = std::mem::transmute(sym);
            let parent = f(pid);
            if parent > 0 {
                Some(parent)
            } else {
                None
            }
        }
    }

    fn address(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    fn add_listener(object: AudioObjectID, selector: u32) {
        let addr = address(selector);
        unsafe {
            AudioObjectAddPropertyListener(
                object,
                NonNull::from(&addr),
                Some(on_property),
                std::ptr::null_mut(),
            );
        }
    }

    fn remove_listener(object: AudioObjectID, selector: u32) {
        let addr = address(selector);
        unsafe {
            AudioObjectRemovePropertyListener(
                object,
                NonNull::from(&addr),
                Some(on_property),
                std::ptr::null_mut(),
            );
        }
    }

    unsafe extern "C-unwind" fn on_property(
        _object: AudioObjectID,
        _n: u32,
        _addresses: NonNull<AudioObjectPropertyAddress>,
        _client: *mut c_void,
    ) -> i32 {
        super::DIRTY.store(true, Ordering::Relaxed);
        super::bump();
        0
    }

    fn property_ids(object: AudioObjectID, selector: u32) -> Vec<AudioObjectID> {
        let addr = address(selector);
        let mut size = 0u32;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                object,
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
            )
        };
        if status != 0 || size == 0 {
            return Vec::new();
        }
        let count = size as usize / std::mem::size_of::<AudioObjectID>();
        let mut ids = vec![0u32; count];
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::new(ids.as_mut_ptr())
                    .expect("audio object id buffer")
                    .cast::<c_void>(),
            )
        };
        if status != 0 {
            return Vec::new();
        }
        ids.into_iter().filter(|id| *id != 0).collect()
    }

    fn read_u32(object: AudioObjectID, selector: u32) -> Option<u32> {
        let addr = address(selector);
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(&mut value).cast::<c_void>(),
            )
        };
        (status == 0).then_some(value)
    }

    fn read_i32(object: AudioObjectID, selector: u32) -> Option<i32> {
        read_u32(object, selector).map(|v| v as i32)
    }

    fn read_cf_string(object: AudioObjectID, selector: u32) -> Option<String> {
        let addr = address(selector);
        let mut cf: *const CFString = std::ptr::null();
        let mut size = std::mem::size_of::<*const CFString>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(&mut cf).cast::<c_void>(),
            )
        };
        if status != 0 {
            return None;
        }
        let cf = NonNull::new(cf as *mut CFString)?;
        let owned = unsafe { CFRetained::from_raw(cf) };
        Some(owned.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn gain_map_defaults_to_unity() {
        let map = GainMap::default();
        assert!(is_unity(map.get("com.spotify.client")));
        assert!(!map.has_active());
        assert!(map.active_ids().is_empty());
    }

    #[test]
    fn gain_map_set_reset_and_persist() {
        let mut map = GainMap::default();
        map.set("com.spotify.client", 0.4);
        map.set("com.apple.Music", 1.0);
        map.set("  ", 0.2);
        assert!((map.get("com.spotify.client") - 0.4).abs() < f32::EPSILON);
        assert!(is_unity(map.get("com.apple.Music")));
        assert!(!map.has_active() || map.active_ids() == ["com.spotify.client"]);
        let persisted = map.to_persist();
        assert_eq!(persisted.len(), 1);
        assert!((persisted["com.spotify.client"] - 0.4).abs() < f32::EPSILON);
        map.reset("com.spotify.client");
        assert!(!map.has_active());
        let restored = GainMap::from_persist(persisted);
        assert!((restored.get("com.spotify.client") - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn gain_map_mute_remembers_previous_level() {
        let mut map = GainMap::default();
        map.set("com.apple.Safari", 0.6);
        map.toggle_mute("com.apple.Safari");
        assert!(map.is_muted("com.apple.Safari"));
        assert!(map.get("com.apple.Safari") <= MUTE_EPS);
        map.toggle_mute("com.apple.Safari");
        assert!((map.get("com.apple.Safari") - 0.6).abs() < f32::EPSILON);
        map.toggle_mute("com.unknown.app");
        assert!(map.is_muted("com.unknown.app"));
        map.toggle_mute("com.unknown.app");
        assert!(is_unity(map.get("com.unknown.app")));
    }

    #[test]
    fn gain_map_reset_all_clears_active() {
        let mut map = GainMap::default();
        map.set("a", 0.2);
        map.set("b", 0.0);
        assert!(map.has_active());
        map.reset_all();
        assert!(!map.has_active());
        assert!(is_unity(map.get("a")));
    }

    #[test]
    fn gain_map_clamps_and_rejects_nan() {
        let mut map = GainMap::default();
        map.set("x", 9.0);
        assert!((map.get("x") - GAIN_MAX).abs() < f32::EPSILON);
        map.set("x", f32::NAN);
        assert!(is_unity(map.get("x")));
    }

    #[test]
    fn scale_sample_unity_and_limiter() {
        assert_eq!(scale_sample(0.5, 1.0), 0.5);
        assert_eq!(scale_sample(0.5, 0.0), 0.0);
        assert!(scale_sample(0.9, 1.5) < 0.9 * 1.5);
        assert!(scale_sample(2.0, 1.0) <= 1.0);
        assert_eq!(scale_sample(f32::NAN, 1.0), 0.0);
    }

    #[test]
    fn grouping_maps_webkit_and_browser_helpers() {
        assert_eq!(
            canonical_bundle_id("com.apple.WebKit.GPU"),
            "com.apple.Safari"
        );
        assert_eq!(
            canonical_bundle_id("com.apple.WebKit.WebContent"),
            "com.apple.Safari"
        );
        assert_eq!(
            canonical_bundle_id("com.google.Chrome.helper.gpu"),
            "com.google.Chrome"
        );
        assert_eq!(
            canonical_bundle_id("com.microsoft.edgemac.helper"),
            "com.microsoft.edgemac"
        );
        assert_eq!(
            canonical_bundle_id("com.google.Chrome.canary.helper"),
            "com.google.Chrome.canary"
        );
        assert_eq!(
            canonical_bundle_id("com.spotify.client"),
            "com.spotify.client"
        );
        assert!(is_helper_bundle("com.apple.WebKit.GPU"));
        assert!(!is_helper_bundle("com.apple.Safari"));
    }

    #[test]
    fn grouping_uses_responsible_parent_for_helpers() {
        assert_eq!(
            group_key("com.apple.WebKit.GPU", Some("com.apple.Safari")),
            "com.apple.Safari"
        );
        assert_eq!(
            group_key("com.figma.Desktop.helper", Some("com.figma.Desktop")),
            "com.figma.Desktop"
        );
        assert_eq!(
            group_key("com.spotify.client", Some("com.apple.Safari")),
            "com.spotify.client"
        );
    }

    #[test]
    fn grouping_merges_helper_pids_into_one_row() {
        let gains = GainMap::from_persist(BTreeMap::from([("com.apple.Safari".into(), 0.5)]));
        let apps = group_playing_apps(
            &[
                RawProcess {
                    pid: 11,
                    object_id: 100,
                    bundle_id: "com.apple.WebKit.GPU".into(),
                    name: "WebKit GPU".into(),
                    running_output: true,
                    responsible_bundle: Some("com.apple.Safari".into()),
                },
                RawProcess {
                    pid: 12,
                    object_id: 101,
                    bundle_id: "com.apple.Safari".into(),
                    name: "Safari".into(),
                    running_output: true,
                    responsible_bundle: None,
                },
                RawProcess {
                    pid: 13,
                    object_id: 102,
                    bundle_id: "com.apple.finder".into(),
                    name: "Finder".into(),
                    running_output: false,
                    responsible_bundle: None,
                },
            ],
            &gains,
        );
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].bundle_id, "com.apple.Safari");
        assert_eq!(apps[0].name, "Safari");
        assert_eq!(apps[0].pids, vec![11, 12]);
        assert!((apps[0].gain - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn display_names_cover_common_players() {
        assert_eq!(display_name_for_bundle("com.spotify.client"), "Spotify");
        assert_eq!(display_name_for_bundle("com.apple.Safari"), "Safari");
        assert_eq!(display_name_for_bundle("org.videolan.vlc"), "VLC");
        assert_eq!(display_name_for_bundle("com.example.FooApp"), "FooApp");
    }

    #[test]
    fn version_gate_matches_14_4_floor() {
        assert!(version_at_least(14, 4, 0, 14, 4, 0));
        assert!(version_at_least(15, 0, 0, 14, 4, 0));
        assert!(version_at_least(14, 5, 1, 14, 4, 0));
        assert!(!version_at_least(14, 3, 9, 14, 4, 0));
        assert!(!version_at_least(13, 6, 0, 14, 4, 0));
    }

    #[test]
    fn tcc_preprompt_explains_recording_is_not_saved() {
        assert!(TCC_PREPROMPT.contains("does not save"));
        assert!(TCC_PREPROMPT.contains("recording"));
        assert!(TCC_PREPROMPT.contains("slider"));
        assert_eq!(
            capture_status_label(CaptureStatus::Unavailable),
            "Requires macOS 14.4 or later"
        );
    }

    #[derive(Serialize, Deserialize)]
    struct PersistShape {
        gains: BTreeMap<String, f32>,
    }

    #[test]
    fn persist_shape_round_trips_non_unity_only() {
        let mut map = GainMap::default();
        map.set("com.spotify.client", 0.25);
        map.set("com.apple.Music", 1.0);
        let blob = PersistShape {
            gains: map.to_persist(),
        };
        let json = serde_json::to_string(&blob).unwrap();
        let parsed: PersistShape = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.gains.len(), 1);
        let restored = GainMap::from_persist(parsed.gains);
        assert!((restored.get("com.spotify.client") - 0.25).abs() < f32::EPSILON);
        assert!(is_unity(restored.get("com.apple.Music")));
    }
}
