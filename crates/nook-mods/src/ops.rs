//! A mod's asynchronous requests: programs, HTTP and files. Each runs on its own thread and
//! answers with JSON (a value on success, an error message otherwise).

use serde::Deserialize;
use serde_json::{Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const EXEC_LIMIT: Duration = Duration::from_secs(30);
const OUTPUT_LIMIT: u64 = 8 << 20;
const READ_LIMIT: u64 = 4 << 20;

pub struct Paths {
    pub home: PathBuf,
    pub app: PathBuf,
}

pub fn run(op: &str, args: Value, mod_dir: &Path, paths: &Paths) -> Result<Value, String> {
    match op {
        "exec" => exec(serde_json::from_value(args).map_err(|e| format!("exec: {e}"))?, paths),
        "fetch" => fetch(serde_json::from_value(args).map_err(|e| format!("fetch: {e}"))?),
        "read" => read(&resolve(str_arg(&args, "path")?, mod_dir, paths)),
        "write" => write(&resolve(str_arg(&args, "path")?, mod_dir, paths), str_arg(&args, "text")?, paths),
        _ => Err(format!("unknown operation {op}")),
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| format!("missing {key}"))
}

/// `~/` is the home folder; `./` and other relative paths are the mod's folder.
pub fn resolve(path: &str, mod_dir: &Path, paths: &Paths) -> PathBuf {
    if path.starts_with('~') {
        crate::files::expand(path, &paths.home)
    } else if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        mod_dir.join(path)
    }
}

#[derive(Deserialize)]
struct Exec {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
}

/// PATH as a login shell would have it: an app started from Finder gets only the system's.
pub fn user_path(home: &Path) -> String {
    let mut dirs: Vec<String> = [".local/bin", ".cargo/bin", ".bun/bin", ".opencode/bin", ".npm-global/bin"]
        .iter().map(|d| home.join(d).to_string_lossy().into_owned()).collect();
    dirs.extend(["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"].map(String::from));
    if let Ok(path) = std::env::var("PATH") {
        dirs.extend(path.split(':').map(String::from));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs.join(":")
}

fn exec(e: Exec, paths: &Paths) -> Result<Value, String> {
    if let Some(answer) = demo_exec(&e) {
        return Ok(answer);
    }
    let mut cmd = Command::new(&e.command);
    cmd.args(&e.args).env("PATH", user_path(&paths.home)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.current_dir(e.cwd.as_deref().map(|c| crate::files::expand(c, &paths.home)).unwrap_or_else(|| paths.home.clone()));
    let mut child = cmd.spawn().map_err(|err| format!("{}: {err}", e.command))?;
    let pipe = |r: Option<Box<dyn Read + Send>>| std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(r) = r {
            let _ = r.take(OUTPUT_LIMIT).read_to_end(&mut out);
        }
        String::from_utf8_lossy(&out).into_owned()
    });
    let stdout = pipe(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let stderr = pipe(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > EXEC_LIMIT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{}: still running after {} s, stopped", e.command, EXEC_LIMIT.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(err) => return Err(format!("{}: {err}", e.command)),
        }
    };
    Ok(json!({
        "code": status.code().unwrap_or(-1),
        "stdout": stdout.join().unwrap_or_default(),
        "stderr": stderr.join().unwrap_or_default(),
    }))
}

/// Demo answers for programs, for screenshots (`OPENNOOK_DEMO`'s `exec`: `[{ command, args,
/// contains, stdout, code }]`). The first entry whose command matches, whose `args` begin the
/// call's and whose `contains` is in one of them (an SQL query, say) answers in place of running
/// it. `stdout` is text, or JSON written out as text, in which `{ "$rate": n }` becomes a counter
/// going up by `n` a second (a CPU time), and `{ "$now": s }` / `{ "$nowSeconds": s }` the time
/// `s` seconds from now in milliseconds / seconds since 1970 (a due date, a message's time),
/// and `{ "$nowIso": s }` the same as an ISO 8601 date (a start time).
fn demo_exec(e: &Exec) -> Option<Value> {
    let (list, start) = demo("exec");
    let args = |d: &Value| d.get("args").and_then(Value::as_array).map_or(true, |a| {
        a.len() <= e.args.len() && a.iter().zip(&e.args).all(|(x, y)| x.as_str() == Some(y.as_str()))
    });
    let contains = |d: &Value| d.get("contains").and_then(Value::as_str).is_none_or(|c| e.args.iter().any(|a| a.contains(c)));
    let d = list.iter().find(|d| d.get("command").and_then(Value::as_str) == Some(e.command.as_str()) && args(d) && contains(d))?;
    let stdout = match d.get("stdout") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => rates(v.clone(), start.elapsed().as_secs_f64()).to_string(),
        None => String::new(),
    };
    Some(json!({ "code": d.get("code").and_then(Value::as_i64).unwrap_or(0), "stdout": stdout, "stderr": "" }))
}

