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
#[cfg(target_os = "macos")]
use sysinfo::Pid;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// CPU % above which a descendant process counts as doing real work. The
/// agent process's own CPU is never a work signal — a TUI burns CPU
/// repainting spinners and streaming text whether or not work is happening
/// (same rationale as abtop). Idle MCP-server polling in the descendant tree
/// still ticks past 1%, so the bar sits well above that noise.
const CPU_ACTIVE: f32 = 5.0;

/// Consecutive polls a CPU reading must persist before the shown status
/// flips. Smooths one-poll spikes/dips so the list doesn't reorder itself
/// every scan. Only applies to CPU-derived status; sidecar status is exact.
const DEBOUNCE_POLLS: u8 = 2;

/// Slack when comparing a sidecar's `startedAt` against the process start
/// time. A session starts moments after its process; a reused pid starts
/// long after the dead session it would otherwise impersonate.
const SIDECAR_START_SLACK_SECS: u64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentKind {
    Claude,
    Codex,
    OpenCode,
    Fx,
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
            AgentKind::Fx => "fx",
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
            AgentKind::Fx => &["fx"],
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
    /// Process start, seconds since the epoch (0 when unknown).
    start_time: u64,
}

/// Live coding-agent sessions on this machine. Cheap enough for a 2s poll.
pub fn snapshot() -> Vec<AgentSession> {
    let procs = scan_processes();
    let sessions = assemble(&procs);
    if !sessions.is_empty() {
        LAST_AGENT_SEEN.store(unix_secs(), std::sync::atomic::Ordering::Relaxed);
    }
    sessions
}

/// Unix time of the last scan that found a live session; drives the adaptive
/// poll cadence below.
static LAST_AGENT_SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn process_system() -> &'static Mutex<System> {
    static SYS: OnceLock<Mutex<System>> = OnceLock::new();
    SYS.get_or_init(|| Mutex::new(System::new()))
}

fn scan_processes() -> HashMap<u32, ProcInfo> {
    #[cfg(target_os = "macos")]
    {
        scan_processes_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        scan_processes_all()
    }
}

fn proc_from_sysinfo(pid_u32: u32, proc_: &sysinfo::Process) -> Option<ProcInfo> {
    let name = proc_.name().to_string_lossy().into_owned();
    let argv: Vec<String> = if proc_.cmd().is_empty() {
        if name.is_empty() {
            return None;
        }
        vec![name.clone()]
    } else {
        proc_
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    };
    Some(ProcInfo {
        pid: pid_u32,
        ppid: proc_.parent().map(|p| p.as_u32()).unwrap_or(0),
        cpu_pct: proc_.cpu_usage(),
        name,
        argv,
        cwd: proc_.cwd().map(PathBuf::from),
        exe: proc_.exe().map(PathBuf::from),
        start_time: proc_.start_time(),
    })
}

/// Full-table refresh. Used off macOS, where the process list is small and
/// there is no `proc_pidpath` two-stage split.
fn scan_processes_all() -> HashMap<u32, ProcInfo> {
    let mut sys = process_system().lock().unwrap_or_else(|e| e.into_inner());
    // argv (KERN_PROCARGS2 sysctl) and the exe path are fixed at exec time, so
    // OnlyIfNotSet fetches them once per new pid instead of re-reading them
    // for every process on the system each scan — that re-read was one of the
    // two biggest slices of openNook's idle CPU. cwd genuinely changes
    // (sessions cd around) and stays Always.
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cpu()
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_cwd(UpdateKind::Always)
            .with_exe(UpdateKind::OnlyIfNotSet),
    );

    let mut map = HashMap::new();
    for (pid, proc_) in sys.processes() {
        if let Some(info) = proc_from_sysinfo(pid.as_u32(), proc_) {
            map.insert(info.pid, info);
        }
    }
    map
}

