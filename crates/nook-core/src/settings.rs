use crate::database;
use crate::high_alert::HighAlertKind;
use crate::observe::ObserveConfig;
use crate::share::ShareSettings;
use crate::sysstats::SysStatsSettings;
use crate::weather::WeatherSettings;
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
    Messages = 11,
    Obsidian = 12,
    Weather = 14,
    Vpn = 15,
    HighAlert = 16,
    SysStats = 17,
    Recorder = 18,
    Meeting = 19,
    Notifications = 20,
}

impl WidgetModule {
    pub const ALL: [Self; 20] = [
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
        Self::Weather,
        Self::Vpn,
        Self::HighAlert,
        Self::SysStats,
        Self::Recorder,
        Self::Meeting,
        Self::Notifications,
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
            Self::Files
            | Self::Notes
            | Self::Observe
            | Self::Reminders
            | Self::Agents
            | Self::Obsidian
            | Self::SysStats
            | Self::Notifications => 4,
            Self::Recorder => 5,
            Self::Messages => 6,
            Self::Timers
            | Self::Speed
            | Self::Mirror
            | Self::Battery
            | Self::Weather
            | Self::Vpn
            | Self::HighAlert
            | Self::Meeting => 3,
        }
    }

    pub fn min_cells(self) -> u8 {
        match self {
            Self::Calendar => 4,
            Self::Music
            | Self::Files
            | Self::Observe
            | Self::Reminders
            | Self::Mirror
            | Self::SysStats
            | Self::Notifications => 3,
            Self::Messages => 4,
            Self::Notes
            | Self::Timers
            | Self::Speed
            | Self::Agents
            | Self::Battery
            | Self::Obsidian
            | Self::Weather
            | Self::Vpn
            | Self::HighAlert
            | Self::Meeting => 2,
            Self::Recorder => 3,
        }
    }

    pub fn max_cells(self) -> u8 {
        match self {
            Self::Timers
            | Self::Speed
            | Self::Mirror
            | Self::Battery
            | Self::Weather
            | Self::Vpn
            | Self::HighAlert
            | Self::Meeting => 6,
            // Raised from 6; Notes/Reminders/etc. already sit at 8 via `_`.
            Self::Agents => 7,
            // Music and Calendar stay at 8.
            _ => 8,
        }
    }

    /// Files lives on the Tray tab, not the Nook cell row.
    pub fn occupies_nook_cells(self) -> bool {
        !matches!(self, Self::Files)
    }

    /// Bundle ids of third-party apps this widget wraps. Empty for first-party.
    pub fn host_apps(self) -> &'static [&'static str] {
        match self {
            Self::Obsidian => &[crate::obsidian::BUNDLE_ID],
            Self::Meeting => &[
                crate::meetings::ZOOM_BUNDLE,
                crate::meetings::TEAMS_BUNDLE,
                crate::meetings::TEAMS_CLASSIC_BUNDLE,
            ],
            _ => &[],
        }
    }

    /// Host app installed (third-party widgets).
    pub fn is_available(self) -> bool {
        self.available_if(crate::apps::is_installed)
    }

    pub fn available_if(self, installed: impl Fn(&str) -> bool) -> bool {
        let apps = self.host_apps();
        apps.is_empty() || apps.iter().copied().any(installed)
    }

    /// Non-baseline widgets hidden unless Settings › Show experimental widgets.
    pub fn is_experimental(self) -> bool {
        matches!(
            self,
            Self::Observe
                | Self::Obsidian
                | Self::Vpn
                | Self::HighAlert
                | Self::SysStats
                | Self::Recorder
                | Self::Meeting
                | Self::Notifications
                | Self::Messages
        )
    }
}

/// Discrete width presets for the expanded Nook row (Control Center–style).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetSize {
    Small,
    Medium,
    Large,
}

impl WidgetSize {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "S",
            Self::Medium => "M",
            Self::Large => "L",
        }
    }
}

/// Saved orders may name widgets that no longer exist (e.g. the removed
/// per-app mixer or process widget). Drop those instead of failing the whole
/// settings load.
fn lenient_widget_order<'de, D>(deserializer: D) -> Result<Vec<WidgetModule>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Vec<serde_json::Value> = serde::Deserialize::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|v| serde_json::from_value::<WidgetModule>(v).ok())
        .collect())
}

