//! openNook: a Dynamic Island for the notch whose every part is a mod. `opennook` runs the
//! island; `opennook check <mod>` and `opennook shot` are for the agents that write mods.

mod gesture;
mod home;
mod icons;
mod images;
mod palette;
mod island;
mod platform;
mod render;
mod services;
mod settings;
mod shader;
mod theme;

use island::{CANVAS_H, Island, Notch};
use nook_mods::{Host, Mods, Output, Store};
use rune::{App, AppSpec, DisplayTarget, OverlayHandle, OverlayInput, OverlayLevel, OverlayOptions, OverlaySpec, RunOptions, WindowSpec, el};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const USAGE: &str = "\
openNook: a Dynamic Island for the notch, made of mods.

usage:
  opennook                      run the island
  opennook check <mod folder>   load a mod, let it settle, report its views or its error
  opennook shot [file.png]      render the island to a PNG (default: island.png)
      --open                      the open island (default: closed)
      --mod <name>                the open island showing this mod's page
      --hover                     the closed island as hovered (its peek)
      --settings [--page mods|yours|about] [--onboarding 0|1]   the Settings window instead
      --wait <ms>                 how long mods get to settle first (default 2500)
      -- <rune flags>             Rune's tooling instead of a still, e.g.
                                  -- --script s.json --strip motion.png --frames 24 --every 2
  opennook paths                print where things are
  opennook --skill              the Agent Skill for coding agents (how to customize openNook)

Everything the island shows is a mod in ~/.config/opennook/mods. Read
~/.config/opennook/AGENTS.md before writing one.";

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => run(),
        Some("check") => std::process::exit(check(&args[1..])),
        Some("shot") => std::process::exit(shot(&args[1..])),
        Some("paths") => {
            let c = home::config();
            println!("home    {}\nmods    {}\nlogs    {}\nguide   {}", c.app.display(), c.mods.display(), c.logs.display(), c.app.join("AGENTS.md").display());
        }
        Some("-h" | "--help" | "help") => println!("{USAGE}"),
        Some("--skill") => print!("{}", settings::skill()),
        // Rune's own tooling flags (--snap, --tree, …) pass through to a headless island, and
        // older macOS hands a Finder launch a process serial number (-psn_…).
        Some(a) if a.starts_with("--") || a.starts_with("-psn") => run(),
        Some(other) => {
            eprintln!("opennook: unknown command {other}\n\n{USAGE}");
            std::process::exit(2);
        }
    }
}

/// Mods' views reach the island on its own thread.
struct OverlayHost(OverlayHandle<Island>, Arc<Store>);

impl Host for OverlayHost {
    fn output(&self, out: Output) {
        self.0.update(move |s, _| s.apply(out));
    }
    fn removed(&self, name: &str) {
        let name = name.to_owned();
        self.0.update(move |s, _| s.remove(&name));
    }
    fn command(&self, from: &str, command: &str, args: Value) {
        let (from, command) = (from.to_owned(), command.to_owned());
        self.0.update(move |s, tx| s.command(&from, &command, &args, tx));
    }
    fn wants(&self, topic: &str) {
        if topic == "agents" {
            services::start_agents(self.1.clone());
        }
        if topic == "location" {
            // An update runs on the main thread, where CoreLocation must be asked.
            let store = self.1.clone();
            self.0.update(move |_, _| services::request_location(store));
        }
    }
}

fn island_setup(s: &mut Island, waker: rune::Waker) {
    s.images = Some(images::Images::new(waker));
}

fn initial_layout(notes: &mut Vec<String>) -> (home::Layout, Value) {
    match home::read_layout(&home::app_dir()) {
        Ok(l) => l,
        Err(e) => {
            notes.push(format!("{e} (see layout.error)"));
            (home::Layout::default(), Value::Null)
        }
    }
}

fn prepare() -> (Island, Arc<Store>) {
    let mut notes = match home::write_workspace() {
        Ok(n) => n,
        Err(e) => vec![format!("Couldn't write {}: {e}", home::app_dir().display())],
    };
    services::start_core();
    let (layout, raw) = initial_layout(&mut notes);
    let mut island = Island::new(Notch::detect(layout.island.display), layout);
    island.notes = notes;
    let store: Arc<Store> = Arc::default();
    store.publish("layout", raw);
    store.publish("theme", island.theme.to_json());
    (island, store)
}

