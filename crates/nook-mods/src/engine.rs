//! The mods' thread: one QuickJS runtime, one context per mod. Mods never run on the UI
//! thread, so a slow or broken one can't stall the island; each call into a mod has a time
//! budget, past which QuickJS stops it.

use crate::files::{self, Found, Manifest, Storage};
use crate::node::{Activity, Node};
use crate::store::Store;
use crate::watch::{self, Kind, Sub, Watches};
use crate::{Config, Host, Output};
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{CatchResultExt, Context, Ctx, Function, Module, Runtime, Value as JsValue};
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const CORE_JS: &str = include_str!("js/core.js");
const UI_JS: &str = include_str!("js/ui.js");
const NOOK_JS: &str = include_str!("js/nook.js");

/// How long one call into a mod (init, a render, a handler) may run.
const CALL_BUDGET: Duration = Duration::from_millis(500);
const INIT_BUDGET: Duration = Duration::from_secs(3);
const MEMORY_LIMIT: usize = 512 << 20;

pub enum Msg {
    /// Live data changed (`Store::publish`).
    Topic,
    Dispatch { name: String, handler: u64, payload: Value },
    Done { name: String, load: u64, id: u64, result: Result<Value, String> },
    Event { name: String, load: u64, sub: u64, payload: Value },
    /// Mod folders changed on disk.
    Reload,
    Stop,
}

/// A mod's half of the bridge: what its natives change while it runs.
struct State {
    name: String,
    dir: PathBuf,
    load: u64,
    manifest: Manifest,
    /// Live-data topics the mod read, with the revision it saw.
    reads: HashMap<String, u64>,
    dirty: bool,
    timers: Vec<Timer>,
    errors: Vec<String>,
    storage: Storage,
    next_op: u64,
}

struct Timer {
    id: u64,
    due: Instant,
    every: Option<Duration>,
}

struct Loaded {
    ctx: Context,
    state: Rc<RefCell<State>>,
    stamp: Option<SystemTime>,
    /// Last rendered output, kept while a later render fails.
    last: Output,
}

/// What the mods share with every native: the world outside the JS thread.
#[derive(Clone)]
struct Shared {
    config: Arc<Config>,
    store: Arc<Store>,
    host: Arc<dyn Host>,
    tx: Sender<Msg>,
    watches: Arc<Mutex<Watches>>,
    paths: Arc<crate::ops::Paths>,
    /// Topics already asked of the host (`Host::wants`).
    wanted: Arc<Mutex<std::collections::HashSet<String>>>,
}

pub struct Engine {
    rt: Runtime,
    deadline: Rc<Cell<Option<Instant>>>,
    /// Promises rejected with nothing to catch them: (mod, reason).
    rejections: Rc<RefCell<Vec<(String, String)>>>,
    shared: Shared,
    mods: BTreeMap<String, Loaded>,
    loads: u64,
    /// Load only this folder (`check`), instead of scanning the mods folder.
    only: Option<Found>,
}

impl Engine {
    pub fn new(config: Arc<Config>, store: Arc<Store>, host: Arc<dyn Host>, tx: Sender<Msg>, watches: Arc<Mutex<Watches>>, only: Option<Found>) -> Result<Self, String> {
        let rt = Runtime::new().map_err(|e| e.to_string())?;
        rt.set_memory_limit(MEMORY_LIMIT);
        rt.set_max_stack_size(1 << 20);
        let deadline: Rc<Cell<Option<Instant>>> = Rc::default();
        let d = deadline.clone();
        rt.set_interrupt_handler(Some(Box::new(move || d.get().is_some_and(|t| Instant::now() > t))));
        rt.set_loader(ModResolver, ModLoader);
        let rejections: Rc<RefCell<Vec<(String, String)>>> = Rc::default();
        let r = rejections.clone();
        rt.set_host_promise_rejection_tracker(Some(Box::new(move |ctx, _promise, reason, handled| {
            let name: String = ctx.globals().get("__nook_mod").unwrap_or_default();
            let describe: Option<Function> = ctx.globals().get("__nook_describe").ok();
            let text = describe.and_then(|f| f.call::<_, String>((reason,)).ok()).unwrap_or_else(|| "rejected".into());
            let entry = (name, format!("unhandled rejection: {text}"));
            let mut pending = r.borrow_mut();
            // QuickJS reports a rejection before an `await` downstream attaches to it, then
            // again as handled; only one never handled by the next render is the mod's error.
            if handled {
                if let Some(i) = pending.iter().rposition(|p| *p == entry) {
                    pending.remove(i);
                }
            } else {
                pending.push(entry);
            }
        })));
        let paths = Arc::new(crate::ops::Paths { home: config.home.clone(), app: config.app.clone() });
        Ok(Self { rt, deadline, rejections, shared: Shared { config, store, host, tx, watches, paths, wanted: Arc::default() }, mods: BTreeMap::new(), loads: 0, only })
    }

