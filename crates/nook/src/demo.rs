//! Demo data for screenshots and recordings: `OPENNOOK_DEMO=<fixture.json>` (or
//! `opennook shot --demo <fixture.json>`) puts a track, coding agents and a music meter in place
//! of the Mac's own, so the website's pictures show the same island on any Mac and nothing of
//! whoever took them. Programs mods run can be answered from the fixture too (its `exec` list,
//! read by nook-mods).
//!
//! ```json
//! { "media": { "title": "…", "artist": "…", "duration": 214, "elapsed": 61, "playing": true,
//!              "app": "Music", "bundleId": "com.apple.Music", "artwork": "cover.jpg" },
//!   "agents": [{ "kind": "Claude", "title": "…", "status": "working", "cwd": "…" }],
//!   "levels": true,
//!   "exec": [{ "command": "container", "args": ["list"], "stdout": [] }] }
//! ```
//!
//! `artwork` is a picture beside the fixture; `levels` drives `LEVEL()` from a synthetic groove;
//! `"star": true` shows the one-time star card.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::OnceLock;

pub struct Demo {
    pub media: Option<Value>,
    pub agents: Option<Vec<Value>>,
    pub levels: bool,
    /// Show the one-time star card, to look at it.
    pub star: bool,
    dir: PathBuf,
}

/// The fixture `OPENNOOK_DEMO` names, read once; None without one (the Mac's own data).
pub fn get() -> Option<&'static Demo> {
    static DEMO: OnceLock<Option<Demo>> = OnceLock::new();
    DEMO.get_or_init(|| {
        let path = PathBuf::from(std::env::var_os("OPENNOOK_DEMO")?);
        let text = std::fs::read_to_string(&path).map_err(|e| log::warn!("demo: {}: {e}", path.display())).ok()?;
        let v: Value = serde_json::from_str(&text).map_err(|e| log::warn!("demo: {}: {e}", path.display())).ok()?;
        log::info!("demo: data from {}", path.display());
        Some(Demo {
            media: v.get("media").filter(|m| !m.is_null()).cloned(),
            agents: v.get("agents").and_then(Value::as_array).cloned(),
            levels: v.get("levels").and_then(Value::as_bool).unwrap_or(false),
            star: v.get("star").and_then(Value::as_bool).unwrap_or(false),
            dir: path.parent().map(PathBuf::from).unwrap_or_default(),
        })
    }).as_ref()
}

impl Demo {
    /// `media()` as the fixture has it, playing from `elapsed` as of `now` (ms).
    pub fn media_json(&self, now: u64) -> Option<Value> {
        let m = self.media.as_ref()?;
        let bytes = m.get("artwork").and_then(Value::as_str).and_then(|a| std::fs::read(self.dir.join(a)).ok());
        let colors = bytes.as_deref().and_then(crate::palette::of);
        let playing = m.get("playing").and_then(Value::as_bool).unwrap_or(true);
        let mut out = m.clone();
        let o = out.as_object_mut()?;
        o.insert("artwork".into(), json!(bytes.map(crate::images::put_art)));
        o.insert("colors".into(), json!(colors));
        o.insert("playing".into(), json!(playing));
        o.insert("sampledAt".into(), json!(if playing { now } else { 0 }));
        Some(out)
    }
}

/// The meter's five bands now, from the synthetic groove the 1.x visualizer used.
pub fn levels() -> [f32; nook_core::audio_levels::BANDS] {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let t = START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64();
    let l = nook_core::audio::visualizer_levels_at(t);
    std::array::from_fn(|i| l[i] as f32)
}
