//! Shortcuts CLI wrappers for Clock.app App Intents.
//!
//! Direct `MTTimerManager` XPC needs `com.apple.private.mobiletimerd` (blocked
//! for a dev-signed app). Clock ships Pause / Resume / Cancel / Start Timer
//! intents; the user imports the bundled `Nook * Timer` shortcuts once, then
//! the island runs them with `/usr/bin/shortcuts`.

use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::sync::{Mutex, OnceLock};

pub const NAME_START: &str = "Nook Start Timer";
pub const NAME_PAUSE: &str = "Nook Pause Timer";
pub const NAME_RESUME: &str = "Nook Resume Timer";
pub const NAME_CANCEL: &str = "Nook Cancel Timer";

pub const CLOCK_BUNDLE: &str = "com.apple.clock";
pub const CLOCK_URL: &str = "x-apple-clock:";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockShortcuts {
    pub start: bool,
    pub pause: bool,
    pub resume: bool,
    pub cancel: bool,
}

impl ClockShortcuts {
    pub fn can_control(self) -> bool {
        self.pause && self.resume && self.cancel
    }

    pub fn any(self) -> bool {
        self.start || self.pause || self.resume || self.cancel
    }
}

pub fn shortcut_names() -> [&'static str; 4] {
    [NAME_START, NAME_PAUSE, NAME_RESUME, NAME_CANCEL]
}

/// Parse `shortcuts list` stdout (one name per line).
pub fn parse_shortcut_list(stdout: &str) -> ClockShortcuts {
    let mut found = ClockShortcuts::default();
    for line in stdout.lines() {
        match line.trim() {
            NAME_START => found.start = true,
            NAME_PAUSE => found.pause = true,
            NAME_RESUME => found.resume = true,
            NAME_CANCEL => found.cancel = true,
            _ => {}
        }
    }
    found
}

fn cache() -> &'static Mutex<Option<ClockShortcuts>> {
    static CACHE: OnceLock<Mutex<Option<ClockShortcuts>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Cached `shortcuts list`. Call [`refresh_shortcuts`] after import.
pub fn cached_shortcuts() -> ClockShortcuts {
    if let Ok(guard) = cache().lock() {
        if let Some(found) = *guard {
            return found;
        }
    }
    refresh_shortcuts()
}

pub fn refresh_shortcuts() -> ClockShortcuts {
    let found = list_clock_shortcuts();
    if let Ok(mut guard) = cache().lock() {
        *guard = Some(found);
    }
    found
}

pub fn list_clock_shortcuts() -> ClockShortcuts {
    #[cfg(not(target_os = "macos"))]
    {
        return ClockShortcuts::default();
    }
    #[cfg(target_os = "macos")]
    {
        match Command::new("/usr/bin/shortcuts").arg("list").output() {
            Ok(out) if out.status.success() => {
                parse_shortcut_list(&String::from_utf8_lossy(&out.stdout))
            }
            Ok(out) => {
                log::debug!(
                    "shortcuts list exited {}: {}",
                    out.status,
                    String::from_utf8_lossy(&out.stderr)
                );
                ClockShortcuts::default()
            }
            Err(err) => {
                log::debug!("shortcuts list: {err}");
                ClockShortcuts::default()
            }
        }
    }
}

pub fn run_shortcut(name: &str, input: Option<&str>) -> Result<String, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (name, input);
        return Err("Shortcuts CLI is only available on macOS".into());
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("/usr/bin/shortcuts");
        cmd.arg("run").arg(name);
        if let Some(input) = input {
            cmd.arg("-i").arg(input);
        }
        let out = cmd.output().map_err(|err| err.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(if err.trim().is_empty() {
                format!("shortcuts run {name} failed")
            } else {
                err.trim().to_string()
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

fn spawn_shortcut(name: &'static str, input: Option<String>) {
    crate::runtime().spawn(async move {
        match run_shortcut(name, input.as_deref()) {
            Ok(_) => log::debug!("ran shortcut {name}"),
            Err(err) => log::info!("shortcut {name}: {err}"),
        }
    });
}

/// Pause the current Clock timer, or open Clock if the shortcut is missing.
pub fn pause_timer() {
    dispatch(NAME_PAUSE);
}

pub fn resume_timer() {
    dispatch(NAME_RESUME);
}

pub fn cancel_timer() {
    dispatch(NAME_CANCEL);
}

pub fn start_timer(seconds: u32) {
    if cached_shortcuts().start {
        spawn_shortcut(NAME_START, Some(seconds.to_string()));
    } else {
        open_clock();
    }
}

fn dispatch(name: &'static str) {
    let found = cached_shortcuts();
    let present = match name {
        NAME_PAUSE => found.pause,
        NAME_RESUME => found.resume,
        NAME_CANCEL => found.cancel,
        NAME_START => found.start,
        _ => false,
    };
    if present {
        spawn_shortcut(name, None);
    } else {
        open_clock();
    }
}

pub fn open_timer(id: &str) {
    let url = format!("{}{id}", crate::system_timers::CLOCK_TIMER_SCHEME);
    let _ = open_url(&url);
}

pub fn open_clock() {
    if open_url(CLOCK_URL).is_err() {
        let _ = open::that_detached(CLOCK_BUNDLE);
    }
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        Command::new("/usr/bin/open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        open::that_detached(url).map_err(|err| err.to_string())
    }
}

/// Directory of bundled `.shortcut` files (app Resources, then repo checkout).
pub fn bundled_shortcut_dir() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(macos) = exe.parent() {
            let resources = macos.join("../Resources/shortcuts");
            if resources.is_dir() {
                return resources.canonicalize().ok();
            }
        }
    }
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/shortcuts");
    if repo.is_dir() {
        return repo.canonicalize().ok();
    }
    None
}

/// Open bundled shortcut files so Shortcuts.app can import them.
pub fn import_bundled_shortcuts() -> Result<(), String> {
    let Some(dir) = bundled_shortcut_dir() else {
        open_clock();
        return Err("Clock shortcuts are not bundled in this build".into());
    };
    let mut opened = 0u32;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("shortcut") {
                if open::that_detached(&path).is_ok() {
                    opened += 1;
                }
            }
        }
    }
    if opened == 0 {
        let _ = open::that_detached(&dir);
    }
    // Re-read after a beat so the next tap sees a fresh `shortcuts list`.
    crate::runtime().spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let _ = refresh_shortcuts();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shortcut_list_detects_nook_clock_names() {
        let list = "All Photos\nNook Pause Timer\nNook Resume Timer\nWeather\nNook Cancel Timer\n";
        let found = parse_shortcut_list(list);
        assert!(found.can_control());
        assert!(!found.start);
        assert!(found.pause && found.resume && found.cancel);
        assert!(found.any());
    }

    #[test]
    fn parse_shortcut_list_empty_is_read_only() {
        let found = parse_shortcut_list("Morning Routine\n");
        assert!(!found.can_control());
        assert!(!found.any());
        assert_eq!(found, ClockShortcuts::default());
    }

    #[test]
    fn parse_shortcut_list_start_only() {
        let found = parse_shortcut_list("Nook Start Timer\n");
        assert!(found.start);
        assert!(!found.can_control());
        assert_eq!(shortcut_names()[0], NAME_START);
    }
}
