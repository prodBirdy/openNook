use crate::database;
use crate::observe::ObserveConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum WidgetModule {
    Calendar = 0,
    Music = 1,
    Files = 2,
    Notes = 3,
    Observe = 4,
    Timers = 5,
    Reminders = 6,
    Speed = 7,
    Agents = 8,
    Mirror = 9,
    Battery = 10,
    Messages = 10,
    Obsidian = 10,
}

impl WidgetModule {
    pub const ALL: [Self; 11] = [
        Self::Calendar,
        Self::Music,
        Self::Files,
        Self::Notes,
        Self::Observe,
        Self::Timers,
        Self::Reminders,
        Self::Speed,
        Self::Agents,
        Self::Mirror,
        Self::Battery,
        Self::Messages,
        Self::Obsidian,
    ];

    pub fn from_u8(value: u8) -> Self {
        Self::ALL
            .into_iter()
            .find(|module| *module as u8 == value)
            .unwrap_or(Self::Calendar)
    }

    /// Default width of this widget on the expanded Nook row.
    pub fn default_cells(self) -> u8 {
        match self {
            Self::Calendar | Self::Music => 5,
            Self::Files | Self::Notes | Self::Observe | Self::Reminders | Self::Agents => 4,
            Self::Timers | Self::Speed | Self::Mirror | Self::Battery => 3,
            Self::Files | Self::Notes | Self::Observe | Self::Reminders | Self::Agents | Self::Messages => {
            Self::Files | Self::Notes | Self::Observe | Self::Reminders | Self::Agents | Self::Obsidian => {
                4
            }
            Self::Timers | Self::Speed | Self::Mirror => 3,
        }
    }

    pub fn min_cells(self) -> u8 {
        match self {
            Self::Calendar => 4,
            Self::Music | Self::Files | Self::Observe | Self::Reminders | Self::Mirror => 3,
            Self::Notes | Self::Timers | Self::Speed | Self::Agents | Self::Battery => 2,
            Self::Music | Self::Files | Self::Observe | Self::Reminders | Self::Mirror | Self::Messages => {
                3
            }
            Self::Notes | Self::Timers | Self::Speed | Self::Agents => 2,
            Self::Notes | Self::Timers | Self::Speed | Self::Agents | Self::Obsidian => 2,
        }
    }

    pub fn max_cells(self) -> u8 {
        match self {
            Self::Timers | Self::Speed | Self::Agents | Self::Mirror | Self::Battery => 6,
            _ => 8,
        }
    }

    /// Files lives on the Tray tab, not the Nook cell row.
    pub fn occupies_nook_cells(self) -> bool {
        !matches!(self, Self::Files)
    }
}

fn default_widget_order() -> Vec<WidgetModule> {
    vec![
        WidgetModule::Music,
        WidgetModule::Calendar,
        WidgetModule::Mirror,
        WidgetModule::Files,
        WidgetModule::Agents,
        WidgetModule::Observe,
        WidgetModule::Reminders,
        WidgetModule::Timers,
        WidgetModule::Notes,
        WidgetModule::Obsidian,
        WidgetModule::Speed,
        WidgetModule::Battery,
        WidgetModule::Messages,
    ]
}

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
    #[serde(default = "default_widget_order")]
    pub widget_order: Vec<WidgetModule>,
    #[serde(default = "default_true")]
    pub show_media: bool,
    /// Opt-in time-synced lyrics beside Now Playing (LRCLIB, cached locally).
    #[serde(default)]
    pub show_lyrics: bool,
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
    #[serde(default = "default_true")]
    pub show_mirror: bool,
    #[serde(default = "default_true")]
    pub show_battery: bool,
    /// Percent at or below which the compact face takes over while discharging.
    #[serde(default = "default_battery_alert_threshold")]
    pub battery_alert_threshold: u8,
    /// Shortcuts.app name for the one-tap LPM toggle. `None` or a missing
    /// shortcut falls back to the osascript-admin prompt.
    #[serde(default = "default_lpm_shortcut_name")]
    pub lpm_shortcut_name: Option<String>,
    pub show_messages: bool,
    /// Fragile Accessibility CGEvent Return after opening `whatsapp://`.
    #[serde(default)]
    pub experimental_whatsapp_autosend: bool,
    /// Mirror Apple Clock timers in the Timers widget (plist / vnode watch).
    #[serde(default = "default_true")]
    pub sync_clock_timers: bool,
    pub show_obsidian: bool,
    /// User-chosen vault folder. `None` until Settings picks one.
    #[serde(default)]
    pub obsidian_vault: Option<PathBuf>,
    /// Optional markdown heading that daily-note capture appends under.
    #[serde(default)]
    pub obsidian_capture_heading: Option<String>,
    /// Use `obsidian://new?append=true` instead of writing the daily note.
    #[serde(default)]
    pub obsidian_uri_capture: bool,
    #[serde(default)]
    pub observe: ObserveConfig,
    #[serde(default)]
    pub liquid_glass_mode: bool,
    #[serde(default)]
    pub non_notch_mode: bool,
    /// Horizontal position of the island centre as a fraction of screen width.
    /// `0.5` (default) centres it on the notch.
    #[serde(default = "default_island_x")]
    pub island_x: f32,
    /// Vertical position of the island top as a fraction of screen height.
    /// `0` (default) pins it to the top edge.
    #[serde(default)]
    pub island_y: f32,
    /// Hide the overlay while another app is full screen or zoomed to fill
    /// the display.
    #[serde(default)]
    pub hide_when_maximized: bool,
    /// Transient volume/brightness HUD on the compact island face.
    #[serde(default = "default_true")]
    pub show_volume_brightness_hud: bool,
    /// SIGSTOP `OSDUIHelper` so the system bezel does not draw on top.
    /// Default off — suppression also hides caps-lock and keyboard-backlight bezels.
    #[serde(default)]
    pub replace_system_hud: bool,
    /// Island fill as `0xRRGGBB`. `None` uses the default black Live Activity
    /// fill.
    #[serde(default)]
    pub island_color: Option<u32>,
    /// Per-widget widths in Nook cells. Missing entries use [`WidgetModule::default_cells`].
    #[serde(default)]
    pub widget_widths: Vec<(WidgetModule, u8)>,
    /// Rectangle-style halves / quarters via Carbon hotkeys. Needs Accessibility.
    #[serde(default)]
    pub window_snap_enabled: bool,
    /// Stretch our own menu-bar separator so extras to its left go off-screen.
    #[serde(default)]
    pub thaw_enabled: bool,
    /// Separator is currently stretched (extras hidden). Ignored when Thaw is off.
    #[serde(default)]
    pub thaw_hidden: bool,
    /// Reserved for drag-to-edge (tier 2). Geometry is implemented; the live
    /// AX tracker is not wired so idle cost stays zero.
    #[serde(default)]
    pub snap_drag_to_edge: bool,
    #[serde(default)]
    pub window: WindowSettings,
}

