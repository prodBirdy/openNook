//! The island's colors by token. Mods name tokens ("fg", "accent", …) or hex colors.
//! `layout.json`'s `"theme"` picks a theme by name (built in, or `themes/<name>.json`), or is
//! `{ "base": "<name>", "<token>": "<color>", … }`: a theme with tokens overridden.

use rune::Color;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

const DEFAULTS: &[(&str, &str)] = &[
    ("island", "#000000"),
    ("bg", "#000000"),
    ("surface", "#1c1c1e"),
    ("raised", "#2c2c2e"),
    ("border", "#ffffff1f"),
    ("fg", "#f5f5f7"),
    ("muted", "#a1a1a6"),
    ("faint", "#6e6e73"),
    ("accent", "#0a84ff"),
    ("green", "#30d158"),
    ("yellow", "#ffd60a"),
    ("orange", "#ff9f0a"),
    ("red", "#ff453a"),
    ("blue", "#0a84ff"),
    ("purple", "#bf5af2"),
];

/// Built-in themes: tokens over the defaults. All dark: the camera housing is black whatever
/// the island's color, so each keeps the island deep enough to sit around it.
const BUILTIN: &[(&str, &[(&str, &str)])] = &[
    ("default", &[]),
    ("graphite", &[("island", "#161618"), ("bg", "#161618"), ("surface", "#232326"), ("raised", "#313135"), ("accent", "#98989f")]),
    ("midnight", &[("island", "#0a1022"), ("bg", "#0a1022"), ("surface", "#141c33"), ("raised", "#1e2844"), ("border", "#8fb0ff26"), ("fg", "#eef2ff"), ("muted", "#9aa7c7"), ("faint", "#5d6987"), ("accent", "#64d2ff")]),
    ("ember", &[("island", "#1a0d08"), ("bg", "#1a0d08"), ("surface", "#2a1710"), ("raised", "#3a2117"), ("border", "#ffb48a26"), ("fg", "#fff3ec"), ("muted", "#c9a594"), ("faint", "#8a6b5e"), ("accent", "#ff9f0a")]),
    ("forest", &[("island", "#08140e"), ("bg", "#08140e"), ("surface", "#11231a"), ("raised", "#1a3125"), ("border", "#9fe0b826"), ("fg", "#effaf3"), ("muted", "#9cbfac"), ("faint", "#5f7f6d"), ("accent", "#30d158")]),
    ("rose", &[("island", "#1a0a12"), ("bg", "#1a0a12"), ("surface", "#2a1220"), ("raised", "#3a1a2c"), ("border", "#ffa3c826"), ("fg", "#fff0f6"), ("muted", "#c99ab0"), ("faint", "#8a6577"), ("accent", "#ff375f")]),
];

/// `base` chains (a file theme built on another) stop here, so a loop can't hang the island.
const MAX_DEPTH: usize = 4;

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    tokens: BTreeMap<String, String>,
}

impl Default for Theme {
    fn default() -> Self {
        Self { tokens: DEFAULTS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }
}

impl Theme {
    /// layout.json's `theme`, with `dir/themes` for named files. A name that resolves to
    /// nothing falls back to the default and says so.
    pub fn resolve(theme: &Value, dir: &Path) -> (Self, Option<String>) {
        let mut t = Self::default();
        let mut problem = None;
        let (base, overrides) = split(theme);
        if let Some(Err(e)) = base.map(|name| t.apply_named(&name, dir, 0)) {
            problem = Some(e);
            t = Self::default();
        }
        t.apply(&overrides);
        (t, problem)
    }

    fn apply_named(&mut self, name: &str, dir: &Path, depth: usize) -> Result<(), String> {
        if depth >= MAX_DEPTH {
            return Err(format!("theme {name:?}: \"base\" goes more than {MAX_DEPTH} deep"));
        }
        if let Some(file) = read_file(dir, name) {
            let (base, overrides) = split(&file?);
            match base {
                // `themes/midnight.json` built on "midnight" tunes the built-in one.
                Some(b) if b == name => self.apply_builtin(name, dir)?,
                Some(b) => self.apply_named(&b, dir, depth + 1)?,
                None => {}
            }
            self.apply(&overrides);
            return Ok(());
        }
        self.apply_builtin(name, dir)
    }

    fn apply_builtin(&mut self, name: &str, dir: &Path) -> Result<(), String> {
        let (_, tokens) = BUILTIN.iter().find(|(n, _)| *n == name).ok_or_else(|| format!("no theme named {name:?} (themes: {})", names(dir).join(", ")))?;
        for (k, v) in *tokens {
            self.tokens.insert(k.to_string(), v.to_string());
        }
        Ok(())
    }

    fn apply(&mut self, overrides: &Map<String, Value>) {
        for (k, v) in overrides {
            if let Some(v) = v.as_str() {
                self.tokens.insert(k.clone(), v.to_owned());
            }
        }
    }

    /// A token or a hex color; unknown names draw magenta so they're found.
    pub fn color(&self, name: &str) -> Color {
        let hex = self.tokens.get(name).map_or(name, String::as_str);
        let hex = self.tokens.get(hex.trim()).map_or(hex, String::as_str);
        rune::color::hex(hex.trim()).unwrap_or(Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 })
    }

    pub fn to_json(&self) -> Value {
        Value::Object(self.tokens.iter().map(|(k, v)| (k.clone(), Value::from(v.as_str()))).collect())
    }
}

/// `"name"` → (name, no overrides); `{ "base": "name", "accent": … }` → (name, the rest).
fn split(theme: &Value) -> (Option<String>, Map<String, Value>) {
    match theme {
        Value::String(name) => (Some(name.clone()), Map::new()),
        Value::Object(map) => {
            let mut rest = map.clone();
            let base = rest.remove("base").and_then(|b| b.as_str().map(str::to_owned));
            (base, rest)
        }
        _ => (None, Map::new()),
    }
}