    pub fn run(mut self, rx: Receiver<Msg>) {
        self.reload();
        loop {
            let wait = self.next_due().map_or(Duration::from_secs(1), |t| t.saturating_duration_since(Instant::now()));
            let first = match rx.recv_timeout(wait) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            for msg in first.into_iter().chain(rx.try_iter()) {
                if !self.handle(msg) {
                    self.unload_all();
                    return;
                }
            }
            self.step();
        }
    }

    /// One turn after messages: due timers, promise jobs, then renders.
    pub fn step(&mut self) {
        self.fire_timers();
        self.run_jobs();
        self.render_dirty();
    }

    pub fn handle(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Stop => return false,
            Msg::Reload => self.reload(),
            Msg::Topic => {
                for m in self.mods.values() {
                    let mut s = m.state.borrow_mut();
                    if s.reads.iter().any(|(topic, seen)| self.shared.store.revision(topic) != *seen) {
                        s.dirty = true;
                    }
                }
            }
            Msg::Dispatch { name, handler, payload } => {
                self.call(&name, CALL_BUDGET, |ctx| {
                    let f: Function = ctx.globals().get("__nook_dispatch")?;
                    f.call::<_, ()>((handler as f64, payload.to_string()))
                });
            }
            Msg::Done { name, load, id, result } => {
                if self.mods.get(&name).is_some_and(|m| m.state.borrow().load == load) {
                    let (ok, value) = match result {
                        Ok(v) => (true, v),
                        Err(e) => (false, Value::from(e)),
                    };
                    self.call(&name, CALL_BUDGET, |ctx| {
                        let f: Function = ctx.globals().get("__nook_settle")?;
                        f.call::<_, ()>((id as f64, ok, value.to_string()))
                    });
                }
            }
            Msg::Event { name, load, sub, payload } => {
                if self.mods.get(&name).is_some_and(|m| m.state.borrow().load == load) {
                    self.call(&name, CALL_BUDGET, |ctx| {
                        let f: Function = ctx.globals().get("__nook_event")?;
                        f.call::<_, ()>((sub as f64, payload.to_string()))
                    });
                }
            }
        }
        true
    }

    fn next_due(&self) -> Option<Instant> {
        self.mods.values().flat_map(|m| m.state.borrow().timers.iter().map(|t| t.due).min()).min()
    }

    fn fire_timers(&mut self) {
        let now = Instant::now();
        let names: Vec<String> = self.mods.keys().cloned().collect();
        for name in names {
            let due: Vec<u64> = {
                let Some(m) = self.mods.get(&name) else { continue };
                let mut s = m.state.borrow_mut();
                let mut due = Vec::new();
                s.timers.retain_mut(|t| {
                    if t.due > now {
                        return true;
                    }
                    due.push(t.id);
                    match t.every {
                        Some(every) => {
                            // Skip missed ticks rather than firing a burst after a stall.
                            while t.due <= now {
                                t.due += every;
                            }
                            true
                        }
                        None => false,
                    }
                });
                due
            };
            for id in due {
                self.call(&name, CALL_BUDGET, |ctx| {
                    let f: Function = ctx.globals().get("__nook_fire")?;
                    f.call::<_, ()>((id as f64,))
                });
            }
        }
    }

    fn run_jobs(&mut self) {
        self.deadline.set(Some(Instant::now() + CALL_BUDGET));
        for _ in 0..10_000 {
            match self.rt.execute_pending_job() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(e) => {
                    let text = e.0.with(|ctx| format!("promise job: {}", rquickjs::CaughtError::from_error(&ctx, rquickjs::Error::Exception)));
                    // The job doesn't say whose it was; tell every mod, the log names it.
                    log::warn!("mods: {text}");
                }
            }
        }
        self.deadline.set(None);
    }

    /// Calls into one mod with a time budget. A throw lands in its errors.
    fn call(&mut self, name: &str, budget: Duration, f: impl for<'js> FnOnce(&Ctx<'js>) -> rquickjs::Result<()>) {
        let Some(m) = self.mods.get(name) else { return };
        self.deadline.set(Some(Instant::now() + budget));
        let result = m.ctx.with(|ctx| f(&ctx).catch(&ctx).map_err(|e| e.to_string()));
        self.deadline.set(None);
        if let Err(e) = result {
            let mut s = m.state.borrow_mut();
            s.errors.push(interrupted(e, budget));
            s.dirty = true;
        }
    }

    fn render_dirty(&mut self) {
        for (name, reason) in self.rejections.borrow_mut().drain(..) {
            if let Some(m) = self.mods.get(&name) {
                let mut s = m.state.borrow_mut();
                s.errors.push(reason);
                s.dirty = true;
            }
        }
        let dirty: Vec<String> = self.mods.iter().filter(|(_, m)| m.state.borrow().dirty).map(|(n, _)| n.clone()).collect();
        for name in dirty {
            self.render(&name);
        }
    }

    fn render(&mut self, name: &str) {
        let Some(m) = self.mods.get(name) else { return };
        m.state.borrow_mut().dirty = false;
        self.deadline.set(Some(Instant::now() + CALL_BUDGET));
        let json = m.ctx.with(|ctx| {
            let f: Function = ctx.globals().get("__nook_render").catch(&ctx).map_err(|e| e.to_string())?;
            f.call::<_, String>(()).catch(&ctx).map_err(|e| e.to_string())
        });
        self.deadline.set(None);
        let Some(m) = self.mods.get_mut(name) else { return };
        let mut errors: Vec<String> = std::mem::take(&mut m.state.borrow_mut().errors);
        match json.map_err(|e| interrupted(e, CALL_BUDGET)).and_then(|j| serde_json::from_str::<Rendered>(&j).map_err(|e| e.to_string())) {
            Ok(r) => {
                m.last.icon = r.icon.clone();
                match r.activity.into_value::<Activity>("activity") {
                    Ok(v) => m.last.activity = v,
                    Err(e) => errors.push(e),
                }
                m.last.has_expanded = !r.expanded.none;
                match r.expanded.into_value::<Node>("expanded") {
                    Ok(v) => m.last.expanded = v,
                    Err(e) => errors.push(e),
                }
            }
            Err(e) => errors.push(format!("render: {e}")),
        }
        let state = m.state.borrow();
        for e in &errors {
            files::log(&self.shared.config.logs, name, "error", e);
        }
        m.last.error = (!errors.is_empty()).then(|| errors.join("\n\n"));
        m.last.title = state.manifest.title.clone().unwrap_or_else(|| name.to_owned());
        m.last.manifest_icon = state.manifest.icon.clone();
        if m.last.icon.is_none() {
            m.last.icon = state.manifest.icon.clone();
        }
        drop(state);
        if self.only.is_none() {
            files::set_error(&self.shared.config.mods, name, m.last.error.as_deref());
        }
        self.shared.host.output(m.last.clone());
    }

    pub fn reload(&mut self) {
        let found = match &self.only {
            Some(f) => vec![f.clone()],
            None => files::scan(&self.shared.config.mods),
        };
        let gone: Vec<String> = self.mods.keys().filter(|n| !found.iter().any(|f| &f.name == *n)).cloned().collect();
        for name in gone {
            self.unload(&name);
            files::set_error(&self.shared.config.mods, &name, None);
            self.shared.host.removed(&name);
        }
        for f in found {
            let stamp = files::stamp(&f.dir);
            if self.mods.get(&f.name).is_some_and(|m| m.stamp == stamp) {
                continue;
            }
            match self.load(&f) {
                Ok(loaded) => {
                    self.unload(&f.name);
                    self.mods.insert(f.name.clone(), Loaded { stamp, ..loaded });
                }
                Err(e) => {
                    // Keep the last good copy running; say why the new one didn't load (its
                    // next render logs it).
                    match self.mods.get_mut(&f.name) {
                        Some(m) => {
                            m.stamp = stamp;
                            m.state.borrow_mut().errors.push(e);
                            m.state.borrow_mut().dirty = true;
                        }
                        None => {
                            files::log(&self.shared.config.logs, &f.name, "error", &e);
                            if self.only.is_none() {
                                files::set_error(&self.shared.config.mods, &f.name, Some(&e));
                            }
                            let out = Output { name: f.name.clone(), title: f.name.clone(), dir: f.dir.clone(), error: Some(e), ..Default::default() };
                            self.shared.host.output(out);
                        }
                    }
                }
            }
        }
    }

    fn unload(&mut self, name: &str) {
        let Some(load) = self.mods.get(name).map(|m| m.state.borrow().load) else { return };
        self.call(name, CALL_BUDGET, |ctx| {
            let f: Function = ctx.globals().get("__nook_unmount")?;
            f.call::<_, ()>(())
        });
        self.mods.remove(name);
        // Only this copy's: a reloaded copy has already subscribed under the same name.
        self.shared.watches.lock().unwrap().drop_load(name, load);
    }

    fn unload_all(&mut self) {
        let names: Vec<String> = self.mods.keys().cloned().collect();
        for n in names {
            self.unload(&n);
        }
    }

    fn load(&mut self, f: &Found) -> Result<Loaded, String> {
        self.loads += 1;
        let manifest = files::read_manifest(&f.dir)?;
        let state = Rc::new(RefCell::new(State {
            name: f.name.clone(),
            dir: f.dir.clone(),
            load: self.loads,
            manifest,
            reads: HashMap::new(),
            dirty: true,
            timers: Vec::new(),
            errors: Vec::new(),
            storage: Storage::open(&self.shared.config.data, &f.name),
            next_op: 0,
        }));
        let ctx = Context::full(&self.rt).map_err(|e| e.to_string())?;
        let main = f.dir.join("main.js");
        let source = std::fs::read_to_string(&main).map_err(|e| format!("main.js: {e}"))?;
        let props = props_for(&state.borrow(), &self.shared.store).to_string();
        self.deadline.set(Some(Instant::now() + INIT_BUDGET));
        let result = ctx.with(|ctx| -> Result<(), String> {
            let fail = |e: rquickjs::CaughtError| e.to_string();
            install(&ctx, state.clone(), self.shared.clone()).catch(&ctx).map_err(fail)?;
            ctx.eval::<(), _>(CORE_JS).catch(&ctx).map_err(fail)?;
            let module = Module::declare(ctx.clone(), main.to_string_lossy().into_owned(), source).catch(&ctx).map_err(fail)?;
            let (module, promise) = module.eval().catch(&ctx).map_err(fail)?;
            promise.finish::<()>().catch(&ctx).map_err(fail)?;
            let class: JsValue = module.get("default").catch(&ctx).map_err(fail)?;
            let mount: Function = ctx.globals().get("__nook_mount").catch(&ctx).map_err(fail)?;
            mount.call::<_, ()>((class, props)).catch(&ctx).map_err(fail)
        });
        self.deadline.set(None);
        result.map_err(|e| interrupted(e, INIT_BUDGET))?;
        let last = Output { name: f.name.clone(), title: f.name.clone(), dir: f.dir.clone(), ..Default::default() };
        Ok(Loaded { ctx, state, stamp: None, last })
    }

}

