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
