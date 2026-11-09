use super::*;
use serde_json::json;
use std::path::Path;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nook-mods-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("mods")).unwrap();
    dir
}

fn write_mod(app: &Path, name: &str, main: &str) -> PathBuf {
    let dir = app.join("mods").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.js"), main).unwrap();
    dir
}

fn run_check(app: &Path, dir: PathBuf, store: Arc<Store>, settle_ms: u64) -> Checked {
    check(Config::at(app.to_path_buf(), app.to_path_buf()), store, dir, Duration::from_millis(settle_ms)).unwrap()
}

#[test]
fn renders_activity_and_expanded() {
    let app = temp("render");
    let dir = write_mod(&app, "hello", r#"
        import { View, row, text, icon } from "nook/ui";
        export default class Hello extends View {
          init(props) { this.name = props.name ?? "world"; }
          activity() { return { leading: icon("music").color("accent"), trailing: text("hi"), priority: 5 }; }
          expanded() { return row().id("root").gap(8).px(12).child(text(`hello ${this.name}`).text_size(15).font_semibold()); }
        }
    "#);
    let out = run_check(&app, dir, Arc::default(), 0).output;
    assert_eq!(out.error, None);
    let activity = out.activity.unwrap();
    assert_eq!(activity.priority, 5.0);
    assert_eq!(activity.leading.unwrap().v.as_deref(), Some("music"));
    let root = out.expanded.unwrap();
    assert_eq!(root.k.as_deref(), Some("root"));
    assert_eq!(root.s.gap, Some(8.0));
    assert_eq!(root.s.p, Some([0.0, 12.0, 0.0, 12.0]));
    assert_eq!(root.c[0].v.as_deref(), Some("hello world"));
    assert_eq!(root.c[0].s.weight, Some(600.0));
}

#[test]
fn rerenders_when_a_read_topic_changes() {
    let app = temp("reads");
    let dir = write_mod(&app, "np", r#"
        import { View, text } from "nook/ui";
        import { media } from "nook";
        export default class Np extends View {
          activity() { const m = media(); return m ? { trailing: text(m.title) } : null; }
        }
    "#);
    let store: Arc<Store> = Arc::default();
    let publisher = store.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        publisher.publish("media", json!({ "title": "Song" }));
    });
    let out = run_check(&app, dir, store, 600).output;
    assert_eq!(out.activity.unwrap().trailing.unwrap().v.as_deref(), Some("Song"));
}

