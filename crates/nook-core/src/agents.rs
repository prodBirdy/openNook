//! Detect running AI coding agents from local processes and session files.
//!
//! Binary matching, process-tree walks, and the Claude / Codex / OpenCode
//! fingerprints follow [abtop](https://github.com/graykode/abtop)
//! (MIT License, Copyright (c) 2026 Tae Hwan Jung). Grok and Cursor Agent
//! workers are matched the same way.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const CPU_ACTIVE: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentKind {
    Claude,
    Codex,
    OpenCode,
    Grok,
    Cursor,
    Aider,
    Gemini,
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude",
            AgentKind::Codex => "Codex",
            AgentKind::OpenCode => "OpenCode",
            AgentKind::Grok => "Grok",
            AgentKind::Cursor => "Cursor",
            AgentKind::Aider => "Aider",
            AgentKind::Gemini => "Gemini",
        }
    }

    fn binaries(self) -> &'static [&'static str] {
        match self {
            AgentKind::Claude => &["claude"],
            AgentKind::Codex => &["codex"],
            AgentKind::OpenCode => &["opencode"],
            AgentKind::Grok => &["grok"],
            AgentKind::Cursor => &["cursor-agent"],
            AgentKind::Aider => &["aider"],
            AgentKind::Gemini => &["gemini"],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Working,
    Waiting,
}

impl AgentStatus {
    pub fn label(self) -> &'static str {
        match self {
            AgentStatus::Working => "Working",
            AgentStatus::Waiting => "Waiting",
        }
    }

    pub fn is_working(self) -> bool {
        matches!(self, AgentStatus::Working)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentSession {
    pub kind: AgentKind,
    pub pid: u32,
    pub project: String,
    pub cwd: String,
    pub status: AgentStatus,
    pub session_id: Option<String>,
    /// Human session title: Claude sidecar `name`, Grok `generated_title`.
    pub name: Option<String>,
    /// Model id when the session file reports one (Grok `current_model_id`).
    pub model: Option<String>,
}

impl AgentSession {
    /// Compact label: named session if we have one, otherwise the project folder.
    pub fn title(&self) -> &str {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(self.project.as_str())
    }
}

#[derive(Clone, Debug)]
struct ProcInfo {
    pid: u32,
    ppid: u32,
    cpu_pct: f32,
    name: String,
    argv: Vec<String>,
    cwd: Option<PathBuf>,
    exe: Option<PathBuf>,
}

/// Live coding-agent sessions on this machine. Cheap enough for a 2s poll.
pub fn snapshot() -> Vec<AgentSession> {
    let procs = scan_processes();
    assemble(&procs)
}

fn process_system() -> &'static Mutex<System> {
    static SYS: OnceLock<Mutex<System>> = OnceLock::new();
    SYS.get_or_init(|| Mutex::new(System::new()))
}

fn scan_processes() -> HashMap<u32, ProcInfo> {
    let mut sys = process_system().lock().unwrap_or_else(|e| e.into_inner());
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cpu()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );

    let mut map = HashMap::new();
    for (pid, proc_) in sys.processes() {
        let pid_u32 = pid.as_u32();
        let name = proc_.name().to_string_lossy().into_owned();
        let argv: Vec<String> = if proc_.cmd().is_empty() {
            if name.is_empty() {
                continue;
            }
            vec![name.clone()]
        } else {
            proc_
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect()
        };
        map.insert(
            pid_u32,
            ProcInfo {
                pid: pid_u32,
                ppid: proc_.parent().map(|p| p.as_u32()).unwrap_or(0),
                cpu_pct: proc_.cpu_usage(),
                name,
                argv,
                cwd: proc_.cwd().map(PathBuf::from),
                exe: proc_.exe().map(PathBuf::from),
            },
        );
    }
    map
}

fn assemble(procs: &HashMap<u32, ProcInfo>) -> Vec<AgentSession> {
    assemble_inner(procs, &grok_sessions(), &claude_sessions())
}