/// macOS two-stage scan: list every pid with a cheap `proc_pidpath` (exe
/// OnlyIfNotSet), then fetch argv / cwd / cpu only for agent candidates and
/// their descendants. Avoids KERN_PROCARGS2 on hundreds of unrelated pids.
#[cfg(target_os = "macos")]
fn scan_processes_macos() -> HashMap<u32, ProcInfo> {
    let mut sys = process_system().lock().unwrap_or_else(|e| e.into_inner());
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_exe(UpdateKind::OnlyIfNotSet),
    );

    let sidecar: HashSet<u32> = grok_sessions()
        .keys()
        .chain(claude_sessions().keys())
        .copied()
        .collect();

    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut seeds = Vec::new();
    for (pid, proc_) in sys.processes() {
        let pid_u32 = pid.as_u32();
        let ppid = proc_.parent().map(|p| p.as_u32()).unwrap_or(0);
        children.entry(ppid).or_default().push(pid_u32);
        let name = proc_.name().to_string_lossy();
        let exe = proc_
            .exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if is_scan_seed(&exe, &name) || sidecar.contains(&pid_u32) {
            seeds.push(pid_u32);
        }
    }

    let mut detail = HashSet::new();
    let mut stack = seeds;
    while let Some(pid) = stack.pop() {
        if detail.insert(pid) {
            if let Some(kids) = children.get(&pid) {
                stack.extend(kids.iter().copied());
            }
        }
    }
    if detail.is_empty() {
        return HashMap::new();
    }

    let pids: Vec<Pid> = detail.iter().copied().map(Pid::from_u32).collect();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cpu()
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_cwd(UpdateKind::Always)
            .with_exe(UpdateKind::OnlyIfNotSet),
    );

    let mut map = HashMap::new();
    for pid in detail {
        let Some(proc_) = sys.process(Pid::from_u32(pid)) else {
            continue;
        };
        if let Some(info) = proc_from_sysinfo(pid, proc_) {
            map.insert(info.pid, info);
        }
    }
    map
}

/// Stage-1 filter: exe path or `comm` looks like an agent or a wrapper that
/// may hide the real binary in argv (`node …/claude`).
fn is_scan_seed(path: &str, comm: &str) -> bool {
    if grok_install_path(path) {
        return true;
    }
    if is_wrapper_comm(comm) || is_wrapper_comm(path.rsplit('/').next().unwrap_or(path)) {
        return true;
    }
    [
        AgentKind::Cursor,
        AgentKind::OpenCode,
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Fx,
        AgentKind::Grok,
        AgentKind::Aider,
        AgentKind::Gemini,
    ]
    .into_iter()
    .any(|kind| {
        kind.binaries()
            .iter()
            .any(|bin| token_has_binary(path, bin) || token_has_binary(comm, bin))
    })
}

fn is_wrapper_comm(name: &str) -> bool {
    const WRAPPERS: &[&str] = &["node", "bun", "deno", "ruby", "perl", "npx", "python", "python3"];
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let base = base.strip_suffix(".exe").unwrap_or(base);
    let lower = base.to_ascii_lowercase();
    WRAPPERS.iter().any(|w| lower == *w) || lower.starts_with("python")
}

fn assemble(procs: &HashMap<u32, ProcInfo>) -> Vec<AgentSession> {
    static DEBOUNCE: OnceLock<Mutex<HashMap<u32, CpuDebounce>>> = OnceLock::new();
    let mut debounce = DEBOUNCE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    assemble_inner(procs, &grok_sessions(), &claude_sessions(), &mut debounce)
}

#[derive(Default)]
struct CpuDebounce {
    shown: bool,
    contrary: u8,
}

/// Debounce a raw CPU busy/idle reading: the shown status only flips after
/// `DEBOUNCE_POLLS` consecutive polls disagree with it.
fn debounced_cpu_busy(state: &mut HashMap<u32, CpuDebounce>, pid: u32, raw: bool) -> bool {
    let entry = state.entry(pid).or_default();
    if raw == entry.shown {
        entry.contrary = 0;
    } else {
        entry.contrary += 1;
        if entry.contrary >= DEBOUNCE_POLLS {
            entry.shown = raw;
            entry.contrary = 0;
        }
    }
    entry.shown
}

/// Reject sidecar rows written by a dead session whose pid was reused: the
/// claiming process must predate (within slack) the session's `startedAt`.
fn sidecar_matches_proc(started_at_ms: Option<u64>, proc: &ProcInfo) -> bool {
    match started_at_ms {
        Some(ms) if proc.start_time > 0 => proc.start_time <= ms / 1000 + SIDECAR_START_SLACK_SECS,
        _ => true,
    }
}

