//! Zoom / Teams / Google Meet detection and island controls.
//!
//! Honest ceiling: Zoom is full (AX menu mute + leave). Teams is a blind
//! keystroke toggle after the localhost:8124 API retirement. Meet is
//! best-effort (focus the tab + Cmd+D/E, or opt-in Apple Events JS).
//!
//! Event-driven: NSWorkspace launch/terminate + CoreAudio
//! `DeviceIsRunningSomewhere` (re-armed on default-input change). No poll at
//! rest. AX / AppleScript run once on a mic-live transition to confirm a
//! meeting, and Zoom menu-title readback runs only while the meeting face is
//! shown.

#[cfg(target_os = "macos")]
use crate::settings::MeetControlMode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Instant;

pub const ZOOM_BUNDLE: &str = "us.zoom.xos";
pub const TEAMS_BUNDLE: &str = "com.microsoft.teams2";
pub const TEAMS_CLASSIC_BUNDLE: &str = "com.microsoft.teams";

#[cfg(target_os = "macos")]
const KEY_D: u16 = 0x02;
#[cfg(target_os = "macos")]
const KEY_E: u16 = 0x0E;
#[cfg(target_os = "macos")]
const KEY_H: u16 = 0x04;
#[cfg(target_os = "macos")]
const KEY_M: u16 = 0x2E;
#[cfg(target_os = "macos")]
const KEY_W: u16 = 0x0D;
#[cfg(target_os = "macos")]
const FLAG_SHIFT: u64 = 0x0002_0000;
#[cfg(target_os = "macos")]
const FLAG_CMD: u64 = 0x0010_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeetingApp {
    Zoom,
    Teams,
    Meet,
}

impl MeetingApp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zoom => "Zoom",
            Self::Teams => "Teams",
            Self::Meet => "Meet",
        }
    }

    pub fn bundle_id(self) -> &'static str {
        match self {
            Self::Zoom => ZOOM_BUNDLE,
            Self::Teams => TEAMS_BUNDLE,
            Self::Meet => "com.google.Chrome",
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Zoom => "video",
            Self::Teams => "users",
            Self::Meet => "video",
        }
    }

    pub fn mute_verified(self) -> bool {
        matches!(self, Self::Zoom)
    }

    pub fn from_bundle(id: &str) -> Option<Self> {
        if id == ZOOM_BUNDLE || id.starts_with("us.zoom.") {
            return Some(Self::Zoom);
        }
        if id == TEAMS_BUNDLE || id == TEAMS_CLASSIC_BUNDLE || id.starts_with("com.microsoft.teams")
        {
            return Some(Self::Teams);
        }
        if crate::browser_media::is_browser_bundle(id) {
            return Some(Self::Meet);
        }
        None
    }
}

/// Idle → AppRunning → MicLive → InMeeting. `muted` is `Some` only when the
/// host can verify it (Zoom AX title). Teams/Meet stay `None`.
#[derive(Clone, Debug, PartialEq)]
pub enum MeetingState {
    Idle,
    AppRunning {
        app: MeetingApp,
        pid: i32,
    },
    MicLive {
        app: MeetingApp,
        pid: i32,
    },
    InMeeting {
        app: MeetingApp,
        pid: i32,
        muted: Option<bool>,
        started: Instant,
    },
}

impl MeetingState {
    pub fn app(&self) -> Option<MeetingApp> {
        match *self {
            Self::Idle => None,
            Self::AppRunning { app, .. }
            | Self::MicLive { app, .. }
            | Self::InMeeting { app, .. } => Some(app),
        }
    }

    pub fn pid(&self) -> Option<i32> {
        match *self {
            Self::Idle => None,
            Self::AppRunning { pid, .. }
            | Self::MicLive { pid, .. }
            | Self::InMeeting { pid, .. } => Some(pid),
        }
    }

    pub fn in_meeting(&self) -> bool {
        matches!(self, Self::InMeeting { .. })
    }

    pub fn muted(&self) -> Option<bool> {
        match *self {
            Self::InMeeting { muted, .. } => muted,
            _ => None,
        }
    }

    pub fn started(&self) -> Option<Instant> {
        match *self {
            Self::InMeeting { started, .. } => Some(started),
            _ => None,
        }
    }
}

impl Default for MeetingState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeetingSnapshot {
    pub state: MeetingState,
    pub accessibility_trusted: bool,
}

impl Default for MeetingSnapshot {
    fn default() -> Self {
        Self {
            state: MeetingState::Idle,
            accessibility_trusted: false,
        }
    }
}

impl MeetingSnapshot {
    pub fn in_meeting(&self) -> bool {
        self.state.in_meeting()
    }

    pub fn app(&self) -> Option<MeetingApp> {
        self.state.app()
    }

    pub fn muted(&self) -> Option<bool> {
        self.state.muted()
    }

    pub fn mute_verified(&self) -> bool {
        self.app().is_some_and(MeetingApp::mute_verified) && self.muted().is_some()
    }

    pub fn elapsed_secs(&self) -> u32 {
        self.state
            .started()
            .map(|t| t.elapsed().as_secs() as u32)
            .unwrap_or(0)
    }
}

/// Inputs the state machine needs. Tests drive this directly; macOS fills it
/// from NSWorkspace + CoreAudio + one-shot confirmation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeetingSignals {
    pub running: Vec<(MeetingApp, i32)>,
    pub mic_live: bool,
    pub attributed: Option<(MeetingApp, i32)>,
    /// Zoom: `Some(muted)` when the Meeting menu exists; `None` if it does not.
    pub zoom_confirmed: Option<bool>,
    pub meet_tab: bool,
    pub enabled_zoom: bool,
    pub enabled_teams: bool,
    pub enabled_meet: bool,
}