fn default_widget_order() -> Vec<WidgetModule> {
    vec![
        WidgetModule::Music,
        WidgetModule::Calendar,
        WidgetModule::Mirror,
        WidgetModule::Files,
        WidgetModule::Agents,
        WidgetModule::Meeting,
        WidgetModule::Observe,
        WidgetModule::Reminders,
        WidgetModule::Timers,
        WidgetModule::Notes,
        WidgetModule::Obsidian,
        WidgetModule::Speed,
        WidgetModule::Battery,
        WidgetModule::Messages,
        WidgetModule::Weather,
        WidgetModule::Vpn,
        WidgetModule::HighAlert,
        WidgetModule::SysStats,
        WidgetModule::Recorder,
        WidgetModule::Notifications,
    ]
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct WindowSettings {
    /// Legacy copy of [`AppSettings::non_notch_mode`]; read on load, not written.
    /// Unknown legacy keys such as `extra_width` / `extra_height` are ignored.
    #[serde(default, skip_serializing)]
    pub non_notch_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppSettings {
    #[serde(
        default = "default_widget_order",
        deserialize_with = "lenient_widget_order"
    )]
    pub widget_order: Vec<WidgetModule>,
    #[serde(default = "default_true")]
    pub show_media: bool,
    /// Opt-in time-synced lyrics beside Now Playing (LRCLIB, cached locally).
    #[serde(default)]
    pub show_lyrics: bool,
    /// Opt-in browser tab artwork and Google site icons.
    #[serde(default)]
    pub browser_artwork: bool,
    /// Upcoming list on the expanded Music pane. Off hides Music queue
    /// fetch entirely (no extra osascript when the card is open). Spotify
    /// has no local queue; the list control is Music-only.
    #[serde(default = "default_true")]
    pub show_media_queue: bool,
    /// Spotify developer-app client ID for PKCE. No client secret is stored.
    #[serde(default)]
    pub spotify_client_id: String,
    #[serde(default = "default_true")]
    pub show_calendar: bool,
    #[serde(default)]
    pub show_reminders: bool,
    /// Natural-language quick-add row on the Calendar and Reminders cards.
    #[serde(default = "default_true")]
    pub quick_add: bool,
    #[serde(default)]
    pub show_agents: bool,
    #[serde(default)]
    pub show_observe: bool,
    #[serde(default = "default_true")]
    pub show_timers: bool,
    #[serde(default)]
    pub show_notes: bool,
    #[serde(default)]
    pub show_speed: bool,
    #[serde(default = "default_true")]
    pub show_files: bool,
    #[serde(default)]
    pub show_mirror: bool,
    #[serde(default)]
    pub show_battery: bool,
    /// Percent at or below which the compact face takes over while discharging.
    #[serde(default = "default_battery_alert_threshold")]
    pub battery_alert_threshold: u8,
    /// Shortcuts.app name for the one-tap LPM toggle. `None` or a missing
    /// shortcut falls back to the osascript-admin prompt.
    #[serde(default = "default_lpm_shortcut_name")]
    pub lpm_shortcut_name: Option<String>,
    #[serde(default)]
    pub show_messages: bool,
    /// Fragile Accessibility CGEvent Return after opening `whatsapp://`.
    #[serde(default)]
    pub experimental_whatsapp_autosend: bool,
    /// Reveal unfinished widgets (Observe, Obsidian, VPN, …) in Customize/Settings.
    #[serde(default)]
    pub experimental_widgets: bool,
    /// Mirror Apple Clock timers in the Timers widget (plist / vnode watch).
    #[serde(default = "default_true")]
    pub sync_clock_timers: bool,
    #[serde(default)]
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
    pub weather: WeatherSettings,
    #[serde(default)]
    pub show_vpn: bool,
    /// Elapsed session clock on the compact VPN face.
    #[serde(default = "default_true")]
    pub vpn_show_timer: bool,
    /// Interface names the classifier must ignore (utun helpers, ZTNA, etc.).
    #[serde(default)]
    pub vpn_ignore_interfaces: Vec<String>,
    #[serde(default)]
    pub show_high_alert: bool,
    /// Seconds; `0` means until turned off. Default is 30 minutes — never forever.
    #[serde(default = "default_high_alert_duration")]
    pub high_alert_default_duration_secs: u32,
    #[serde(default)]
    pub high_alert_kind: HighAlertKind,
    /// Auto-release the assertion at or below this battery percent. `0` disables.
    #[serde(default = "default_low_battery_pct")]
    pub low_battery_release_pct: u8,
    #[serde(default = "default_pomo_work")]
    pub pomodoro_work_secs: u32,
    #[serde(default = "default_pomo_break")]
    pub pomodoro_break_secs: u32,
    #[serde(default = "default_pomo_long")]
    pub pomodoro_long_break_secs: u32,
    #[serde(default = "default_pomo_cycles")]
    pub pomodoro_cycles_per_long: u8,
    #[serde(default = "default_true")]
    pub pomodoro_auto_advance: bool,
    #[serde(default = "default_true")]
    pub pomodoro_keep_awake: bool,
    #[serde(default)]
    pub focus_shortcut_work: Option<String>,
    #[serde(default)]
    pub focus_shortcut_break: Option<String>,
    #[serde(default)]
    pub show_sysstats: bool,
    #[serde(default)]
    pub sysstats: SysStatsSettings,
    #[serde(default)]
    pub show_recorder: bool,
    /// On-device Speech while recording. Off = record-only (cheaper).
    #[serde(default = "default_true")]
    pub recorder_transcribe: bool,
    #[serde(default)]
    pub show_meetings: bool,
    #[serde(default)]
    pub meetings: MeetingsConfig,
    /// Off by default: Accessibility (and optional Full Disk Access) must be
    /// granted before the shelf can see other apps' notifications.
    #[serde(default)]
    pub show_notifications: bool,
    /// User opted into reading the usernoted SQLite store. Still requires a
    /// manual Full Disk Access grant — there is no programmatic prompt.
    #[serde(default)]
    pub notification_fda_opt_in: bool,
    /// Bundle IDs (or app names) hidden from the shelf.
    #[serde(default)]
    pub notification_blocked_apps: Vec<String>,
    #[serde(default)]
    pub observe: ObserveConfig,
    #[serde(default)]
    pub liquid_glass_mode: bool,
    /// How far the expanded Liquid Glass sheet stays black before it opens
    /// into the material. `0` is bare glass, `1` holds black the longest.
    /// Compact pills ignore this.
    #[serde(default = "default_glass_gradient")]
    pub liquid_glass_gradient: f32,
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
    #[serde(default = "default_widget_widths")]
    pub widget_widths: Vec<(WidgetModule, u8)>,
    #[serde(default)]
    pub share: ShareSettings,
    /// Termi-Notch interactive login-shell card. Off until the user opts in —
    /// this is an arbitrary-code-execution surface and must stay unreachable
    /// from `opennook://` URLs, the CLI, and Finder Services.
    #[serde(default)]
    pub terminal_enabled: bool,
    /// Login shell used for `-l`. Empty means `$SHELL`.
    #[serde(default)]
    pub terminal_shell: String,
    /// Monospace font family for the terminal card. Empty means the built-in
    /// stack (SF Mono → Menlo → Monaco). Any installed family name works.
    #[serde(default)]
    pub terminal_font: String,
    /// Terminal font size in points.
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: f32,
    /// Persist typed commands in the settings DB. Off by default.
    #[serde(default)]
    pub terminal_history: bool,
    /// AirPlay / output-device picker on the expanded media card. CoreAudio
    /// HAL only — cannot initiate a new AirPlay route to a HomePod / Apple TV.
    #[serde(default = "default_true")]
    pub audio_output_picker: bool,
    /// Network lookup for Apple Music editorialVideo loops. Opt-in; ToS-gray.
    #[serde(default)]
    pub animated_album_art: bool,
    /// Local dominant-color glow behind the expanded media card.
    #[serde(default = "default_true")]
    pub ambient_art_glow: bool,
    #[serde(default)]
    pub window: WindowSettings,
}