fn assemble_inner(
    procs: &HashMap<u32, ProcInfo>,
    grok_meta: &HashMap<u32, GrokActive>,
    claude_meta: &HashMap<u32, ClaudeSidecar>,
    debounce: &mut HashMap<u32, CpuDebounce>,
) -> Vec<AgentSession> {
    let claude_meta: HashMap<u32, ClaudeSidecar> = claude_meta
        .iter()
        .filter(|(pid, row)| {
            procs
                .get(pid)
                .is_none_or(|p| sidecar_matches_proc(row.started_at, p))
        })
        .map(|(pid, row)| (*pid, row.clone()))
        .collect();
    let claude_meta = &claude_meta;

    let children = children_map(procs);
    let self_pid = std::process::id();

    let mut candidates: Vec<u32> = procs
        .values()
        .filter(|p| !is_descendant_of(p.pid, self_pid, procs))
        .filter(|p| kind_of(p, grok_meta, claude_meta).is_some())
        .map(|p| p.pid)
        .collect();

    // Collapse same-kind candidates that sit in one process chain: an agent
    // spawns short-lived helper children from its own binary (Claude does),
    // and wrappers (`node .../claude`) sit above the real process. A pid
    // backed by a session file is the real session and wins over related
    // pids without one; between two file-less pids the descendant wins
    // (real binary under a wrapper). Two file-backed pids are two sessions.
    let kinds: HashMap<u32, AgentKind> = candidates
        .iter()
        .filter_map(|&pid| {
            procs
                .get(&pid)
                .and_then(|p| kind_of(p, grok_meta, claude_meta))
                .map(|k| (pid, k))
        })
        .collect();
    let has_session_file = |pid: u32, kind: AgentKind| match kind {
        AgentKind::Claude => claude_meta.contains_key(&pid),
        AgentKind::Grok => grok_meta.contains_key(&pid),
        _ => false,
    };
    let all_candidates = candidates.clone();
    candidates.retain(|&pid| {
        let Some(&kind) = kinds.get(&pid) else {
            return false;
        };
        let file_backed = has_session_file(pid, kind);
        !all_candidates.iter().any(|&other| {
            if other == pid || kinds.get(&other) != Some(&kind) {
                return false;
            }
            match (file_backed, has_session_file(other, kind)) {
                // A helper/wrapper anywhere in the real session's chain loses.
                (false, true) => {
                    is_descendant_of(other, pid, procs) || is_descendant_of(pid, other, procs)
                }
                // The real session never loses to a file-less relative.
                (true, false) => false,
                // Two real sessions (e.g. claude launched inside claude).
                (true, true) => false,
                // Wrapper chain: keep the deepest process.
                (false, false) => is_descendant_of(other, pid, procs),
            }
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

        // The sidecar status is authoritative when present: an idle Claude
        // stays Waiting even while an MCP-server child or a TUI repaint burns
        // CPU. Only sessions without one fall back to the CPU heuristic.
        let busy = match file_busy {
            Some(busy) => busy,
            None => {
                // Descendants only: a running tool (shell, build, script)
                // shows up as child CPU; the agent's own CPU is TUI noise.
                let raw = has_active_descendant(pid, &children, procs, CPU_ACTIVE);
                debounced_cpu_busy(debounce, pid, raw)
            }
        };
        let status = if busy {
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

    debounce.retain(|pid, _| procs.contains_key(pid));

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

    [
        AgentKind::Cursor,
        AgentKind::OpenCode,
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Fx,
        AgentKind::Grok,
        AgentKind::Aider,
        AgentKind::Gemini,
    ]
    .into_iter()
    .find(|kind| {
        kind.binaries().iter().any(|bin| {
            argv_has_binary(argv, bin)
                || token_has_binary(name, bin)
                || exe.is_some_and(|p| token_has_binary(&p.to_string_lossy(), bin))
        })
    })
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

#[derive(Clone, Deserialize)]
struct ClaudeSidecar {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(rename = "sessionId", default)]
    session_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    name: Option<String>,
    /// Session start, ms since the epoch; used to reject reused pids.
    #[serde(rename = "startedAt", default)]
    started_at: Option<u64>,
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
/// itself is never something AppKit can activate. Prefer the macOS responsible
/// process (the terminal .app), then walk parents. `login` is root, so sysinfo
/// often omits its parent; `ps` still sees Ghostty/Terminal above it.
pub fn focus(pid: u32) -> bool {
    host_pids(pid).into_iter().any(activate)
}

fn host_pids(pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    if let Some(resp) = responsible_pid(pid) {
        out.push(resp);
    }
    for p in ancestry(pid) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// `pid` first, then each parent up to (but not including) the init process.
/// Bounded so a cycle in a stale process table cannot spin forever.
fn ancestry(pid: u32) -> Vec<u32> {
    walk_ancestry(pid, &process_parents())
}

fn walk_ancestry(pid: u32, parents: &HashMap<u32, u32>) -> Vec<u32> {
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

fn process_parents() -> HashMap<u32, u32> {
    #[cfg(unix)]
    {
        let ps = ps_parents();
        if !ps.is_empty() {
            return ps;
        }
    }
    sysinfo_parents()
}

fn sysinfo_parents() -> HashMap<u32, u32> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes()
        .iter()
        .filter_map(|(pid, proc_)| Some((pid.as_u32(), proc_.parent()?.as_u32())))
        .collect()
}

#[cfg(unix)]
fn ps_parents() -> HashMap<u32, u32> {
    let Ok(out) = std::process::Command::new("/bin/ps")
        .args(["-ax", "-o", "pid=,ppid="])
        .output()
    else {
        return HashMap::new();
    };
    let mut parents = HashMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut cols = line.split_whitespace();
        let Some(pid) = cols.next().and_then(|s| s.parse().ok()) else {
            continue;
        };
        let Some(ppid) = cols.next().and_then(|s| s.parse().ok()) else {
            continue;
        };
        parents.insert(pid, ppid);
    }
    parents
}

/// App that owns this CLI pid on macOS (`responsibility_get_pid_responsible_for_pid`).
/// Ghostty/Terminal/iTerm show up here even when `login` (root) hides them from
/// a parent walk that can't see privileged processes.
fn responsible_pid(pid: u32) -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        let r = unsafe { responsibility_get_pid_responsible_for_pid(pid as i32) };
        if r > 1 {
            return Some(r as u32);
        }
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn responsibility_get_pid_responsible_for_pid(pid: i32) -> i32;
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
        let utf8: *const i8 = if name.is_null() {
            std::ptr::null()
        } else {
            msg_send![name, UTF8String]
        };
        let label = if utf8.is_null() {
            "?".to_string()
        } else {
            std::ffi::CStr::from_ptr(utf8)
                .to_string_lossy()
                .into_owned()
        };
        format!("{label} policy={policy}")
    }
}

#[cfg(target_os = "macos")]
fn activate(pid: u32) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::*;

    // NSApplicationActivationPolicyRegular.
    const POLICY_REGULAR: i64 = 0;
    const ACTIVATE_ALL_WINDOWS: u64 = 1;
    // AllWindows | IgnoringOtherApps. The island is an accessory
    // (LSUIElement); without IgnoringOtherApps, AppKit reports success and
    // leaves the host app in the background.
    const ACTIVATE_OPTIONS: u64 = ACTIVATE_ALL_WINDOWS | 2;

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
        // macOS 14+: activate from this click's app instead of the deprecated
        // `activateWithOptions:` steal. Falls back if the selector is missing
        // or AppKit refuses the hand-off.
        let current: *mut AnyObject = msg_send![class!(NSRunningApplication), currentApplication];
        let can_from: bool = msg_send![
            app,
            respondsToSelector: sel!(activateFromApplication:options:)
        ];
        if can_from && !current.is_null() {
            let ok: bool = msg_send![
                app,
                activateFromApplication: current,
                options: ACTIVATE_ALL_WINDOWS
            ];
            if ok {
                return true;
            }
        }
        msg_send![app, activateWithOptions: ACTIVATE_OPTIONS]
    }
}

#[cfg(not(target_os = "macos"))]
fn activate(_pid: u32) -> bool {
    false
}

/// How long the island should wait between scans: quick while sessions are
/// live (status changes matter), relaxed once none have been seen for a
/// while — a newly started agent then appears within one slow tick, which is
/// fine for a status glance and keeps the recurring process-table walk off
/// the battery.
pub fn poll_interval() -> Duration {
    let last = LAST_AGENT_SEEN.load(std::sync::atomic::Ordering::Relaxed);
    if unix_secs().saturating_sub(last) < 30 {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(6)
    }
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
            start_time: 0,
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
            classify_process("fx", &["/Users/me/.local/bin/fx".into()], None),
            Some(AgentKind::Fx)
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
    fn scan_seed_matches_agent_paths_and_wrappers() {
        assert!(is_scan_seed("/usr/local/bin/claude", "claude"));
        assert!(is_scan_seed(
            "/Users/a/.local/share/claude/versions/2.1.121",
            "claude"
        ));
        assert!(is_scan_seed("/bin/cursor-agent", "cursor-agent"));
        assert!(is_scan_seed("/opt/homebrew/bin/node", "node"));
        assert!(is_scan_seed("/usr/bin/python3.12", "Python"));
        assert!(is_scan_seed("/Users/a/.grok/bin/agent", "agent"));
        assert!(!is_scan_seed("/usr/bin/grep", "grep"));
        assert!(!is_scan_seed("/Applications/Safari.app/Contents/MacOS/Safari", "Safari"));
        assert!(!is_scan_seed("/usr/sbin/syslogd", "syslogd"));
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
        let sessions = assemble_inner(
            &procs,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );
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
        let sessions = assemble_inner(&procs, &grok_meta, &HashMap::new(), &mut HashMap::new());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].kind, AgentKind::Grok);
        assert_eq!(sessions[0].pid, 77);
        assert_eq!(sessions[0].session_id.as_deref(), Some("abc"));
        assert_eq!(sessions[0].cwd, "/tmp/proj");
    }

    fn sidecar(status: Option<&str>, started_at: Option<u64>) -> ClaudeSidecar {
        ClaudeSidecar {
            cwd: Some("/tmp/proj".into()),
            session_id: Some("abc".into()),
            status: status.map(str::to_string),
            name: None,
            started_at,
        }
    }

    #[test]
    fn sidecar_idle_beats_cpu() {
        let mut procs = HashMap::new();
        let mut claude = proc(50, 1, "claude", &["/usr/local/bin/claude"]);
        claude.cpu_pct = 80.0;
        procs.insert(50, claude);
        let mut meta = HashMap::new();
        meta.insert(50, sidecar(Some("idle"), None));

        let sessions = assemble_inner(&procs, &HashMap::new(), &meta, &mut HashMap::new());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, AgentStatus::Waiting);

        meta.insert(50, sidecar(Some("busy"), None));
        let sessions = assemble_inner(&procs, &HashMap::new(), &meta, &mut HashMap::new());
        assert_eq!(sessions[0].status, AgentStatus::Working);
    }

    #[test]
    fn cpu_status_needs_consecutive_polls() {
        let mut procs = HashMap::new();
        procs.insert(60, proc(60, 1, "codex", &["/usr/local/bin/codex"]));
        let mut tool = proc(61, 60, "cargo", &["cargo", "build"]);
        tool.cpu_pct = 50.0;
        procs.insert(61, tool);
        let mut debounce = HashMap::new();

        // One busy poll is a spike, two flip the status.
        let s = assemble_inner(&procs, &HashMap::new(), &HashMap::new(), &mut debounce);
        assert_eq!(s[0].status, AgentStatus::Waiting);
        let s = assemble_inner(&procs, &HashMap::new(), &HashMap::new(), &mut debounce);
        assert_eq!(s[0].status, AgentStatus::Working);

        // Same on the way down: one quiet poll doesn't drop it back.
        procs.get_mut(&61).unwrap().cpu_pct = 0.0;
        let s = assemble_inner(&procs, &HashMap::new(), &HashMap::new(), &mut debounce);
        assert_eq!(s[0].status, AgentStatus::Working);
        let s = assemble_inner(&procs, &HashMap::new(), &HashMap::new(), &mut debounce);
        assert_eq!(s[0].status, AgentStatus::Waiting);
    }

    #[test]
    fn own_process_cpu_is_not_work() {
        // A TUI repainting or streaming text burns CPU on the agent process
        // itself; only descendant CPU counts as a running tool.
        let mut procs = HashMap::new();
        let mut codex = proc(60, 1, "codex", &["/usr/local/bin/codex"]);
        codex.cpu_pct = 80.0;
        procs.insert(60, codex);
        let mut debounce = HashMap::new();

        for _ in 0..3 {
            let s = assemble_inner(&procs, &HashMap::new(), &HashMap::new(), &mut debounce);
            assert_eq!(s[0].status, AgentStatus::Waiting);
        }
    }

    #[test]
    fn stale_sidecar_for_reused_pid_is_ignored() {
        let mut procs = HashMap::new();
        // Unrelated process that started long after the dead session.
        let mut python = proc(70, 1, "python3", &["python3", "serve.py"]);
        python.start_time = 2_000_000_000;
        procs.insert(70, python);
        let mut meta = HashMap::new();
        meta.insert(70, sidecar(Some("busy"), Some(1_000_000_000_000)));

        let sessions = assemble_inner(&procs, &HashMap::new(), &meta, &mut HashMap::new());
        assert!(sessions.is_empty(), "reused pid must not surface as Claude");

        // A process that predates the session start is accepted.
        procs.get_mut(&70).unwrap().start_time = 999_999_998;
        let sessions = assemble_inner(&procs, &HashMap::new(), &meta, &mut HashMap::new());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].kind, AgentKind::Claude);
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

        let sessions = assemble_inner(&procs, &HashMap::new(), &HashMap::new(), &mut HashMap::new());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].pid, 11);
    }

    #[test]
    fn session_beats_its_own_spawned_helper() {
        // Claude spawns short-lived children from its own binary; the
        // sidecar-backed parent is the session, not the helper.
        let mut procs = HashMap::new();
        procs.insert(100, proc(100, 1, "claude", &["claude"]));
        procs.insert(
            101,
            proc_exe(
                101,
                100,
                "claude",
                &["claude"],
                "/Users/me/.local/share/claude/versions/2.1.239",
            ),
        );
        let mut meta = HashMap::new();
        meta.insert(100, sidecar(Some("idle"), None));

        let sessions = assemble_inner(&procs, &HashMap::new(), &meta, &mut HashMap::new());
        assert_eq!(sessions.len(), 1, "helper must not surface: {sessions:?}");
        assert_eq!(sessions[0].pid, 100);

        // Two sidecar-backed sessions in one chain are both real
        // (claude launched from inside claude's shell).
        meta.insert(101, sidecar(Some("busy"), None));
        let mut sessions = assemble_inner(&procs, &HashMap::new(), &meta, &mut HashMap::new());
        sessions.sort_by_key(|s| s.pid);
        assert_eq!(sessions.len(), 2);
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
    fn ancestry_starts_with_self() {
        let pid = std::process::id();
        let chain = ancestry(pid);
        assert_eq!(chain.first().copied(), Some(pid));
        assert!(!chain.is_empty() && chain.len() <= 32);
        let mut seen = HashSet::new();
        assert!(chain.iter().all(|p| seen.insert(*p)));
    }

    #[test]
    fn walk_crosses_root_login_to_terminal() {
        // Real tree: claude → zsh → login (root) → Ghostty → launchd.
        // sysinfo used to stop at login because it couldn't read the root ppid.
        let mut parents = HashMap::new();
        parents.insert(98546, 98344);
        parents.insert(98344, 98343);
        parents.insert(98343, 81976);
        parents.insert(81976, 1);
        assert_eq!(
            walk_ancestry(98546, &parents),
            vec![98546, 98344, 98343, 81976]
        );
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
        std::process::Command::new("/bin/kill")
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

#[cfg(all(test, target_os = "macos"))]
mod focus_probe {
    #[test]
    fn probe() {
        for s in super::snapshot() {
            let hosts = super::host_pids(s.pid);
            println!(
                "agent {} pid={} hosts={:?} responsible={:?}",
                s.kind.label(),
                s.pid,
                hosts,
                super::responsible_pid(s.pid)
            );
            for p in &hosts {
                println!("   pid {p} -> activatable={}", super::activate_probe(*p));
            }
            assert!(
                hosts.iter().any(|p| {
                    let probe = super::activate_probe(*p);
                    probe.contains("policy=0")
                }),
                "no Dock-visible host for {} pid {} hosts={hosts:?}",
                s.kind.label(),
                s.pid
            );
        }
    }
}
