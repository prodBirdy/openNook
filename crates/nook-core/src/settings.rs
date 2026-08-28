use crate::database;
use crate::high_alert::HighAlertKind;
use crate::observe::ObserveConfig;
use crate::share::ShareSettings;
use crate::weather::WeatherSettings;
use crate::sysstats::SysStatsSettings;
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
    Mixer = 10,
    Weather = 10,
    Vpn = 10,
    HighAlert = 10,
    SysStats = 10,
    Recorder = 10,
    Meeting = 10,
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
        Self::Mixer,
        Self::Weather,
        Self::Vpn,
        Self::HighAlert,
        Self::SysStats,
        Self::Recorder,
        Self::Meeting,
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
            Self::Files | Self::Notes | Self::Observe | Self::Reminders | Self::Agents | Self::Mixer => {
                4
            }
            Self::Files
            | Self::Notes
            | Self::Observe
            | Self::Reminders
            | Self::Agents
            | Self::SysStats => 4,
            Self::Files | Self::Notes | Self::Observe | Self::Reminders | Self::Agents | Self::Recorder => 4,
            Self::Timers | Self::Speed | Self::Mirror => 3,
            Self::Timers | Self::Speed | Self::Mirror | Self::Weather => 3,
            Self::Timers | Self::Speed | Self::Mirror | Self::Vpn => 3,
            Self::Timers | Self::Speed | Self::Mirror | Self::HighAlert => 3,
            Self::Timers | Self::Speed | Self::Mirror | Self::Meeting => 3,
        }
    }

    pub fn min_cells(self) -> u8 {
        match self {
            Self::Calendar => 4,
            Self::Music | Self::Files | Self::Observe | Self::Reminders | Self::Mirror => 3,
            Self::Notes | Self::Timers | Self::Speed | Self::Agents | Self::Battery => 2,
            Self::Music | Self::Files | Self::Observe | Self::Reminders | Self::Mirror | Self::Messages => {
            Self::Music | Self::Files | Self::Observe | Self::Reminders | Self::Mirror | Self::Mixer => {
                3
            }
            Self::Music
            | Self::Files
            | Self::Observe
            | Self::Reminders
            | Self::Mirror
            | Self::SysStats => 3,
            Self::Notes | Self::Timers | Self::Speed | Self::Agents => 2,
            Self::Notes | Self::Timers | Self::Speed | Self::Agents | Self::Obsidian => 2,
            Self::Notes | Self::Timers | Self::Speed | Self::Agents | Self::Weather => 2,
            Self::Notes | Self::Timers | Self::Speed | Self::Agents | Self::Vpn => 2,
            Self::Notes | Self::Timers | Self::Speed | Self::Agents | Self::HighAlert => 2,
            Self::Notes | Self::Timers | Self::Speed | Self::Agents | Self::Recorder => 2,
            Self::Notes | Self::Timers | Self::Speed | Self::Agents | Self::Meeting => 2,
        }
    }

    pub fn max_cells(self) -> u8 {
        match self {
            Self::Timers | Self::Speed | Self::Agents | Self::Mirror | Self::Battery => 6,
            Self::Timers | Self::Speed | Self::Agents | Self::Mirror | Self::Weather => 6,
            Self::Timers | Self::Speed | Self::Agents | Self::Mirror | Self::Vpn => 6,
            Self::Timers | Self::Speed | Self::Agents | Self::Mirror | Self::HighAlert => 6,
            Self::Timers | Self::Speed | Self::Agents | Self::Mirror | Self::Meeting => 6,
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
        WidgetModule::Mixer,
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
    /// Upcoming list on the expanded Music pane. Off hides Music/Spotify queue
    /// fetch entirely (no extra osascript / HTTPS when the card is open).
    #[serde(default = "default_true")]
    pub show_media_queue: bool,
    /// Spotify developer-app client ID for PKCE. No client secret is stored.
    #[serde(default)]
    pub spotify_client_id: String,
    #[serde(default = "default_true")]
    pub show_calendar: bool,
    #[serde(default = "default_true")]
    pub show_reminders: bool,
    /// Natural-language quick-add row on the Calendar and Reminders cards.
    #[serde(default = "default_true")]
    pub quick_add: bool,
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
    pub show_mixer: bool,
    #[serde(default)]
    pub weather: WeatherSettings,
    pub show_vpn: bool,
    /// Elapsed session clock on the compact VPN face.
    #[serde(default = "default_true")]
    pub vpn_show_timer: bool,
    /// Interface names the classifier must ignore (utun helpers, ZTNA, etc.).
    #[serde(default)]
    pub vpn_ignore_interfaces: Vec<String>,
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
    pub show_sysstats: bool,
    #[serde(default)]
    pub sysstats: SysStatsSettings,
    pub show_recorder: bool,
    /// On-device Speech while recording. Off = record-only (cheaper).
    #[serde(default = "default_true")]
    pub recorder_transcribe: bool,
    pub show_meetings: bool,
    #[serde(default)]
    pub meetings: MeetingsConfig,
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
    pub share: ShareSettings,
    /// Termi-Notch one-shot shell card. Off until the user opts in — this is
    /// an arbitrary-code-execution surface and must stay unreachable from
    /// `opennook://` URLs, the CLI, and Finder Services.
    #[serde(default)]
    pub terminal_enabled: bool,
    /// Login shell used for `-lc`. Empty means `$SHELL`.
    #[serde(default)]
    pub terminal_shell: String,
    #[serde(default = "default_terminal_timeout")]
    pub terminal_timeout_secs: u32,
    /// Persist typed commands in the settings DB. Off by default.
    #[serde(default)]
    pub terminal_history: bool,
    /// AirPlay / output-device picker on the expanded media card. CoreAudio
    /// HAL only — cannot initiate a new AirPlay route to a HomePod / Apple TV.
    #[serde(default = "default_true")]
    pub audio_output_picker: bool,
    /// Mechey: mechanical keyboard sounds. Opt-in; needs Input Monitoring.
    #[serde(default)]
    pub keysounds_enabled: bool,
    /// Builtin pack id (`nook-click` / `nook-thock`) or a user folder name.
    #[serde(default = "default_keysound_pack")]
    pub keysound_pack: String,
    /// 0..=1 playback gain.
    #[serde(default = "default_keysound_volume")]
    pub keysound_volume: f32,
    /// LiquidMouse: smooth pixel scrolling for discrete wheel mice.
    #[serde(default)]
    pub smooth_scroll_enabled: bool,
    /// Pixel multiplier applied to each wheel notch (0.25..=4).
    #[serde(default = "default_scroll_speed")]
    pub scroll_speed: f32,
    /// Exponential-decay time constant in seconds (0.08..=1.2).
    #[serde(default = "default_scroll_duration")]
    pub scroll_duration: f32,
    /// Negate discrete-mouse wheel deltas (Scroll Reverser).
    #[serde(default)]
    pub reverse_mouse_scroll: bool,
    /// Frontmost bundle ids that skip the scroll tap (games, VMs, remotes).
    #[serde(default)]
    pub scroll_excluded_apps: Vec<String>,
    /// Reserved: per-device overrides need private sender IDs (phase 2).
    #[serde(default)]
    pub scroll_device_overrides: std::collections::BTreeMap<String, ScrollDeviceOverride>,
    /// Network lookup for Apple Music editorialVideo loops. Opt-in; ToS-gray.
    #[serde(default)]
    pub animated_album_art: bool,
    /// Local dominant-color glow behind the expanded media card.
    #[serde(default = "default_true")]
    pub ambient_art_glow: bool,
    #[serde(default)]
    pub window: WindowSettings,
    /// Universal search + clipboard history (WP21). Clipboard capture stays
    /// off until the user opts in.
    #[serde(default)]
    pub search: SearchSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchHotkey {
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub meta: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default = "default_hotkey_key")]
    pub key: String,
}

fn default_hotkey_key() -> String {
    "Space".into()
}

impl Default for SearchHotkey {
    fn default() -> Self {
        // Option+Space — common launcher binding that does not steal Spotlight.
        Self {
            alt: true,
            ctrl: false,
            meta: false,
            shift: false,
            key: default_hotkey_key(),
        }
    }
}

impl SearchHotkey {
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("⌃");
        }
        if self.alt {
            parts.push("⌥");
        }
        if self.shift {
            parts.push("⇧");
        }
        if self.meta {
            parts.push("⌘");
        }
        let key = if self.key.is_empty() {
            "Space"
        } else {
            self.key.as_str()
        };
        parts.push(key);
        parts.join(" ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchSettings {
    /// Register the global hotkey. Search itself needs no TCC.
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub hotkey: SearchHotkey,
    /// Persist clipboard history. Default OFF — privacy.
    #[serde(default)]
    pub clipboard_history: bool,
    #[serde(default = "default_clipboard_cap")]
    pub clipboard_history_size: u32,
    /// Bundle identifiers skipped at copy time (frontmost-app heuristic).
    #[serde(default)]
    pub clipboard_exclude_apps: Vec<String>,
    /// Synthesize Cmd-V after paste-back. Off + Accessibility-gated.
    #[serde(default)]
    pub auto_paste: bool,
    /// Optional magnifier on the compact idle face.
    #[serde(default)]
    pub show_magnifier: bool,
}

fn default_clipboard_cap() -> u32 {
    500
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            hotkey: SearchHotkey::default(),
            clipboard_history: false,
            clipboard_history_size: default_clipboard_cap(),
            clipboard_exclude_apps: Vec::new(),
            auto_paste: false,
            show_magnifier: false,
        }
    }
}