/// Demo answers for HTTP (`OPENNOOK_DEMO`'s `fetch`: `[{ url, status, body }]`): the first
/// entry whose `url` begins the request's answers it, `body` text or JSON written out as text.
fn demo_fetch(f: &Fetch) -> Option<Value> {
    let (list, _) = demo("fetch");
    let d = list.iter().find(|d| d.get("url").and_then(Value::as_str).is_some_and(|u| f.url.starts_with(u)))?;
    let body = match d.get("body") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    Some(json!({ "status": d.get("status").and_then(Value::as_u64).unwrap_or(200), "headers": {}, "body": body }))
}

/// The demo fixture's `key` list, read once, and when it was read.
fn demo(key: &str) -> (Vec<Value>, Instant) {
    static FIXTURE: std::sync::OnceLock<(Value, Instant)> = std::sync::OnceLock::new();
    let (v, start) = FIXTURE.get_or_init(|| {
        let v = std::env::var_os("OPENNOOK_DEMO")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .unwrap_or(Value::Null);
        (v, Instant::now())
    });
    (v.get(key).and_then(Value::as_array).cloned().unwrap_or_default(), *start)
}

/// Seconds since 1970 as "2026-11-09T21:30:00Z" (Howard Hinnant's days-to-civil).
fn iso_date(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn rates(v: Value, secs: f64) -> Value {
    let now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let num = |o: &serde_json::Map<String, Value>, k: &str| o[k].as_f64().unwrap_or(0.0);
    match v {
        Value::Object(o) if o.len() == 1 && o.contains_key("$rate") => {
            json!((num(&o, "$rate") * secs).round() as u64)
        }
        Value::Object(o) if o.len() == 1 && o.contains_key("$now") => json!(((now() + num(&o, "$now")) * 1000.0).round() as u64),
        Value::Object(o) if o.len() == 1 && o.contains_key("$nowSeconds") => json!((now() + num(&o, "$nowSeconds")).round() as u64),
        Value::Object(o) if o.len() == 1 && o.contains_key("$nowIso") => json!(iso_date((now() + num(&o, "$nowIso")) as i64)),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, rates(v, secs))).collect()),
        Value::Array(a) => Value::Array(a.into_iter().map(|v| rates(v, secs)).collect()),
        v => v,
    }
}

#[derive(Deserialize)]
struct Fetch {
    url: String,
    #[serde(default = "get")]
    method: String,
    #[serde(default)]
    headers: std::collections::BTreeMap<String, String>,
    body: Option<String>,
}

fn get() -> String {
    "GET".into()
}

fn fetch(f: Fetch) -> Result<Value, String> {
    if let Some(answer) = demo_fetch(&f) {
        return Ok(answer);
    }
    let config = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(EXEC_LIMIT)).build();
    let agent = ureq::Agent::new_with_config(config);
    let mut builder = ureq::http::Request::builder().method(f.method.to_uppercase().as_str()).uri(&f.url);
    for (k, v) in &f.headers {
        builder = builder.header(k, v);
    }
    let result = match f.body {
        Some(body) => agent.run(builder.body(body).map_err(|e| e.to_string())?),
        None => agent.run(builder.body(()).map_err(|e| e.to_string())?),
    };
    let mut response = result.map_err(|e| format!("{}: {e}", f.url))?;
    let status = response.status().as_u16();
    let headers: serde_json::Map<String, Value> = response.headers().iter()
        .filter_map(|(k, v)| Some((k.as_str().to_owned(), Value::from(v.to_str().ok()?))))
        .collect();
    let body = response.body_mut().with_config().limit(OUTPUT_LIMIT).read_to_string().map_err(|e| format!("{}: {e}", f.url))?;
    Ok(json!({ "status": status, "headers": headers, "body": body }))
}

fn read(path: &Path) -> Result<Value, String> {
    match std::fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(READ_LIMIT).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(Value::from(String::from_utf8_lossy(&bytes).into_owned()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn write(path: &Path, text: &str, paths: &Paths) -> Result<Value, String> {
    // Mods change their own world (this folder), not the rest of the disk.
    let app = paths.app.canonicalize().unwrap_or_else(|_| paths.app.clone());
    let parent = path.parent().ok_or("no folder")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let real = parent.canonicalize().map_err(|e| format!("{}: {e}", parent.display()))?;
    if !real.starts_with(&app) {
        return Err(format!("{}: writes go under {}", path.display(), paths.app.display()));
    }
    crate::files::write_atomic(path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Value::Null)
}

#[cfg(test)]
mod demo_tests {
    #[test]
    fn iso_dates_are_civil_utc() {
        assert_eq!(super::iso_date(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::iso_date(1_794_249_600), "2026-11-09T18:40:00Z");
        assert_eq!(super::iso_date(951_782_400), "2000-02-29T00:00:00Z");
    }
}
