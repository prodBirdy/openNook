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
    std::fs::create_dir_all(dir.join("themes"))?;
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

/// The island's shape: joined to the screen's edge around a notch, or a pill floating around
/// a camera cutout (the MacBook Pro with the PROLED XDR display).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IslandShape {
    /// The display's own: a pill around a narrow cutout, else the notch's shape.
    #[default]
    Auto,
    Notch,
    Pill,
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
    pub shape: IslandShape,
    /// Out of the way while an app is full screen on the island's display.
    #[serde(rename = "hideInFullScreen")]
    pub hide_in_full_screen: bool,
    /// How much of the glass the black veil covers from the top: 0 bare glass, 0.5 the camera
    /// band, 1 twice that.
    #[serde(rename = "glassTint")]
    pub glass_tint: f32,
    /// The island's own look: fill, gradient, border, glow, corners, wings, shadow.
    pub style: nook_mods::IslandStyle,
}

impl Default for IslandOptions {
    fn default() -> Self {
        Self { width: 560.0, hover: HoverMode::Peek, hover_delay_ms: 120, collapse_delay_ms: 200, glass: true, haptics: true, display: DisplayMode::Notch, shape: IslandShape::Auto, hide_in_full_screen: true, glass_tint: 0.5, style: Default::default() }
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
    /// A theme's name (`"midnight"`, or `themes/<name>.json`'s), or tokens over one:
    /// `{ "base": "midnight", "accent": "#ff9f0a" }` (no base: over the default).
    pub theme: Value,
    /// Where mods' `location()` is: this Mac, or a place set in Settings.
    pub location: LocationSetting,
}

/// `layout.json`'s `location`: `{ "mode": "live" }`, or `{ "mode": "set", "name": "Berlin",
/// "latitude": 52.52, "longitude": 13.41, "timezone": "Europe/Berlin" }` (Settings looks the
/// place up, so mods get it without asking macOS).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LocationSetting {
    pub mode: LocationMode,
    pub name: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub timezone: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LocationMode {
    #[default]
    Live,
    Set,
}

impl LocationSetting {
    /// The set place as mods see it, when one is set.
    pub fn place(&self) -> Option<Value> {
        let (latitude, longitude) = (self.latitude?, self.longitude?);
        (self.mode == LocationMode::Set).then(|| serde_json::json!({ "latitude": latitude, "longitude": longitude, "name": self.name, "timezone": self.timezone }))
    }
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

/// When layout.json or a theme file last changed: either restyles the island.
pub fn layout_stamp(dir: &Path) -> Option<std::time::SystemTime> {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let themes = std::fs::read_dir(dir.join("themes")).into_iter().flatten().flatten().filter_map(|e| modified(&e.path()));
    // The folder's own time catches a theme being deleted.
    modified(&dir.join("layout.json")).into_iter().chain(modified(&dir.join("themes"))).chain(themes).max()
}

/// Changes layout.json, read fresh so an edit in between isn't lost, and returns what it wrote.
/// The layout watcher applies it.
pub fn edit_layout(f: impl FnOnce(&mut Value)) -> Value {
    let path = app_dir().join("layout.json");
    let mut v: Value = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| serde_json::json!({}));
    if !v.is_object() {
        v = serde_json::json!({});
    }
    f(&mut v);
    let _ = write_if_changed(&path, serde_json::to_string_pretty(&v).unwrap_or_default() + "\n");
    v
}

/// A JSON merge patch (RFC 7386): objects merge key by key, null deletes, anything else
/// replaces.
pub fn merge_patch(target: &mut Value, patch: &Value) {
    let Value::Object(patch) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    let map = target.as_object_mut().expect("an object");
    for (k, v) in patch {
        if v.is_null() {
            map.remove(k);
        } else {
            merge_patch(map.entry(k.clone()).or_insert(Value::Null), v);
        }
    }
}

/// `layout.patch {patch}` from a mod: the patched layout.json, written, or why not.
pub fn patch_layout(patch: &Value) -> Result<(), String> {
    if !patch.is_object() {
        return Err("layout.patch: `patch` must be an object".into());
    }
    let path = app_dir().join("layout.json");
    let mut v: Value = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| serde_json::json!({}));
    merge_patch(&mut v, patch);
    serde_json::from_value::<Layout>(v.clone()).map_err(|e| format!("layout.patch: the result isn't a valid layout.json: {e}"))?;
    edit_layout(move |l| *l = v);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_patch_merges_replaces_and_deletes() {
        let mut v = json!({ "island": { "width": 560, "style": { "wings": 14 } }, "hidden": ["a"], "theme": "ember" });
        merge_patch(&mut v, &json!({ "island": { "style": { "wings": null, "glow": { "color": "accent" } } }, "hidden": [], "theme": null }));
        assert_eq!(v, json!({ "island": { "width": 560, "style": { "glow": { "color": "accent" } } }, "hidden": [] }));
    }

    #[test]
    fn island_style_and_theme_names_parse() {
        let l: Layout = serde_json::from_value(json!({
            "island": { "style": { "fill": "#0a1022", "border": { "width": 1, "color": "accent" }, "compactRadius": 10, "wings": 0 } },
            "theme": "midnight"
        })).unwrap();
        assert_eq!(l.island.style.compact_radius, Some(10.0));
        assert_eq!(l.island.style.wings, Some(0.0));
        assert_eq!(l.theme, json!("midnight"));
        assert!(serde_json::from_value::<Layout>(json!({ "island": { "style": { "radius": 3 } } })).is_err());
    }
}
