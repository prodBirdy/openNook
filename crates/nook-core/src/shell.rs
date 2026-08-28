//! One-shot login-shell commands for Termi-Notch (pipe MVP).
//!
//! Reachable only from typing in the island UI. URLs, the CLI, Services, and
//! Alfred must never call this module. Children run in their own process group
//! so cancel = `killpg`. Output is capped and timed out.

use crate::app_data_dir;
use crate::database;
use crate::settings::get_app_settings;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::Notify;

pub const OUTPUT_CAP: usize = 256 * 1024;
const HISTORY_KEY: &str = "shell_history";
const HISTORY_CAP: usize = 50;
const PGID_FILE: &str = "shell.pgid";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSnapshot {
    pub output: String,
    pub done: bool,
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub capped: bool,
}

struct JobInner {
    output: Mutex<String>,
    done: AtomicBool,
    timed_out: AtomicBool,
    capped: AtomicBool,
    exit: Mutex<Option<i32>>,
    notify: Notify,
    pgid: AtomicU32,
    cancel: AtomicBool,
}

#[derive(Clone)]
pub struct JobHandle {
    inner: Arc<JobInner>,
}

impl JobHandle {
    fn new() -> Self {
        Self {
            inner: Arc::new(JobInner {
                output: Mutex::new(String::new()),
                done: AtomicBool::new(false),
                timed_out: AtomicBool::new(false),
                capped: AtomicBool::new(false),
                exit: Mutex::new(None),
                notify: Notify::new(),
                pgid: AtomicU32::new(0),
                cancel: AtomicBool::new(false),
            }),
        }
    }

    pub fn snapshot(&self) -> JobSnapshot {
        JobSnapshot {
            output: self
                .inner
                .output
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
            done: self.inner.done.load(Ordering::SeqCst),
            exit: self.inner.exit.lock().ok().and_then(|g| *g),
            timed_out: self.inner.timed_out.load(Ordering::SeqCst),
            capped: self.inner.capped.load(Ordering::SeqCst),
        }
    }

    pub async fn wait_update(&self) -> JobSnapshot {
        self.inner.notify.notified().await;
        self.snapshot()
    }

    pub fn cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
        let pgid = self.inner.pgid.load(Ordering::SeqCst);
        if pgid != 0 {
            kill_group(pgid as i32);
        }
        self.inner.notify.notify_waiters();
    }
}

/// `$SHELL` (or the settings override), used only by the in-island card.
pub fn resolved_shell(override_path: &str) -> String {
    let trimmed = override_path.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}

pub fn default_timeout_secs() -> u32 {
    30
}

/// Strip CSI / OSC / other C1 escapes. Color mapping is optional; the pipe
/// MVP keeps a monochrome scrollback.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next.is_ascii_alphabetic() || next == '~' {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' {
                            let _ = chars.next_if_eq(&'\\');
                            break;
                        }
                    }
                }
                Some(next) if matches!(*next, '(' | ')' | '#' | '%') => {
                    chars.next();
                    let _ = chars.next();
                }
                Some(_) => {
                    let _ = chars.next();
                }
                None => {}
            }
            continue;
        }
        if ch == '\r' {
            continue;
        }
        out.push(ch);
    }
    out
}

fn pgid_path() -> PathBuf {
    app_data_dir().join(PGID_FILE)
}

fn record_pgid(pgid: u32) {
    let _ = std::fs::write(pgid_path(), pgid.to_string());
}

fn clear_pgid() {
    let _ = std::fs::remove_file(pgid_path());
}

fn kill_group(pgid: i32) {
    if pgid <= 0 {
        return;
    }
    #[cfg(unix)]
    unsafe {
        libc_killpg(pgid, 15);
    }
    #[cfg(not(unix))]
    let _ = pgid;
}

#[cfg(unix)]
unsafe fn libc_killpg(pgid: i32, sig: i32) {
    extern "C" {
        fn killpg(pgrp: i32, sig: i32) -> i32;
    }
    let _ = killpg(pgid, sig);
}

/// Kill a process group left behind by a crash, then forget the pid file.
pub fn reap_orphaned_jobs() {
    let path = pgid_path();
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(pgid) = raw.trim().parse::<i32>() {
            kill_group(pgid);
        }
    }
    let _ = std::fs::remove_file(path);
}