fn run() {
    rune::prewarm_gpu();
    if let Some(pid) = already_running() {
        eprintln!("openNook is already running (pid {pid}).");
        std::process::exit(1);
    }
    let (island, store) = prepare();
    let canvas = (island.canvas_w(), CANVAS_H);
    rune::run_windows_with(RunOptions { accessory: true }, move |_| {
        let mut app = App::new(AppSpec::new("openNook", (1.0, 1.0), || (), |_, _| el())).windowless();
        let start = island::pick(island::display_mode(), &platform::displays(), platform::pointer())
            .map_or(DisplayTarget::Primary, |d| DisplayTarget::Native(rune::NativeDisplayId::MacOS(d.id)));
        let options = OverlayOptions { display: start, input: OverlayInput::Regions, level: OverlayLevel::AboveStatus, prewarm: true, ..Default::default() };
        // Scrolls and right-clicks: Rune routes only left presses to an overlay, so the panel's
        // own events are watched once it exists.
        let slot: Arc<std::sync::OnceLock<OverlayHandle<Island>>> = Arc::default();
        let created = slot.clone();
        let overlay = app.overlay(
            OverlaySpec::new("openNook", canvas, island::view).options(options).poll(island::poll).setup(island_setup)
                .compositor(shader::Shaders::default())
                .on_created(move |c| {
                    let Some(window) = c.window_number else { return };
                    platform::watch_island_input(window, move |input| {
                        let Some(handle) = created.get() else { return };
                        match input {
                            platform::IslandInput::Scroll { dx, dy, phase, momentum } => handle.update(move |s, tx| {
                                log::debug!("island scroll dx={dx:.1} dy={dy:.1} {phase:?} momentum={momentum}");
                                s.scroll(dx, dy, phase, momentum, tx);
                            }),
                            platform::IslandInput::SecondaryClick { x, y } => handle.update(move |s, tx| s.menu((x, y), tx)),
                        }
                    });
                }),
            island,
        );
        let _ = slot.set(overlay.clone());
        // Settings (and the first launch's onboarding), hidden until asked for.
        let spec = AppSpec { poll: Some(settings::poll), ..AppSpec::new("openNook", settings::SIZE, settings::Settings::new, settings::view) };
        let window = WindowSpec::new("settings", spec)
            .chrome(rune::Chrome { full_size_content: true, traffic_lights: Some((18.0, 18.0)), drag_regions: false })
            .options(rune::WindowOptions { visible: false, min_size: Some((680.0, 480.0)), ..Default::default() });
        let (settings_app, settings_window) = app.window(window);
        let opener = settings_app.clone();
        overlay.update(move |s, _| {
            s.open_settings = Some(Box::new(move || {
                opener.update(|s, _| s.refresh());
                opener.open();
            }));
        });
        if settings::first_launch() {
            settings_app.update(|s, _| s.onboarding = Some(0));
            settings_window.open();
        }
        let mods = Mods::start(home::config(), store.clone(), OverlayHost(overlay.clone(), store.clone()));
        overlay.update(move |s, _| {
            s.mods = Some(mods);
            s.publish();
        });
        services::start(store);
        watch_layout(overlay.clone());
        watch_displays(overlay.clone());
        overlay.show();
        let probe = overlay.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            log::info!("island: {:?} window={:?} {:?}", probe.availability(), probe.window_number(), probe.failure());
        });
        vec![WindowSpec::from_app("main", app)]
    });
}

/// layout.json applies as it's saved; a broken edit keeps the last good layout.
fn watch_layout(overlay: OverlayHandle<Island>) {
    std::thread::Builder::new().name("nook-layout".into()).spawn(move || {
        let dir = home::app_dir();
        let mut stamp = home::layout_stamp(&dir);
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let now = home::layout_stamp(&dir);
            if now == stamp {
                continue;
            }
            stamp = now;
            if let Ok((layout, raw)) = home::read_layout(&dir) {
                overlay.update(move |s, _| s.set_layout(layout, raw));
            }
        }
    }).expect("layout thread");
}