fn default_terminal_timeout() -> u32 {
    30
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

    pub fn cycle(self) -> Self {
        match self {
            Self::FocusTab => Self::AppleEventsJs,
            Self::AppleEventsJs => Self::FocusTab,
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
fn default_keysound_pack() -> String {
    "nook-click".into()
}

fn default_keysound_volume() -> f32 {
    0.7
}

fn default_scroll_speed() -> f32 {
    1.0
}

fn default_scroll_duration() -> f32 {
    0.35
}

/// Best-effort per-device scroll knobs. Unused until sender-ID matching ships.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScrollDeviceOverride {
    #[serde(default)]
    pub reverse: Option<bool>,
    #[serde(default)]
    pub speed: Option<f32>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            widget_order: default_widget_order(),
            show_media: true,
            show_lyrics: false,
            show_media_queue: true,
            spotify_client_id: String::new(),
            show_calendar: true,
            show_reminders: true,
            quick_add: true,
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
            show_mixer: true,
            weather: WeatherSettings::default(),
            show_vpn: true,
            vpn_show_timer: true,
            vpn_ignore_interfaces: Vec::new(),
            show_high_alert: true,
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
            show_sysstats: true,
            sysstats: SysStatsSettings::default(),
            show_recorder: true,
            recorder_transcribe: true,
            show_meetings: true,
            meetings: MeetingsConfig::default(),
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
            share: ShareSettings::default(),
            terminal_enabled: false,
            terminal_shell: String::new(),
            terminal_timeout_secs: default_terminal_timeout(),
            terminal_history: false,
            audio_output_picker: true,
            keysounds_enabled: false,
            keysound_pack: default_keysound_pack(),
            keysound_volume: default_keysound_volume(),
            smooth_scroll_enabled: false,
            scroll_speed: default_scroll_speed(),
            scroll_duration: default_scroll_duration(),
            reverse_mouse_scroll: false,
            scroll_excluded_apps: Vec::new(),
            scroll_device_overrides: std::collections::BTreeMap::new(),
            animated_album_art: false,
            ambient_art_glow: true,
            window: WindowSettings::default(),
            search: SearchSettings::default(),
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
            WidgetModule::Mixer => self.show_mixer && crate::mixer::is_available(),
            WidgetModule::Weather => self.weather.enabled,
            WidgetModule::Vpn => self.show_vpn,
            WidgetModule::HighAlert => self.show_high_alert,
            WidgetModule::SysStats => self.show_sysstats,
            WidgetModule::Recorder => self.show_recorder,
            WidgetModule::Meeting => self.show_meetings,
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
            WidgetModule::Mixer => self.show_mixer = !self.show_mixer,
            WidgetModule::Weather => self.weather.enabled = !self.weather.enabled,
            WidgetModule::Vpn => self.show_vpn = !self.show_vpn,
            WidgetModule::HighAlert => self.show_high_alert = !self.show_high_alert,
            WidgetModule::SysStats => self.show_sysstats = !self.show_sysstats,
            WidgetModule::Recorder => self.show_recorder = !self.show_recorder,
            WidgetModule::Meeting => self.show_meetings = !self.show_meetings,
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
const SHARE_SECRET_SERVICE: &str = "com.prodBirdy.openNook.share";
#[cfg(target_os = "macos")]
const SHARE_WEBDAV_ACCOUNT: &str = "webdav-password";
#[cfg(target_os = "macos")]
const SHARE_S3_ACCESS_ACCOUNT: &str = "s3-access-key";
#[cfg(target_os = "macos")]
const SHARE_S3_SECRET_ACCOUNT: &str = "s3-secret-key";

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

#[cfg(target_os = "macos")]
fn load_share_secret(account: &str) -> Option<String> {
    security_framework::passwords::get_generic_password(SHARE_SECRET_SERVICE, account)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

#[cfg(target_os = "macos")]
fn store_share_secret(account: &str, secret: &str) -> Result<(), String> {
    if secret.is_empty() {
        let _ = security_framework::passwords::delete_generic_password(
            SHARE_SECRET_SERVICE,
            account,
        );
        Ok(())
    } else {
        security_framework::passwords::set_generic_password(
            SHARE_SECRET_SERVICE,
            account,
            secret.as_bytes(),
        )
        .map_err(|err| err.to_string())
    }
}

#[cfg(target_os = "macos")]
fn hydrate_share_secrets(share: &mut ShareSettings) {
    if share.webdav_password.is_empty() {
        if let Some(secret) = load_share_secret(SHARE_WEBDAV_ACCOUNT) {
            share.webdav_password = secret;
        }
    }
    if share.s3_access_key.is_empty() {
        if let Some(secret) = load_share_secret(SHARE_S3_ACCESS_ACCOUNT) {
            share.s3_access_key = secret;
        }
    }
    if share.s3_secret_key.is_empty() {
        if let Some(secret) = load_share_secret(SHARE_S3_SECRET_ACCOUNT) {
            share.s3_secret_key = secret;
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn hydrate_share_secrets(_share: &mut ShareSettings) {}

#[cfg(target_os = "macos")]
fn persist_share_secrets(share: &ShareSettings) -> Result<(), String> {
    store_share_secret(SHARE_WEBDAV_ACCOUNT, &share.webdav_password)?;
    store_share_secret(SHARE_S3_ACCESS_ACCOUNT, &share.s3_access_key)?;
    store_share_secret(SHARE_S3_SECRET_ACCOUNT, &share.s3_secret_key)?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn persist_share_secrets(_share: &ShareSettings) -> Result<(), String> {
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
    crate::eventtap::sync();
    crate::keysounds::sync();
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
            hydrate_share_secrets(&mut settings.share);
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
    if let Err(err) = persist_share_secrets(&settings.share) {
        log::warn!("failed to persist share secrets: {err}");
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
    fn search_defaults_keep_clipboard_off() {
        let settings = AppSettings::default();
        assert!(settings.search.enabled);
        assert!(!settings.search.clipboard_history);
        assert!(!settings.search.auto_paste);
        assert_eq!(settings.search.hotkey.label(), "⌥ Space");
        let parsed: SearchSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, SearchSettings::default());
    }

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
        assert!(parsed.show_media_queue);
        assert!(parsed.spotify_client_id.is_empty());
        assert!(parsed.show_calendar);
        assert!(parsed.show_reminders);
        assert!(parsed.quick_add);
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
        assert!(parsed.show_mixer);
        assert!(parsed.weather.enabled);
        assert!(parsed.weather.show_on_compact_face);
        assert!(parsed.show_vpn);
        assert!(parsed.vpn_show_timer);
        assert!(parsed.vpn_ignore_interfaces.is_empty());
        assert!(parsed.show_high_alert);
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
        assert!(parsed.show_sysstats);
        assert_eq!(parsed.sysstats, SysStatsSettings::default());
        assert!(parsed.show_recorder);
        assert!(parsed.recorder_transcribe);
        assert!(parsed.show_meetings);
        assert!(parsed.meetings.zoom);
        assert!(parsed.meetings.teams);
        assert!(parsed.meetings.meet);
        assert_eq!(parsed.meetings.meet_mode, MeetControlMode::FocusTab);
        assert!(parsed.liquid_glass_mode);
        assert!(!parsed.non_notch_mode);
        assert!((parsed.island_x - 0.5).abs() < f32::EPSILON);
        assert_eq!(parsed.island_y, 0.0);
        assert!(!parsed.hide_when_maximized);
        assert!(parsed.show_volume_brightness_hud);
        assert!(!parsed.replace_system_hud);
        assert_eq!(parsed.island_color, None);
        assert!(!parsed.share.localsend_receive);
        assert_eq!(parsed.share.device_alias, "openNook");
        assert_eq!(
            parsed.share.link_backend,
            crate::share::LinkBackendKind::ZeroXZero
        );
        assert!(!parsed.terminal_enabled);
        assert!(parsed.terminal_shell.is_empty());
        assert_eq!(parsed.terminal_timeout_secs, 30);
        assert!(!parsed.terminal_history);
        assert!(!parsed.animated_album_art);
        assert!(parsed.ambient_art_glow);
    }

    #[test]
    fn motion_art_toggles_default_network_off_aura_on() {
        let parsed: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!parsed.animated_album_art);
        assert!(parsed.ambient_art_glow);
        assert_eq!(parsed.animated_album_art, AppSettings::default().animated_album_art);
        assert_eq!(parsed.ambient_art_glow, AppSettings::default().ambient_art_glow);
        assert!(parsed.search.enabled);
        assert!(!parsed.search.clipboard_history);
        assert!(!parsed.search.auto_paste);
        assert_eq!(parsed.search.clipboard_history_size, 500);
        assert_eq!(parsed.search.hotkey, SearchHotkey::default());
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
        settings.show_mixer = false;
        settings.weather.enabled = false;
        settings.show_vpn = false;
        settings.show_high_alert = false;
        settings.show_sysstats = false;
        settings.show_recorder = false;
        settings.show_meetings = false;
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
    fn audio_output_picker_defaults_on() {
        let parsed: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(parsed.audio_output_picker);
        assert_eq!(
            parsed.audio_output_picker,
            AppSettings::default().audio_output_picker
        );
    fn input_feel_flags_default_off() {
        let parsed: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!parsed.keysounds_enabled);
        assert!(!parsed.smooth_scroll_enabled);
        assert!(!parsed.reverse_mouse_scroll);
        assert_eq!(parsed.keysound_pack, "nook-click");
        assert!((parsed.keysound_volume - 0.7).abs() < f32::EPSILON);
        assert!((parsed.scroll_speed - 1.0).abs() < f32::EPSILON);
        assert!((parsed.scroll_duration - 0.35).abs() < f32::EPSILON);
        assert!(parsed.scroll_excluded_apps.is_empty());
        assert!(parsed.scroll_device_overrides.is_empty());
    }
}