fn default_terminal_font_size() -> f32 {
    11.0
}

fn default_island_x() -> f32 {
    0.5
}

fn default_glass_gradient() -> f32 {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MeetControlMode {
    /// Activate the Meet tab then send Cmd+D / Cmd+E (focus-stealing).
    #[default]
    FocusTab,
    /// Chrome/Safari `execute javascript` — needs "Allow JavaScript from Apple Events".
    AppleEventsJs,
}

impl MeetControlMode {
    pub fn caption(self) -> &'static str {
        match self {
            Self::FocusTab => "Focus tab",
            Self::AppleEventsJs => "Apple Events JS",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeetingsConfig {
    #[serde(default = "default_true")]
    pub zoom: bool,
    #[serde(default = "default_true")]
    pub teams: bool,
    #[serde(default = "default_true")]
    pub meet: bool,
    #[serde(default)]
    pub meet_mode: MeetControlMode,
}

impl Default for MeetingsConfig {
    fn default() -> Self {
        Self {
            zoom: true,
            teams: true,
            meet: true,
            meet_mode: MeetControlMode::FocusTab,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_battery_alert_threshold() -> u8 {
    20
}

fn default_lpm_shortcut_name() -> Option<String> {
    Some(crate::power::default_lpm_shortcut_name().into())
}
fn default_high_alert_duration() -> u32 {
    30 * 60
}

fn default_low_battery_pct() -> u8 {
    10
}

fn default_pomo_work() -> u32 {
    25 * 60
}

fn default_pomo_break() -> u32 {
    5 * 60
}

fn default_pomo_long() -> u32 {
    15 * 60
}

fn default_pomo_cycles() -> u8 {
    4
}
fn default_widget_widths() -> Vec<(WidgetModule, u8)> {
    // 11 of TOTAL_CELLS (17) — leaves room for three small widgets.
    vec![
        (WidgetModule::Music, 5),
        (WidgetModule::Calendar, 4),
        (WidgetModule::Timers, 2),
    ]
}

/// Greedy first-fit-in-order packing: walk `items`, append to the current row
/// while the row's cell sum stays ≤ `cap`, otherwise start a new row. An item
/// wider than `cap` is clamped to `cap` and gets its own row. Never returns
/// empty rows; returns an empty `Vec` for empty input.
pub fn pack_rows(items: &[(WidgetModule, u8)], cap: u8) -> Vec<Vec<(WidgetModule, u8)>> {
    if items.is_empty() {
        return Vec::new();
    }
    let mut rows: Vec<Vec<(WidgetModule, u8)>> = Vec::new();
    let mut current: Vec<(WidgetModule, u8)> = Vec::new();
    let mut used = 0u8;
    for &(module, raw) in items {
        let cells = raw.min(cap);
        if !current.is_empty() && used.saturating_add(cells) > cap {
            rows.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push((module, cells));
        used = used.saturating_add(cells);
    }
    if !current.is_empty() {
        rows.push(current);
    }
    rows
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            widget_order: default_widget_order(),
            show_media: true,
            show_lyrics: false,
            browser_artwork: false,
            show_media_queue: true,
            spotify_client_id: String::new(),
            show_calendar: true,
            show_reminders: false,
            quick_add: true,
            show_agents: false,
            show_observe: false,
            show_timers: true,
            show_notes: false,
            show_speed: false,
            show_files: true,
            show_mirror: false,
            show_battery: false,
            battery_alert_threshold: default_battery_alert_threshold(),
            lpm_shortcut_name: default_lpm_shortcut_name(),
            show_messages: false,
            experimental_whatsapp_autosend: false,
            experimental_widgets: false,
            sync_clock_timers: true,
            show_obsidian: false,
            obsidian_vault: None,
            obsidian_capture_heading: None,
            obsidian_uri_capture: false,
            weather: WeatherSettings::default(),
            show_vpn: false,
            vpn_show_timer: true,
            vpn_ignore_interfaces: Vec::new(),
            show_high_alert: false,
            high_alert_default_duration_secs: default_high_alert_duration(),
            high_alert_kind: HighAlertKind::default(),
            low_battery_release_pct: default_low_battery_pct(),
            pomodoro_work_secs: default_pomo_work(),
            pomodoro_break_secs: default_pomo_break(),
            pomodoro_long_break_secs: default_pomo_long(),
            pomodoro_cycles_per_long: default_pomo_cycles(),
            pomodoro_auto_advance: true,
            pomodoro_keep_awake: true,
            focus_shortcut_work: None,
            focus_shortcut_break: None,
            show_sysstats: false,
            sysstats: SysStatsSettings::default(),
            show_recorder: false,
            recorder_transcribe: true,
            show_meetings: false,
            meetings: MeetingsConfig::default(),
            show_notifications: false,
            notification_fda_opt_in: false,
            notification_blocked_apps: Vec::new(),
            observe: ObserveConfig::default(),
            liquid_glass_mode: false,
            liquid_glass_gradient: default_glass_gradient(),
            non_notch_mode: false,
            island_x: default_island_x(),
            island_y: 0.0,
            hide_when_maximized: false,
            show_volume_brightness_hud: true,
            replace_system_hud: false,
            island_color: None,
            widget_widths: default_widget_widths(),
            share: ShareSettings::default(),
            terminal_enabled: false,
            terminal_shell: String::new(),
            terminal_font: String::new(),
            terminal_font_size: default_terminal_font_size(),
            terminal_history: false,
            audio_output_picker: true,
            animated_album_art: false,
            ambient_art_glow: true,
            window: WindowSettings::default(),
        }
    }
}

impl AppSettings {
    /// Slider value for the expanded glass fall, clamped to `0..=1`.
    pub fn glass_gradient(&self) -> f32 {
        if self.liquid_glass_gradient.is_finite() {
            self.liquid_glass_gradient.clamp(0.0, 1.0)
        } else {
            default_glass_gradient()
        }
    }

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

    /// Cells in the Nook row (one row; the island is 1120 pt wide).
    pub const TOTAL_CELLS: u8 = 17;
    /// Nook widgets live on a single row; packing never wraps.
    pub const MAX_ROWS: usize = 1;

    pub fn is_enabled(&self, module: WidgetModule) -> bool {
        if !module.is_available() {
            return false;
        }
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
            WidgetModule::Weather => self.weather.enabled,
            WidgetModule::Vpn => self.show_vpn,
            WidgetModule::HighAlert => self.show_high_alert,
            WidgetModule::SysStats => self.show_sysstats,
            WidgetModule::Recorder => self.show_recorder,
            WidgetModule::Meeting => self.show_meetings,
            WidgetModule::Notifications => self.show_notifications,
        }
    }

    /// Whether this widget may appear in the UI right now (baseline always;
    /// experimental only when the toggle is on). Code stays regardless.
    pub fn widget_visible(&self, module: WidgetModule) -> bool {
        !module.is_experimental() || self.experimental_widgets
    }

    pub fn notification_app_blocked(&self, id: &str) -> bool {
        self.notification_blocked_apps
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(id))
    }

    pub fn toggle_notification_app(&mut self, id: &str) {
        if let Some(index) = self
            .notification_blocked_apps
            .iter()
            .position(|entry| entry.eq_ignore_ascii_case(id))
        {
            self.notification_blocked_apps.remove(index);
        } else if !id.is_empty() {
            self.notification_blocked_apps.push(id.to_string());
        }
    }

    pub fn toggle_enabled(&mut self, module: WidgetModule) {
        let _ = self.set_enabled(module, !self.is_enabled(module));
    }

    /// Turn a widget on or off. Enabling fails (returns `false`) when the
    /// widget would need a Nook row beyond [`Self::MAX_ROWS`].
    pub fn set_enabled(&mut self, module: WidgetModule, on: bool) -> bool {
        if on == self.is_enabled(module) {
            return true;
        }
        if on && !self.can_enable(module) {
            return false;
        }
        self.write_enabled(module, on);
        true
    }

    /// Whether enabling `module` would still pack into at most [`Self::MAX_ROWS`].
    pub fn can_enable(&self, module: WidgetModule) -> bool {
        if !module.is_available() {
            return false;
        }
        if self.is_enabled(module) {
            return true;
        }
        if !module.occupies_nook_cells() {
            return true;
        }
        // Trial at the widget's ordered_widgets() slot — appending to nook_items()
        // can under-count rows because packing is order-sensitive.
        let mut trial = self.clone();
        trial.write_enabled(module, true);
        trial.nook_rows().len() <= Self::MAX_ROWS
    }

    fn write_enabled(&mut self, module: WidgetModule, on: bool) {
        match module {
            WidgetModule::Calendar => self.show_calendar = on,
            WidgetModule::Music => self.show_media = on,
            WidgetModule::Files => self.show_files = on,
            WidgetModule::Notes => self.show_notes = on,
            WidgetModule::Observe => self.show_observe = on,
            WidgetModule::Timers => self.show_timers = on,
            WidgetModule::Reminders => self.show_reminders = on,
            WidgetModule::Speed => self.show_speed = on,
            WidgetModule::Agents => self.show_agents = on,
            WidgetModule::Mirror => self.show_mirror = on,
            WidgetModule::Battery => self.show_battery = on,
            WidgetModule::Messages => {
                self.show_messages = on;
                if on {
                    crate::messages::start_watchers();
                }
                crate::messages::request_refresh();
            }
            WidgetModule::Obsidian => self.show_obsidian = on,
            WidgetModule::Weather => self.weather.enabled = on,
            WidgetModule::Vpn => {
                self.show_vpn = on;
                if on {
                    crate::vpn::start();
                }
            }
            WidgetModule::HighAlert => self.show_high_alert = on,
            WidgetModule::SysStats => self.show_sysstats = on,
            WidgetModule::Recorder => self.show_recorder = on,
            WidgetModule::Meeting => self.show_meetings = on,
            WidgetModule::Notifications => self.show_notifications = on,
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

    /// Set a widget's width. Returns `false` and leaves the previous width when
    /// applying the change would pack past [`Self::MAX_ROWS`].
    pub fn set_cells(&mut self, module: WidgetModule, cells: u8) -> bool {
        let cells = cells.clamp(module.min_cells(), self.max_cells_for(module));
        let previous = self.cells_for(module);
        if let Some(entry) = self
            .widget_widths
            .iter_mut()
            .find(|(item, _)| *item == module)
        {
            entry.1 = cells;
        } else {
            self.widget_widths.push((module, cells));
        }
        if self.is_enabled(module)
            && module.occupies_nook_cells()
            && self.nook_rows().len() > Self::MAX_ROWS
        {
            if let Some(entry) = self
                .widget_widths
                .iter_mut()
                .find(|(item, _)| *item == module)
            {
                entry.1 = previous;
            }
            return false;
        }
        true
    }

    /// Map current cells onto S / M / L using min / default / one-row-capped max.
    pub fn size_for(&self, module: WidgetModule) -> WidgetSize {
        let cells = self.cells_for(module);
        let min = module.min_cells();
        let max = self.max_cells_for(module);
        let mid = module.default_cells().clamp(min, max);
        let dist = |a: u8, b: u8| (a as i16 - b as i16).unsigned_abs();
        let to_min = dist(cells, min);
        let to_mid = dist(cells, mid);
        let to_max = dist(cells, max);
        if to_min <= to_mid && to_min <= to_max {
            WidgetSize::Small
        } else if to_max < to_mid {
            WidgetSize::Large
        } else {
            WidgetSize::Medium
        }
    }

    pub fn set_size(&mut self, module: WidgetModule, size: WidgetSize) -> bool {
        let min = module.min_cells();
        let def = module.default_cells().max(min);
        let max = self.max_cells_for(module);
        let cells = match size {
            WidgetSize::Small => min,
            WidgetSize::Medium => def.min(max),
            WidgetSize::Large => max,
        };
        self.set_cells(module, cells)
    }

    /// Max width a widget may grow to, capped by one row ([`Self::TOTAL_CELLS`]).
    pub fn max_cells_for(&self, module: WidgetModule) -> u8 {
        module.max_cells().min(Self::TOTAL_CELLS)
    }

    /// Enabled Nook widgets in [`Self::ordered_widgets`] order with their widths.
    pub fn nook_items(&self) -> Vec<(WidgetModule, u8)> {
        self.ordered_widgets()
            .into_iter()
            .filter(|module| module.occupies_nook_cells() && self.is_enabled(*module))
            .map(|module| (module, self.cells_for(module)))
            .collect()
    }

    /// [`nook_items`] packed into rows of at most [`Self::TOTAL_CELLS`] cells.
    pub fn nook_rows(&self) -> Vec<Vec<(WidgetModule, u8)>> {
        pack_rows(&self.nook_items(), Self::TOTAL_CELLS)
    }

    /// Number of Nook rows, at least 1 even when empty.
    pub fn nook_row_count(&self) -> usize {
        self.nook_rows().len().max(1)
    }

    /// Sum of cells across all enabled Nook widgets.
    pub fn used_cells(&self) -> u8 {
        self.nook_items()
            .into_iter()
            .map(|(_, cells)| cells)
            .fold(0u8, |sum, cells| sum.saturating_add(cells))
    }

    /// Cells still free in the last packed row ([`Self::TOTAL_CELLS`] when empty).
    pub fn remaining_cells(&self) -> u8 {
        match self.nook_rows().last() {
            None => Self::TOTAL_CELLS,
            Some(row) => {
                let used = row
                    .iter()
                    .map(|(_, cells)| *cells)
                    .fold(0u8, |sum, cells| sum.saturating_add(cells));
                Self::TOTAL_CELLS.saturating_sub(used)
            }
        }
    }

    /// Disable trailing Nook widgets until the layout fits within [`Self::MAX_ROWS`].
    pub fn clamp_to_budget(&mut self) {
        while self.nook_rows().len() > Self::MAX_ROWS {
            let Some((module, _)) = self.nook_items().into_iter().last() else {
                break;
            };
            self.write_enabled(module, false);
        }
    }

    /// Pixel width of the widest packed Nook row (insets + cells + dividers).
    pub fn nook_content_width(&self, cell_px: f32, divider_px: f32, inset_px: f32) -> f32 {
        let rows = self.nook_rows();
        if rows.is_empty() {
            return inset_px * 2.0;
        }
        rows.iter()
            .map(|row| {
                let cells: f32 = row.iter().map(|(_, c)| *c as f32).sum();
                let dividers = (row.len().saturating_sub(1)) as f32 * divider_px;
                inset_px * 2.0 + cells * cell_px + dividers
            })
            .fold(0.0_f32, f32::max)
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

    /// Like [`Self::move_widget_to`], but refuses a reorder that would pack past
    /// [`Self::MAX_ROWS`] and leaves `widget_order` unchanged.
    pub fn try_move_widget_to(&mut self, module: WidgetModule, target: WidgetModule) -> bool {
        let snapshot = self.widget_order.clone();
        self.move_widget_to(module, target);
        if self.nook_rows().len() > Self::MAX_ROWS {
            self.widget_order = snapshot;
            return false;
        }
        true
    }

    /// Drop an app from the customize dock onto a filled slot.
    ///
    /// Already-enabled widgets reorder to `target`. New widgets replace
    /// `target` at their current width; the swap reverts if it would exceed
    /// [`Self::MAX_ROWS`].
    pub fn place_widget_on(&mut self, incoming: WidgetModule, target: WidgetModule) -> bool {
        if !incoming.occupies_nook_cells() || !incoming.is_available() {
            return false;
        }
        if incoming == target {
            return true;
        }
        if self.is_enabled(incoming) {
            return self.try_move_widget_to(incoming, target);
        }
        if !self.is_enabled(target) || !target.occupies_nook_cells() {
            return false;
        }

        self.write_enabled(target, false);
        self.write_enabled(incoming, true);
        self.move_widget_to(incoming, target);
        if self.nook_rows().len() > Self::MAX_ROWS {
            self.write_enabled(incoming, false);
            self.write_enabled(target, true);
            return false;
        }
        true
    }

    /// Drop an app onto an empty dashed slot — enable if the budget allows.
    pub fn place_widget_append(&mut self, incoming: WidgetModule) -> bool {
        if self.is_enabled(incoming) {
            return true;
        }
        self.set_enabled(incoming, true)
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

pub fn get_app_settings() -> AppSettings {
    app_store()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
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
    crate::ui_tick::poke();
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
            let over_budget = settings.nook_rows().len() > AppSettings::MAX_ROWS;
            if over_budget {
                settings.clamp_to_budget();
            }
            if let Ok(mut guard) = app_store().write() {
                *guard = settings.clone();
            }
            if let Ok(mut win) = window_store().write() {
                *win = settings.window;
            }
            if filled_url || legacy_token || over_budget {
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

/// Show first-run tips on the next island launch.
pub fn reset_onboarded() -> Result<(), String> {
    let conn = database::get_connection().map_err(|err| err.to_string())?;
    conn.execute("DELETE FROM settings WHERE key = ?1", ["onboarded"])
        .map_err(|err| err.to_string())?;
    Ok(())
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
    fn unknown_settings_are_ignored() {
        let parsed: AppSettings =
            serde_json::from_str(r#"{"retired_feature":true,"search":{"enabled":true}}"#).unwrap();
        assert_eq!(parsed, AppSettings::default());
    }

    #[test]
    fn empty_json_matches_default() {
        let parsed: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, AppSettings::default());
    }

    #[test]
    fn missing_widget_flags_follow_curated_defaults() {
        let parsed: AppSettings = serde_json::from_str(r#"{"liquid_glass_mode":true}"#).unwrap();
        assert_eq!(parsed.widget_order, default_widget_order());
        assert!(parsed.show_media);
        assert!(!parsed.show_lyrics);
        assert!(parsed.show_media_queue);
        assert!(parsed.spotify_client_id.is_empty());
        assert!(parsed.show_calendar);
        assert!(!parsed.show_reminders);
        assert!(parsed.quick_add);
        assert!(!parsed.show_agents);
        assert!(!parsed.show_observe);
        assert!(parsed.show_timers);
        assert!(!parsed.show_notes);
        assert!(!parsed.show_speed);
        assert!(parsed.show_files);
        assert!(!parsed.show_mirror);
        assert!(!parsed.show_battery);
        assert_eq!(parsed.battery_alert_threshold, 20);
        assert_eq!(
            parsed.lpm_shortcut_name.as_deref(),
            Some(crate::power::default_lpm_shortcut_name())
        );
        assert!(!parsed.show_messages);
        assert!(!parsed.experimental_whatsapp_autosend);
        assert!(parsed.sync_clock_timers);
        assert!(!parsed.show_obsidian);
        assert_eq!(parsed.obsidian_vault, None);
        assert_eq!(parsed.obsidian_capture_heading, None);
        assert!(!parsed.obsidian_uri_capture);
        assert!(!parsed.weather.enabled);
        assert!(parsed.weather.show_on_compact_face);
        assert!(!parsed.show_vpn);
        assert!(parsed.vpn_show_timer);
        assert!(parsed.vpn_ignore_interfaces.is_empty());
        assert!(!parsed.show_high_alert);
        assert_eq!(parsed.high_alert_default_duration_secs, 30 * 60);
        assert_eq!(parsed.high_alert_kind, HighAlertKind::Display);
        assert_eq!(parsed.low_battery_release_pct, 10);
        assert_eq!(parsed.pomodoro_work_secs, 25 * 60);
        assert_eq!(parsed.pomodoro_break_secs, 5 * 60);
        assert_eq!(parsed.pomodoro_long_break_secs, 15 * 60);
        assert_eq!(parsed.pomodoro_cycles_per_long, 4);
        assert!(parsed.pomodoro_auto_advance);
        assert!(parsed.pomodoro_keep_awake);
        assert_eq!(parsed.focus_shortcut_work, None);
        assert!(!parsed.show_sysstats);
        assert_eq!(parsed.sysstats, SysStatsSettings::default());
        assert!(!parsed.show_recorder);
        assert!(parsed.recorder_transcribe);
        assert!(!parsed.show_meetings);
        assert!(parsed.meetings.zoom);
        assert!(parsed.meetings.teams);
        assert!(parsed.meetings.meet);
        assert_eq!(parsed.meetings.meet_mode, MeetControlMode::FocusTab);
        assert!(!parsed.show_notifications);
        assert!(!parsed.notification_fda_opt_in);
        assert!(parsed.notification_blocked_apps.is_empty());
        assert!(parsed.liquid_glass_mode);
        assert!((parsed.liquid_glass_gradient - default_glass_gradient()).abs() < f32::EPSILON);
        assert!(!parsed.non_notch_mode);
        assert!((parsed.island_x - 0.5).abs() < f32::EPSILON);
        assert_eq!(parsed.island_y, 0.0);
        assert!(!parsed.hide_when_maximized);
        assert!(parsed.show_volume_brightness_hud);
        assert!(!parsed.replace_system_hud);
        assert_eq!(parsed.island_color, None);
        assert_eq!(parsed.share.device_alias, "openNook");
        assert!(parsed.share.localsend_pin.is_empty());
        assert!(!parsed.terminal_enabled);
        assert!(parsed.terminal_shell.is_empty());
        assert!(!parsed.terminal_history);
        assert!(!parsed.animated_album_art);
        assert!(parsed.ambient_art_glow);
        assert_eq!(parsed.used_cells(), 11);
        assert_eq!(parsed.remaining_cells(), 6);
        assert_eq!(parsed.nook_row_count(), 1);
    }

    #[test]
    fn glass_gradient_clamps_and_defaults() {
        let s = AppSettings::default();
        assert!((s.glass_gradient() - 0.5).abs() < f32::EPSILON);
        let mut high = s.clone();
        high.liquid_glass_gradient = 4.0;
        assert_eq!(high.glass_gradient(), 1.0);
        high.liquid_glass_gradient = f32::NAN;
        assert!((high.glass_gradient() - 0.5).abs() < f32::EPSILON);
        let parsed: AppSettings =
            serde_json::from_str(r#"{"liquid_glass_gradient":0.25}"#).unwrap();
        assert!((parsed.glass_gradient() - 0.25).abs() < f32::EPSILON);
        let missing: AppSettings = serde_json::from_str("{}").unwrap();
        assert!((missing.glass_gradient() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn browser_artwork_is_opt_in() {
        assert!(!AppSettings::default().browser_artwork);
        assert!(
            !serde_json::from_str::<AppSettings>("{}")
                .unwrap()
                .browser_artwork
        );
        assert!(
            serde_json::from_str::<AppSettings>(r#"{"browser_artwork":true}"#)
                .unwrap()
                .browser_artwork
        );
    }

    #[test]
    fn motion_art_toggles_default_network_off_aura_on() {
        let parsed: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!parsed.animated_album_art);
        assert!(parsed.ambient_art_glow);
        assert_eq!(
            parsed.animated_album_art,
            AppSettings::default().animated_album_art
        );
        assert_eq!(
            parsed.ambient_art_glow,
            AppSettings::default().ambient_art_glow
        );
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
    fn place_widget_on_replaces_target_slot() {
        let mut settings = AppSettings::default();
        assert!(settings.show_media);
        assert!(settings.show_calendar);
        assert_eq!(settings.remaining_cells(), 6);

        assert!(settings.place_widget_on(WidgetModule::Battery, WidgetModule::Calendar));
        assert!(settings.show_battery);
        assert!(!settings.show_calendar);
        assert!(settings.show_media);
        assert!(settings.nook_rows().len() <= AppSettings::MAX_ROWS);

        let order = settings.ordered_widgets();
        let battery = order.iter().position(|m| *m == WidgetModule::Battery);
        let music = order.iter().position(|m| *m == WidgetModule::Music);
        assert!(battery.is_some() && music.is_some());
    }

    #[test]
    fn place_widget_on_reorders_when_already_enabled() {
        let mut settings = AppSettings::default();
        assert!(settings.show_media);
        assert!(settings.show_timers);
        let used = settings.used_cells();
        assert!(settings.place_widget_on(WidgetModule::Music, WidgetModule::Timers));
        assert!(settings.show_media);
        assert!(settings.show_timers);
        assert_eq!(settings.used_cells(), used);
        // Music lands at Timers' former index in the saved order.
        let order = settings.ordered_widgets();
        let music = order
            .iter()
            .position(|m| *m == WidgetModule::Music)
            .unwrap();
        let timers = order
            .iter()
            .position(|m| *m == WidgetModule::Timers)
            .unwrap();
        assert!((music as isize - timers as isize).abs() <= 1);
    }

    #[test]
    fn place_widget_append_respects_budget() {
        let mut settings = AppSettings::default();
        // Defaults use 11 of 17; Battery (3) still fits on the same row.
        assert!(settings.place_widget_append(WidgetModule::Battery));
        assert!(settings.show_battery);
        assert_eq!(settings.nook_rows().len(), 1);
        assert_eq!(settings.nook_row_count(), 1);
    }

    #[test]
    fn widget_order_moves_and_repairs_saved_values() {
        let mut settings = AppSettings {
            widget_order: vec![WidgetModule::Music, WidgetModule::Music],
            ..Default::default()
        };
        assert_eq!(settings.ordered_widgets().len(), WidgetModule::ALL.len());

        settings.move_widget_to(WidgetModule::Music, WidgetModule::Files);
        assert_eq!(settings.widget_order[2], WidgetModule::Music);
        settings.move_widget_to(WidgetModule::Music, WidgetModule::Calendar);
        assert_eq!(settings.widget_order[0], WidgetModule::Music);
    }

    #[test]
    fn cells_default_clamp_and_budget() {
        let mut settings = AppSettings::default();
        assert_eq!(settings.cells_for(WidgetModule::Calendar), 4);
        assert_eq!(settings.cells_for(WidgetModule::Timers), 2);
        settings.set_cells(WidgetModule::Calendar, 1);
        assert_eq!(
            settings.cells_for(WidgetModule::Calendar),
            WidgetModule::Calendar.min_cells()
        );
        // Growing past one row is clamped by max_cells_for (one-row cap).
        settings.set_cells(WidgetModule::Calendar, 99);
        assert_eq!(
            settings.cells_for(WidgetModule::Calendar),
            settings.max_cells_for(WidgetModule::Calendar)
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
        settings.weather.enabled = false;
        settings.show_vpn = false;
        settings.show_high_alert = false;
        settings.show_sysstats = false;
        settings.show_recorder = false;
        settings.show_meetings = false;
        settings.show_notifications = false;
        settings.set_cells(WidgetModule::Music, 5);
        assert_eq!(settings.used_cells(), 5);
        assert_eq!(settings.remaining_cells(), AppSettings::TOTAL_CELLS - 5);
        assert_eq!(settings.max_cells_for(WidgetModule::Music), 8);
    }

    #[test]
    fn pack_rows_wraps_at_cap() {
        let items = [
            (WidgetModule::Music, 5),
            (WidgetModule::Calendar, 4),
            (WidgetModule::Timers, 2),
            (WidgetModule::Battery, 3),
            (WidgetModule::Weather, 3),
            (WidgetModule::Speed, 4),
        ];
        let rows = pack_rows(&items, 11);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].iter().map(|(_, c)| *c).collect::<Vec<_>>(),
            vec![5, 4, 2]
        );
        assert_eq!(
            rows[1].iter().map(|(_, c)| *c).collect::<Vec<_>>(),
            vec![3, 3, 4]
        );
        let wide = pack_rows(&[(WidgetModule::Music, 12)], 11);
        assert_eq!(wide.len(), 1);
        assert_eq!(wide[0][0].1, 11);
    }

    #[test]
    fn max_cells_for_is_capped_by_one_row() {
        let settings = AppSettings::default();
        assert_eq!(settings.remaining_cells(), 6);
        assert_eq!(
            settings.max_cells_for(WidgetModule::Music),
            WidgetModule::Music
                .max_cells()
                .min(AppSettings::TOTAL_CELLS)
        );
    }

    #[test]
    fn reorder_of_a_fitting_set_never_exceeds_one_row() {
        // Sum 5+4+2+3 = 14 ≤ 17, so any reorder still packs into one row.
        let mut settings = AppSettings::default();
        assert!(settings.set_enabled(WidgetModule::Battery, true));
        assert_eq!(settings.nook_rows().len(), 1);
        let order_before = settings.widget_order.clone();
        assert!(settings.try_move_widget_to(WidgetModule::Battery, WidgetModule::Music));
        assert_ne!(settings.widget_order, order_before);
        assert_eq!(settings.nook_rows().len(), 1);
        assert_eq!(settings.nook_row_count(), 1);
    }

    #[test]
    fn set_size_large_refused_when_it_would_overflow_row() {
        let mut settings = AppSettings::default();
        // Defaults 11 + Battery 3 + Weather 3 = 17. Growing Battery to Large (6)
        // would need 20 cells and wrap — refused under MAX_ROWS = 1.
        assert!(settings.set_enabled(WidgetModule::Battery, true));
        assert!(settings.set_enabled(WidgetModule::Weather, true));
        assert_eq!(settings.used_cells(), AppSettings::TOTAL_CELLS);
        assert_eq!(settings.nook_rows().len(), 1);
        let before = settings.cells_for(WidgetModule::Battery);
        assert!(!settings.set_size(WidgetModule::Battery, WidgetSize::Large));
        assert_eq!(settings.cells_for(WidgetModule::Battery), before);
        assert_eq!(settings.nook_rows().len(), 1);
    }

    #[test]
    fn can_enable_refuses_when_row_is_full() {
        let mut settings = AppSettings::default();
        settings.experimental_widgets = true;
        // Defaults 11 + Battery 3 + Weather 3 = 17; HighAlert needs another row.
        assert!(settings.set_enabled(WidgetModule::Battery, true));
        assert!(settings.set_enabled(WidgetModule::Weather, true));
        assert_eq!(settings.used_cells(), AppSettings::TOTAL_CELLS);
        assert_eq!(settings.nook_row_count(), 1);
        assert!(!settings.can_enable(WidgetModule::HighAlert));
        assert!(!settings.set_enabled(WidgetModule::HighAlert, true));
        assert!(settings.set_enabled(WidgetModule::Weather, false));
        assert!(settings.can_enable(WidgetModule::HighAlert));
        assert!(settings.set_enabled(WidgetModule::HighAlert, true));
    }

    #[test]
    fn clamp_to_budget_disables_trailing_widgets_beyond_one_row() {
        let mut settings = AppSettings::default();
        settings.experimental_widgets = true;
        // Defaults first so clamp drops later-added extras, not Timers
        // (Timers sits after Mirror/Agents/Reminders in default_widget_order).
        settings.widget_order = vec![
            WidgetModule::Music,
            WidgetModule::Calendar,
            WidgetModule::Timers,
            WidgetModule::Battery,
            WidgetModule::Weather,
            WidgetModule::Speed,
            WidgetModule::Agents,
            WidgetModule::Mirror,
            WidgetModule::Reminders,
            WidgetModule::HighAlert,
            WidgetModule::Notes,
        ];
        for module in [
            WidgetModule::Battery,
            WidgetModule::Weather,
            WidgetModule::Speed,
            WidgetModule::Agents,
            WidgetModule::Mirror,
            WidgetModule::Reminders,
            WidgetModule::HighAlert,
            WidgetModule::Notes,
        ] {
            settings.write_enabled(module, true);
        }
        assert!(settings.nook_rows().len() > AppSettings::MAX_ROWS);
        settings.clamp_to_budget();
        assert_eq!(settings.nook_rows().len(), 1);
        assert!(settings.used_cells() <= AppSettings::TOTAL_CELLS);
        // Leading defaults fit in 17 and stay; everything after the overflow is off.
        assert!(settings.show_media);
        assert!(settings.show_calendar);
        assert!(settings.show_timers);
        assert!(!settings.show_high_alert);
        assert!(!settings.show_notes);
    }

    #[test]
    fn remaining_cells_is_last_row_remainder() {
        let settings = AppSettings::default();
        assert_eq!(settings.used_cells(), 11);
        assert_eq!(settings.remaining_cells(), 6);
        assert_eq!(settings.nook_row_count(), 1);

        let mut settings = settings;
        settings.set_cells(WidgetModule::Weather, 3);
        assert!(settings.set_enabled(WidgetModule::Weather, true));
        assert_eq!(settings.remaining_cells(), 3);
        assert_eq!(settings.nook_rows().len(), 1);

        settings.set_cells(WidgetModule::Battery, 3);
        assert!(settings.set_enabled(WidgetModule::Battery, true));
        assert_eq!(settings.remaining_cells(), 0);
        assert_eq!(settings.nook_rows().len(), 1);
    }

    #[test]
    fn size_presets_map_to_cells() {
        let mut settings = AppSettings {
            show_calendar: false,
            show_timers: false,
            ..Default::default()
        };
        settings.set_size(WidgetModule::Music, WidgetSize::Small);
        assert_eq!(
            settings.cells_for(WidgetModule::Music),
            WidgetModule::Music.min_cells()
        );
        assert_eq!(settings.size_for(WidgetModule::Music), WidgetSize::Small);
        settings.set_size(WidgetModule::Music, WidgetSize::Medium);
        assert_eq!(
            settings.cells_for(WidgetModule::Music),
            WidgetModule::Music.default_cells()
        );
        settings.set_size(WidgetModule::Music, WidgetSize::Large);
        assert_eq!(
            settings.cells_for(WidgetModule::Music),
            settings.max_cells_for(WidgetModule::Music)
        );
    }

    #[test]
    fn notifications_default_off_and_filter_toggles() {
        let parsed: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!parsed.show_notifications);
        assert!(!parsed.notification_fda_opt_in);
        assert!(!parsed.is_enabled(WidgetModule::Notifications));
        let mut settings = AppSettings::default();
        assert!(settings.set_enabled(WidgetModule::Calendar, false));
        assert!(settings.set_enabled(WidgetModule::Timers, false));
        assert!(settings.set_enabled(WidgetModule::Notifications, true));
        assert!(settings.show_notifications);
        settings.toggle_notification_app("com.apple.mail");
        assert!(settings.notification_app_blocked("com.apple.mail"));
        settings.toggle_notification_app("com.apple.mail");
        assert!(!settings.notification_app_blocked("com.apple.mail"));
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
    fn audio_output_picker_defaults_on() {
        let parsed: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(parsed.audio_output_picker);
        assert_eq!(
            parsed.audio_output_picker,
            AppSettings::default().audio_output_picker
        );
    }

    #[test]
    fn third_party_widgets_need_their_host_app() {
        assert_eq!(
            WidgetModule::Obsidian.host_apps(),
            &[crate::obsidian::BUNDLE_ID]
        );
        assert_eq!(
            WidgetModule::Meeting.host_apps(),
            &[
                crate::meetings::ZOOM_BUNDLE,
                crate::meetings::TEAMS_BUNDLE,
                crate::meetings::TEAMS_CLASSIC_BUNDLE,
            ]
        );
        let third_party: Vec<_> = WidgetModule::ALL
            .iter()
            .copied()
            .filter(|module| !module.host_apps().is_empty())
            .collect();
        assert_eq!(third_party, [WidgetModule::Obsidian, WidgetModule::Meeting]);
        for module in WidgetModule::ALL {
            if module.host_apps().is_empty() {
                assert!(module.available_if(|_| false));
            }
        }
        assert!(!WidgetModule::Obsidian.available_if(|_| false));
        assert!(WidgetModule::Obsidian.available_if(|id| id == crate::obsidian::BUNDLE_ID));
        assert!(!WidgetModule::Meeting.available_if(|_| false));
        assert!(WidgetModule::Meeting.available_if(|id| id == crate::meetings::ZOOM_BUNDLE));
        assert!(WidgetModule::Meeting.available_if(|id| id == crate::meetings::TEAMS_BUNDLE));
    }
}