/// Keeps the island on the display its mode picks (following the pointer, or a display being
/// plugged in), and out of the way of a full-screen app there.
fn watch_displays(overlay: OverlayHandle<Island>) {
    std::thread::Builder::new().name("nook-displays".into()).spawn(move || {
        let mut on: Option<platform::Display> = None;
        let mut hidden = false;
        let mut checked = std::time::Instant::now() - Duration::from_secs(10);
        loop {
            std::thread::sleep(Duration::from_millis(300));
            let all = platform::displays();
            let Some(target) = island::pick(island::display_mode(), &all, platform::pointer()).copied() else { continue };
            if on != Some(target) {
                if on.map(|d| d.id) != Some(target.id) {
                    overlay.set_display(DisplayTarget::Native(rune::NativeDisplayId::MacOS(target.id)));
                }
                on = Some(target);
                let notch = Notch::of(&target);
                overlay.update(move |s, _| s.set_notch(notch));
                // A move re-measures at once.
                checked = std::time::Instant::now() - Duration::from_secs(10);
            }
            // The window list costs about a millisecond: once a second is plenty.
            if checked.elapsed() < Duration::from_secs(1) {
                continue;
            }
            checked = std::time::Instant::now();
            let hide = island::HIDE_IN_FULL_SCREEN.load(std::sync::atomic::Ordering::Relaxed) && platform::fullscreen_on(&target);
            if hide != hidden {
                hidden = hide;
                log::info!("island: {} (full screen on display {})", if hide { "hidden" } else { "shown" }, target.id);
                if hide {
                    overlay.fade_out(Duration::from_millis(180));
                } else {
                    overlay.fade_in(Duration::from_millis(180));
                }
            }
        }
    }).expect("display thread");
}

fn already_running() -> Option<u32> {
    let path = home::app_dir().join("opennook.pid");
    let _ = std::fs::create_dir_all(home::app_dir());
    if let Some(pid) = std::fs::read_to_string(&path).ok().and_then(|t| t.trim().parse::<i32>().ok()) {
        // Signal 0 only asks whether the process exists.
        if pid != std::process::id() as i32 && unsafe { libc::kill(pid, 0) } == 0 {
            return Some(pid as u32);
        }
    }
    let _ = std::fs::write(&path, std::process::id().to_string());
    None
}

