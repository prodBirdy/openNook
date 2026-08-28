//! Battery state and Low Power Mode.
//!
//! Event-driven on macOS: Darwin `notify_register_dispatch` for IOKit power
//! sources plus `NSProcessInfoPowerStateDidChangeNotification` for LPM. No
//! polling loop. Desktop Macs (empty power-source list) hide the gauge and
//! still expose the LPM toggle.

use std::sync::OnceLock;
use tokio::sync::watch;

/// IOKit / `IOPSGetBatteryWarningLevel` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatteryWarning {
    #[default]
    None,
    Early,
    Final,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerSnapshot {
    pub percent: Option<u8>,
    pub is_charging: bool,
    pub on_ac: bool,
    pub time_to_empty_min: Option<u32>,
    pub warning_level: BatteryWarning,
    pub low_power_mode: bool,
    pub has_battery: bool,
}

impl Default for PowerSnapshot {
    fn default() -> Self {
        Self {
            percent: None,
            is_charging: false,
            on_ac: true,
            time_to_empty_min: None,
            warning_level: BatteryWarning::None,
            low_power_mode: false,
            has_battery: false,
        }
    }
}

impl PowerSnapshot {
    /// Compact-face takeover: discharging at or below the user threshold, or
    /// the OS early/final low-battery warning. Charging (or AC with no
    /// internal battery) clears the alert. Desktop Macs never alert.
    pub fn is_alerting(self, threshold: u8) -> bool {
        if !self.has_battery {
            return false;
        }
        if self.is_charging || self.on_ac {
            return false;
        }
        let below = self
            .percent
            .map(|percent| percent <= threshold)
            .unwrap_or(false);
        below || self.warning_level != BatteryWarning::None
    }

    /// `kIOPSNotifyTimeRemaining` is chatty. Subscribe only on the expanded
    /// battery card or while discharging below ~30%.
    pub fn should_watch_time_remaining(self, detail: bool) -> bool {
        if detail {
            return true;
        }
        self.has_battery
            && !self.is_charging
            && !self.on_ac
            && self.percent.map(|percent| percent <= 30).unwrap_or(false)
    }

    pub fn compact_icon(self) -> &'static str {
        if self.is_charging {
            "battery-charging"
        } else {
            "battery-low"
        }
    }
}

/// How the next LPM write will be issued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LpmRoute {
    Shortcut(String),
    OsascriptAdmin,
}

/// Prefer a named Shortcuts action when `shortcuts list` contains it.
pub fn lpm_route(shortcut_name: Option<&str>, listed: &[String]) -> LpmRoute {
    if let Some(name) = shortcut_name.map(str::trim).filter(|name| !name.is_empty()) {
        if listed.iter().any(|entry| entry == name) {
            return LpmRoute::Shortcut(name.to_string());
        }
    }
    LpmRoute::OsascriptAdmin
}

pub fn default_lpm_shortcut_name() -> &'static str {
    "Toggle Low Power Mode"
}

pub fn clamp_alert_threshold(value: u8) -> u8 {
    value.clamp(5, 80)
}

pub fn format_time_remaining(minutes: Option<u32>) -> String {
    match minutes {
        None => "—".into(),
        Some(0) => "Calculating…".into(),
        Some(m) if m < 60 => format!("{m}m left"),
        Some(m) => format!("{}h {:02}m left", m / 60, m % 60),
    }
}

pub fn format_percent(percent: Option<u8>) -> String {
    match percent {
        Some(value) => format!("{value}%"),
        None => "—".into(),
    }
}

fn channel() -> &'static watch::Sender<PowerSnapshot> {
    static TX: OnceLock<watch::Sender<PowerSnapshot>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, _rx) = watch::channel(PowerSnapshot::default());
        tx
    })
}

/// Latest snapshot. Cheap; the island tick should not poll this.
pub fn current() -> PowerSnapshot {
    *channel().borrow()
}

pub fn subscribe() -> watch::Receiver<PowerSnapshot> {
    channel().subscribe()
}

/// Register Darwin / NSProcessInfo observers. Idempotent, no-op off macOS.
pub fn start() {
    #[cfg(target_os = "macos")]
    macos::start();
}