/// Theme names are plain words, so a name can't reach outside `themes/`.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 40 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn read_file(dir: &Path, name: &str) -> Option<Result<Value, String>> {
    if !valid_name(name) {
        return None;
    }
    let path = dir.join("themes").join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).ok()?;
    Some(serde_json::from_str::<Value>(&text).map_err(|e| format!("themes/{name}.json: {e}")).and_then(|v| {
        if v.is_object() { Ok(v) } else { Err(format!("themes/{name}.json: not an object of tokens")) }
    }))
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join("themes")).into_iter().flatten().flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json").map(str::to_owned))
        .filter(|n| valid_name(n))
        .collect();
    names.sort();
    names
}

/// Every theme by name: the built-in ones, then files (a file named like a built-in replaces it).
pub fn names(dir: &Path) -> Vec<String> {
    let files = file_names(dir);
    let mut all: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
    all.extend(files.into_iter().filter(|f| !BUILTIN.iter().any(|(n, _)| n == f)));
    all
}

pub fn exists(dir: &Path, name: &str) -> bool {
    names(dir).iter().any(|n| n == name)
}

/// The name layout.json's theme is built on ("default" when it names none).
pub fn current_name(theme: &Value) -> String {
    split(theme).0.unwrap_or_else(|| "default".into())
}

/// What `themes()` reads: each theme with its colors.
pub fn list(dir: &Path, theme: &Value) -> Value {
    let current = current_name(theme);
    let files = file_names(dir);
    Value::Array(names(dir).into_iter().map(|name| {
        let (t, _) = Theme::resolve(&Value::String(name.clone()), dir);
        json!({ "name": name, "builtin": !files.contains(&name), "current": name == current, "colors": t.to_json() })
    }).collect())
}

/// `theme.set {name?, colors?}` applied to layout.json's `theme`: a name picks that theme
/// outright (earlier overrides go); colors merge over it, null dropping one.
pub fn set(theme: &Value, name: Option<&str>, colors: Option<&Map<String, Value>>) -> Value {
    let (base, mut overrides) = split(theme);
    let base = match name {
        Some(n) => {
            overrides.clear();
            Some(n.to_owned())
        }
        None => base,
    };
    for (k, v) in colors.into_iter().flatten() {
        if k == "base" {
            continue;
        }
        match v {
            Value::String(_) => {
                overrides.insert(k.clone(), v.clone());
            }
            _ => {
                overrides.remove(k);
            }
        }
    }
    match (base, overrides.is_empty()) {
        (None, true) => Value::Null,
        (Some(b), true) => Value::String(b),
        (base, false) => {
            let mut m = Map::new();
            if let Some(b) = base {
                m.insert("base".into(), Value::String(b));
            }
            m.extend(overrides);
            Value::Object(m)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nook-theme-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        dir
    }

    #[test]
    fn old_token_maps_still_override_the_default() {
        let dir = temp("old");
        let (t, problem) = Theme::resolve(&json!({ "accent": "#ff9f0a" }), &dir);
        assert!(problem.is_none());
        assert_eq!(t.to_json()["accent"], "#ff9f0a");
        assert_eq!(t.to_json()["island"], "#000000");
    }

    #[test]
    fn names_files_and_bases_resolve() {
        let dir = temp("names");
        let (t, _) = Theme::resolve(&json!("midnight"), &dir);
        assert_eq!(t.to_json()["island"], "#0a1022");
        // A file built on a built-in, overridden again in layout.json.
        std::fs::write(dir.join("themes/night-owl.json"), r##"{ "base": "midnight", "accent": "#c792ea" }"##).unwrap();
        let (t, problem) = Theme::resolve(&json!({ "base": "night-owl", "red": "#ff0000" }), &dir);
        assert!(problem.is_none());
        let j = t.to_json();
        assert_eq!((j["island"].as_str(), j["accent"].as_str(), j["red"].as_str()), (Some("#0a1022"), Some("#c792ea"), Some("#ff0000")));
        assert!(names(&dir).contains(&"night-owl".to_owned()));
        assert_eq!(list(&dir, &json!("night-owl")).as_array().unwrap().iter().filter(|t| t["current"] == true).count(), 1);
    }

    #[test]
    fn an_unknown_or_looping_theme_falls_back_and_says_why() {
        let dir = temp("bad");
        let (t, problem) = Theme::resolve(&json!("nope"), &dir);
        assert_eq!(t, Theme::default());
        assert!(problem.unwrap().contains("no theme named \"nope\""));
        std::fs::write(dir.join("themes/a.json"), r#"{ "base": "b" }"#).unwrap();
        std::fs::write(dir.join("themes/b.json"), r#"{ "base": "a" }"#).unwrap();
        let (_, problem) = Theme::resolve(&json!("a"), &dir);
        assert!(problem.unwrap().contains("deep"));
        // Names can't climb out of themes/.
        assert!(Theme::resolve(&json!("../layout"), &dir).1.is_some());
    }

    #[test]
    fn set_picks_merges_and_drops() {
        let picked = set(&json!({ "accent": "#ff9f0a" }), Some("ember"), None);
        assert_eq!(picked, json!("ember"));
        let tuned = set(&picked, None, Some(json!({ "accent": "#ffffff" }).as_object().unwrap()));
        assert_eq!(tuned, json!({ "base": "ember", "accent": "#ffffff" }));
        let back = set(&tuned, None, Some(json!({ "accent": null }).as_object().unwrap()));
        assert_eq!(back, json!("ember"));
        assert_eq!(set(&Value::Null, None, Some(&Map::new())), Value::Null);
    }
}