fn default_island_x() -> f32 {
    0.5
}

/// Named island fills shown in Settings. `None` is the default black.
#[derive(Clone, Copy)]
pub struct IslandSwatch {
    pub name: &'static str,
    pub rgb: Option<u32>,
}

pub const ISLAND_SWATCHES: [IslandSwatch; 7] = [
    IslandSwatch {
        name: "Black",
        rgb: None,
    },
    IslandSwatch {
        name: "Graphite",
        rgb: Some(0x1C1C1E),
    },
    IslandSwatch {
        name: "Navy",
        rgb: Some(0x0B1C33),
    },
    IslandSwatch {
        name: "Forest",
        rgb: Some(0x0C1F14),
    },
    IslandSwatch {
        name: "Burgundy",
        rgb: Some(0x2A0D12),
    },
    IslandSwatch {
        name: "Indigo",
        rgb: Some(0x1A1233),
    },
    IslandSwatch {
        name: "Olive",
        rgb: Some(0x1A1C10),
    },
];

fn default_true() -> bool {
    true
}

fn default_battery_alert_threshold() -> u8 {
    20
}

fn default_lpm_shortcut_name() -> Option<String> {
    Some(crate::power::default_lpm_shortcut_name().into())
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            widget_order: default_widget_order(),
            show_media: true,
            show_lyrics: false,
            show_calendar: true,
            show_reminders: true,
            show_agents: true,
            show_observe: true,
            show_timers: true,
            show_notes: true,
            show_speed: true,
            show_files: true,
            show_mirror: true,
            show_battery: true,
            battery_alert_threshold: default_battery_alert_threshold(),
            lpm_shortcut_name: default_lpm_shortcut_name(),
            show_messages: true,
            experimental_whatsapp_autosend: false,
            sync_clock_timers: true,
            show_obsidian: true,
            obsidian_vault: None,
            obsidian_capture_heading: None,
            obsidian_uri_capture: false,
            observe: ObserveConfig::default(),
            liquid_glass_mode: false,
            non_notch_mode: false,
            island_x: default_island_x(),
            island_y: 0.0,
            hide_when_maximized: false,
            show_volume_brightness_hud: true,
            replace_system_hud: false,
            island_color: None,
            widget_widths: Vec::new(),
            window_snap_enabled: false,
            thaw_enabled: false,
            thaw_hidden: false,
            snap_drag_to_edge: false,
            window: WindowSettings::default(),
        }
    }
}