fn interrupted(e: String, budget: Duration) -> String {
    if e.contains("interrupted") {
        format!("{e}\n(stopped after {} ms: a loop that never ends, or work that belongs in exec)", budget.as_millis())
    } else {
        e
    }
}

#[derive(Deserialize)]
struct Rendered {
    activity: Slot,
    expanded: Slot,
    /// `icon()`: the tab icon for now, in place of mod.json's.
    #[serde(default)]
    icon: Option<String>,
}

#[derive(Deserialize)]
struct Slot {
    #[serde(default)]
    none: bool,
    #[serde(default)]
    value: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

impl Slot {
    /// `Ok(None)`: the mod has no such view, or rendered nothing this time.
    fn into_value<T: serde::de::DeserializeOwned>(self, slot: &str) -> Result<Option<T>, String> {
        if let Some(e) = self.error {
            return Err(e);
        }
        match self.value {
            None | Some(Value::Null) => Ok(None),
            Some(v) => serde_json::from_value(v).map(Some).map_err(|e| format!("{slot}(): not a valid element tree: {e}")),
        }
    }
}

fn props_for(state: &State, store: &Store) -> Value {
    let (_, layout) = store.get("layout");
    layout.get("mods").and_then(|m| m.get(&state.name)).and_then(|m| m.get("props")).cloned()
        .or_else(|| state.manifest.props.clone())
        .unwrap_or_else(|| json!({}))
}

/// The two natives every mod's context gets: `__host_call` and `__host_start`.
fn install<'js>(ctx: &Ctx<'js>, state: Rc<RefCell<State>>, shared: Shared) -> rquickjs::Result<()> {
    ctx.globals().set("__nook_mod", state.borrow().name.clone())?;
    let s = state.clone();
    let sh = shared.clone();
    let host_call = Function::new(ctx.clone(), move |op: String, json: String| -> String {
        let args: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
        host_call(&s, &sh, &op, args).map_or_else(String::new, |v| v.to_string())
    })?;
    let host_start = Function::new(ctx.clone(), move |op: String, json: String| -> f64 {
        let args: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
        let mut st = state.borrow_mut();
        st.next_op += 1;
        let id = st.next_op;
        let (name, load, dir) = (st.name.clone(), st.load, st.dir.clone());
        let (tx, paths) = (shared.tx.clone(), shared.paths.clone());
        std::thread::spawn(move || {
            let result = crate::ops::run(&op, args, &dir, &paths);
            let _ = tx.send(Msg::Done { name, load, id, result });
        });
        id as f64
    })?;
    ctx.globals().set("__host_call", host_call)?;
    ctx.globals().set("__host_start", host_start)
}

