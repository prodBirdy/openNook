//! Shared openNook backend, detached from Tauri IPC.
//!
//! The GPUI app (and any other native frontend) talks to these modules
//! directly instead of going through `invoke`.

pub mod agents;
pub mod audio;
pub mod brightness;
pub mod automation;
pub mod audio_devices;
pub mod browser_media;
pub mod calendar;
pub mod database;
pub mod eventtap;
pub mod files;
pub mod focus;
pub mod haptics;
pub mod hotkeys;
pub mod location;
pub mod high_alert;
pub mod keysounds;
#[cfg(target_os = "macos")]
pub mod lyrics;
#[cfg(any(target_os = "macos", test))]
mod mediaremote;
pub mod menubar;
pub mod messages;
pub mod mixer;
pub mod models;
pub mod motion_artwork;
pub mod mouse;
pub mod nl_parse;
pub mod notch;
pub mod notes;
pub mod obsidian;
pub mod observe;
pub mod occupancy;
pub mod power;
pub mod pomodoro;
pub mod scroll;
pub mod settings;
pub mod shortcuts;
pub mod system_timers;
pub mod osd;
pub mod settings;
pub mod sysvol;
pub mod share;
pub mod shell;
pub mod sysstats;
pub mod utils;
pub mod weather;
pub mod vpn;
pub mod widgets;
pub mod window_snap;

pub use models::{LyricLine, NotchInfo, NowPlayingData, SyncedLyrics};
pub use settings::{AppSettings, WindowSettings};

use std::sync::{Once, OnceLock};
use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static INIT: Once = Once::new();

/// Shared multi-thread Tokio runtime for EventKit / Now Playing / HTTP.
pub fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("nook-core")
            .build()
            .expect("tokio runtime")
    })
}

/// App data directory (`~/Library/Application Support/openNook-gpui` on macOS).
pub fn app_data_dir() -> std::path::PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openNook-gpui");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// One-shot init for caches, sqlite, and audio visualizer thread.
pub fn init() {
    INIT.call_once(|| {
        let _ = runtime();
        if let Err(err) = database::init_db() {
            log::error!("database unavailable ({err}); settings and tray will not persist");
        } else {
            settings::load_from_db();
        }
        audio::init_audio_state();
        audio::setup_audio_monitoring();
        mixer::init();
        audio_devices::start();
        mouse::start_polling();
        power::start();
        messages::start_watchers();
        system_timers::start_watcher();
        sysvol::start();
        brightness::start();
        osd::install();
        vpn::start();
        shell::reap_orphaned_jobs();
        eventtap::sync();
    });
}

/// Carbon snap hotkeys + Thaw separator. Call on the AppKit main thread
/// after the Nook status item exists so the separator sits to its left.
pub fn install_window_management() {
    hotkeys::install();
    menubar::install();
}