/// Keep per-percent `timeremaining` notifications only while the battery card
/// is on screen (or while [`PowerSnapshot::should_watch_time_remaining`]
/// decides we are critically discharging).
pub fn set_detail_watch(on: bool) {
    #[cfg(target_os = "macos")]
    macos::set_detail_watch(on);
    #[cfg(not(target_os = "macos"))]
    let _ = on;
}

/// Shortcuts, then osascript-admin. Success is the NSProcessInfo LPM flag
/// flipping within ~3 s, not the command exit code.
pub async fn toggle_low_power_mode() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        macos::toggle_low_power_mode().await
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Low Power Mode is only available on macOS".into())
    }
}

/// Write the bundled shortcut to a temp file and `/usr/bin/open` it so the
/// user can confirm the import in Shortcuts.app (cannot be silent).
pub fn install_lpm_shortcut() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::install_lpm_shortcut()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Shortcuts are only available on macOS".into())
    }
}

pub async fn list_shortcuts() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        macos::list_shortcuts().await
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_int, c_void};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
    use std::sync::Once;
    use std::time::Duration;

    const NOTIFY_STATUS_OK: u32 = 0;
    const K_CFSTRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_CF_NUMBER_SINT32_TYPE: i32 = 3;
    const K_CF_NUMBER_DOUBLE_TYPE: i32 = 13;
    const K_IOPS_TIME_REMAINING_UNKNOWN: f64 = -1.0;
    const K_IOPS_TIME_REMAINING_UNLIMITED: f64 = -2.0;
    const K_IOPS_WARNING_NONE: i32 = 1;
    const K_IOPS_WARNING_EARLY: i32 = 2;
    const K_IOPS_WARNING_FINAL: i32 = 3;
    const DEBOUNCE_MS: u64 = 250;
    const LPM_CONFIRM_SECS: u64 = 3;

    const NOTIFY_SOURCE: &CStr = c"com.apple.system.powersources.source";
    const NOTIFY_LOW: &CStr = c"com.apple.system.powersources.lowbattery";
    const NOTIFY_TIME: &CStr = c"com.apple.system.powersources.timeremaining";

    type CfTypeRef = *const c_void;
    type CfStringRef = *const c_void;
    type CfArrayRef = *const c_void;
    type CfDictionaryRef = *const c_void;
    type CfNumberRef = *const c_void;
    type CfBooleanRef = *const c_void;
    type CfAllocatorRef = *const c_void;
    type DispatchQueue = *mut c_void;

    #[link(name = "IOKit", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> CfTypeRef;
        fn IOPSCopyPowerSourcesList(blob: CfTypeRef) -> CfArrayRef;
        fn IOPSGetPowerSourceDescription(blob: CfTypeRef, ps: CfTypeRef) -> CfDictionaryRef;
        fn IOPSGetBatteryWarningLevel() -> i32;
        fn IOPSGetTimeRemainingEstimate() -> f64;

        fn CFRelease(cf: CfTypeRef);
        fn CFGetTypeID(cf: CfTypeRef) -> usize;
        fn CFArrayGetCount(array: CfArrayRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CfArrayRef, idx: isize) -> CfTypeRef;
        fn CFDictionaryGetValue(dict: CfDictionaryRef, key: CfTypeRef) -> CfTypeRef;
        fn CFStringCreateWithCString(
            alloc: CfAllocatorRef,
            c_str: *const c_char,
            encoding: u32,
        ) -> CfStringRef;
        fn CFStringGetTypeID() -> usize;
        fn CFStringGetCString(
            string: CfStringRef,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
        fn CFNumberGetTypeID() -> usize;
        fn CFNumberGetValue(number: CfNumberRef, the_type: i32, value_ptr: *mut c_void) -> u8;
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(boolean: CfBooleanRef) -> u8;

        fn notify_register_dispatch(
            name: *const c_char,
            out_token: *mut c_int,
            queue: DispatchQueue,
            handler: *mut block2::Block<dyn Fn(c_int)>,
        ) -> u32;
        fn notify_cancel(token: c_int) -> u32;
        fn dispatch_get_global_queue(identifier: isize, flags: usize) -> DispatchQueue;
    }

    static STARTED: Once = Once::new();
    static DEBOUNCE_GEN: AtomicU64 = AtomicU64::new(0);
    static DETAIL_WATCH: AtomicBool = AtomicBool::new(false);
    static TIME_TOKEN: AtomicI32 = AtomicI32::new(-1);

    pub fn start() {
        STARTED.call_once(|| {
            register_notify(NOTIFY_SOURCE);
            register_notify(NOTIFY_LOW);
            install_lpm_observer();
            publish_now();
        });
    }

    pub fn set_detail_watch(on: bool) {
        DETAIL_WATCH.store(on, Ordering::Relaxed);
        let snap = super::current();
        set_time_remaining_subscription(snap.should_watch_time_remaining(on));
    }

    fn register_notify(name: &CStr) {
        unsafe {
            let queue = dispatch_get_global_queue(0, 0);
            if queue.is_null() {
                log::warn!("power: dispatch_get_global_queue failed");
                return;
            }
            let block = block2::RcBlock::new(move |_token: c_int| {
                schedule_publish();
            });
            let mut token: c_int = 0;
            let status = notify_register_dispatch(
                name.as_ptr(),
                &mut token,
                queue,
                &*block as *const block2::Block<_> as *mut block2::Block<_>,
            );
            std::mem::forget(block);
            if status != NOTIFY_STATUS_OK {
                log::warn!("power: notify_register_dispatch({name:?}) = {status}");
            }
        }
    }

    fn set_time_remaining_subscription(on: bool) {
        if on {
            if TIME_TOKEN.load(Ordering::Relaxed) >= 0 {
                return;
            }
            unsafe {
                let queue = dispatch_get_global_queue(0, 0);
                if queue.is_null() {
                    return;
                }
                let block = block2::RcBlock::new(move |_token: c_int| {
                    schedule_publish();
                });
                let mut token: c_int = 0;
                let status = notify_register_dispatch(
                    NOTIFY_TIME.as_ptr(),
                    &mut token,
                    queue,
                    &*block as *const block2::Block<_> as *mut block2::Block<_>,
                );
                std::mem::forget(block);
                if status == NOTIFY_STATUS_OK {
                    TIME_TOKEN.store(token, Ordering::Relaxed);
                }
            }
        } else {
            let token = TIME_TOKEN.swap(-1, Ordering::Relaxed);
            if token >= 0 {
                unsafe {
                    notify_cancel(token);
                }
            }
        }
    }

    fn schedule_publish() {
        let gen = DEBOUNCE_GEN.fetch_add(1, Ordering::Relaxed) + 1;
        crate::runtime().spawn(async move {
            tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;
            if DEBOUNCE_GEN.load(Ordering::Relaxed) == gen {
                publish_now();
            }
        });
    }

    fn publish_now() {
        let snap = read_snapshot();
        set_time_remaining_subscription(
            snap.should_watch_time_remaining(DETAIL_WATCH.load(Ordering::Relaxed)),
        );
        let _ = super::channel().send(snap);
    }

    fn install_lpm_observer() {
        unsafe {
            use objc2::runtime::AnyObject;
            use objc2::*;

            let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
            if center.is_null() {
                return;
            }
            let name: *mut AnyObject = msg_send![
                class!(NSString),
                stringWithUTF8String: c"NSProcessInfoPowerStateDidChangeNotification".as_ptr()
            ];
            let block = block2::RcBlock::new(move |_note: *mut AnyObject| {
                schedule_publish();
            });
            let _token: *mut AnyObject = msg_send![
                center,
                addObserverForName: name,
                object: ptr::null_mut::<AnyObject>(),
                queue: ptr::null_mut::<AnyObject>(),
                usingBlock: &*block
            ];
            std::mem::forget(block);
        }
    }

    fn read_lpm() -> bool {
        unsafe {
            use objc2::runtime::AnyObject;
            use objc2::*;
            let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
            if info.is_null() {
                return false;
            }
            let enabled: bool = msg_send![info, isLowPowerModeEnabled];
            enabled
        }
    }

    fn read_snapshot() -> PowerSnapshot {
        let mut snap = PowerSnapshot {
            low_power_mode: read_lpm(),
            ..PowerSnapshot::default()
        };
        snap.warning_level = match unsafe { IOPSGetBatteryWarningLevel() } {
            K_IOPS_WARNING_EARLY => BatteryWarning::Early,
            K_IOPS_WARNING_FINAL => BatteryWarning::Final,
            K_IOPS_WARNING_NONE | _ => BatteryWarning::None,
        };

        unsafe {
            let blob = IOPSCopyPowerSourcesInfo();
            if blob.is_null() {
                return snap;
            }
            let list = IOPSCopyPowerSourcesList(blob);
            if list.is_null() {
                CFRelease(blob);
                return snap;
            }
            let count = CFArrayGetCount(list);
            for i in 0..count {
                let ps = CFArrayGetValueAtIndex(list, i);
                if ps.is_null() {
                    continue;
                }
                let desc = IOPSGetPowerSourceDescription(blob, ps);
                if desc.is_null() {
                    continue;
                }
                let kind = cf_dict_string(desc, c"Type");
                if kind.as_deref() != Some("InternalBattery") {
                    continue;
                }
                if cf_dict_bool(desc, c"Is Present") == Some(false) {
                    continue;
                }
                snap.has_battery = true;
                let current = cf_dict_i32(desc, c"Current Capacity");
                let max = cf_dict_i32(desc, c"Max Capacity").filter(|value| *value > 0);
                snap.percent = match (current, max) {
                    (Some(cur), Some(max)) => Some(((cur.clamp(0, max) * 100) / max) as u8),
                    (Some(cur), None) if (0..=100).contains(&cur) => Some(cur as u8),
                    _ => None,
                };
                snap.is_charging = cf_dict_bool(desc, c"Is Charging").unwrap_or(false);
                let state = cf_dict_string(desc, c"Power Source State");
                snap.on_ac = state.as_deref() == Some("AC Power");
                if let Some(minutes) = cf_dict_i32(desc, c"Time to Empty").filter(|m| *m > 0) {
                    snap.time_to_empty_min = Some(minutes as u32);
                }
                break;
            }
            CFRelease(list);
            CFRelease(blob);
        }

        if snap.has_battery && !snap.on_ac {
            let estimate = unsafe { IOPSGetTimeRemainingEstimate() };
            if estimate > 0.0
                && estimate != K_IOPS_TIME_REMAINING_UNKNOWN
                && estimate != K_IOPS_TIME_REMAINING_UNLIMITED
            {
                snap.time_to_empty_min = Some((estimate / 60.0).round().max(0.0) as u32);
            }
        }

        snap
    }

    fn cf_key(name: &CStr) -> CfStringRef {
        unsafe { CFStringCreateWithCString(ptr::null(), name.as_ptr(), K_CFSTRING_ENCODING_UTF8) }
    }

    fn cf_dict_get(dict: CfDictionaryRef, key: &CStr) -> CfTypeRef {
        let key_ref = cf_key(key);
        if key_ref.is_null() {
            return ptr::null();
        }
        let value = unsafe { CFDictionaryGetValue(dict, key_ref) };
        unsafe { CFRelease(key_ref) };
        value
    }

    fn cf_dict_bool(dict: CfDictionaryRef, key: &CStr) -> Option<bool> {
        let value = cf_dict_get(dict, key);
        if value.is_null() {
            return None;
        }
        unsafe {
            let ty = CFGetTypeID(value);
            if ty == CFBooleanGetTypeID() {
                return Some(CFBooleanGetValue(value as CfBooleanRef) != 0);
            }
            if ty == CFNumberGetTypeID() {
                let mut n: i32 = 0;
                if CFNumberGetValue(
                    value as CfNumberRef,
                    K_CF_NUMBER_SINT32_TYPE,
                    &mut n as *mut i32 as *mut c_void,
                ) != 0
                {
                    return Some(n != 0);
                }
            }
        }
        None
    }

    fn cf_dict_i32(dict: CfDictionaryRef, key: &CStr) -> Option<i32> {
        let value = cf_dict_get(dict, key);
        if value.is_null() {
            return None;
        }
        unsafe {
            if CFGetTypeID(value) != CFNumberGetTypeID() {
                return None;
            }
            let mut n: i32 = 0;
            if CFNumberGetValue(
                value as CfNumberRef,
                K_CF_NUMBER_SINT32_TYPE,
                &mut n as *mut i32 as *mut c_void,
            ) != 0
            {
                return Some(n);
            }
            let mut d: f64 = 0.0;
            if CFNumberGetValue(
                value as CfNumberRef,
                K_CF_NUMBER_DOUBLE_TYPE,
                &mut d as *mut f64 as *mut c_void,
            ) != 0
            {
                return Some(d as i32);
            }
        }
        None
    }

    fn cf_dict_string(dict: CfDictionaryRef, key: &CStr) -> Option<String> {
        let value = cf_dict_get(dict, key);
        if value.is_null() {
            return None;
        }
        unsafe {
            if CFGetTypeID(value) != CFStringGetTypeID() {
                return None;
            }
            let mut buf = [0i8; 128];
            if CFStringGetCString(
                value as CfStringRef,
                buf.as_mut_ptr(),
                buf.len() as isize,
                K_CFSTRING_ENCODING_UTF8,
            ) == 0
            {
                return None;
            }
            CStr::from_ptr(buf.as_ptr()).to_str().ok().map(str::to_string)
        }
    }

    pub async fn list_shortcuts() -> Vec<String> {
        let output = tokio::process::Command::new("/usr/bin/shortcuts")
            .arg("list")
            .output()
            .await;
        match output {
            Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect(),
            Ok(output) => {
                log::debug!(
                    "shortcuts list failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                Vec::new()
            }
            Err(err) => {
                log::debug!("shortcuts list: {err}");
                Vec::new()
            }
        }
    }

    async fn run_shortcut(name: &str) -> Result<(), String> {
        let output = tokio::process::Command::new("/usr/bin/shortcuts")
            .arg("run")
            .arg(name)
            .output()
            .await
            .map_err(|err| err.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }

    async fn run_osascript_admin(on: bool) -> Result<(), String> {
        let flag = if on { "1" } else { "0" };
        let script =
            format!(r#"do shell script "pmset -a lowpowermode {flag}" with administrator privileges"#);
        let output = tokio::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .output()
            .await
            .map_err(|err| err.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(if err.is_empty() {
                "Administrator approval was cancelled".into()
            } else {
                err
            })
        }
    }

    pub async fn toggle_low_power_mode() -> Result<bool, String> {
        let before = super::current().low_power_mode;
        let settings = crate::settings::get_app_settings();
        let name = settings
            .lpm_shortcut_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string);
        let listed = list_shortcuts().await;
        match super::lpm_route(name.as_deref(), &listed) {
            LpmRoute::Shortcut(name) => {
                if let Err(err) = run_shortcut(&name).await {
                    log::warn!("shortcuts run '{name}' failed ({err}); falling back to admin prompt");
                    run_osascript_admin(!before).await?;
                }
            }
            LpmRoute::OsascriptAdmin => {
                run_osascript_admin(!before).await?;
            }
        }

        let mut rx = super::subscribe();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(LPM_CONFIRM_SECS);
        loop {
            if super::current().low_power_mode != before {
                return Ok(super::current().low_power_mode);
            }
            match tokio::time::timeout_at(deadline, rx.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => break,
            }
        }
        publish_now();
        if super::current().low_power_mode != before {
            return Ok(super::current().low_power_mode);
        }
        Err("Low Power Mode did not change. Import the shortcut or approve the admin prompt.".into())
    }

    pub fn install_lpm_shortcut() -> Result<(), String> {
        const SHORTCUT: &[u8] = include_bytes!("assets/Toggle Low Power Mode.shortcut");
        let name = crate::settings::get_app_settings()
            .lpm_shortcut_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| super::default_lpm_shortcut_name().into());
        let safe: String = name
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() || ch == ' ' { ch } else { '_' })
            .collect();
        let path = std::env::temp_dir().join(format!("{safe}.shortcut"));
        std::fs::write(&path, SHORTCUT).map_err(|err| err.to_string())?;
        std::process::Command::new("/usr/bin/open")
            .arg(&path)
            .spawn()
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(percent: u8) -> PowerSnapshot {
        PowerSnapshot {
            percent: Some(percent),
            is_charging: false,
            on_ac: false,
            time_to_empty_min: Some(40),
            warning_level: BatteryWarning::None,
            low_power_mode: false,
            has_battery: true,
        }
    }

    #[test]
    fn desktop_macs_never_alert() {
        let snap = PowerSnapshot::default();
        assert!(!snap.has_battery);
        assert!(!snap.is_alerting(20));
        assert!(!snap.is_alerting(100));
        assert!(!snap.should_watch_time_remaining(false));
        assert!(snap.should_watch_time_remaining(true));
    }

    #[test]
    fn discharging_below_threshold_alerts() {
        assert!(battery(20).is_alerting(20));
        assert!(battery(5).is_alerting(20));
        assert!(!battery(21).is_alerting(20));
        assert!(!battery(50).is_alerting(20));
    }

    #[test]
    fn charging_or_ac_clears_the_alert() {
        let mut snap = battery(8);
        snap.is_charging = true;
        snap.on_ac = true;
        assert!(!snap.is_alerting(20));
        snap.is_charging = false;
        assert!(!snap.is_alerting(20), "AC without charge still clears");
    }

    #[test]
    fn os_warning_alerts_even_above_threshold() {
        let mut snap = battery(40);
        snap.warning_level = BatteryWarning::Early;
        assert!(snap.is_alerting(20));
        snap.warning_level = BatteryWarning::Final;
        assert!(snap.is_alerting(5));
    }

    #[test]
    fn time_remaining_watch_is_gated() {
        assert!(!battery(55).should_watch_time_remaining(false));
        assert!(battery(30).should_watch_time_remaining(false));
        assert!(battery(10).should_watch_time_remaining(false));
        assert!(battery(90).should_watch_time_remaining(true));
        let mut ac = battery(10);
        ac.on_ac = true;
        ac.is_charging = true;
        assert!(!ac.should_watch_time_remaining(false));
    }

    #[test]
    fn compact_icon_follows_charge_state() {
        assert_eq!(battery(12).compact_icon(), "battery-low");
        let mut charging = battery(12);
        charging.is_charging = true;
        assert_eq!(charging.compact_icon(), "battery-charging");
    }

    #[test]
    fn lpm_prefers_an_installed_shortcut() {
        let listed = vec!["Focus".into(), "Toggle Low Power Mode".into()];
        assert_eq!(
            lpm_route(Some("Toggle Low Power Mode"), &listed),
            LpmRoute::Shortcut("Toggle Low Power Mode".into())
        );
        assert_eq!(
            lpm_route(Some("Missing"), &listed),
            LpmRoute::OsascriptAdmin
        );
        assert_eq!(lpm_route(Some("  "), &listed), LpmRoute::OsascriptAdmin);
        assert_eq!(lpm_route(None, &listed), LpmRoute::OsascriptAdmin);
    }

    #[test]
    fn threshold_clamps_to_a_usable_range() {
        assert_eq!(clamp_alert_threshold(0), 5);
        assert_eq!(clamp_alert_threshold(20), 20);
        assert_eq!(clamp_alert_threshold(80), 80);
        assert_eq!(clamp_alert_threshold(99), 80);
    }

    #[test]
    fn time_remaining_and_percent_labels() {
        assert_eq!(format_time_remaining(None), "—");
        assert_eq!(format_time_remaining(Some(0)), "Calculating…");
        assert_eq!(format_time_remaining(Some(12)), "12m left");
        assert_eq!(format_time_remaining(Some(75)), "1h 15m left");
        assert_eq!(format_percent(Some(47)), "47%");
        assert_eq!(format_percent(None), "—");
    }

    #[test]
    fn watch_channel_starts_as_desktop() {
        assert_eq!(current(), PowerSnapshot::default());
        let rx = subscribe();
        assert_eq!(*rx.borrow(), PowerSnapshot::default());
    }

    #[test]
    fn bundled_shortcut_is_a_plist() {
        let bytes = include_bytes!("assets/Toggle Low Power Mode.shortcut");
        let text = std::str::from_utf8(bytes).expect("shortcut is UTF-8 plist");
        assert!(text.contains("WFWorkflowActions"));
        assert!(text.contains("lowpowermode"));
        assert!(text.contains("Toggle Low Power Mode"));
    }
}