fn assemble_inner(
    procs: &HashMap<u32, ProcInfo>,
    grok_meta: &HashMap<u32, GrokActive>,
    claude_meta: &HashMap<u32, ClaudeSidecar>,
) -> Vec<AgentSession> {
    let children = children_map(procs);
    let self_pid = std::process::id();

    let mut candidates: Vec<u32> = procs
        .values()
        .filter(|p| !is_descendant_of(p.pid, self_pid, procs))
        .filter(|p| kind_of(p, grok_meta, claude_meta).is_some())
        .map(|p| p.pid)
        .collect();

    // Prefer the real agent binary over a wrapper ancestor of the same kind.
    let kinds: HashMap<u32, AgentKind> = candidates
        .iter()
        .filter_map(|&pid| {
            procs
                .get(&pid)
                .and_then(|p| kind_of(p, grok_meta, claude_meta))
                .map(|k| (pid, k))
        })
        .collect();
    let all_candidates = candidates.clone();
    candidates.retain(|&pid| {
        let Some(kind) = kinds.get(&pid) else {
            return false;
        };
        !all_candidates.iter().any(|&other| {
            other != pid && kinds.get(&other) == Some(kind) && is_descendant_of(other, pid, procs)
        })
    });

    let mut sessions = Vec::new();
    for pid in candidates {
        let Some(proc) = procs.get(&pid) else {
            continue;
        };
        let Some(kind) = kind_of(proc, grok_meta, claude_meta) else {
            continue;
        };

        let mut cwd = match kind {
            AgentKind::Grok => grok_meta
                .get(&pid)
                .map(|s| s.cwd.clone())
                .or_else(|| proc.cwd.as_ref().map(|p| p.to_string_lossy().into_owned())),
            AgentKind::Claude => claude_meta
                .get(&pid)
                .and_then(|s| s.cwd.clone())
                .or_else(|| proc.cwd.as_ref().map(|p| p.to_string_lossy().into_owned())),
            AgentKind::Cursor => proc
                .cwd
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .or_else(|| cursor_worker_dir(&proc.argv)),
            _ => proc.cwd.as_ref().map(|p| p.to_string_lossy().into_owned()),
        }
        .unwrap_or_default();

        if cwd.is_empty() {
            cwd = String::from("?");
        }

        let session_id = match kind {
            AgentKind::Grok => grok_meta.get(&pid).map(|s| s.session_id.clone()),
            AgentKind::Claude => claude_meta.get(&pid).and_then(|s| s.session_id.clone()),
            _ => None,
        };

        let (name, model) = match kind {
            AgentKind::Grok => grok_meta
                .get(&pid)
                .map(|s| grok_session_detail(&s.cwd, &s.session_id))
                .unwrap_or((None, None)),
            AgentKind::Claude => {
                let name = claude_meta
                    .get(&pid)
                    .and_then(|s| nonempty(s.name.as_deref()));
                (name, None)
            }
            _ => (None, None),
        };

        let file_busy = match kind {
            AgentKind::Claude => claude_meta
                .get(&pid)
                .and_then(|s| s.status.as_deref())
                .map(|s| !s.eq_ignore_ascii_case("idle")),
            _ => None,
        };

        let cpu_busy =
            proc.cpu_pct > CPU_ACTIVE || has_active_descendant(pid, &children, procs, CPU_ACTIVE);
        let status = if file_busy.unwrap_or(false) || cpu_busy {
            AgentStatus::Working
        } else {
            AgentStatus::Waiting
        };

        sessions.push(AgentSession {
            kind,
            pid,
            project: project_name(&cwd),
            cwd,
            status,
            session_id,
            name,
            model,
        });
    }

    sessions.sort_by(|a, b| {
        b.status
            .is_working()
            .cmp(&a.status.is_working())
            .then_with(|| a.kind.label().cmp(b.kind.label()))
            .then_with(|| a.project.cmp(&b.project))
            .then_with(|| a.pid.cmp(&b.pid))
    });
    sessions
}