#[test]
fn an_endless_loop_is_stopped() {
    let app = temp("loop");
    let dir = write_mod(&app, "spin", r#"
        import { View, text } from "nook/ui";
        export default class Spin extends View {
          expanded() { for (;;) {} }
        }
    "#);
    let out = run_check(&app, dir, Arc::default(), 0).output;
    let error = out.error.unwrap();
    assert!(error.contains("at expanded"), "{error}");
    assert!(error.contains("stopped after"), "{error}");
}

#[test]
fn a_syntax_error_is_reported() {
    let app = temp("syntax");
    let dir = write_mod(&app, "broken", "export default class extends {");
    let out = run_check(&app, dir, Arc::default(), 0).output;
    let error = out.error.unwrap();
    assert!(error.contains("main.js:1:"), "{error}");
}

#[test]
fn timers_and_exec_settle() {
    let app = temp("async");
    let dir = write_mod(&app, "later", r#"
        import { View, text } from "nook/ui";
        import { exec } from "nook";
        export default class Later extends View {
          init(_p, cx) {
            this.ticks = 0;
            cx.timer.every(50, (cx) => { this.ticks++; cx.notify(); });
            cx.spawn(async (cx) => { this.out = (await exec("echo", ["from exec"])).stdout.trim(); cx.notify(); });
          }
          expanded() { return text(`${this.out ?? "…"} ${this.ticks > 1 ? "ticked" : ""}`); }
        }
    "#);
    let out = run_check(&app, dir, Arc::default(), 400).output;
    assert_eq!(out.error, None);
    assert_eq!(out.expanded.unwrap().v.as_deref(), Some("from exec ticked"));
}

#[test]
fn an_unhandled_rejection_is_reported() {
    let app = temp("reject");
    let dir = write_mod(&app, "reject", r#"
        import { View, text } from "nook/ui";
        export default class R extends View {
          init() { Promise.reject(new Error("nope")); }
          expanded() { return text("x"); }
        }
    "#);
    let out = run_check(&app, dir, Arc::default(), 100).output;
    assert!(out.error.unwrap().contains("nope"));
}

#[test]
fn a_caught_async_throw_is_not_reported() {
    let app = temp("caught");
    let dir = write_mod(&app, "caught", r#"
        import { View, text } from "nook/ui";
        export default class C extends View {
          async inner() { throw new Error("handled"); }
          init(_p, cx) { cx.spawn(async (cx) => { try { await this.inner(); } catch { this.ok = true; cx.notify(); } }); }
          expanded() { return text(this.ok ? "caught" : "…"); }
        }
    "#);
    let out = run_check(&app, dir, Arc::default(), 100).output;
    assert_eq!(out.error, None);
    assert_eq!(out.expanded.unwrap().v.as_deref(), Some("caught"));
}

#[test]
fn run_reaches_the_host_and_storage_persists() {
    let app = temp("run");
    let main = r#"
        import { View, text } from "nook/ui";
        import { run } from "nook";
        export default class R extends View {
          init() {
            this.n = Number(localStorage.getItem("n") ?? 0) + 1;
            localStorage.setItem("n", String(this.n));
            run("island.peek", { ms: 1000 });
          }
          expanded() { return text(String(this.n)); }
        }
    "#;
    let dir = write_mod(&app, "run", main);
    let first = run_check(&app, dir.clone(), Arc::default(), 0);
    assert_eq!(first.commands, vec![("island.peek".to_owned(), json!({ "ms": 1000 }))]);
    let second = run_check(&app, dir, Arc::default(), 0);
    assert_eq!(second.output.expanded.unwrap().v.as_deref(), Some("2"));
}

#[test]
fn shipped_mods_update_and_keep_edits() {
    let app = temp("ship");
    let mods = app.join("mods");
    let installed = app.join("installed.json");
    let v1: &[(&str, &[u8])] = &[("clock/main.js", b"v1"), ("clock/mod.json", b"{}")];
    assert!(files::sync_shipped(&mods, &installed, v1).unwrap().is_empty());
    assert_eq!(std::fs::read(mods.join("clock/main.js")).unwrap(), b"v1");
    // Untouched: a new version replaces it.
    let v2: &[(&str, &[u8])] = &[("clock/main.js", b"v2"), ("clock/mod.json", b"{}")];
    assert!(files::sync_shipped(&mods, &installed, v2).unwrap().is_empty());
    assert_eq!(std::fs::read(mods.join("clock/main.js")).unwrap(), b"v2");
    // Edited: the edit moves aside before the update lands.
    std::fs::write(mods.join("clock/main.js"), b"mine").unwrap();
    let v3: &[(&str, &[u8])] = &[("clock/main.js", b"v3"), ("clock/mod.json", b"{}")];
    assert_eq!(files::sync_shipped(&mods, &installed, v3).unwrap(), vec!["clock-edited".to_owned()]);
    assert_eq!(std::fs::read(mods.join("clock-edited/main.js")).unwrap(), b"mine");
    assert_eq!(std::fs::read(mods.join("clock/main.js")).unwrap(), b"v3");
}

#[test]
fn tail_follows_appended_lines() {
    let app = temp("tail");
    let log = app.join("live.ndjson");
    std::fs::write(&log, "old\n").unwrap();
    let dir = write_mod(&app, "tailer", &format!(r#"
        import {{ View, text }} from "nook/ui";
        import {{ tail }} from "nook";
        export default class T extends View {{
          init(_p, cx) {{ this.lines = []; tail({:?}, (line, cx) => {{ this.lines.push(line); cx.notify(); }}); }}
          expanded() {{ return text(this.lines.join(",")); }}
        }}
    "#, log.to_string_lossy()));
    let writer = log.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(writer).unwrap();
        writeln!(f, "a").unwrap();
        writeln!(f, "b").unwrap();
    });
    // check() doesn't run the watcher thread; drive one here.
    let config = Config::at(app.clone(), app.clone());
    let store: Arc<Store> = Arc::default();
    let (tx, rx) = std::sync::mpsc::channel();
    let watches = engine::new_watches();
    watch::spawn(watches.clone(), config.mods.clone(), tx.clone());
    let recorder: Arc<Recorder> = Arc::default();
    let mut e = engine::Engine::new(Arc::new(config), store, Arc::new(recorder.clone()), tx, watches, Some(files::Found { name: "tailer".into(), dir })).unwrap();
    e.reload();
    let until = Instant::now() + Duration::from_millis(900);
    while Instant::now() < until {
        if let Ok(m) = rx.recv_timeout(Duration::from_millis(50)) {
            e.handle(m);
        }
        e.step();
    }
    let out = recorder.outputs.lock().unwrap().last().cloned().unwrap();
    assert_eq!(out.expanded.unwrap().v.as_deref(), Some("a,b"));
}

#[test]
fn style_reaches_the_output_and_a_bad_one_is_reported() {
    let app = temp("style");
    let dir = write_mod(&app, "glow", r##"
        import { View, text } from "nook/ui";
        export default class Glow extends View {
          activity() { return { trailing: text("!") }; }
          style() { return { glow: { color: "orange" }, border: { width: 1, color: "#ff9f0a" }, compactRadius: 10 }; }
        }
    "##);
    let out = run_check(&app, dir, Arc::default(), 0).output;
    assert_eq!(out.error, None);
    let style = out.style.unwrap();
    assert_eq!(style.glow.unwrap(), Glow { color: "orange".into(), radius: 18.0 });
    assert_eq!(style.border.unwrap().width, 1.0);
    assert_eq!(style.compact_radius, Some(10.0));

    let dir = write_mod(&app, "typo", r#"
        import { View } from "nook/ui";
        export default class Typo extends View { style() { return { colour: "red" }; } }
    "#);
    let out = run_check(&app, dir, Arc::default(), 0).output;
    assert!(out.style.is_none());
    assert!(out.error.unwrap().contains("style(): unknown field `colour`"));
}

#[test]
fn a_style_fills_in_from_its_base() {
    let base = IslandStyle { fill: Some("#111111".into()), compact_radius: Some(14.0), shadow: Some(false), ..Default::default() };
    let over = IslandStyle { gradient: Some(("blue".into(), "purple".into())), compact_radius: Some(8.0), ..Default::default() };
    let s = over.over(&base);
    // A gradient replaces the base's fill rather than sitting beside it.
    assert_eq!(s.fill, None);
    assert_eq!(s.gradient, Some(("blue".into(), "purple".into())));
    assert_eq!(s.compact_radius, Some(8.0));
    assert_eq!(s.shadow, Some(false));
}

#[test]
fn a_text_field_keeps_its_state_in_the_app() {
    let app = temp("input");
    let dir = write_mod(&app, "todo", r#"
        import { View, row, InputState, Input } from "nook/ui";
        export default class Todo extends View {
          init() { this.field = InputState.new({ placeholder: "New item", value: "milk" }); }
          expanded() { return row().child(Input.new(this.field).flex_1().text_size(14).on_key_down(() => {})); }
        }
    "#);
    let checked = run_check(&app, dir, Arc::default(), 0);
    assert_eq!(checked.output.error, None);
    let field = &checked.output.expanded.unwrap().c[0];
    assert_eq!(field.t, node::Kind::Input);
    assert_eq!(field.s.placeholder.as_deref(), Some("New item"));
    assert!(field.on.key.is_some());
    let id: u64 = field.v.as_deref().unwrap().parse().unwrap();
    assert_eq!(checked.inputs, vec![(id, json!({ "op": "new", "value": "milk" }))]);
}

#[test]
fn a_text_field_made_in_a_render_is_an_error() {
    let app = temp("input-render");
    let dir = write_mod(&app, "bad", r#"
        import { View, InputState, Input } from "nook/ui";
        export default class Bad extends View {
          expanded() { return Input.new(InputState.new()); }
        }
    "#);
    let out = run_check(&app, dir, Arc::default(), 0).output;
    assert!(out.error.unwrap().contains("belongs in init()"));
}

#[test]
fn text_field_events_reach_the_mod() {
    let app = temp("input-events");
    write_mod(&app, "echo", r#"
        import { View, text, InputState } from "nook/ui";
        export default class Echo extends View {
          init() {
            this.field = InputState.new();
            this.log = [];
            this.field.on("change", (e, cx) => { this.log.push(`change:${this.field.value()}`); cx.notify(); });
            this.field.on("focus", (e, cx) => { this.log.push(`focus:${this.field.is_focused()}`); cx.notify(); });
            this.field.on("submit", (e, cx) => { this.log.push("submit"); this.field.set_value(""); cx.notify(); });
          }
          expanded() { return text(`${this.field.id} ${this.log.join(" ")}`); }
        }
    "#);
    #[derive(Clone, Default)]
    struct Seen(Arc<std::sync::Mutex<Vec<String>>>, Arc<std::sync::Mutex<Vec<Value>>>);
    impl Host for Seen {
        fn output(&self, out: Output) {
            if let Some(n) = out.expanded {
                self.0.lock().unwrap().push(n.v.unwrap_or_default());
            }
        }
        fn removed(&self, _: &str) {}
        fn command(&self, _: &str, _: &str, _: Value) {}
        fn input(&self, _: &str, _: u64, op: Value) {
            self.1.lock().unwrap().push(op);
        }
    }
    let seen = Seen::default();
    let mods = Mods::start(Config::at(app.clone(), app.clone()), Arc::default(), seen.clone());
    let wait_for = |want: &str| {
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if let Some(last) = seen.0.lock().unwrap().last().filter(|l| l.ends_with(want)) {
                return last.clone();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("never rendered {want:?}; saw {:?}", seen.0.lock().unwrap());
    };
    let first = wait_for(" ");
    let id: u64 = first.split(' ').next().unwrap().parse().unwrap();
    mods.input_event("echo", id, "focus", json!({}));
    mods.input_event("echo", id, "change", json!({ "value": "eggs" }));
    mods.input_event("echo", id, "submit", json!({}));
    // An id the mod never made is nobody's.
    mods.input_event("echo", id + 1000, "submit", json!({}));
    wait_for("focus:true change:eggs submit");
    mods.stop();
    assert!(seen.1.lock().unwrap().contains(&json!({ "op": "set", "value": "" })));
}

#[test]
fn a_scrolling_element_needs_an_id() {
    let app = temp("scroll-id");
    let dir = write_mod(&app, "list", r#"
        import { View, col, text } from "nook/ui";
        export default class List extends View {
          expanded() { return col().max_h(100).overflow_y_scroll().child(text("a")); }
        }
    "#);
    assert!(run_check(&app, dir, Arc::default(), 0).output.error.unwrap().contains("needs an .id()"));
    let ok = write_mod(&app, "list2", r#"
        import { View, col, text } from "nook/ui";
        export default class List extends View {
          expanded() { return col().id("l").max_h(100).stick_bottom().child(text("a")); }
        }
    "#);
    let out = run_check(&app, ok, Arc::default(), 0).output;
    assert_eq!(out.error, None);
    let s = out.expanded.unwrap().s;
    assert!(s.scroll && s.stick_bottom);
}