pub fn load_history() -> Vec<String> {
    database::get_setting(HISTORY_KEY)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

pub fn push_history(command: &str) {
    if !get_app_settings().terminal_history {
        return;
    }
    let command = command.trim();
    if command.is_empty() {
        return;
    }
    let mut history = load_history();
    history.retain(|row| row != command);
    history.push(command.to_string());
    if history.len() > HISTORY_CAP {
        let drop = history.len() - HISTORY_CAP;
        history.drain(0..drop);
    }
    if let Ok(json) = serde_json::to_string(&history) {
        let _ = database::set_setting(HISTORY_KEY, &json);
    }
}

/// Spawn `$SHELL -lc <command>` in a new process group. Streaming reader
/// parks on a blocking read; nothing is left running at idle.
pub fn spawn_login_command(shell: &str, command: &str, timeout: Duration) -> JobHandle {
    let handle = JobHandle::new();
    let job = handle.clone();
    let shell = shell.to_string();
    let command = command.to_string();
    thread::Builder::new()
        .name("nook-shell".into())
        .spawn(move || run_job(job, shell, command, timeout))
        .ok();
    handle
}

fn run_job(job: JobHandle, shell: String, command: String, timeout: Duration) {
    let mut cmd = Command::new(&shell);
    cmd.arg("-lc")
        .arg(&command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            append_output(&job, &format!("failed to spawn {shell}: {err}\n"));
            finish(&job, Some(1), false);
            return;
        }
    };
    let pgid = child.id();
    job.inner.pgid.store(pgid, Ordering::SeqCst);
    record_pgid(pgid);

    if let Some(stdout) = child.stdout.take() {
        spawn_reader(job.clone(), stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(job.clone(), stderr);
    }

    let watchdog = job.clone();
    thread::spawn(move || {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if watchdog.inner.done.load(Ordering::SeqCst)
                || watchdog.inner.cancel.load(Ordering::SeqCst)
            {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        if !watchdog.inner.done.load(Ordering::SeqCst) {
            watchdog.inner.timed_out.store(true, Ordering::SeqCst);
            watchdog.cancel();
        }
    });

    let status = child.wait();
    clear_pgid();
    let code = match status {
        Ok(status) => status.code().or_else(|| {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                status.signal()
            }
            #[cfg(not(unix))]
            {
                None
            }
        }),
        Err(_) => Some(1),
    };
    let timed_out = job.inner.timed_out.load(Ordering::SeqCst);
    finish(&job, code, timed_out);
}

fn spawn_reader(job: JobHandle, mut pipe: impl Read + Send + 'static) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    append_output(&job, &chunk);
                    if job.inner.capped.load(Ordering::SeqCst) {
                        job.cancel();
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        job.inner.notify.notify_waiters();
    });
}

fn append_output(job: &JobHandle, chunk: &str) {
    let cleaned = strip_ansi(chunk);
    if let Ok(mut guard) = job.inner.output.lock() {
        if guard.len() >= OUTPUT_CAP {
            job.inner.capped.store(true, Ordering::SeqCst);
            return;
        }
        let room = OUTPUT_CAP.saturating_sub(guard.len());
        if cleaned.len() > room {
            guard.push_str(&cleaned[..room]);
            job.inner.capped.store(true, Ordering::SeqCst);
        } else {
            guard.push_str(&cleaned);
        }
    }
    job.inner.notify.notify_waiters();
}

fn finish(job: &JobHandle, exit: Option<i32>, timed_out: bool) {
    if let Ok(mut guard) = job.inner.exit.lock() {
        *guard = exit;
    }
    if timed_out {
        job.inner.timed_out.store(true, Ordering::SeqCst);
        append_output(job, "\n[timed out]\n");
    }
    job.inner.done.store(true, Ordering::SeqCst);
    job.inner.notify.notify_waiters();
}

/// Used by tests (and the watchdog) so a leftover child cannot hang CI.
pub fn force_kill(handle: &JobHandle) {
    handle.cancel();
}

#[allow(dead_code)]
fn write_all_or_log(mut w: impl Write, bytes: &[u8]) {
    let _ = w.write_all(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_drops_sgr_and_keeps_text() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("a\rb\n"), "ab\n");
        assert_eq!(
            strip_ansi("\u{1b}]0;title\u{7}prompt"),
            "prompt"
        );
    }

    #[test]
    fn resolved_shell_prefers_override() {
        assert_eq!(resolved_shell("/bin/sh"), "/bin/sh");
        assert!(!resolved_shell("").is_empty());
    }

    #[test]
    fn spawn_echo_captures_stdout() {
        let job = spawn_login_command("/bin/sh", "printf 'hello-nook'", Duration::from_secs(5));
        let start = Instant::now();
        let mut snap = job.snapshot();
        while !snap.done && start.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(20));
            snap = job.snapshot();
        }
        force_kill(&job);
        assert!(snap.done, "job should finish");
        assert!(
            snap.output.contains("hello-nook"),
            "output was {:?}",
            snap.output
        );
        assert_eq!(snap.exit, Some(0));
    }

    #[test]
    fn spawn_caps_output() {
        let job = spawn_login_command(
            "/bin/sh",
            &format!("dd if=/dev/zero bs=1024 count=400 2>/dev/null | tr '\\0' 'x'"),
            Duration::from_secs(5),
        );
        let start = Instant::now();
        let mut snap = job.snapshot();
        while !snap.done && start.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(20));
            snap = job.snapshot();
        }
        force_kill(&job);
        assert!(snap.output.len() <= OUTPUT_CAP);
        assert!(snap.capped || snap.output.len() == OUTPUT_CAP);
    }

    #[test]
    fn spawn_times_out() {
        let job = spawn_login_command("/bin/sh", "sleep 8", Duration::from_millis(200));
        let start = Instant::now();
        let mut snap = job.snapshot();
        while !snap.done && start.elapsed() < Duration::from_secs(4) {
            thread::sleep(Duration::from_millis(20));
            snap = job.snapshot();
        }
        force_kill(&job);
        assert!(snap.done);
        assert!(snap.timed_out || snap.output.contains("timed out"));
    }
}