fn kind_of(
    proc: &ProcInfo,
    grok_meta: &HashMap<u32, GrokActive>,
    claude_meta: &HashMap<u32, ClaudeSidecar>,
) -> Option<AgentKind> {
    classify(proc)
        .or_else(|| grok_meta.contains_key(&proc.pid).then_some(AgentKind::Grok))
        .or_else(|| {
            claude_meta
                .contains_key(&proc.pid)
                .then_some(AgentKind::Claude)
        })
}

fn classify(proc: &ProcInfo) -> Option<AgentKind> {
    classify_process(&proc.name, &proc.argv, proc.exe.as_deref())
}

fn classify_process(name: &str, argv: &[String], exe: Option<&Path>) -> Option<AgentKind> {
    if is_excluded(name, argv) {
        return None;
    }

    // Live Grok TUI rewrites argv0 to `agent`; the path only survives on `exe`
    // (`~/.grok/bin/agent` or `~/.grok/downloads/grok-*`).
    if is_grok_process(name, argv, exe) {
        return Some(AgentKind::Grok);
    }

    for kind in [
        AgentKind::Cursor,
        AgentKind::OpenCode,
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Grok,
        AgentKind::Aider,
        AgentKind::Gemini,
    ] {
        if kind.binaries().iter().any(|bin| {
            argv_has_binary(argv, bin)
                || token_has_binary(name, bin)
                || exe.is_some_and(|p| token_has_binary(&p.to_string_lossy(), bin))
        }) {
            return Some(kind);
        }
    }
    None
}

fn is_grok_process(name: &str, argv: &[String], exe: Option<&Path>) -> bool {
    if exe.is_some_and(|p| grok_install_path(&p.to_string_lossy())) {
        return true;
    }
    argv.iter().take(2).any(|tok| {
        grok_install_path(tok) && (token_has_binary(tok, "agent") || token_has_binary(tok, "grok"))
    }) || (token_has_binary(name, "agent") && argv.iter().any(|tok| grok_install_path(tok)))
}

fn grok_install_path(s: &str) -> bool {
    s.contains("/.grok/") || s.contains("\\.grok\\")
}

fn is_excluded(name: &str, argv: &[String]) -> bool {
    let cmd = argv.join(" ");
    if name.contains("Grok Bot")
        || name.contains("CursorUIViewService")
        || name.contains("crashpad")
        || name.contains("Helper (Renderer)")
        || name.contains("Helper (GPU)")
        || name.ends_with(" Helper")
    {
        return true;
    }
    let hay = cmd.as_str();
    hay.contains("mcp-server")
        || hay.contains(" app-server")
        || hay.contains("crashpad")
        || hay.contains("Grok Bot.app")
        || hay.contains("CursorUIViewService")
        || argv_has_binary(argv, "grep")
}

fn argv_has_binary(argv: &[String], name: &str) -> bool {
    argv.iter().take(2).any(|tok| token_has_binary(tok, name))
}

/// Check if an argv token is the named binary.
///
/// Adapted from abtop `cmd_has_binary`: basename match, `.exe` strip, and the
/// `<name>/versions/<file>` autoupdater layout used by Claude Code 2.x.
fn token_has_binary(tok: &str, name: &str) -> bool {
    #[cfg(windows)]
    {
        windows_token_has_binary(tok, name)
    }
    #[cfg(not(windows))]
    {
        unix_token_has_binary(tok, name)
    }
}

#[cfg(not(windows))]
fn unix_token_has_binary(tok: &str, name: &str) -> bool {
    let mut iter = tok.rsplit('/');
    let base = iter.next().unwrap_or(tok);
    if base == name {
        return true;
    }
    if let Some(stripped) = base.strip_suffix(".exe") {
        if stripped == name {
            return true;
        }
    }
    matches!((iter.next(), iter.next()), (Some("versions"), Some(parent)) if parent == name)
}

