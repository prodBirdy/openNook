//! `~/.config/opennook`: the island's whole interface as files an agent edits. Written on
//! every start (AGENTS.md, type files, shipped mods); layout.json is the person's.

use nook_mods::files::{self, write_if_changed};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/mods.rs"));

const AGENTS_MD: &str = include_str!("../home/AGENTS.md");
const DEFAULT_LAYOUT: &str = include_str!("../home/layout.json");
const JSCONFIG: &str = r#"{
  "compilerOptions": { "checkJs": true, "module": "es2022", "target": "es2022", "paths": { "nook": ["../../nook.d.ts"], "nook/ui": ["../../ui.d.ts"] } }
}
"#;

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into())
}

/// `$OPENNOOK_HOME`, else `$XDG_CONFIG_HOME/opennook`, else `~/.config/opennook`.
pub fn app_dir() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    env("OPENNOOK_HOME").unwrap_or_else(|| env("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config")).join("opennook"))
}

pub fn config() -> nook_mods::Config {
    nook_mods::Config::at(app_dir(), home())
}

/// Writes the guide and types, installs or updates the shipped mods, and creates layout.json
/// the first time. Returns notes for the person (edited shipped mods that were set aside).
pub fn write_workspace() -> std::io::Result<Vec<String>> {
    let dir = app_dir();
    std::fs::create_dir_all(dir.join("mods"))?;
    let icons: Vec<&str> = crate::icons::names().collect();
    let guide = AGENTS_MD.replace("{{ICONS}}", &icons.join(", "));
    write_if_changed(&dir.join("AGENTS.md"), guide)?;
    write_if_changed(&dir.join("nook.d.ts"), nook_mods::NOOK_D_TS)?;
    write_if_changed(&dir.join("ui.d.ts"), nook_mods::UI_D_TS)?;
    let clones = files::sync_shipped(&dir.join("mods"), &dir.join("installed.json"), SHIPPED_MODS)?;
    // Editors resolve `nook` and `nook/ui` to the type files.
    for found in files::scan(&dir.join("mods")) {
        let path = found.dir.join("jsconfig.json");
        if !path.exists() && !SHIPPED_MODS.iter().any(|(p, _)| p.starts_with(&format!("{}/", found.name))) {
            let _ = std::fs::write(path, JSCONFIG);
        }
    }
    if !dir.join("layout.json").exists() {
        std::fs::write(dir.join("layout.json"), DEFAULT_LAYOUT)?;
    }
    Ok(clones.into_iter().map(|c| format!("Your edits to a shipped mod were kept as mods/{c}.")).collect())
}

/// What hovering the closed island does.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase", try_from = "HoverSetting")]
pub enum HoverMode {
    /// Lean out and show the activity's detail; a click opens (1.x).
    #[default]
    Peek,
    /// Open after `hoverDelayMs`.
    Open,
    /// Nothing; a click opens.
    None,
}

/// `"hover"` as written: a mode, or the true/false of 2.0's first builds. Their `true` was the
/// shipped default, not a choice, so it means today's default, the peek.
#[derive(Deserialize)]
#[serde(untagged)]
enum HoverSetting {
    Old(bool),
    Mode(String),
}

impl TryFrom<HoverSetting> for HoverMode {
    type Error = String;
    fn try_from(v: HoverSetting) -> Result<Self, String> {
        match v {
            HoverSetting::Old(true) => Ok(Self::Peek),
            HoverSetting::Old(false) => Ok(Self::None),
            HoverSetting::Mode(m) => match m.as_str() {
                "peek" => Ok(Self::Peek),
                "open" => Ok(Self::Open),
                "none" => Ok(Self::None),
                other => Err(format!("hover: {other:?} isn't \"peek\", \"open\" or \"none\"")),
            },
        }
    }
}

/// Which display the island lives on.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DisplayMode {
    /// The display with a camera housing (the MacBook's), else the main one.
    #[default]
    Notch,
    /// The one with the menu bar's origin (System Settings › Displays: "main display").
    Main,
    /// Whichever display the pointer is on.
    Pointer,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct IslandOptions {
    /// Width of the open island, in points.
    pub width: f32,
    pub hover: HoverMode,
    #[serde(rename = "hoverDelayMs")]
    pub hover_delay_ms: u64,
    #[serde(rename = "collapseDelayMs")]
    pub collapse_delay_ms: u64,
    /// Liquid Glass under the open island (off under Reduce Transparency).
    pub glass: bool,
    /// A tap on the trackpad when a swipe opens the island.
    pub haptics: bool,
    pub display: DisplayMode,
    /// Out of the way while an app is full screen on the island's display.
    #[serde(rename = "hideInFullScreen")]
    pub hide_in_full_screen: bool,
    /// How much of the glass the black veil covers from the top: 0 bare glass, 0.5 the camera
    /// band, 1 twice that.
    #[serde(rename = "glassTint")]
    pub glass_tint: f32,
}

impl Default for IslandOptions {
    fn default() -> Self {
        Self { width: 560.0, hover: HoverMode::Peek, hover_delay_ms: 120, collapse_delay_ms: 200, glass: true, haptics: true, display: DisplayMode::Notch, hide_in_full_screen: true, glass_tint: 0.5 }
    }
}

/// `layout.json`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    pub island: IslandOptions,
    /// Who owns the closed island, first wins; unlisted mods follow by priority.
    pub compact: Vec<String>,
    /// The open island's pages, in order; unlisted mods with a page follow by name.
    pub expanded: Vec<String>,
    /// Mods loaded but never shown.
    pub hidden: Vec<String>,
    /// Per mod: `{ "props": { … } }`.
    pub mods: Map<String, Value>,
    /// Theme token overrides: `{ "accent": "#ff9f0a" }`.
    pub theme: Map<String, Value>,
}

/// Reads layout.json; on error writes `layout.error` and returns the error.
pub fn read_layout(dir: &Path) -> Result<(Layout, Value), String> {
    let path = dir.join("layout.json");
    let result = std::fs::read_to_string(&path)
        .map_err(|e| format!("layout.json: {e}"))
        .and_then(|t| {
            let raw: Value = serde_json::from_str(&t).map_err(|e| format!("layout.json: {e}"))?;
            let layout: Layout = serde_json::from_value(raw.clone()).map_err(|e| format!("layout.json: {e}"))?;
            Ok((layout, raw))
        });
    let error = dir.join("layout.error");
    match &result {
        Ok(_) => {
            let _ = std::fs::remove_file(error);
        }
        Err(e) => {
            let _ = write_if_changed(&error, format!("{e}\nThe island keeps the last layout that worked.\n"));
        }
    }
    result
}

pub fn layout_stamp(dir: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(dir.join("layout.json")).and_then(|m| m.modified()).ok()
}
