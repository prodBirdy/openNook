//! The island's colors by token. Mods name tokens ("fg", "accent", …) or hex colors;
//! `layout.json`'s `"theme"` overrides any token.

use rune::Color;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

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
    pub fn with_overrides(overrides: Option<&Map<String, Value>>) -> Self {
        let mut t = Self::default();
        for (k, v) in overrides.into_iter().flatten() {
            if let Some(v) = v.as_str() {
                t.tokens.insert(k.clone(), v.to_owned());
            }
        }
        t
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