#[cfg(windows)]
fn windows_token_has_binary(tok: &str, name: &str) -> bool {
    let mut iter = tok.rsplit(['/', '\\']);
    let base = iter.next().unwrap_or(tok);
    let base = base
        .strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".js"))
        .or_else(|| base.strip_suffix(".sh"))
        .or_else(|| base.strip_suffix(".py"))
        .unwrap_or(base);
    if base.eq_ignore_ascii_case(name) {
        return true;
    }
    matches!(
        (iter.next(), iter.next()),
        (Some(versions), Some(parent))
            if versions.eq_ignore_ascii_case("versions") && parent.eq_ignore_ascii_case(name)
    )
}

fn children_map(procs: &HashMap<u32, ProcInfo>) -> HashMap<u32, Vec<u32>> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for proc in procs.values() {
        children.entry(proc.ppid).or_default().push(proc.pid);
    }
    children
}

fn is_descendant_of(pid: u32, ancestor: u32, procs: &HashMap<u32, ProcInfo>) -> bool {
    if pid == 0 || ancestor == 0 || pid == ancestor {
        return false;
    }
    let mut current = pid;
    let mut visited = HashSet::new();
    while visited.insert(current) {
        let Some(info) = procs.get(&current) else {
            return false;
        };
        if info.ppid == ancestor {
            return true;
        }
        if info.ppid == 0 || info.ppid == 1 {
            return false;
        }
        current = info.ppid;
    }
    false
}

fn has_active_descendant(
    pid: u32,
    children: &HashMap<u32, Vec<u32>>,
    procs: &HashMap<u32, ProcInfo>,
    cpu_threshold: f32,
) -> bool {
    let mut stack = vec![pid];
    let mut visited = HashSet::new();
    while let Some(p) = stack.pop() {
        if !visited.insert(p) {
            continue;
        }
        if let Some(kids) = children.get(&p) {
            for &kid in kids {
                if procs
                    .get(&kid)
                    .is_some_and(|info| info.cpu_pct > cpu_threshold)
                {
                    return true;
                }
                stack.push(kid);
            }
        }
    }
    false
}

fn project_name(cwd: &str) -> String {
    if cwd == "?" || cwd.is_empty() {
        return "?".into();
    }
    if let Some(home) = dirs::home_dir() {
        if Path::new(cwd) == home {
            return "~".into();
        }
    }
    last_path_segment(cwd).unwrap_or("?").to_string()
}

fn last_path_segment(s: &str) -> Option<&str> {
    #[cfg(windows)]
    {
        s.rsplit(['/', '\\']).next().filter(|p| !p.is_empty())
    }
    #[cfg(not(windows))]
    {
        s.rsplit('/').next().filter(|p| !p.is_empty())
    }
}

fn cursor_worker_dir(argv: &[String]) -> Option<String> {
    let mut iter = argv.iter();
    while let Some(tok) = iter.next() {
        if let Some(rest) = tok.strip_prefix("--worker-dir=") {
            return Some(rest.to_string());
        }
        if tok == "--worker-dir" {
            return iter.next().cloned();
        }
    }
    None
}

#[derive(Deserialize)]
struct GrokActive {
    session_id: String,
    pid: u32,
    cwd: String,
}

#[derive(Deserialize)]
struct GrokSummary {
    #[serde(default)]
    generated_title: Option<String>,
    #[serde(default)]
    session_summary: Option<String>,
    #[serde(default)]
    current_model_id: Option<String>,
}

fn grok_session_detail(cwd: &str, session_id: &str) -> (Option<String>, Option<String>) {
    let path = grok_session_dir(cwd, session_id).join("summary.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    let Ok(row) = serde_json::from_str::<GrokSummary>(&text) else {
        return (None, None);
    };
    let title = nonempty(row.generated_title.as_deref())
        .or_else(|| nonempty(row.session_summary.as_deref()));
    (title, nonempty(row.current_model_id.as_deref()))
}

fn grok_session_dir(cwd: &str, session_id: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".grok")
        .join("sessions")
        .join(percent_encode_path(cwd))
        .join(session_id)
}