impl AppSettings {
    pub fn ordered_widgets(&self) -> Vec<WidgetModule> {
        self.widget_order
            .iter()
            .chain(&WidgetModule::ALL)
            .copied()
            .fold(
                Vec::with_capacity(WidgetModule::ALL.len()),
                |mut order, module| {
                    if !order.contains(&module) {
                        order.push(module);
                    }
                    order
                },
            )
    }

    /// Expanded Nook row budget. Widgets share these cells left to right.
    pub const TOTAL_CELLS: u8 = 11;

    pub fn is_enabled(&self, module: WidgetModule) -> bool {
        match module {
            WidgetModule::Calendar => self.show_calendar,
            WidgetModule::Music => self.show_media,
            WidgetModule::Files => self.show_files,
            WidgetModule::Notes => self.show_notes,
            WidgetModule::Observe => self.show_observe,
            WidgetModule::Timers => self.show_timers,
            WidgetModule::Reminders => self.show_reminders,
            WidgetModule::Speed => self.show_speed,
            WidgetModule::Agents => self.show_agents,
            WidgetModule::Mirror => self.show_mirror,
            WidgetModule::Battery => self.show_battery,
            WidgetModule::Messages => self.show_messages,
            WidgetModule::Obsidian => self.show_obsidian,
        }
    }

    pub fn toggle_enabled(&mut self, module: WidgetModule) {
        match module {
            WidgetModule::Calendar => self.show_calendar = !self.show_calendar,
            WidgetModule::Music => self.show_media = !self.show_media,
            WidgetModule::Files => self.show_files = !self.show_files,
            WidgetModule::Notes => self.show_notes = !self.show_notes,
            WidgetModule::Observe => self.show_observe = !self.show_observe,
            WidgetModule::Timers => self.show_timers = !self.show_timers,
            WidgetModule::Reminders => self.show_reminders = !self.show_reminders,
            WidgetModule::Speed => self.show_speed = !self.show_speed,
            WidgetModule::Agents => self.show_agents = !self.show_agents,
            WidgetModule::Mirror => self.show_mirror = !self.show_mirror,
            WidgetModule::Battery => self.show_battery = !self.show_battery,
            WidgetModule::Messages => {
                self.show_messages = !self.show_messages;
                crate::messages::request_refresh();
            }
            WidgetModule::Obsidian => self.show_obsidian = !self.show_obsidian,
        }
    }

    pub fn cells_for(&self, module: WidgetModule) -> u8 {
        let raw = self
            .widget_widths
            .iter()
            .find(|(item, _)| *item == module)
            .map(|(_, width)| *width)
            .unwrap_or_else(|| module.default_cells());
        raw.clamp(module.min_cells(), module.max_cells())
    }

    pub fn set_cells(&mut self, module: WidgetModule, cells: u8) {
        let cells = cells.clamp(module.min_cells(), module.max_cells());
        if let Some(entry) = self
            .widget_widths
            .iter_mut()
            .find(|(item, _)| *item == module)
        {
            entry.1 = cells;
        } else {
            self.widget_widths.push((module, cells));
        }
    }

    /// Max width available to this widget in the horizontally scrollable Nook row.
    pub fn max_cells_for(&self, module: WidgetModule) -> u8 {
        module.max_cells()
    }

    pub fn used_cells(&self) -> u8 {
        self.ordered_widgets()
            .into_iter()
            .filter(|module| module.occupies_nook_cells() && self.is_enabled(*module))
            .map(|module| self.cells_for(module))
            .fold(0u8, |sum, cells| sum.saturating_add(cells))
    }

    pub fn remaining_cells(&self) -> u8 {
        Self::TOTAL_CELLS.saturating_sub(self.used_cells())
    }

