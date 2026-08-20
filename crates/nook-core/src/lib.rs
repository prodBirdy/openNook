//! Shared openNook backend, detached from Tauri IPC.
//!
//! The GPUI app (and any other native frontend) talks to these modules
//! directly instead of going through `invoke`.

pub mod audio;
pub mod calendar;
pub mod database;
pub mod files;
pub mod haptics;
pub mod models;
pub mod mouse;
pub mod notes;
pub mod notch;
pub mod observe;
pub mod runtime;
pub mod settings;
pub mod utils;
pub mod widgets;

pub use models::{NotchInfo, NowPlayingData};
pub use settings::{AppSettings, WindowSettings};

use std::sync::OnceLock;
use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

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
    let _ = env_logger::try_init();
    let _ = runtime();
    database::init_db().expect("database");
    settings::load_from_db();
    audio::init_audio_state();
    audio::setup_audio_monitoring();
}
