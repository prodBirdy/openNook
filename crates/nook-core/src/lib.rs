//! Shared openNook backend, detached from Tauri IPC.
//!
//! The GPUI app (and any other native frontend) talks to these modules
//! directly instead of going through `invoke`.

pub mod agents;
pub mod apps;
pub mod audio;
pub mod audio_devices;
pub mod automation;
pub mod brightness;
pub mod browser_media;
pub mod calendar;
pub mod database;
pub mod eventtap;
pub mod ffi;
pub mod files;
pub mod focus;
pub mod haptics;
pub mod high_alert;
pub mod location;
pub mod login_item;
#[cfg(target_os = "macos")]
pub mod lyrics;
#[cfg(any(target_os = "macos", test))]
mod mediaremote;
pub mod meetings;
pub mod messages;
pub mod models;
pub mod motion_artwork;
pub mod mouse;
pub mod nl_parse;
pub mod notch;
pub mod notes;
pub mod notifications;
pub mod observe;
pub mod obsidian;
pub mod occupancy;
pub mod osd;
pub mod pomodoro;
pub mod power;
pub mod queue;
pub mod recorder;
pub mod settings;
pub mod share;
pub mod shell;
pub mod shortcuts;
pub mod spotify;
pub mod sysstats;
pub mod system_timers;
pub mod sysvol;
pub mod ui_tick;
pub mod utils;
pub mod vpn;
pub mod weather;
pub mod widgets;

pub use models::{LyricLine, NotchInfo, NowPlayingData, PlaybackQueue, QueueItem, SyncedLyrics};
pub use settings::{AppSettings, WidgetSize, WindowSettings};

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

/// One-shot init for caches and sqlite.
pub fn init() {
    INIT.call_once(|| {
        let _ = runtime();
        if let Err(err) = database::init_db() {
            log::error!("database unavailable ({err}); settings and tray will not persist");
        } else {
            settings::load_from_db();
        }
        audio::init_audio_state();
        audio_devices::start();
        crate::spotify::hydrate_status();
        mouse::start_polling();
        power::start();
        {
            let settings = settings::get_app_settings();
            if settings.show_messages {
                messages::start_watchers();
            }
            if settings.sync_clock_timers {
                system_timers::start_watcher();
            }
            if settings.show_vpn {
                vpn::start();
            }
        }
        sysvol::start();
        brightness::start();
        osd::install();
        shell::reap_orphaned_jobs();
        notifications::load_persisted();
    });
}