    pub fn move_widget_to(&mut self, module: WidgetModule, target: WidgetModule) {
        let mut order = self.ordered_widgets();
        let Some(from) = order.iter().position(|item| *item == module) else {
            return;
        };
        let Some(to) = order.iter().position(|item| *item == target) else {
            return;
        };
        let module = order.remove(from);
        order.insert(to, module);
        self.widget_order = order;
    }

    /// Top-left of the island body on a display of `screen_w` × `screen_h`.
    pub fn island_origin(
        &self,
        screen_w: f32,
        screen_h: f32,
        island_w: f32,
        island_h: f32,
    ) -> (f32, f32) {
        (
            self.island_left(screen_w, island_w),
            self.island_top(screen_h, island_h),
        )
    }

    pub fn island_left(&self, screen_w: f32, island_w: f32) -> f32 {
        let span = (screen_w - island_w).max(0.0);
        let center = self.island_x.clamp(0.0, 1.0) * screen_w;
        (center - island_w * 0.5).clamp(0.0, span)
    }

    pub fn island_top(&self, screen_h: f32, island_h: f32) -> f32 {
        let span = (screen_h - island_h).max(0.0);
        (self.island_y.clamp(0.0, 1.0) * screen_h).clamp(0.0, span)
    }

    /// Notch-attached: sitting on the top edge so the silhouette can keep its
    /// concave wings. A couple of points of slack so float noise from a drag
    /// does not flip the chrome.
    pub fn island_attached(&self, screen_h: f32) -> bool {
        self.island_top(screen_h, 0.0) < 2.0
    }

