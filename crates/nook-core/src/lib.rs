//! Shared openNook backend, detached from Tauri IPC.
//!
//! The GPUI app (and any other native frontend) talks to these modules
//! directly instead of going through `invoke`.

pub mod agents;
pub mod audio;
pub mod browser_media;
pub mod calendar;
pub mod database;
pub mod files;
pub mod haptics;
pub mod location;
#[cfg(target_os = "macos")]
mod mediaremote;
pub mod models;
pub mod mouse;
pub mod notch;
pub mod notes;
pub mod observe;
pub mod occupancy;
pub mod settings;
pub mod utils;
pub mod weather;
pub mod widgets;

pub use models::{NotchInfo, NowPlayingData};
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
        mouse::start_polling();
    });
}