impl MeetingSignals {
    fn enabled(&self, app: MeetingApp) -> bool {
        match app {
            MeetingApp::Zoom => self.enabled_zoom,
            MeetingApp::Teams => self.enabled_teams,
            MeetingApp::Meet => self.enabled_meet,
        }
    }
}

/// Pure transition. Confirmation flags are the only place Zoom/Meet become
/// `InMeeting`; Teams is app + mic.
pub fn next_state(prev: &MeetingState, sig: &MeetingSignals, now: Instant) -> MeetingState {
    let Some((app, pid)) = pick_candidate(sig) else {
        return MeetingState::Idle;
    };
    if !sig.mic_live {
        return MeetingState::AppRunning { app, pid };
    }
    match app {
        MeetingApp::Teams => keep_started(
            prev,
            MeetingState::InMeeting {
                app,
                pid,
                muted: None,
                started: now,
            },
            now,
        ),
        MeetingApp::Zoom => match sig.zoom_confirmed {
            Some(muted) => keep_started(
                prev,
                MeetingState::InMeeting {
                    app,
                    pid,
                    muted: Some(muted),
                    started: now,
                },
                now,
            ),
            None => MeetingState::MicLive { app, pid },
        },
        MeetingApp::Meet => {
            if sig.meet_tab {
                keep_started(
                    prev,
                    MeetingState::InMeeting {
                        app,
                        pid,
                        muted: None,
                        started: now,
                    },
                    now,
                )
            } else {
                MeetingState::MicLive { app, pid }
            }
        }
    }
}

fn keep_started(prev: &MeetingState, next: MeetingState, now: Instant) -> MeetingState {
    match (prev, &next) {
        (
            MeetingState::InMeeting {
                app: pa,
                pid: pp,
                started,
                ..
            },
            MeetingState::InMeeting { app, pid, muted, .. },
        ) if pa == app && pp == pid => MeetingState::InMeeting {
            app: *app,
            pid: *pid,
            muted: *muted,
            started: *started,
        },
        _ => match next {
            MeetingState::InMeeting {
                app,
                pid,
                muted,
                started: _,
            } => MeetingState::InMeeting {
                app,
                pid,
                muted,
                started: now,
            },
            other => other,
        },
    }
}

fn pick_candidate(sig: &MeetingSignals) -> Option<(MeetingApp, i32)> {
    if let Some((app, pid)) = sig.attributed {
        if sig.enabled(app) && sig.running.iter().any(|(a, p)| *a == app && *p == pid) {
            return Some((app, pid));
        }
        if sig.enabled(app) {
            if let Some((_, pid)) = sig.running.iter().copied().find(|(a, _)| *a == app) {
                return Some((app, pid));
            }
        }
    }
    let rank = |app: MeetingApp| match app {
        MeetingApp::Zoom => 0,
        MeetingApp::Teams => 1,
        MeetingApp::Meet => 2,
    };
    sig.running
        .iter()
        .copied()
        .filter(|(app, _)| sig.enabled(*app))
        .min_by_key(|(app, _)| rank(*app))
}

/// Locale-aware Zoom "Meeting" menu titles (plus a few structural aliases).
pub fn title_is_meeting_menu(title: &str) -> bool {
    let t = normalize_title(title);
    matches!(
        t.as_str(),
        "meeting"
            | "reunion"
            | "réunion"
            | "reunión"
            | "besprechung"
            | "reunião"
            | "riunione"
            | "vergadering"
            | "møde"
            | "møte"
            | "mote"
            | "möte"
            | "kokous"
            | "toplantı"
            | "ミーティング"
            | "会议"
            | "會議"
            | "회의"
    ) || t.contains("meeting")
}

/// `Some(true)` = currently muted (menu offers Unmute). Unmute is checked
/// first because it contains "mute".
pub fn mute_title_state(title: &str) -> Option<bool> {
    let t = normalize_title(title);
    if is_unmute_title(&t) {
        return Some(true);
    }
    if is_mute_title(&t) {
        return Some(false);
    }
    None
}

pub fn title_is_leave(title: &str) -> bool {
    let t = normalize_title(title);
    t.contains("leave")
        || t.contains("end meeting")
        || t.contains("end the meeting")
        || t.contains("verlassen")
        || t.contains("beenden")
        || t.contains("quitter")
        || t.contains("salir")
        || t.contains("terminar")
        || t.contains("hang up")
        || t.contains("hangup")
        || t == "end"
        || t.contains("退出")
        || t.contains("離開")
        || t.contains("离开")
        || t.contains("終了")
}

fn is_unmute_title(t: &str) -> bool {
    t.contains("unmute")
        || t.contains("un-mute")
        || t.contains("réactiver")
        || t.contains("reactiver")
        || t.contains("aktivieren")
        || t.contains("stummschaltung aufheben")
        || t.contains("activar")
        || t.contains("ativar")
        || t.contains("riattiva")
        || t.contains("hef dempen")
        || t.contains("slå på")
        || t.contains("ミュート解除")
        || t.contains("取消静音")
        || t.contains("取消靜音")
        || t.contains("음소거 해제")
}

fn is_mute_title(t: &str) -> bool {
    t.contains("mute audio")
        || t.contains("mute microphone")
        || t.contains("mute mic")
        || t == "mute"
        || t.starts_with("mute ")
        || t.contains("stummschalten")
        || t.contains("couper le")
        || t.contains("silenciar")
        || t.contains("disattiva")
        || t.contains("dempen")
        || t.contains("slå av")
        || t.contains("ミュート")
        || t.contains("静音")
        || t.contains("靜音")
        || t.contains("음소거")
}