fn host_call(state: &Rc<RefCell<State>>, shared: &Shared, op: &str, args: Value) -> Option<Value> {
    let arg = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    let mut s = state.borrow_mut();
    match op {
        "read" => {
            let topic = arg("topic");
            let (rev, value) = shared.store.get(&topic);
            if rev == 0 && shared.wanted.lock().unwrap().insert(topic.clone()) {
                shared.host.wants(&topic);
            }
            s.reads.entry(topic).or_insert(rev);
            Some(value)
        }
        "notify" => {
            s.dirty = true;
            None
        }
        "error" => {
            s.errors.push(args.as_str().unwrap_or_default().to_owned());
            s.dirty = true;
            None
        }
        "log" => {
            files::log(&shared.config.logs, &s.name, &arg("level"), &arg("text"));
            None
        }
        "timer" => {
            let ms = args.get("ms").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
            let every = Duration::from_secs_f64(ms / 1000.0);
            let id = args.get("id").and_then(Value::as_u64).unwrap_or(0);
            let repeat = args.get("repeat").and_then(Value::as_bool).unwrap_or(false);
            s.timers.push(Timer { id, due: Instant::now() + every, every: repeat.then_some(every) });
            None
        }
        "timer.cancel" => {
            let id = args.get("id").and_then(Value::as_u64).unwrap_or(0);
            s.timers.retain(|t| t.id != id);
            None
        }
        "storage.get" => s.storage.items.get(&arg("key")).map(|v| Value::from(v.as_str())),
        "storage.set" => {
            let value = args.get("value").and_then(Value::as_str).map(str::to_owned);
            s.storage.set(arg("key"), value);
            None
        }
        "storage.clear" => {
            s.storage.clear();
            None
        }
        "shared.set" => {
            let (_, mut all) = shared.store.get("shared");
            if !all.is_object() {
                all = json!({});
            }
            all[arg("key")] = args.get("value").cloned().unwrap_or(Value::Null);
            let _ = files::write_if_changed(&shared.config.app.join("shared.json"), serde_json::to_string_pretty(&all).unwrap_or_default());
            drop(s);
            shared.store.publish("shared", all);
            None
        }
        "props" => {
            let rev = shared.store.revision("layout");
            s.reads.entry("layout".into()).or_insert(rev);
            Some(props_for(&s, &shared.store))
        }
        "path" => Some(Value::from(match arg("which").as_str() {
            "home" => shared.config.home.to_string_lossy().into_owned(),
            "app" => shared.config.app.to_string_lossy().into_owned(),
            "mod" => s.dir.to_string_lossy().into_owned(),
            _ => std::env::consts::OS.to_owned(),
        })),
        "run" => {
            let name = s.name.clone();
            drop(s);
            shared.host.command(&name, &arg("command"), args.get("args").cloned().unwrap_or(Value::Null));
            None
        }
        "tail" | "watch" => {
            let path = crate::ops::resolve(&arg("path"), &s.dir, &shared.paths);
            let kind = if op == "tail" {
                let from_end = args.get("fromEnd").and_then(Value::as_u64).unwrap_or(0) as usize;
                Kind::Tail { offset: watch::tail_start(&path, from_end), partial: Vec::new() }
            } else {
                Kind::Watch { seen: None }
            };
            let id = shared.watches.lock().unwrap().add(Sub { name: s.name.clone(), load: s.load, path, kind });
            Some(Value::from(id))
        }
        "unsubscribe" => {
            let id = args.get("id").and_then(Value::as_u64).unwrap_or(0);
            shared.watches.lock().unwrap().subs.remove(&id);
            None
        }
        _ => {
            s.errors.push(format!("unknown host call {op}"));
            None
        }
    }
}