/// Encode a cwd the way Grok stores session folders (`/` → `%2F`).
fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn nonempty(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn grok_sessions() -> HashMap<u32, GrokActive> {
    let path = dirs::home_dir()
        .unwrap_or_default()
        .join(".grok")
        .join("active_sessions.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(rows) = serde_json::from_str::<Vec<GrokActive>>(&text) else {
        return HashMap::new();
    };
    rows.into_iter().map(|row| (row.pid, row)).collect()
}

#[derive(Deserialize)]
struct ClaudeSidecar {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(rename = "sessionId", default)]
    session_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

fn claude_sessions() -> HashMap<u32, ClaudeSidecar> {
    let mut out = HashMap::new();
    for dir in claude_config_dirs() {
        let sessions = dir.join("sessions");
        let Ok(entries) = std::fs::read_dir(&sessions) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(pid) = stem.parse::<u32>().ok() else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Ok(row) = serde_json::from_str::<ClaudeSidecar>(&text) {
                out.insert(pid, row);
            }
        }
    }
    out
}

fn claude_config_dirs() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let mut dirs = vec![home.join(".claude")];
    if let Ok(entries) = std::fs::read_dir(&home) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with(".claude-") {
                let path = entry.path();
                if path.join("sessions").is_dir() {
                    dirs.push(path);
                }
            }
        }
    }
    dirs
}

/// Reveal the agent's working directory in the system file manager.
pub fn reveal(cwd: &str) {
    if cwd.is_empty() || cwd == "?" {
        return;
    }
    let _ = open::that(cwd);
}

/// Bring the window hosting an agent to the front.
///
/// A CLI agent owns no window — it runs inside a terminal emulator — so the pid
/// itself is never something AppKit can activate. Walk up the process tree to
/// the nearest ancestor that is a regular application and activate that.
/// Returns false when no ancestor qualifies, which lets the caller fall back.
pub fn focus(pid: u32) -> bool {
    ancestry(pid).into_iter().any(activate)
}

/// `pid` first, then each parent up to (but not including) the init process.
/// Bounded so a cycle in a stale process table cannot spin forever.
fn ancestry(pid: u32) -> Vec<u32> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let parents: HashMap<u32, u32> = sys
        .processes()
        .iter()
        .filter_map(|(pid, proc_)| Some((pid.as_u32(), proc_.parent()?.as_u32())))
        .collect();

    let mut chain = vec![pid];
    let mut seen: HashSet<u32> = HashSet::from([pid]);
    let mut current = pid;
    while chain.len() < 32 {
        let Some(&parent) = parents.get(&current) else {
            break;
        };
        if parent <= 1 || !seen.insert(parent) {
            break;
        }
        chain.push(parent);
        current = parent;
    }
    chain
}

/// True when `pid` is a regular (Dock-visible) app that AppKit brought forward.
/// Same lookup as `activate`, but reports the policy instead of activating.
#[cfg(all(test, target_os = "macos"))]
pub(crate) fn activate_probe(pid: u32) -> String {
    use objc2::runtime::AnyObject;
    use objc2::*;
    unsafe {
        let app: *mut AnyObject = msg_send![
            class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid as i32
        ];
        if app.is_null() {
            return "no NSRunningApplication".into();
        }
        let policy: i64 = msg_send![app, activationPolicy];
        let name: *mut AnyObject = msg_send![app, localizedName];
        let utf8: *const i8 = if name.is_null() { std::ptr::null() } else { msg_send![name, UTF8String] };
        let label = if utf8.is_null() { "?".to_string() } else { std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned() };
        format!("{label} policy={policy}")
    }
}

#[cfg(target_os = "macos")]
fn activate(pid: u32) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::*;

    // NSApplicationActivationPolicyRegular / NSApplicationActivateAllWindows.
    const POLICY_REGULAR: i64 = 0;
    const ACTIVATE_ALL_WINDOWS: u64 = 1;

    unsafe {
        let app: *mut AnyObject = msg_send![
            class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid as i32
        ];
        if app.is_null() {
            return false;
        }
        // Agents also have daemon/accessory ancestors; activating those would
        // steal focus without showing anything.
        let policy: i64 = msg_send![app, activationPolicy];
        if policy != POLICY_REGULAR {
            return false;
        }
        msg_send![app, activateWithOptions: ACTIVATE_ALL_WINDOWS]
    }
}