fn normalize_title(title: &str) -> String {
    title
        .trim()
        .trim_end_matches('…')
        .trim_end_matches("...")
        .to_lowercase()
}

fn store() -> &'static RwLock<MeetingSnapshot> {
    static STORE: OnceLock<RwLock<MeetingSnapshot>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(MeetingSnapshot::default()))
}

static EVENT: AtomicBool = AtomicBool::new(false);
static GEN: AtomicU64 = AtomicU64::new(1);

pub fn snapshot() -> MeetingSnapshot {
    store()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn take_meeting_event() -> bool {
    EVENT.swap(false, Ordering::SeqCst)
}

pub fn note_meeting_event() {
    EVENT.store(true, Ordering::SeqCst);
    GEN.fetch_add(1, Ordering::Relaxed);
}

pub fn meeting_generation() -> u64 {
    GEN.load(Ordering::Relaxed)
}

fn publish(next: MeetingSnapshot) {
    let mut guard = store().write().unwrap_or_else(|e| e.into_inner());
    if *guard != next {
        *guard = next;
        drop(guard);
        note_meeting_event();
    }
}

#[cfg(target_os = "macos")]
fn settings_enabled() -> (bool, bool, bool, bool) {
    let s = crate::settings::get_app_settings();
    (
        s.show_meetings,
        s.meetings.zoom,
        s.meetings.teams,
        s.meetings.meet,
    )
}

/// Arm NSWorkspace + CoreAudio listeners. Cheap no-op off macOS and on a
/// second call.
pub fn start() {
    #[cfg(target_os = "macos")]
    macos::start();
}

pub fn accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::ax_trusted(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn prompt_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::ax_trusted(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Re-read hardware + confirm a meeting. Safe to call from a background
/// worker; the CoreAudio callback only flips atomics.
pub fn refresh() -> MeetingSnapshot {
    #[cfg(target_os = "macos")]
    {
        macos::refresh()
    }
    #[cfg(not(target_os = "macos"))]
    {
        snapshot()
    }
}

/// Zoom menu-title mute readback. Call only while the meeting face is shown.
pub fn read_zoom_mute() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        macos::read_zoom_mute()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn apply_zoom_mute(muted: Option<bool>) {
    let mut snap = snapshot();
    if let MeetingState::InMeeting {
        app: MeetingApp::Zoom,
        muted: slot,
        ..
    } = &mut snap.state
    {
        if *slot != muted {
            *slot = muted;
            publish(snap);
        }
    }
}

pub fn toggle_mute() {
    let snap = snapshot();
    let Some(app) = snap.app() else {
        return;
    };
    let Some(pid) = snap.state.pid() else {
        return;
    };
    #[cfg(target_os = "macos")]
    macos::toggle_mute(app, pid, snap.accessibility_trusted);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, pid);
    }
}

pub fn leave_meeting() {
    let snap = snapshot();
    let Some(app) = snap.app() else {
        return;
    };
    let Some(pid) = snap.state.pid() else {
        return;
    };
    #[cfg(target_os = "macos")]
    macos::leave_meeting(app, pid, snap.accessibility_trusted);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, pid);
    }
}

pub fn post_keys_to_pid(pid: i32, keycode: u16, flags: u64) {
    #[cfg(target_os = "macos")]
    macos::post_keys_to_pid(pid, keycode, flags);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, keycode, flags);
    }
}