/// `nook` and `nook/ui` are built in; anything else is a path relative to the importer.
struct ModResolver;

impl Resolver for ModResolver {
    fn resolve<'js>(&mut self, _: &Ctx<'js>, base: &str, name: &str, _: Option<ImportAttributes<'js>>) -> rquickjs::Result<String> {
        if matches!(name, "nook" | "nook/ui") {
            return Ok(name.to_owned());
        }
        let path = if Path::new(name).is_absolute() { PathBuf::from(name) } else { Path::new(base).parent().unwrap_or(Path::new("/")).join(name) };
        let mut out = PathBuf::new();
        for c in path.components() {
            match c {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                c => out.push(c),
            }
        }
        Ok(out.to_string_lossy().into_owned())
    }
}

struct ModLoader;

impl Loader for ModLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str, _: Option<ImportAttributes<'js>>) -> rquickjs::Result<Module<'js, rquickjs::module::Declared>> {
        let source = match name {
            "nook" => NOOK_JS.to_owned(),
            "nook/ui" => UI_JS.to_owned(),
            path => std::fs::read_to_string(path).map_err(|e| rquickjs::Error::new_loading_message(path, e.to_string()))?,
        };
        Module::declare(ctx.clone(), name, source)
    }
}

pub fn spawn(config: Arc<Config>, store: Arc<Store>, host: Arc<dyn Host>, tx: Sender<Msg>, rx: Receiver<Msg>) {
    let watches: Arc<Mutex<Watches>> = Arc::default();
    watch::spawn(watches.clone(), config.mods.clone(), tx.clone());
    std::thread::Builder::new().name("nook-mods".into()).spawn(move || {
        match Engine::new(config, store, host, tx, watches, None) {
            Ok(engine) => engine.run(rx),
            Err(e) => log::error!("mods: QuickJS didn't start: {e}"),
        }
    }).expect("mods thread");
}

pub fn new_watches() -> Arc<Mutex<Watches>> {
    Arc::default()
}
