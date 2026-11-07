//! openNook's mods: folders of JavaScript that describe what the island shows, run on QuickJS
//! off the UI thread, reloaded when their files change.
//!
//! The app publishes live data into the [`Store`] (`Mods::publish`), forwards presses on mod
//! elements (`Mods::dispatch`), and receives every mod's latest views through [`Host`].

mod engine;
pub mod files;
pub mod node;
mod ops;
mod store;
mod watch;

pub use node::{Activity, Glow, IslandStyle, Node, Stroke};
pub use ops::user_path;
pub use store::Store;

use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

pub const UI_D_TS: &str = include_str!("js/ui.d.ts");
pub const NOOK_D_TS: &str = include_str!("js/nook.d.ts");

/// Where mods live and keep things.
#[derive(Clone, Debug)]
pub struct Config {
    /// `~/.config/opennook`: AGENTS.md, layout.json, shared.json, mods/.
    pub app: PathBuf,
    pub mods: PathBuf,
    /// Per-mod localStorage.
    pub data: PathBuf,
    pub logs: PathBuf,
    pub home: PathBuf,
}

impl Config {
    pub fn at(app: PathBuf, home: PathBuf) -> Self {
        Self { mods: app.join("mods"), data: app.join("data"), logs: app.join("logs"), app, home }
    }
}

/// One mod's latest views.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub name: String,
    pub title: String,
    /// The tab icon: `icon()`'s, else mod.json's.
    pub icon: Option<String>,
    /// mod.json's icon, for when `icon()` names something that can't be drawn.
    pub manifest_icon: Option<String>,
    pub dir: PathBuf,
    /// What it shows in the closed island now, if anything.
    pub activity: Option<Activity>,
    /// Its page in the open island, if it has one.
    pub expanded: Option<Node>,
    pub has_expanded: bool,
    /// `style()`: how the island looks while this mod owns it.
    pub style: Option<IslandStyle>,
    pub error: Option<String>,
}

/// The app's side: called from the mods' thread.
pub trait Host: Send + Sync + 'static {
    fn output(&self, out: Output);
    fn removed(&self, name: &str);
    /// `run(command, args)` from a mod.
    fn command(&self, from: &str, command: &str, args: Value);
    /// A mod read a topic nothing has published yet (once per topic): start what produces it,
    /// such as asking for the Mac's location.
    fn wants(&self, _topic: &str) {}
}

#[derive(Clone)]
pub struct Mods {
    tx: Sender<engine::Msg>,
    store: Arc<Store>,
}

impl Mods {
    pub fn start(config: Config, store: Arc<Store>, host: impl Host) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = tx.clone();
        store.on_change(move || {
            let _ = wake.send(engine::Msg::Topic);
        });
        engine::spawn(Arc::new(config), store.clone(), Arc::new(host), tx.clone(), rx);
        Self { tx, store }
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn publish(&self, topic: &str, value: Value) -> bool {
        self.store.publish(topic, value)
    }

    /// A press or hover on an element a mod rendered.
    pub fn dispatch(&self, name: &str, handler: u64, payload: Value) {
        let _ = self.tx.send(engine::Msg::Dispatch { name: name.to_owned(), handler, payload });
    }

    pub fn stop(&self) {
        let _ = self.tx.send(engine::Msg::Stop);
    }
}

/// What `opennook check` reports.
pub struct Checked {
    pub output: Output,
    pub commands: Vec<(String, Value)>,
}

#[derive(Default)]
struct Recorder {
    outputs: std::sync::Mutex<Vec<Output>>,
    commands: std::sync::Mutex<Vec<(String, Value)>>,
}

impl Host for Arc<Recorder> {
    fn output(&self, out: Output) {
        self.outputs.lock().unwrap().push(out);
    }
    fn removed(&self, _: &str) {}
    fn command(&self, _: &str, command: &str, args: Value) {
        self.commands.lock().unwrap().push((command.to_owned(), args));
    }
}

/// Loads one mod folder, lets it settle for `settle` (timers, exec and fetch answers), and
/// returns what it last rendered. Errors are in `output.error`.
pub fn check(config: Config, store: Arc<Store>, dir: PathBuf, settle: Duration) -> Result<Checked, String> {
    let dir = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !dir.join("main.js").is_file() {
        return Err(format!("{}: no main.js", dir.display()));
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let wake = tx.clone();
    store.on_change(move || {
        let _ = wake.send(engine::Msg::Topic);
    });
    let recorder: Arc<Recorder> = Arc::default();
    let watches = engine::new_watches();
    // tail() and watchFile() need the watcher; it stops once this check's channel closes.
    watch::spawn(watches.clone(), dir.parent().map(Into::into).unwrap_or_default(), tx.clone());
    let mut engine = engine::Engine::new(Arc::new(config), store, Arc::new(recorder.clone()), tx, watches, Some(files::Found { name, dir }))
        .map_err(|e| format!("QuickJS: {e}"))?;
    engine.reload();
    engine.step();
    let until = Instant::now() + settle;
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left.min(Duration::from_millis(50))) {
            Ok(msg) => {
                engine.handle(msg);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
        engine.step();
    }
    let output = recorder.outputs.lock().unwrap().last().cloned().ok_or("the mod rendered nothing")?;
    let commands = std::mem::take(&mut *recorder.commands.lock().unwrap());
    Ok(Checked { output, commands })
}

#[cfg(test)]
mod tests;
