use crate::database;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowSettings {
    pub extra_width: f64,
    pub extra_height: f64,
    #[serde(default)]
    pub non_notch_mode: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            extra_width: 400.0,
            extra_height: 800.0,
            non_notch_mode: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub show_media: bool,
    #[serde(default)]
    pub show_calendar: bool,
    #[serde(default)]
    pub show_reminders: bool,
    #[serde(default)]
    pub liquid_glass_mode: bool,
    #[serde(default)]
    pub non_notch_mode: bool,
    #[serde(default)]
    pub window: WindowSettings,
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            show_media: true,
            show_calendar: true,
            show_reminders: true,
            liquid_glass_mode: false,
            non_notch_mode: false,
            window: WindowSettings::default(),
        }
    }
}

static WINDOW_SETTINGS: std::sync::OnceLock<RwLock<WindowSettings>> = std::sync::OnceLock::new();
static APP_SETTINGS: std::sync::OnceLock<RwLock<AppSettings>> = std::sync::OnceLock::new();

fn window_store() -> &'static RwLock<WindowSettings> {
    WINDOW_SETTINGS.get_or_init(|| RwLock::new(WindowSettings::default()))
}

fn app_store() -> &'static RwLock<AppSettings> {
    APP_SETTINGS.get_or_init(|| RwLock::new(AppSettings::default()))
}

pub fn get_window_settings() -> WindowSettings {
    *window_store().read().unwrap_or_else(|e| e.into_inner())
}

pub fn get_app_settings() -> AppSettings {
    app_store()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn update_window_settings(settings: WindowSettings) {
    if let Ok(mut guard) = window_store().write() {
        *guard = settings;
    }
    if let Ok(mut app) = app_store().write() {
        app.window = settings;
        app.non_notch_mode = settings.non_notch_mode;
    }
    persist();
}

pub fn update_app_settings(settings: AppSettings) {
    if let Ok(mut guard) = app_store().write() {
        *guard = settings.clone();
    }
    if let Ok(mut win) = window_store().write() {
        *win = settings.window;
        win.non_notch_mode = settings.non_notch_mode;
    }
    persist();
}

pub fn load_from_db() {
    if let Some(json) = database::get_setting("app_settings") {
        if let Ok(settings) = serde_json::from_str::<AppSettings>(&json) {
            if let Ok(mut guard) = app_store().write() {
                *guard = settings.clone();
            }
            if let Ok(mut win) = window_store().write() {
                *win = settings.window;
                win.non_notch_mode = settings.non_notch_mode;
            }
            return;
        }
    }
    if let Some(json) = database::get_setting("window_settings") {
        if let Ok(settings) = serde_json::from_str::<WindowSettings>(&json) {
            if let Ok(mut guard) = window_store().write() {
                *guard = settings;
            }
        }
    }
}

fn persist() {
    let settings = get_app_settings();
    if let Ok(json) = serde_json::to_string(&settings) {
        let _ = database::set_setting("app_settings", &json);
    }
}

pub fn accent_color() -> String {
    #[cfg(target_os = "macos")]
    {
        return crate::utils::get_macos_accent_color();
    }
    #[cfg(target_os = "windows")]
    {
        return crate::utils::get_windows_accent_color();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    "#007AFF".to_string()
}