/// `check` and `shot` have no main run loop for CoreLocation to answer on.
fn no_location(store: &Store) {
    store.publish("location", serde_json::json!({ "error": "No location in check and shot; give the mod a city (props) to try it here." }));
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn check(args: &[String]) -> i32 {
    let Some(dir) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: opennook check <mod folder> [--wait ms]");
        return 2;
    };
    let wait = flag(args, "--wait").and_then(|w| w.parse().ok()).unwrap_or(1500);
    let _ = home::write_workspace();
    services::start_core();
    let store: Arc<Store> = Arc::default();
    if let Ok((_, raw)) = home::read_layout(&home::app_dir()) {
        store.publish("layout", raw);
    }
    store.publish("theme", theme::Theme::default().to_json());
    services::start(store.clone());
    services::start_agents(store.clone());
    no_location(&store);
    let notch = Notch::detect(island::display_mode());
    store.publish("island", serde_json::json!({ "expanded": false, "selected": null, "active": null,
        "notch": { "width": notch.w, "height": notch.h, "hasNotch": notch.has }, "screen": { "width": notch.screen.0, "height": notch.screen.1 } }));
    // Let the first media and agent answers land, as they would in the running island.
    std::thread::sleep(Duration::from_millis(300));
    let checked = match nook_mods::check(home::config(), store, PathBuf::from(dir), Duration::from_millis(wait)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let out = checked.output;
    let mut problems: Vec<String> = out.error.iter().cloned().collect();
    let mut nodes = Vec::new();
    if let Some(a) = &out.activity {
        nodes.extend(a.leading.iter().chain(a.trailing.iter()));
    }
    nodes.extend(out.expanded.iter());
    for n in nodes {
        n.walk(&mut |n| {
            if n.t == nook_mods::node::Kind::Shader {
                let path = render::shader_path(n.v.as_deref().unwrap_or_default(), &out.dir);
                if let Err(e) = shader::source(&path) {
                    problems.push(format!("shader: {e}"));
                }
            }
            if n.t == nook_mods::node::Kind::Icon {
                let name = n.v.as_deref().unwrap_or_default();
                if icons::get(name, &out.dir).is_none() {
                    problems.push(format!("unknown icon {name:?} (AGENTS.md lists the built-in ones)"));
                }
            }
        });
    }
    if let Some(icon) = out.icon.as_deref().filter(|i| !i.starts_with("app:")) {
        if icons::is_picture(icon) {
            if !out.dir.join(icon).is_file() {
                problems.push(format!("mod.json: no icon file {icon:?} in the mod's folder"));
            }
        } else if icons::get(icon, &out.dir).is_none() {
            problems.push(format!("mod.json: unknown icon {icon:?}"));
        }
    }
    if !problems.is_empty() {
        eprintln!("{}: {} problem(s)\n", out.name, problems.len());
        for p in &problems {
            eprintln!("{p}\n");
        }
        return 1;
    }
    let activity = match &out.activity {
        Some(a) => format!("shows an activity (priority {})", a.priority),
        None => "no activity right now".into(),
    };
    let page = match (&out.expanded, out.has_expanded) {
        (Some(_), _) => "has a page",
        (None, true) => "has a page (empty right now)",
        (None, false) => "no page",
    };
    println!("ok: {} — {activity}, {page}", out.name);
    for (c, a) in &checked.commands {
        println!("  ran {c} {a}");
    }
    0
}

/// Renders the island headless with live mods: what an agent looks at after a change.
fn shot(args: &[String]) -> i32 {
    let out = args.first().filter(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| "island.png".into());
    if args.iter().any(|a| a == "--settings") {
        let mut state = settings::Settings::new();
        state.page = match flag(args, "--page").as_deref() {
            Some("mods") => settings::Page::Mods,
            Some("yours") => settings::Page::Yours,
            Some("about") => settings::Page::About,
            _ => settings::Page::General,
        };
        state.onboarding = flag(args, "--onboarding").and_then(|n| n.parse().ok());
        let spec = AppSpec::new("openNook", settings::SIZE, settings::Settings::new, settings::view);
        let size = format!("{}x{}", settings::SIZE.0, settings::SIZE.1);
        let path = PathBuf::from(&out);
        App::new(spec).init(move || state)
            .args(["--snap".to_owned(), out.clone(), "--size".into(), size, "--scale".into(), "2".into(), "--chrome".into()])
            .chrome(rune::Chrome { full_size_content: true, traffic_lights: Some((18.0, 18.0)), drag_regions: false })
            .run();
        println!("{}", path.canonicalize().unwrap_or(path).display());
        return 0;
    }
    let open = args.iter().any(|a| a == "--open") || flag(args, "--mod").is_some();
    let wait = flag(args, "--wait").and_then(|w| w.parse().ok()).unwrap_or(2500);
    let (mut island, store) = prepare();
    let outputs: Arc<Mutex<BTreeMap<String, Output>>> = Arc::default();
    struct Collect(Arc<Mutex<BTreeMap<String, Output>>>);
    impl Host for Collect {
        fn output(&self, out: Output) {
            self.0.lock().unwrap().insert(out.name.clone(), out);
        }
        fn removed(&self, name: &str) {
            self.0.lock().unwrap().remove(name);
        }
        fn command(&self, _: &str, _: &str, _: Value) {}
    }
    no_location(&store);
    let mods = Mods::start(home::config(), store.clone(), Collect(outputs.clone()));
    services::start_agents(store.clone());
    services::start(store);
    std::thread::sleep(Duration::from_millis(wait));
    mods.stop();
    island.outputs = outputs.lock().unwrap().clone();
    island.expanded = open;
    island.hover = args.iter().any(|a| a == "--hover");
    if let Some(m) = flag(args, "--mod") {
        island.select(&m);
    }
    let size = format!("{}x{}", island.canvas_w(), CANVAS_H);
    // After `--`: Rune's own tooling flags in place of the still (`--script s.json --strip x.png
    // --frames 30 --every 2`, `--tree t.json`, …), for looking at motion.
    let rune_args: Vec<String> = args.iter().skip_while(|a| *a != "--").skip(1).cloned().collect();
    let spec = AppSpec { setup: Some(island_setup), ..AppSpec::new("openNook", (island.canvas_w(), CANVAS_H), || Island::new(Notch::detect(island::display_mode()), home::Layout::default()), island::view) };
    let path = PathBuf::from(&out);
    App::new(spec)
        .init(move || island)
        .compositor(shader::Shaders::default())
        .window_options(rune::WindowOptions { transparent: true, ..Default::default() })
        .args(if rune_args.is_empty() {
            vec!["--snap".to_owned(), out.clone(), "--size".into(), size, "--scale".into(), "2".into(), "--at".into(), "1".into()]
        } else {
            [vec!["--size".to_owned(), size, "--scale".into(), "2".into()], rune_args].concat()
        })
        .run();
    println!("{}", path.canonicalize().unwrap_or(path).display());
    0
}