#[cfg(not(target_os = "macos"))]
fn activate(_pid: u32) -> bool {
    false
}

/// How long the island should wait between scans.
pub fn poll_interval() -> Duration {
    Duration::from_secs(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, ppid: u32, name: &str, argv: &[&str]) -> ProcInfo {
        ProcInfo {
            pid,
            ppid,
            cpu_pct: 0.0,
            name: name.into(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
            cwd: None,
            exe: None,
        }
    }

    fn proc_exe(pid: u32, ppid: u32, name: &str, argv: &[&str], exe: &str) -> ProcInfo {
        let mut p = proc(pid, ppid, name, argv);
        p.exe = Some(PathBuf::from(exe));
        p
    }

    #[test]
    fn token_matches_basename_and_rejects_prefix() {
        assert!(token_has_binary("/usr/local/bin/claude", "claude"));
        assert!(token_has_binary("claude", "claude"));
        assert!(!token_has_binary("/usr/local/bin/claude-launch", "claude"));
    }

    #[cfg(not(windows))]
    #[test]
    fn token_matches_exe_suffix_and_autoupdater() {
        assert!(token_has_binary(
            "/usr/local/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
            "claude",
        ));
        assert!(token_has_binary(
            "/Users/a/.local/share/claude/versions/2.1.121",
            "claude",
        ));
        assert!(!token_has_binary(
            "/Users/a/.local/share/claude/foo",
            "claude"
        ));
        assert!(!token_has_binary("/some/versions/2.1.121", "claude"));
    }

    #[test]
    fn classify_interactive_clis() {
        assert_eq!(
            classify_process("claude", &["/usr/local/bin/claude".into()], None),
            Some(AgentKind::Claude)
        );
        assert_eq!(
            classify_process("grok", &["grok".into()], None),
            Some(AgentKind::Grok)
        );
        assert_eq!(
            classify_process(
                "codex",
                &["/opt/codex/versions/0.42.0".into(), "--foo".into()],
                None
            ),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            classify_process(
                "cursor-agent",
                &[
                    "/bin/cursor-agent".into(),
                    "--worker-dir".into(),
                    "/tmp/app".into()
                ],
                None,
            ),
            Some(AgentKind::Cursor)
        );
    }

    #[test]
    fn classify_skips_desktop_helpers_and_mcp() {
        assert_eq!(
            classify_process(
                "Grok Bot",
                &["/Applications/Grok Bot.app/Contents/MacOS/Grok Bot".into()],
                None,
            ),
            None
        );
        assert_eq!(
            classify_process(
                "CursorUIViewService",
                &["/System/Library/CursorUIViewService".into()],
                None,
            ),
            None
        );
        assert_eq!(
            classify_process("codex", &["codex".into(), "mcp-server".into()], None),
            None
        );
        assert_eq!(
            classify_process(
                "Cursor",
                &["/Applications/Cursor.app/Contents/MacOS/Cursor".into()],
                None
            ),
            None
        );
    }

    #[test]
    fn classify_grok_helper_agent_binary() {
        assert_eq!(
            classify_process("agent", &["/Users/me/.grok/bin/agent".into()], None),
            Some(AgentKind::Grok)
        );
        assert_eq!(
            classify_process("agent", &["/usr/bin/agent".into()], None),
            None
        );
        // Live Grok TUI: argv0 rewritten to `agent`, path only on exe.
        assert_eq!(
            classify_process(
                "agent",
                &["agent".into()],
                Some(Path::new("/Users/me/.grok/bin/agent")),
            ),
            Some(AgentKind::Grok)
        );
        assert_eq!(
            classify_process(
                "grok-1.0.5-macos-aarch64",
                &["agent".into()],
                Some(Path::new(
                    "/Users/me/.grok/downloads/grok-1.0.5-macos-aarch64"
                )),
            ),
            Some(AgentKind::Grok)
        );
        assert_eq!(classify_process("agent", &["agent".into()], None), None);
    }

    #[test]
    fn assemble_detects_bare_agent_via_exe_and_session_file() {
        let mut procs = HashMap::new();
        procs.insert(
            99,
            proc_exe(99, 1, "agent", &["agent"], "/Users/me/.grok/bin/agent"),
        );
        let sessions = assemble_inner(&procs, &HashMap::new(), &HashMap::new());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].kind, AgentKind::Grok);
        assert_eq!(sessions[0].pid, 99);

        let mut procs = HashMap::new();
        procs.insert(77, proc(77, 1, "agent", &["agent"]));
        let mut grok_meta = HashMap::new();
        grok_meta.insert(
            77,
            GrokActive {
                session_id: "abc".into(),
                pid: 77,
                cwd: "/tmp/proj".into(),
            },
        );
        let sessions = assemble_inner(&procs, &grok_meta, &HashMap::new());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].kind, AgentKind::Grok);
        assert_eq!(sessions[0].pid, 77);
        assert_eq!(sessions[0].session_id.as_deref(), Some("abc"));
        assert_eq!(sessions[0].cwd, "/tmp/proj");
    }

    #[test]
    fn wrapper_ancestor_is_dropped_for_same_kind() {
        let mut procs = HashMap::new();
        procs.insert(10, proc(10, 1, "node", &["node", "/opt/claude/claude"]));
        procs.insert(
            11,
            proc(11, 10, "claude", &["/opt/claude/versions/2.1.121"]),
        );
        procs.insert(12, proc(12, 1, "zsh", &["zsh"]));

        // Directly test retain logic via assemble without file enrichment.
        let children_ok = {
            let kinds: HashMap<u32, AgentKind> = procs
                .values()
                .filter_map(|p| classify(p).map(|k| (p.pid, k)))
                .collect();
            let mut candidates: Vec<u32> = kinds.keys().copied().collect();
            let all_candidates = candidates.clone();
            candidates.retain(|&pid| {
                let kind = kinds.get(&pid).unwrap();
                !all_candidates.iter().any(|&other| {
                    other != pid
                        && kinds.get(&other) == Some(kind)
                        && is_descendant_of(other, pid, &procs)
                })
            });
            candidates.sort_unstable();
            candidates
        };
        assert_eq!(children_ok, vec![11]);
    }

    #[test]
    fn descendant_walk_and_self_are_safe() {
        let mut procs = HashMap::new();
        procs.insert(10, proc(10, 1, "a", &["a"]));
        procs.insert(20, proc(20, 10, "b", &["b"]));
        procs.insert(30, proc(30, 20, "c", &["c"]));
        assert!(is_descendant_of(30, 10, &procs));
        assert!(!is_descendant_of(20, 30, &procs));
        assert!(!is_descendant_of(10, 10, &procs));
        assert!(!is_descendant_of(0, 10, &procs));
    }

    #[test]
    fn project_name_uses_last_segment() {
        assert_eq!(
            project_name("/Users/jonasvogel/openNook-gpui"),
            "openNook-gpui"
        );
        assert_eq!(project_name("?"), "?");
        assert_eq!(project_name(""), "?");
    }

    #[test]
    fn cursor_worker_dir_parses_flag() {
        let argv = vec![
            "cursor-agent".into(),
            "--worker-dir".into(),
            "/Users/jonasvogel/social".into(),
        ];
        assert_eq!(
            cursor_worker_dir(&argv).as_deref(),
            Some("/Users/jonasvogel/social")
        );
        let argv = vec!["cursor-agent".into(), "--worker-dir=/tmp/app".into()];
        assert_eq!(cursor_worker_dir(&argv).as_deref(), Some("/tmp/app"));
    }

    #[test]
    fn snapshot_does_not_panic() {
        let sessions = snapshot();
        for session in &sessions {
            assert!(session.pid > 1);
            assert!(!session.project.is_empty());
            assert!(!session.cwd.is_empty());
            assert!(!session.title().is_empty());
        }
    }

    fn pid_is_alive(pid: u32) -> bool {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Live coding agents listed in Grok/Claude session files must show up in
    /// `snapshot()`. This is the user-facing miss: a running Grok TUI whose
    /// argv is just `agent` (no `/.grok/` path) is invisible today.
    #[test]
    fn snapshot_detects_live_session_file_agents() {
        let sessions = snapshot();
        eprintln!("snapshot={sessions:?}");

        let mut expected = 0usize;
        for (pid, row) in grok_sessions() {
            if !pid_is_alive(pid) {
                continue;
            }
            expected += 1;
            assert!(
                sessions
                    .iter()
                    .any(|s| s.pid == pid && s.kind == AgentKind::Grok),
                "live Grok pid {pid} cwd={} not in snapshot={sessions:?}",
                row.cwd
            );
        }
        for (pid, row) in claude_sessions() {
            if !pid_is_alive(pid) {
                continue;
            }
            expected += 1;
            assert!(
                sessions
                    .iter()
                    .any(|s| s.pid == pid && s.kind == AgentKind::Claude),
                "live Claude pid {pid} cwd={:?} not in snapshot={sessions:?}",
                row.cwd
            );
        }
        eprintln!("live session-file agents checked: {expected}");
    }

    #[test]
    fn percent_encode_matches_grok_session_folders() {
        assert_eq!(
            percent_encode_path("/Users/jonasvogel/openNook-gpui"),
            "%2FUsers%2Fjonasvogel%2FopenNook-gpui"
        );
    }

    #[test]
    fn grok_summary_prefers_generated_title() {
        let row: GrokSummary = serde_json::from_str(
            r#"{
                "generated_title": "GPUI circular Dot Matrix agent indicator",
                "session_summary": "fallback",
                "current_model_id": "grok-4.6"
            }"#,
        )
        .unwrap();
        let title = nonempty(row.generated_title.as_deref())
            .or_else(|| nonempty(row.session_summary.as_deref()));
        assert_eq!(
            title.as_deref(),
            Some("GPUI circular Dot Matrix agent indicator")
        );
        assert_eq!(
            nonempty(row.current_model_id.as_deref()).as_deref(),
            Some("grok-4.6")
        );
    }

    #[test]
    fn claude_sidecar_reads_name() {
        let row: ClaudeSidecar = serde_json::from_str(
            r#"{"pid":1,"sessionId":"abc","cwd":"/tmp","name":"opennook-gpui-83","status":"idle"}"#,
        )
        .unwrap();
        assert_eq!(row.name.as_deref(), Some("opennook-gpui-83"));
        assert_eq!(row.session_id.as_deref(), Some("abc"));
    }

    #[test]
    fn title_falls_back_to_project() {
        let named = AgentSession {
            kind: AgentKind::Grok,
            pid: 1,
            project: "openNook-gpui".into(),
            cwd: "/tmp".into(),
            status: AgentStatus::Working,
            session_id: None,
            name: Some("GPUI circular Dot Matrix agent indicator".into()),
            model: Some("grok-4.6".into()),
        };
        assert_eq!(named.title(), "GPUI circular Dot Matrix agent indicator");
        let unnamed = AgentSession {
            name: None,
            model: None,
            ..named.clone()
        };
        assert_eq!(unnamed.title(), "openNook-gpui");
        let blank = AgentSession {
            name: Some("  ".into()),
            model: None,
            ..named
        };
        assert_eq!(blank.title(), "openNook-gpui");
    }
}

#[cfg(test)]
mod focus_probe {
    #[test]
    fn probe() {
        for s in super::snapshot() {
            let chain = super::ancestry(s.pid);
            println!("agent {} pid={} chain={:?}", s.kind.label(), s.pid, chain);
            for p in chain {
                println!("   pid {p} -> activatable={}", super::activate_probe(p));
            }
        }
    }
}