pub fn open_accessibility_settings() {
    let _ = std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

#[cfg(target_os = "macos")]
fn meet_mode() -> MeetControlMode {
    crate::settings::get_app_settings().meetings.meet_mode
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use crate::browser_media;
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::{c_void, CStr};
    use std::ptr;

    const SYS: u32 = 1;
    const SCOPE_GLOBAL: u32 = 0x676C_6F62;
    const ELEMENT_MAIN: u32 = 0;
    const DEFAULT_INPUT: u32 = 0x6449_6E20;
    const RUNNING_SOMEWHERE: u32 = 0x676F_6E65;
    const PROCESS_LIST: u32 = 0x7072_7323;
    const PROC_PID: u32 = 0x7070_6964;
    const PROC_BUNDLE: u32 = 0x7062_6964;
    const PROC_RUNNING_INPUT: u32 = 0x7069_7269;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    impl AudioObjectPropertyAddress {
        fn new(selector: u32) -> Self {
            Self {
                selector,
                scope: SCOPE_GLOBAL,
                element: ELEMENT_MAIN,
            }
        }
    }

    type AudioObjectID = u32;
    type OSStatus = i32;
    type AudioListener = Option<
        unsafe extern "C" fn(
            AudioObjectID,
            u32,
            *const AudioObjectPropertyAddress,
            *mut c_void,
        ) -> OSStatus,
    >;

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(
            object: AudioObjectID,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
        ) -> OSStatus;
        fn AudioObjectGetPropertyData(
            object: AudioObjectID,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> OSStatus;
        fn AudioObjectAddPropertyListener(
            object: AudioObjectID,
            address: *const AudioObjectPropertyAddress,
            listener: AudioListener,
            client: *mut c_void,
        ) -> OSStatus;
        fn AudioObjectRemovePropertyListener(
            object: AudioObjectID,
            address: *const AudioObjectPropertyAddress,
            listener: AudioListener,
            client: *mut c_void,
        ) -> OSStatus;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
        fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
        fn AXUIElementCopyAttributeValue(
            element: *mut c_void,
            attribute: *const c_void,
            value: *mut *const c_void,
        ) -> i32;
        fn AXUIElementPerformAction(element: *mut c_void, action: *const c_void) -> i32;
        static kAXTrustedCheckOptionPrompt: *const c_void;
        static kAXMenuBarAttribute: *const c_void;
        static kAXChildrenAttribute: *const c_void;
        static kAXTitleAttribute: *const c_void;
        static kAXRoleAttribute: *const c_void;
        static kAXWindowsAttribute: *const c_void;
        static kAXPressAction: *const c_void;
        static kAXIdentifierAttribute: *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFArrayGetTypeID() -> usize;
        fn CFStringGetLength(s: *const c_void) -> isize;
        fn CFStringGetCString(
            s: *const c_void,
            buf: *mut i8,
            size: isize,
            encoding: u32,
        ) -> bool;
        fn CFArrayGetCount(arr: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(arr: *const c_void, idx: isize) -> *const c_void;
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            n: isize,
            key_cb: *const c_void,
            value_cb: *const c_void,
        ) -> *const c_void;
        static kCFBooleanTrue: *const c_void;
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreateKeyboardEvent(
            source: *mut c_void,
            virtual_key: u16,
            key_down: bool,
        ) -> *mut c_void;
        fn CGEventSetFlags(event: *mut c_void, flags: u64);
        fn CGEventPostToPid(pid: i32, event: *mut c_void);
    }

    const UTF8: u32 = 0x0800_0100;
    static STARTED: Once = Once::new();
    static INPUT_DEVICE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    static MIC_LIVE: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" fn on_audio(
        _id: AudioObjectID,
        _n: u32,
        _addrs: *const AudioObjectPropertyAddress,
        _client: *mut c_void,
    ) -> OSStatus {
        rearm_input_listener();
        MIC_LIVE.store(input_running_somewhere(), Ordering::Relaxed);
        super::note_meeting_event();
        0
    }

    pub fn start() {
        STARTED.call_once(|| {
            install_workspace_observers();
            install_audio_listeners();
            MIC_LIVE.store(input_running_somewhere(), Ordering::Relaxed);
            super::note_meeting_event();
        });
    }

    fn install_audio_listeners() {
        unsafe {
            let default_addr = AudioObjectPropertyAddress::new(DEFAULT_INPUT);
            let _ = AudioObjectAddPropertyListener(SYS, &default_addr, Some(on_audio), ptr::null_mut());
            let list_addr = AudioObjectPropertyAddress::new(PROCESS_LIST);
            let _ = AudioObjectAddPropertyListener(SYS, &list_addr, Some(on_audio), ptr::null_mut());
            rearm_input_listener();
        }
    }

    fn rearm_input_listener() {
        unsafe {
            let new_id = default_input_device();
            let old = INPUT_DEVICE.swap(new_id, Ordering::Relaxed);
            let addr = AudioObjectPropertyAddress::new(RUNNING_SOMEWHERE);
            if old != 0 && old != new_id {
                let _ = AudioObjectRemovePropertyListener(
                    old,
                    &addr,
                    Some(on_audio),
                    ptr::null_mut(),
                );
            }
            if new_id != 0 && new_id != old {
                let _ = AudioObjectAddPropertyListener(
                    new_id,
                    &addr,
                    Some(on_audio),
                    ptr::null_mut(),
                );
            }
        }
    }

    fn default_input_device() -> AudioObjectID {
        unsafe {
            let addr = AudioObjectPropertyAddress::new(DEFAULT_INPUT);
            let mut id: AudioObjectID = 0;
            let mut size = std::mem::size_of::<AudioObjectID>() as u32;
            if AudioObjectGetPropertyData(
                SYS,
                &addr,
                0,
                ptr::null(),
                &mut size,
                &mut id as *mut _ as *mut c_void,
            ) == 0
            {
                id
            } else {
                0
            }
        }
    }

    fn input_running_somewhere() -> bool {
        let id = default_input_device();
        if id == 0 {
            return false;
        }
        unsafe {
            let addr = AudioObjectPropertyAddress::new(RUNNING_SOMEWHERE);
            let mut value: u32 = 0;
            let mut size = std::mem::size_of::<u32>() as u32;
            if AudioObjectGetPropertyData(
                id,
                &addr,
                0,
                ptr::null(),
                &mut size,
                &mut value as *mut _ as *mut c_void,
            ) != 0
            {
                return false;
            }
            value != 0
        }
    }

    fn process_object_list() -> Vec<AudioObjectID> {
        unsafe {
            let addr = AudioObjectPropertyAddress::new(PROCESS_LIST);
            let mut size = 0u32;
            if AudioObjectGetPropertyDataSize(SYS, &addr, 0, ptr::null(), &mut size) != 0
                || size == 0
            {
                return Vec::new();
            }
            let count = size as usize / std::mem::size_of::<AudioObjectID>();
            let mut ids = vec![0u32; count];
            if AudioObjectGetPropertyData(
                SYS,
                &addr,
                0,
                ptr::null(),
                &mut size,
                ids.as_mut_ptr() as *mut c_void,
            ) != 0
            {
                return Vec::new();
            }
            ids.into_iter().filter(|id| *id != 0).collect()
        }
    }

    fn process_uint(id: AudioObjectID, selector: u32) -> Option<u32> {
        unsafe {
            let addr = AudioObjectPropertyAddress::new(selector);
            let mut value = 0u32;
            let mut size = std::mem::size_of::<u32>() as u32;
            if AudioObjectGetPropertyData(
                id,
                &addr,
                0,
                ptr::null(),
                &mut size,
                &mut value as *mut _ as *mut c_void,
            ) == 0
            {
                Some(value)
            } else {
                None
            }
        }
    }

    fn process_pid(id: AudioObjectID) -> Option<i32> {
        unsafe {
            let addr = AudioObjectPropertyAddress::new(PROC_PID);
            let mut value: i32 = 0;
            let mut size = std::mem::size_of::<i32>() as u32;
            if AudioObjectGetPropertyData(
                id,
                &addr,
                0,
                ptr::null(),
                &mut size,
                &mut value as *mut _ as *mut c_void,
            ) == 0
                && value > 0
            {
                Some(value)
            } else {
                None
            }
        }
    }

    fn process_bundle(id: AudioObjectID) -> Option<String> {
        unsafe {
            let addr = AudioObjectPropertyAddress::new(PROC_BUNDLE);
            let mut cf: *const c_void = ptr::null();
            let mut size = std::mem::size_of::<*const c_void>() as u32;
            if AudioObjectGetPropertyData(
                id,
                &addr,
                0,
                ptr::null(),
                &mut size,
                &mut cf as *mut _ as *mut c_void,
            ) != 0
                || cf.is_null()
            {
                return None;
            }
            let s = cf_string(cf);
            CFRelease(cf);
            s.filter(|v| !v.is_empty())
        }
    }

    fn attribute_mic() -> Option<(MeetingApp, i32)> {
        for id in process_object_list() {
            if process_uint(id, PROC_RUNNING_INPUT).unwrap_or(0) == 0 {
                continue;
            }
            let pid = process_pid(id).unwrap_or(0);
            if let Some(bundle) = process_bundle(id) {
                if let Some(app) = MeetingApp::from_bundle(&bundle) {
                    return Some((app, pid));
                }
            }
        }
        None
    }

    fn running_meeting_apps() -> Vec<(MeetingApp, i32)> {
        objc2::rc::autoreleasepool(|_| unsafe {
            let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            if ws.is_null() {
                return Vec::new();
            }
            let apps: *mut AnyObject = msg_send![ws, runningApplications];
            if apps.is_null() {
                return Vec::new();
            }
            let n: usize = msg_send![apps, count];
            let mut out = Vec::new();
            for i in 0..n {
                let app: *mut AnyObject = msg_send![apps, objectAtIndex: i];
                if app.is_null() {
                    continue;
                }
                let bundle_ns: *mut AnyObject = msg_send![app, bundleIdentifier];
                if bundle_ns.is_null() {
                    continue;
                }
                let cstr: *const i8 = msg_send![bundle_ns, UTF8String];
                if cstr.is_null() {
                    continue;
                }
                let id = CStr::from_ptr(cstr).to_string_lossy();
                if let Some(kind) = MeetingApp::from_bundle(&id) {
                    let pid: i32 = msg_send![app, processIdentifier];
                    if pid > 0 {
                        out.push((kind, pid));
                    }
                }
            }
            out
        })
    }

    fn install_workspace_observers() {
        unsafe {
            let center: *mut AnyObject = {
                let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
                if ws.is_null() {
                    return;
                }
                msg_send![ws, notificationCenter]
            };
            if center.is_null() {
                return;
            }
            for name in [
                c"NSWorkspaceDidLaunchApplicationNotification",
                c"NSWorkspaceDidTerminateApplicationNotification",
            ] {
                let ns_name: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
                let block = block2::RcBlock::new(move |note: *mut AnyObject| {
                    if !note.is_null() {
                        let info: *mut AnyObject = msg_send![note, userInfo];
                        if !info.is_null() {
                            let key: *mut AnyObject = msg_send![
                                class!(NSString),
                                stringWithUTF8String: c"NSWorkspaceApplicationKey".as_ptr()
                            ];
                            let app: *mut AnyObject = msg_send![info, objectForKey: key];
                            if !app.is_null() {
                                let bundle_ns: *mut AnyObject = msg_send![app, bundleIdentifier];
                                if !bundle_ns.is_null() {
                                    let cstr: *const i8 = msg_send![bundle_ns, UTF8String];
                                    if !cstr.is_null() {
                                        let id = CStr::from_ptr(cstr).to_string_lossy();
                                        if MeetingApp::from_bundle(&id).is_none() {
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    super::note_meeting_event();
                });
                let token: *mut AnyObject = msg_send![
                    center,
                    addObserverForName: ns_name,
                    object: ptr::null_mut::<AnyObject>(),
                    queue: ptr::null_mut::<AnyObject>(),
                    usingBlock: &*block
                ];
                std::mem::forget(block);
                let _ = token;
            }
        }
    }

    pub fn ax_trusted(prompt: bool) -> bool {
        unsafe {
            if !prompt {
                return AXIsProcessTrustedWithOptions(ptr::null());
            }
            let key = kAXTrustedCheckOptionPrompt;
            let val = kCFBooleanTrue;
            let dict = CFDictionaryCreate(
                ptr::null(),
                &key as *const *const c_void,
                &val as *const *const c_void,
                1,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            let ok = AXIsProcessTrustedWithOptions(dict);
            if !dict.is_null() {
                CFRelease(dict);
            }
            ok
        }
    }

    fn cf_string(cf: *const c_void) -> Option<String> {
        unsafe {
            if cf.is_null() || CFGetTypeID(cf) != CFStringGetTypeID() {
                return None;
            }
            let len = CFStringGetLength(cf);
            if len < 0 || len > 8 * 1024 {
                return None;
            }
            let mut buf = vec![0i8; (len as usize) * 4 + 1];
            if !CFStringGetCString(cf, buf.as_mut_ptr(), buf.len() as isize, UTF8) {
                return None;
            }
            CStr::from_ptr(buf.as_ptr())
                .to_str()
                .ok()
                .map(|s| s.to_string())
        }
    }

    fn ax_attr(el: *mut c_void, attr: *const c_void) -> *const c_void {
        unsafe {
            if el.is_null() {
                return ptr::null();
            }
            let mut out: *const c_void = ptr::null();
            if AXUIElementCopyAttributeValue(el, attr, &mut out) != 0 {
                return ptr::null();
            }
            out
        }
    }

    fn ax_title(el: *mut c_void) -> Option<String> {
        unsafe {
            let cf = ax_attr(el, kAXTitleAttribute);
            if cf.is_null() {
                return None;
            }
            let s = cf_string(cf);
            CFRelease(cf);
            s
        }
    }

    fn ax_role(el: *mut c_void) -> Option<String> {
        unsafe {
            let cf = ax_attr(el, kAXRoleAttribute);
            if cf.is_null() {
                return None;
            }
            let s = cf_string(cf);
            CFRelease(cf);
            s
        }
    }

    fn ax_identifier(el: *mut c_void) -> Option<String> {
        unsafe {
            let cf = ax_attr(el, kAXIdentifierAttribute);
            if cf.is_null() {
                return None;
            }
            let s = cf_string(cf);
            CFRelease(cf);
            s
        }
    }

    fn ax_children(el: *mut c_void) -> Vec<*mut c_void> {
        unsafe {
            let arr = ax_attr(el, kAXChildrenAttribute);
            if arr.is_null() {
                return Vec::new();
            }
            let kids = ax_array(arr);
            CFRelease(arr);
            kids
        }
    }

    fn ax_array(arr: *const c_void) -> Vec<*mut c_void> {
        unsafe {
            if arr.is_null() || CFGetTypeID(arr) != CFArrayGetTypeID() {
                return Vec::new();
            }
            let n = CFArrayGetCount(arr);
            if n < 0 || n > 256 {
                return Vec::new();
            }
            (0..n)
                .filter_map(|i| {
                    let v = CFArrayGetValueAtIndex(arr, i);
                    if v.is_null() {
                        None
                    } else {
                        Some(v as *mut c_void)
                    }
                })
                .collect()
        }
    }

    fn ax_press(el: *mut c_void) -> bool {
        unsafe {
            if el.is_null() {
                return false;
            }
            AXUIElementPerformAction(el, kAXPressAction) == 0
        }
    }

    fn zoom_app(pid: i32) -> *mut c_void {
        unsafe { AXUIElementCreateApplication(pid) }
    }

    fn find_meeting_menu(pid: i32) -> Option<*mut c_void> {
        unsafe {
            let app = zoom_app(pid);
            if app.is_null() {
                return None;
            }
            let bar = ax_attr(app, kAXMenuBarAttribute);
            CFRelease(app);
            if bar.is_null() {
                return None;
            }
            let menus = ax_children(bar as *mut c_void);
            CFRelease(bar);
            for menu_extra in menus {
                let title = ax_title(menu_extra).unwrap_or_default();
                let kids = ax_children(menu_extra);
                let menu = kids.first().copied();
                if title_is_meeting_menu(&title) {
                    return menu.or(Some(menu_extra));
                }
                if let Some(menu) = menu {
                    if ax_children(menu)
                        .iter()
                        .any(|item| mute_title_state(&ax_title(*item).unwrap_or_default()).is_some())
                    {
                        return Some(menu);
                    }
                }
            }
            None
        }
    }

    fn zoom_mute_from_menu(pid: i32) -> Option<bool> {
        let menu = find_meeting_menu(pid)?;
        for item in ax_children(menu) {
            if let Some(state) = mute_title_state(&ax_title(item).unwrap_or_default()) {
                return Some(state);
            }
        }
        None
    }

    fn zoom_in_meeting(pid: i32) -> Option<bool> {
        if find_meeting_menu(pid).is_none() {
            return None;
        }
        Some(zoom_mute_from_menu(pid).unwrap_or(false))
    }

    fn press_zoom_mute(pid: i32) -> bool {
        let Some(menu) = find_meeting_menu(pid) else {
            return false;
        };
        for item in ax_children(menu) {
            if mute_title_state(&ax_title(item).unwrap_or_default()).is_some() {
                return ax_press(item);
            }
        }
        false
    }

    fn press_zoom_leave_sheet(pid: i32) -> bool {
        unsafe {
            let app = zoom_app(pid);
            if app.is_null() {
                return false;
            }
            let windows = ax_attr(app, kAXWindowsAttribute);
            CFRelease(app);
            if windows.is_null() {
                return false;
            }
            let list = ax_array(windows);
            CFRelease(windows);
            for win in list {
                if press_leave_in(win, 0) {
                    return true;
                }
            }
            false
        }
    }

    fn press_leave_in(el: *mut c_void, depth: u8) -> bool {
        if el.is_null() || depth > 6 {
            return false;
        }
        let title = ax_title(el).unwrap_or_default();
        let role = ax_role(el).unwrap_or_default();
        let ident = ax_identifier(el).unwrap_or_default();
        if (role.contains("Button") || role.contains("button"))
            && (title_is_leave(&title) || ident.to_ascii_lowercase().contains("leave"))
        {
            return ax_press(el);
        }
        for child in ax_children(el) {
            if press_leave_in(child, depth + 1) {
                return true;
            }
        }
        false
    }

    pub fn post_keys_to_pid(pid: i32, keycode: u16, flags: u64) {
        if pid <= 0 {
            return;
        }
        unsafe {
            for down in [true, false] {
                let ev = CGEventCreateKeyboardEvent(ptr::null_mut(), keycode, down);
                if ev.is_null() {
                    continue;
                }
                CGEventSetFlags(ev, flags);
                CGEventPostToPid(pid, ev);
                CFRelease(ev);
            }
        }
    }

    pub fn read_zoom_mute() -> Option<bool> {
        let snap = super::snapshot();
        match snap.state {
            MeetingState::InMeeting {
                app: MeetingApp::Zoom,
                pid,
                ..
            } if snap.accessibility_trusted => zoom_mute_from_menu(pid),
            _ => None,
        }
    }

    pub fn refresh() -> MeetingSnapshot {
        let (master, zoom_on, teams_on, meet_on) = super::settings_enabled();
        let trusted = ax_trusted(false);
        if !master {
            let snap = MeetingSnapshot {
                state: MeetingState::Idle,
                accessibility_trusted: trusted,
            };
            super::publish(snap.clone());
            return snap;
        }
        let running = running_meeting_apps();
        let mic_live = MIC_LIVE.load(Ordering::Relaxed) || input_running_somewhere();
        MIC_LIVE.store(mic_live, Ordering::Relaxed);
        let attributed = if mic_live { attribute_mic() } else { None };
        let mut sig = MeetingSignals {
            running,
            mic_live,
            attributed,
            zoom_confirmed: None,
            meet_tab: false,
            enabled_zoom: zoom_on,
            enabled_teams: teams_on,
            enabled_meet: meet_on,
        };
        if let Some((app, pid)) = pick_candidate(&sig) {
            if mic_live {
                match app {
                    MeetingApp::Zoom if trusted => {
                        sig.zoom_confirmed = zoom_in_meeting(pid);
                    }
                    MeetingApp::Meet => {
                        sig.meet_tab = browser_media::find_meet_tab_blocking().is_some();
                    }
                    _ => {}
                }
            }
        }
        let prev = super::snapshot();
        let state = next_state(&prev.state, &sig, Instant::now());
        let snap = MeetingSnapshot {
            state,
            accessibility_trusted: trusted,
        };
        super::publish(snap.clone());
        snap
    }

    pub fn toggle_mute(app: MeetingApp, pid: i32, trusted: bool) {
        match app {
            MeetingApp::Zoom => {
                if trusted && press_zoom_mute(pid) {
                    return;
                }
                zoom_osascript_mute();
            }
            MeetingApp::Teams => {
                if trusted {
                    post_keys_to_pid(pid, KEY_M, FLAG_CMD | FLAG_SHIFT);
                }
            }
            MeetingApp::Meet => match super::meet_mode() {
                MeetControlMode::AppleEventsJs => {
                    if browser_media::meet_click_mute_js().is_none() {
                        meet_focus_hotkey(pid, trusted, KEY_D);
                    }
                }
                MeetControlMode::FocusTab => meet_focus_hotkey(pid, trusted, KEY_D),
            },
        }
        super::note_meeting_event();
    }

    pub fn leave_meeting(app: MeetingApp, pid: i32, trusted: bool) {
        match app {
            MeetingApp::Zoom => {
                if trusted {
                    post_keys_to_pid(pid, KEY_W, FLAG_CMD);
                    std::thread::sleep(std::time::Duration::from_millis(280));
                    let _ = press_zoom_leave_sheet(pid);
                } else {
                    zoom_osascript_leave();
                }
            }
            MeetingApp::Teams => {
                if trusted {
                    post_keys_to_pid(pid, KEY_H, FLAG_CMD | FLAG_SHIFT);
                }
            }
            MeetingApp::Meet => match super::meet_mode() {
                MeetControlMode::AppleEventsJs => {
                    if !browser_media::meet_click_leave_js() {
                        meet_focus_hotkey(pid, trusted, KEY_E);
                    }
                }
                MeetControlMode::FocusTab => meet_focus_hotkey(pid, trusted, KEY_E),
            },
        }
        super::note_meeting_event();
    }

    fn meet_focus_hotkey(pid: i32, trusted: bool, key: u16) {
        let _ = browser_media::activate_meet_tab_blocking();
        if trusted {
            std::thread::sleep(std::time::Duration::from_millis(120));
            post_keys_to_pid(pid, key, FLAG_CMD);
        } else {
            let letter = if key == KEY_D { "d" } else { "e" };
            system_events_keystroke(letter, true, false);
        }
    }

    fn zoom_osascript_mute() {
        let script = r#"tell application "System Events"
  tell process "zoom.us"
    try
      click menu item 1 of menu "Meeting" of menu bar 1
    end try
  end tell
end tell"#;
        let _ = browser_media::run_osascript_blocking(script);
    }

    fn zoom_osascript_leave() {
        let script = r#"tell application "System Events"
  tell process "zoom.us"
    keystroke "w" using command down
  end tell
end tell"#;
        let _ = browser_media::run_osascript_blocking(script);
    }

    fn system_events_keystroke(key: &str, cmd: bool, shift: bool) {
        let mods = match (cmd, shift) {
            (true, true) => "command down, shift down",
            (true, false) => "command down",
            (false, true) => "shift down",
            (false, false) => "",
        };
        let using = if mods.is_empty() {
            String::new()
        } else {
            format!(" using {mods}")
        };
        let script = format!(
            r#"tell application "System Events" to keystroke "{key}"{using}"#
        );
        let _ = browser_media::run_osascript_blocking(&script);
    }

    #[allow(dead_code)]
    static _KEEP_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sig(running: MeetingApp, pid: i32) -> MeetingSignals {
        MeetingSignals {
            running: vec![(running, pid)],
            mic_live: false,
            attributed: None,
            zoom_confirmed: None,
            meet_tab: false,
            enabled_zoom: true,
            enabled_teams: true,
            enabled_meet: true,
        }
    }

    #[test]
    fn idle_without_apps() {
        let now = Instant::now();
        assert_eq!(
            next_state(&MeetingState::Idle, &MeetingSignals::default(), now),
            MeetingState::Idle
        );
    }

    #[test]
    fn app_running_then_mic_then_zoom_confirm() {
        let now = Instant::now();
        let mut s = sig(MeetingApp::Zoom, 9);
        let running = next_state(&MeetingState::Idle, &s, now);
        assert_eq!(
            running,
            MeetingState::AppRunning {
                app: MeetingApp::Zoom,
                pid: 9
            }
        );
        s.mic_live = true;
        s.attributed = Some((MeetingApp::Zoom, 9));
        let live = next_state(&running, &s, now);
        assert_eq!(
            live,
            MeetingState::MicLive {
                app: MeetingApp::Zoom,
                pid: 9
            }
        );
        s.zoom_confirmed = Some(true);
        let meet = next_state(&live, &s, now + Duration::from_secs(2));
        assert!(matches!(
            meet,
            MeetingState::InMeeting {
                app: MeetingApp::Zoom,
                muted: Some(true),
                ..
            }
        ));
    }

    #[test]
    fn teams_is_app_plus_mic_unverified() {
        let now = Instant::now();
        let mut s = sig(MeetingApp::Teams, 4);
        s.mic_live = true;
        s.attributed = Some((MeetingApp::Teams, 4));
        let state = next_state(&MeetingState::Idle, &s, now);
        assert!(matches!(
            state,
            MeetingState::InMeeting {
                app: MeetingApp::Teams,
                muted: None,
                ..
            }
        ));
    }

    #[test]
    fn meet_needs_tab_url() {
        let now = Instant::now();
        let mut s = sig(MeetingApp::Meet, 11);
        s.mic_live = true;
        s.attributed = Some((MeetingApp::Meet, 11));
        assert!(matches!(
            next_state(&MeetingState::Idle, &s, now),
            MeetingState::MicLive {
                app: MeetingApp::Meet,
                ..
            }
        ));
        s.meet_tab = true;
        assert!(matches!(
            next_state(&MeetingState::Idle, &s, now),
            MeetingState::InMeeting {
                app: MeetingApp::Meet,
                muted: None,
                ..
            }
        ));
    }

    #[test]
    fn disabled_app_is_ignored() {
        let now = Instant::now();
        let mut s = sig(MeetingApp::Zoom, 1);
        s.enabled_zoom = false;
        s.mic_live = true;
        assert_eq!(next_state(&MeetingState::Idle, &s, now), MeetingState::Idle);
    }

    #[test]
    fn zoom_keeps_elapsed_across_mute_flips() {
        let t0 = Instant::now();
        let mut s = sig(MeetingApp::Zoom, 3);
        s.mic_live = true;
        s.zoom_confirmed = Some(false);
        let first = next_state(&MeetingState::Idle, &s, t0);
        s.zoom_confirmed = Some(true);
        let second = next_state(&first, &s, t0 + Duration::from_secs(40));
        match (first, second) {
            (
                MeetingState::InMeeting { started: a, .. },
                MeetingState::InMeeting {
                    started: b,
                    muted: Some(true),
                    ..
                },
            ) => assert_eq!(a, b),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn mute_titles_prefer_unmute() {
        assert_eq!(mute_title_state("Unmute audio"), Some(true));
        assert_eq!(mute_title_state("Mute audio"), Some(false));
        assert_eq!(mute_title_state("Stummschaltung aufheben"), Some(true));
        assert_eq!(mute_title_state("Couper le micro"), Some(false));
        assert_eq!(mute_title_state("ミュート解除"), Some(true));
        assert_eq!(mute_title_state("View"), None);
    }

    #[test]
    fn meeting_menu_locales() {
        assert!(title_is_meeting_menu("Meeting"));
        assert!(title_is_meeting_menu("Réunion"));
        assert!(title_is_meeting_menu("ミーティング"));
        assert!(!title_is_meeting_menu("View"));
        assert!(title_is_leave("Leave Meeting"));
        assert!(title_is_leave("End"));
        assert!(!title_is_leave("Cancel"));
    }

    #[test]
    fn bundle_ids_map() {
        assert_eq!(MeetingApp::from_bundle(ZOOM_BUNDLE), Some(MeetingApp::Zoom));
        assert_eq!(
            MeetingApp::from_bundle(TEAMS_BUNDLE),
            Some(MeetingApp::Teams)
        );
        assert_eq!(
            MeetingApp::from_bundle("com.google.Chrome"),
            Some(MeetingApp::Meet)
        );
        assert_eq!(
            MeetingApp::from_bundle("com.google.Chrome.helper"),
            Some(MeetingApp::Meet)
        );
        assert_eq!(MeetingApp::from_bundle("com.spotify.client"), None);
    }

    #[test]
    fn snapshot_defaults_idle() {
        let snap = MeetingSnapshot::default();
        assert!(!snap.in_meeting());
        assert!(!snap.mute_verified());
        assert_eq!(snap.elapsed_secs(), 0);
    }
}
