//! SlimHUD-style suppression of the system bezel renderer (`OSDUIHelper`).
//!
//! Default is off — Settings must opt in. Startup always SIGCONTs a leftover
//! stopped helper so a previous crash cannot leave the user without bezels.
//! SIGTERM/SIGINT only send SIGCONT (async-signal-safe); `atexit` and [`Drop`]
//! run the full launchctl restore.

use crate::settings;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Once;

pub const HELPER_LABEL: &str = "com.apple.OSDUIHelper";
pub const HELPER_PROCESS: &str = "OSDUIHelper";
pub const LAUNCHCTL: &str = "/bin/launchctl";
pub const PKILL: &str = "/usr/bin/pkill";
pub const PGREP: &str = "/usr/bin/pgrep";

/// `launchctl kickstart` arguments for the per-user OSDUIHelper service.
pub fn kickstart_args(uid: u32, kill: bool) -> Vec<String> {
    let mut args = vec!["kickstart".to_string()];
    if kill {
        args.push("-k".into());
    }
    args.push(format!("gui/{uid}/{HELPER_LABEL}"));
    args
}

pub fn pgrep_args() -> [&'static str; 2] {
    ["-x", HELPER_PROCESS]
}

pub fn is_suppressed() -> bool {
    SUPPRESSED.load(Ordering::Relaxed)
}

/// True when we last saw the helper process (so we can claim suppression).
pub fn helper_present() -> bool {
    HELPER_SEEN.load(Ordering::Relaxed)
}

/// Crash-safe restore, then apply the current Settings value.
pub fn install() {
    STARTED.call_once(|| {
        restore();
        apply_from_settings();
        #[cfg(target_os = "macos")]
        macos::install_exit_hooks();
        let _ = GUARD.set(OsdGuard);
    });
}

pub fn apply_from_settings() {
    apply(settings::get_app_settings().replace_system_hud);
}

pub fn apply(replace: bool) {
    if replace {
        let _ = suppress();
    } else {
        restore();
    }
}

/// Stop the helper so it stays "alive" to launchd but cannot draw. Returns
/// whether a process was actually stopped.
pub fn suppress() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::suppress()
    }
    #[cfg(not(target_os = "macos"))]
    {
        HELPER_SEEN.store(false, Ordering::Relaxed);
        SUPPRESSED.store(false, Ordering::Relaxed);
        false
    }
}

pub fn restore() {
    #[cfg(target_os = "macos")]
    macos::restore();
    #[cfg(not(target_os = "macos"))]
    {
        SUPPRESSED.store(false, Ordering::Relaxed);
    }
}

/// Parse `pgrep -x OSDUIHelper` stdout into pids.
pub fn parse_pgrep_stdout(stdout: &str) -> Vec<i32> {
    stdout
        .split_whitespace()
        .filter_map(|token| token.parse::<i32>().ok())
        .filter(|pid| *pid > 1)
        .collect()
}

static STARTED: Once = Once::new();
static SUPPRESSED: AtomicBool = AtomicBool::new(false);
static HELPER_SEEN: AtomicBool = AtomicBool::new(false);
#[allow(dead_code)]
static CACHED_PID: AtomicI32 = AtomicI32::new(0);
static GUARD: std::sync::OnceLock<OsdGuard> = std::sync::OnceLock::new();

struct OsdGuard;

impl Drop for OsdGuard {
    fn drop(&mut self) {
        restore();
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::process::Command;

    // Darwin signal numbers (async-signal-safe restore uses these only).
    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;
    const SIGSTOP: i32 = 17;
    const SIGCONT: i32 = 19;

    unsafe extern "C" {
        fn getuid() -> u32;
        fn kill(pid: i32, sig: i32) -> i32;
        fn atexit(cb: extern "C" fn()) -> i32;
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> *mut std::ffi::c_void;
    }

    fn uid() -> u32 {
        unsafe { getuid() }
    }

    fn helper_pids() -> Vec<i32> {
        let output = Command::new(PGREP).args(pgrep_args()).output();
        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                super::parse_pgrep_stdout(&text)
            }
            Err(err) => {
                log::debug!("pgrep OSDUIHelper: {err}");
                Vec::new()
            }
        }
    }

    fn kickstart(kill: bool) {
        let args = kickstart_args(uid(), kill);
        if let Err(err) = Command::new(LAUNCHCTL).args(&args).status() {
            log::debug!("launchctl {}: {err}", args.join(" "));
        }
    }

    pub(super) fn suppress() -> bool {
        kickstart(false);
        let pids = helper_pids();
        if pids.is_empty() {
            log::warn!("OSDUIHelper not running; island HUD will show beside the system bezel");
            HELPER_SEEN.store(false, Ordering::Relaxed);
            SUPPRESSED.store(false, Ordering::Relaxed);
            CACHED_PID.store(0, Ordering::Relaxed);
            return false;
        }
        HELPER_SEEN.store(true, Ordering::Relaxed);
        CACHED_PID.store(pids[0], Ordering::Relaxed);
        for pid in pids {
            unsafe {
                let _ = kill(pid, SIGSTOP);
            }
        }
        SUPPRESSED.store(true, Ordering::Relaxed);
        true
    }

    pub(super) fn restore() {
        let cached = CACHED_PID.swap(0, Ordering::Relaxed);
        if cached > 1 {
            unsafe {
                let _ = kill(cached, SIGCONT);
            }
        }
        for pid in helper_pids() {
            unsafe {
                let _ = kill(pid, SIGCONT);
            }
        }
        kickstart(true);
        SUPPRESSED.store(false, Ordering::Relaxed);
    }

    extern "C" fn restore_on_exit() {
        let pid = CACHED_PID.load(Ordering::Relaxed);
        if pid > 1 {
            unsafe {
                let _ = kill(pid, SIGCONT);
            }
        }
    }

    extern "C" fn restore_on_signal(_sig: i32) {
        restore_on_exit();
    }

    pub(super) fn install_exit_hooks() {
        unsafe {
            let _ = atexit(restore_on_exit);
            let _ = signal(SIGTERM, restore_on_signal);
            let _ = signal(SIGINT, restore_on_signal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kickstart_args_target_the_gui_domain() {
        assert_eq!(
            kickstart_args(501, false),
            vec![
                "kickstart".to_string(),
                "gui/501/com.apple.OSDUIHelper".into()
            ]
        );
        assert_eq!(
            kickstart_args(501, true),
            vec![
                "kickstart".to_string(),
                "-k".into(),
                "gui/501/com.apple.OSDUIHelper".into()
            ]
        );
    }

    #[test]
    fn pgrep_is_exact_process_name() {
        assert_eq!(pgrep_args(), ["-x", "OSDUIHelper"]);
        assert_eq!(HELPER_LABEL, "com.apple.OSDUIHelper");
    }

    #[test]
    fn parse_pgrep_stdout_ignores_noise() {
        assert_eq!(parse_pgrep_stdout(""), Vec::<i32>::new());
        assert_eq!(parse_pgrep_stdout("0\n1\n"), Vec::<i32>::new());
        assert_eq!(parse_pgrep_stdout("4321\n4400\n"), vec![4321, 4400]);
        assert_eq!(parse_pgrep_stdout("not-a-pid"), Vec::<i32>::new());
    }

    #[test]
    fn suppress_is_a_no_op_off_macos() {
        #[cfg(not(target_os = "macos"))]
        {
            assert!(!suppress());
            assert!(!is_suppressed());
            assert!(!helper_present());
            restore();
        }
    }
}
