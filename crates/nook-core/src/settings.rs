use crate::database;
use crate::observe::ObserveConfig;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct WindowSettings {
    #[serde(default = "default_extra_width")]
    #[allow(dead_code)]
    pub extra_width: f64,
    /// Kept for config compatibility. The overlay window is the whole display
    /// now, so neither slack value sizes anything.
    #[serde(default = "default_extra_height")]
    #[allow(dead_code)]
    pub extra_height: f64,
    /// Legacy copy of [`AppSettings::non_notch_mode`]; read on load, not written.
    #[serde(default, skip_serializing)]
    pub non_notch_mode: bool,
}

fn default_extra_width() -> f64 {
    400.0
}

fn default_extra_height() -> f64 {
    800.0
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            extra_width: default_extra_width(),
            extra_height: default_extra_height(),
            non_notch_mode: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub show_media: bool,
    #[serde(default = "default_true")]
    pub show_calendar: bool,
    #[serde(default = "default_true")]
    pub show_reminders: bool,
    #[serde(default = "default_true")]
    pub show_agents: bool,
    #[serde(default = "default_true")]
    pub show_observe: bool,
    #[serde(default = "default_true")]
    pub show_timers: bool,
    #[serde(default = "default_true")]
    pub show_notes: bool,
    #[serde(default = "default_true")]
    pub show_speed: bool,
    #[serde(default = "default_true")]
    pub show_files: bool,
    #[serde(default)]
    pub observe: ObserveConfig,
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
            show_agents: true,
            show_observe: true,
            show_timers: true,
            show_notes: true,
            show_speed: true,
            show_files: true,
            observe: ObserveConfig::default(),
            liquid_glass_mode: false,
            non_notch_mode: false,
            window: WindowSettings::default(),
        }
    }
}

static WINDOW_SETTINGS: std::sync::OnceLock<RwLock<WindowSettings>> = std::sync::OnceLock::new();
static APP_SETTINGS: std::sync::OnceLock<RwLock<AppSettings>> = std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
const METRICS_TOKEN_SERVICE: &str = "com.prodBirdy.openNook.metrics";
#[cfg(target_os = "macos")]
const METRICS_TOKEN_ACCOUNT: &str = "warmup-bearer";

#[cfg(target_os = "macos")]
fn load_metrics_token() -> Option<String> {
    security_framework::passwords::get_generic_password(
        METRICS_TOKEN_SERVICE,
        METRICS_TOKEN_ACCOUNT,
    )
    .ok()
    .and_then(|bytes| String::from_utf8(bytes).ok())
}

#[cfg(target_os = "macos")]
fn store_metrics_token(token: &str) -> Result<(), String> {
    if token.is_empty() {
        let _ = security_framework::passwords::delete_generic_password(
            METRICS_TOKEN_SERVICE,
            METRICS_TOKEN_ACCOUNT,
        );
        Ok(())
    } else {
        security_framework::passwords::set_generic_password(
            METRICS_TOKEN_SERVICE,
            METRICS_TOKEN_ACCOUNT,
            token.as_bytes(),
        )
        .map_err(|err| err.to_string())
    }
}

#[cfg(not(target_os = "macos"))]
fn load_metrics_token() -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
fn store_metrics_token(_token: &str) -> Result<(), String> {
    Ok(())
}

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
    }
    persist();
}

pub fn update_app_settings(settings: AppSettings) {
    if let Ok(mut guard) = app_store().write() {
        *guard = settings.clone();
    }
    if let Ok(mut win) = window_store().write() {
        *win = settings.window;
    }
    persist();
}

/// Read-modify-write the app settings in one step.
pub fn tweak_app_settings(tweak: impl FnOnce(&mut AppSettings)) {
    let mut settings = get_app_settings();
    tweak(&mut settings);
    update_app_settings(settings);
}

pub fn load_from_db() {
    if let Some(json) = database::get_setting("app_settings") {
        if let Ok(settings) = serde_json::from_str::<AppSettings>(&json) {
            let mut settings = settings;
            let legacy_token = !settings.observe.metrics_token.is_empty();
            if legacy_token {
                if let Err(err) = store_metrics_token(&settings.observe.metrics_token) {
                    log::warn!("failed to migrate metrics token to Keychain: {err}");
                }
            } else if let Some(token) = load_metrics_token() {
                settings.observe.metrics_token = token;
            }
            if settings.window.non_notch_mode {
                settings.non_notch_mode = true;
            }
            let filled_url = settings.observe.prometheus_url.trim().is_empty();
            crate::observe::fill_default_url(&mut settings.observe);
            if let Ok(mut guard) = app_store().write() {
                *guard = settings.clone();
            }
            if let Ok(mut win) = window_store().write() {
                *win = settings.window;
            }
            if filled_url || legacy_token {
                persist();
            }
            return;
        }
    }
    if let Some(json) = database::get_setting("window_settings") {
        if let Ok(settings) = serde_json::from_str::<WindowSettings>(&json) {
            if let Ok(mut guard) = window_store().write() {
                *guard = settings;
            }
            if let Ok(mut app) = app_store().write() {
                app.window = settings;
            }
        }
    }
    // Persist defaults so missing keys can't silently appear on a later load,
    // and so first-run is tracked by `onboarded` rather than "settings exist".
    persist();
}

/// First launch until the user dismisses the onboarding pill.
pub fn is_first_run() -> bool {
    database::get_setting("onboarded").is_none()
}

pub fn mark_onboarded() {
    if let Err(err) = database::set_setting("onboarded", "1") {
        log::warn!("failed to persist onboarded flag: {err}");
    }
    persist();
}

fn persist() {
    let settings = get_app_settings();
    #[cfg(target_os = "macos")]
    if let Err(err) = store_metrics_token(&settings.observe.metrics_token) {
        log::warn!("failed to persist metrics token to Keychain: {err}");
        return;
    }
    if let Ok(json) = serde_json::to_string(&settings) {
        if let Err(err) = database::set_setting("app_settings", &json) {
            log::warn!("failed to persist app settings: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_json_matches_default() {
        let parsed: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, AppSettings::default());
    }

    #[test]
    fn missing_widget_flags_default_on() {
        let parsed: AppSettings = serde_json::from_str(r#"{"liquid_glass_mode":true}"#).unwrap();
        assert!(parsed.show_media);
        assert!(parsed.show_calendar);
        assert!(parsed.show_reminders);
        assert!(parsed.show_agents);
        assert!(parsed.show_observe);
        assert!(parsed.show_timers);
        assert!(parsed.show_notes);
        assert!(parsed.show_speed);
        assert!(parsed.show_files);
        assert!(parsed.liquid_glass_mode);
        assert!(!parsed.non_notch_mode);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn serialized_settings_omit_the_metrics_token() {
        let mut settings = AppSettings::default();
        settings.observe.metrics_token = "secret-value".into();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("secret-value"));
        assert!(!json.contains("metrics_token"));

        let legacy: AppSettings =
            serde_json::from_str(r#"{"observe":{"metrics_token":"legacy-secret"}}"#).unwrap();
        assert_eq!(legacy.observe.metrics_token, "legacy-secret");
    }
}