    /// Store a drag so the island's top-left lands at `(left, top)`.
    pub fn set_island_origin(
        &mut self,
        left: f32,
        top: f32,
        screen_w: f32,
        screen_h: f32,
        island_w: f32,
    ) {
        let center = left + island_w * 0.5;
        self.island_x = if screen_w > 1.0 {
            (center / screen_w).clamp(0.0, 1.0)
        } else {
            default_island_x()
        };
        self.island_y = if screen_h > 1.0 {
            (top / screen_h).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    pub fn reset_island_position(&mut self) {
        self.island_x = default_island_x();
        self.island_y = 0.0;
    }

    pub fn island_swatch_name(&self) -> &'static str {
        ISLAND_SWATCHES
            .iter()
            .find(|swatch| swatch.rgb == self.island_color)
            .map(|swatch| swatch.name)
            .unwrap_or("Custom")
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
    SETTINGS_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    persist();
    crate::hotkeys::sync();
    crate::menubar::sync();
}

/// Bumped on every [`update_app_settings`]. Hot loops compare this before
/// paying for a [`get_app_settings`] clone — the settings struct holds
/// strings and vecs, and cloning it 50×/sec was pure allocator churn.
static SETTINGS_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn settings_generation() -> u64 {
    SETTINGS_GEN.load(std::sync::atomic::Ordering::Relaxed)
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
        assert_eq!(parsed.widget_order, default_widget_order());
        assert!(parsed.show_media);
        assert!(!parsed.show_lyrics);
        assert!(parsed.show_calendar);
        assert!(parsed.show_reminders);
        assert!(parsed.show_agents);
        assert!(parsed.show_observe);
        assert!(parsed.show_timers);
        assert!(parsed.show_notes);
        assert!(parsed.show_speed);
        assert!(parsed.show_files);
        assert!(parsed.show_mirror);
        assert!(parsed.show_battery);
        assert_eq!(parsed.battery_alert_threshold, 20);
        assert_eq!(
            parsed.lpm_shortcut_name.as_deref(),
            Some(crate::power::default_lpm_shortcut_name())
        );
        assert!(parsed.show_messages);
        assert!(!parsed.experimental_whatsapp_autosend);
        assert!(parsed.sync_clock_timers);
        assert!(parsed.show_obsidian);
        assert_eq!(parsed.obsidian_vault, None);
        assert_eq!(parsed.obsidian_capture_heading, None);
        assert!(!parsed.obsidian_uri_capture);
        assert!(parsed.liquid_glass_mode);
        assert!(!parsed.non_notch_mode);
        assert!((parsed.island_x - 0.5).abs() < f32::EPSILON);
        assert_eq!(parsed.island_y, 0.0);
        assert!(!parsed.hide_when_maximized);
        assert!(parsed.show_volume_brightness_hud);
        assert!(!parsed.replace_system_hud);
        assert_eq!(parsed.island_color, None);
    }

    #[test]
    fn island_origin_defaults_to_top_center() {
        let settings = AppSettings::default();
        let (x, y) = settings.island_origin(1512.0, 982.0, 180.0, 32.0);
        assert!((x - (1512.0 - 180.0) / 2.0).abs() < 0.01);
        assert_eq!(y, 0.0);
        assert!(settings.island_attached(982.0));
    }

    #[test]
    fn island_origin_tracks_a_drag_and_clamps() {
        let mut settings = AppSettings::default();
        settings.set_island_origin(0.0, 120.0, 1512.0, 982.0, 180.0);
        let (x, y) = settings.island_origin(1512.0, 982.0, 180.0, 32.0);
        assert!((x - 0.0).abs() < 0.5, "left edge stays left, got {x}");
        assert!((y - 120.0).abs() < 0.5, "top tracks the drag, got {y}");
        assert!(!settings.island_attached(982.0));

        settings.set_island_origin(2000.0, 4000.0, 1512.0, 982.0, 180.0);
        let (x, y) = settings.island_origin(1512.0, 982.0, 180.0, 32.0);
        assert!((x - (1512.0 - 180.0)).abs() < 0.5);
        assert!(y <= 982.0 - 32.0);

        settings.reset_island_position();
        assert!((settings.island_x - 0.5).abs() < f32::EPSILON);
        assert_eq!(settings.island_y, 0.0);
    }

    #[test]
    fn island_swatch_name_matches_the_palette() {
        let mut settings = AppSettings::default();
        assert_eq!(settings.island_swatch_name(), "Black");
        settings.island_color = Some(0x1C1C1E);
        assert_eq!(settings.island_swatch_name(), "Graphite");
        settings.island_color = Some(0x123456);
        assert_eq!(settings.island_swatch_name(), "Custom");
    }

    #[test]
    fn widget_order_moves_and_repairs_saved_values() {
        let mut settings = AppSettings::default();
        settings.widget_order = vec![WidgetModule::Music, WidgetModule::Music];
        assert_eq!(settings.ordered_widgets().len(), WidgetModule::ALL.len());

        settings.move_widget_to(WidgetModule::Music, WidgetModule::Files);
        assert_eq!(settings.widget_order[2], WidgetModule::Music);
        settings.move_widget_to(WidgetModule::Music, WidgetModule::Calendar);
        assert_eq!(settings.widget_order[0], WidgetModule::Music);
    }

    #[test]
    fn cells_default_clamp_and_budget() {
        let mut settings = AppSettings::default();
        assert_eq!(settings.cells_for(WidgetModule::Calendar), 5);
        assert_eq!(settings.cells_for(WidgetModule::Timers), 3);
        settings.set_cells(WidgetModule::Calendar, 1);
        assert_eq!(
            settings.cells_for(WidgetModule::Calendar),
            WidgetModule::Calendar.min_cells()
        );
        settings.set_cells(WidgetModule::Calendar, 99);
        assert_eq!(
            settings.cells_for(WidgetModule::Calendar),
            WidgetModule::Calendar.max_cells()
        );
        assert!(!WidgetModule::Files.occupies_nook_cells());
        assert!(WidgetModule::Calendar.occupies_nook_cells());
        settings.show_calendar = false;
        settings.show_media = true;
        settings.show_files = true;
        settings.show_notes = false;
        settings.show_observe = false;
        settings.show_timers = false;
        settings.show_reminders = false;
        settings.show_speed = false;
        settings.show_agents = false;
        settings.show_mirror = false;
        settings.show_battery = false;
        settings.show_messages = false;
        settings.show_obsidian = false;
        settings.set_cells(WidgetModule::Music, 5);
        assert_eq!(settings.used_cells(), 5);
        assert_eq!(settings.remaining_cells(), AppSettings::TOTAL_CELLS - 5);
        assert_eq!(settings.max_cells_for(WidgetModule::Music), 8);
    }

    #[test]
    fn enabled_widget_can_grow_when_the_scrollable_row_exceeds_the_cell_budget() {
        let settings = AppSettings::default();
        assert_eq!(settings.remaining_cells(), 0);
        assert_eq!(settings.cells_for(WidgetModule::Music), 5);
        assert_eq!(
            settings.max_cells_for(WidgetModule::Music),
            WidgetModule::Music.max_cells()
        );
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

    #[test]
    fn window_management_flags_default_off() {
        let parsed: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!parsed.window_snap_enabled);
        assert!(!parsed.thaw_enabled);
        assert!(!parsed.thaw_hidden);
        assert!(!parsed.snap_drag_to_edge);
    }
}
