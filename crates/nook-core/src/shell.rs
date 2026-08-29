//! Interactive login PTY for Termi-Notch.
//!
//! Reachable only from typing in the island UI. URLs, the CLI, Services, and
//! Alfred must never call this module. The child lives in its own process
//! group so cancel = `killpg`. Keep the session only while the card is open.

use crate::app_data_dir;
use crate::database;
use crate::settings::get_app_settings;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::sync::Notify;

const HISTORY_KEY: &str = "shell_history";
const HISTORY_CAP: usize = 50;
const PGID_FILE: &str = "shell.pgid";
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub display: String,
    /// Styled view of the visible grid, one entry per screen row. Spans are
    /// runs of cells sharing the same attributes; colors are resolved to RGB.
    pub styled: Vec<StyledRow>,
    pub cursor_row: u16,
    pub cursor_col: u16,
    /// True while the app hid the cursor (DECTCEM) or the view is scrolled
    /// back into history.
    pub cursor_hidden: bool,
    pub cols: u16,
    pub rows: u16,
    pub done: bool,
    pub exit: Option<i32>,
}

struct SessionInner {
    parser: Mutex<vt100::Parser>,
    display: Mutex<String>,
    styled: Mutex<Vec<StyledRow>>,
    cursor: Mutex<(u16, u16, bool)>,
    size: Mutex<(u16, u16)>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    done: AtomicBool,
    exit: Mutex<Option<i32>>,
    notify: Notify,
    pgid: AtomicU32,
    cancel: AtomicBool,
}

/// A run of cells with identical attributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    /// `None` = terminal default foreground.
    pub fg: Option<[u8; 3]>,
    /// `None` = transparent (terminal default background).
    pub bg: Option<[u8; 3]>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

pub type StyledRow = Vec<Span>;

/// Default 16-color palette (ghostty/kitty-like, readable on dark glass).
pub const ANSI_PALETTE: [[u8; 3]; 16] = [
    [0x1c, 0x1c, 0x1c], // black
    [0xff, 0x5c, 0x57], // red
    [0x5a, 0xf7, 0x8e], // green
    [0xf3, 0xf9, 0x9d], // yellow
    [0x57, 0xc7, 0xff], // blue
    [0xff, 0x6a, 0xc1], // magenta
    [0x9a, 0xed, 0xfe], // cyan
    [0xf1, 0xf1, 0xf0], // white
    [0x68, 0x68, 0x68], // bright black
    [0xff, 0x7b, 0x76], // bright red
    [0x7d, 0xff, 0xa6], // bright green
    [0xff, 0xfd, 0xb3], // bright yellow
    [0x7f, 0xd7, 0xff], // bright blue
    [0xff, 0x8f, 0xd2], // bright magenta
    [0xb8, 0xf5, 0xff], // bright cyan
    [0xff, 0xff, 0xff], // bright white
];

/// Resolve a vt100 color to RGB. `bold` promotes the basic 8 to bright, as
/// most terminals do.
pub fn resolve_color(color: vt100::Color, bold: bool) -> Option<[u8; 3]> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Rgb(r, g, b) => Some([r, g, b]),
        vt100::Color::Idx(i) => Some(match i {
            0..=7 if bold => ANSI_PALETTE[i as usize + 8],
            0..=15 => ANSI_PALETTE[i as usize],
            16..=231 => {
                let i = i - 16;
                let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                [step(i / 36), step((i / 6) % 6), step(i % 6)]
            }
            _ => {
                let v = 8 + (i - 232) * 10;
                [v, v, v]
            }
        }),
    }
}

/// Convert the visible screen into styled rows.
pub fn styled_rows(screen: &vt100::Screen) -> Vec<StyledRow> {
    let (rows, cols) = screen.size();
    let mut out = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut spans: StyledRow = Vec::new();
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else { continue };
            if cell.is_wide_continuation() {
                continue;
            }
            let bold = cell.bold();
            let mut fg = resolve_color(cell.fgcolor(), bold);
            let mut bg = resolve_color(cell.bgcolor(), false);
            if cell.inverse() {
                // Default fg/bg are not concrete colors; fall back to white on
                // black-ish so inverse stays visible.
                let f = fg.unwrap_or(ANSI_PALETTE[7]);
                let b = bg.unwrap_or([0x00, 0x00, 0x00]);
                fg = Some(b);
                bg = Some(f);
            }
            let text = if cell.has_contents() {
                cell.contents()
            } else {
                " ".to_string()
            };
            let same = spans.last().is_some_and(|s: &Span| {
                s.fg == fg
                    && s.bg == bg
                    && s.bold == bold
                    && s.italic == cell.italic()
                    && s.underline == cell.underline()
            });
            if same {
                spans.last_mut().unwrap().text.push_str(&text);
            } else {
                spans.push(Span {
                    text,
                    fg,
                    bg,
                    bold,
                    italic: cell.italic(),
                    underline: cell.underline(),
                });
            }
        }
        // Drop trailing unstyled blanks so lines stay light to paint.
        while let Some(last) = spans.last() {
            if last.bg.is_none() && !last.underline && last.text.trim().is_empty() {
                spans.pop();
            } else {
                break;
            }
        }
        out.push(spans);
    }
    out
}

#[derive(Clone)]
pub struct SessionHandle {
    inner: Arc<SessionInner>,
}

impl SessionHandle {
    fn new(cols: u16, rows: u16) -> Self {
        let cols = cols.max(2);
        let rows = rows.max(2);
        Self {
            inner: Arc::new(SessionInner {
                parser: Mutex::new(vt100::Parser::new(rows, cols, 2000)),
                display: Mutex::new(String::new()),
                styled: Mutex::new(Vec::new()),
                cursor: Mutex::new((0, 0, false)),
                size: Mutex::new((cols, rows)),
                writer: Mutex::new(None),
                master: Mutex::new(None),
                done: AtomicBool::new(false),
                exit: Mutex::new(None),
                notify: Notify::new(),
                pgid: AtomicU32::new(0),
                cancel: AtomicBool::new(false),
            }),
        }
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let (cols, rows) = self
            .inner
            .size
            .lock()
            .map(|g| *g)
            .unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));
        let (cursor_row, cursor_col, cursor_hidden) =
            self.inner.cursor.lock().map(|g| *g).unwrap_or((0, 0, false));
        SessionSnapshot {
            display: self
                .inner
                .display
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
            styled: self
                .inner
                .styled
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
            cursor_row,
            cursor_col,
            cursor_hidden,
            cols,
            rows,
            done: self.inner.done.load(Ordering::SeqCst),
            exit: self.inner.exit.lock().ok().and_then(|g| *g),
        }
    }

    pub async fn wait_update(&self) -> SessionSnapshot {
        self.inner.notify.notified().await;
        self.snapshot()
    }

    /// Scroll the viewport through vt100 scrollback. Positive `lines` moves
    /// toward older output; the parser clamps at the scrollback limit.
    /// Returns the resulting scrollback offset in rows.
    pub fn scroll_lines(&self, lines: i32) -> usize {
        let Ok(mut parser) = self.inner.parser.lock() else {
            return 0;
        };
        let cur = parser.screen().scrollback() as i64;
        let next = (cur + lines as i64).max(0) as usize;
        parser.set_scrollback(next);
        let now = parser.screen().scrollback();
        publish_screen(&self.inner, &parser);
        drop(parser);
        self.inner.notify.notify_waiters();
        now
    }

    /// Jump back to the live (bottom) view.
    pub fn scroll_to_bottom(&self) {
        if let Ok(mut parser) = self.inner.parser.lock() {
            if parser.screen().scrollback() != 0 {
                parser.set_scrollback(0);
                publish_screen(&self.inner, &parser);
                drop(parser);
                self.inner.notify.notify_waiters();
            }
        }
    }

    pub fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() || self.inner.done.load(Ordering::SeqCst) {
            return;
        }
        // Typing snaps back to the live view, like every terminal.
        self.scroll_to_bottom();
        if let Ok(mut guard) = self.inner.writer.lock() {
            if let Some(writer) = guard.as_mut() {
                let _ = writer.write_all(bytes);
                let _ = writer.flush();
            }
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let cols = cols.max(2);
        let rows = rows.max(2);
        if let Ok(mut size) = self.inner.size.lock() {
            if *size == (cols, rows) {
                return;
            }
            *size = (cols, rows);
        }
        if let Ok(mut parser) = self.inner.parser.lock() {
            parser.set_size(rows, cols);
            publish_screen(&self.inner, &parser);
        }
        if let Ok(guard) = self.inner.master.lock() {
            if let Some(master) = guard.as_ref() {
                let _ = master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            }
        }
        self.inner.notify.notify_waiters();
    }

    pub fn cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
        let pgid = self.inner.pgid.load(Ordering::SeqCst);
        if pgid != 0 {
            kill_group(pgid as i32);
        }
        if let Ok(mut writer) = self.inner.writer.lock() {
            *writer = None;
        }
        if let Ok(mut master) = self.inner.master.lock() {
            *master = None;
        }
        self.inner.notify.notify_waiters();
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.cancel();
        }
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

/// Strip CSI / OSC / other C1 escapes. Kept for tests and log sanitizing.
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

/// Spawn an interactive login shell on a PTY. Nothing is left running at idle;
/// the caller must `cancel` when the card closes.
pub fn spawn_login_pty(shell: &str, cols: u16, rows: u16) -> SessionHandle {
    spawn_pty(shell, &["-l"], &[], cols, rows)
}

/// Spawn `shell` with extra args/env. Tests use a non-login `/bin/sh`.
pub fn spawn_pty(
    shell: &str,
    args: &[&str],
    extra_env: &[(&str, &str)],
    cols: u16,
    rows: u16,
) -> SessionHandle {
    let handle = SessionHandle::new(cols, rows);
    let job = handle.clone();
    let shell = shell.to_string();
    let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let extra_env: Vec<(String, String)> = extra_env
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    let cols = cols.max(2);
    let rows = rows.max(2);
    thread::Builder::new()
        .name("nook-shell".into())
        .spawn(move || run_session(job, shell, args, extra_env, cols, rows))
        .ok();
    handle
}

fn run_session(
    job: SessionHandle,
    shell: String,
    args: Vec<String>,
    extra_env: Vec<(String, String)>,
    cols: u16,
    rows: u16,
) {
    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(err) => {
            append_message(&job, &format!("failed to open pty: {err}\n"));
            finish(&job, Some(1));
            return;
        }
    };

    let mut cmd = CommandBuilder::new(&shell);
    for arg in &args {
        cmd.arg(arg);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    for (key, value) in &extra_env {
        cmd.env(key, value);
    }
    if let Some(home) = dirs::home_dir() {
        cmd.cwd(home);
    }

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(err) => {
            append_message(&job, &format!("failed to spawn {shell}: {err}\n"));
            finish(&job, Some(1));
            return;
        }
    };

    let pgid = child.process_id().unwrap_or(0);
    job.inner.pgid.store(pgid, Ordering::SeqCst);
    if pgid != 0 {
        record_pgid(pgid);
    }

    match pair.master.try_clone_reader() {
        Ok(reader) => {
            let reader_job = job.clone();
            thread::Builder::new()
                .name("nook-shell-out".into())
                .spawn(move || read_pty(reader_job, reader))
                .ok();
        }
        Err(err) => {
            append_message(&job, &format!("failed to read pty: {err}\n"));
            let _ = child.kill();
            finish(&job, Some(1));
            return;
        }
    }

    match pair.master.take_writer() {
        Ok(writer) => {
            if let Ok(mut guard) = job.inner.writer.lock() {
                *guard = Some(writer);
            }
        }
        Err(err) => {
            append_message(&job, &format!("failed to write pty: {err}\n"));
            let _ = child.kill();
            finish(&job, Some(1));
            return;
        }
    }

    if let Ok(mut guard) = job.inner.master.lock() {
        *guard = Some(pair.master);
    }

    let status = child.wait();
    clear_pgid();
    if let Ok(mut writer) = job.inner.writer.lock() {
        *writer = None;
    }
    if let Ok(mut master) = job.inner.master.lock() {
        *master = None;
    }
    let code = match status {
        Ok(status) => status.exit_code() as i32,
        Err(_) => 1,
    };
    finish(&job, Some(code));
}

fn read_pty(job: SessionHandle, mut reader: Box<dyn Read + Send>) {
    let mut buf = [0u8; 4096];
    loop {
        if job.inner.cancel.load(Ordering::SeqCst) || job.inner.done.load(Ordering::SeqCst) {
            break;
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Ok(mut parser) = job.inner.parser.lock() {
                    parser.process(&buf[..n]);
                    publish_screen(&job.inner, &parser);
                }
                job.inner.notify.notify_waiters();
            }
            Err(_) => break,
        }
    }
    job.inner.notify.notify_waiters();
}

fn publish_screen(inner: &SessionInner, parser: &vt100::Parser) {
    let screen = parser.screen();
    let (row, col) = screen.cursor_position();
    let hidden = screen.hide_cursor() || screen.scrollback() > 0;
    if let Ok(mut cursor) = inner.cursor.lock() {
        *cursor = (row, col, hidden);
    }
    if let Ok(mut display) = inner.display.lock() {
        *display = screen.contents();
    }
    if let Ok(mut styled) = inner.styled.lock() {
        *styled = styled_rows(screen);
    }
}

fn append_message(job: &SessionHandle, message: &str) {
    if let Ok(mut display) = job.inner.display.lock() {
        display.push_str(message);
    }
    job.inner.notify.notify_waiters();
}

fn finish(job: &SessionHandle, exit: Option<i32>) {
    if let Ok(mut guard) = job.inner.exit.lock() {
        *guard = exit;
    }
    job.inner.done.store(true, Ordering::SeqCst);
    job.inner.notify.notify_waiters();
}

/// Used by tests so a leftover child cannot hang CI.
pub fn force_kill(handle: &SessionHandle) {
    handle.cancel();
}

#[cfg(test)]
mod tests {
    #[test]
    fn styled_rows_split_on_color_and_bold() {
        let mut parser = vt100::Parser::new(2, 20, 0);
        parser.process(b"\x1b[31mred\x1b[0m ok \x1b[1;34mB\x1b[0m");
        let rows = super::styled_rows(parser.screen());
        assert_eq!(rows.len(), 2);
        let r = &rows[0];
        assert_eq!(r[0].text, "red");
        assert_eq!(r[0].fg, Some(super::ANSI_PALETTE[1]));
        assert_eq!(r[1].text, " ok ");
        assert_eq!(r[1].fg, None);
        assert_eq!(r[2].text, "B");
        assert!(r[2].bold);
        // bold promotes basic blue to bright blue
        assert_eq!(r[2].fg, Some(super::ANSI_PALETTE[12]));
        assert!(rows[1].is_empty());
    }

    #[test]
    fn resolve_256_and_rgb() {
        assert_eq!(super::resolve_color(vt100::Color::Idx(16), false), Some([0, 0, 0]));
        assert_eq!(super::resolve_color(vt100::Color::Idx(231), false), Some([255, 255, 255]));
        assert_eq!(super::resolve_color(vt100::Color::Idx(232), false), Some([8, 8, 8]));
        assert_eq!(super::resolve_color(vt100::Color::Rgb(1, 2, 3), true), Some([1, 2, 3]));
    }

    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn strip_ansi_drops_sgr_and_keeps_text() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("a\rb\n"), "ab\n");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}prompt"), "prompt");
    }

    #[test]
    fn resolved_shell_prefers_override() {
        assert_eq!(resolved_shell("/bin/sh"), "/bin/sh");
        assert!(!resolved_shell("").is_empty());
    }

    fn wait_done(job: &SessionHandle, timeout: Duration) -> SessionSnapshot {
        let start = Instant::now();
        let mut snap = job.snapshot();
        while !snap.done && start.elapsed() < timeout {
            thread::sleep(Duration::from_millis(20));
            snap = job.snapshot();
        }
        snap
    }

    fn wait_contains(job: &SessionHandle, needle: &str, timeout: Duration) -> SessionSnapshot {
        let start = Instant::now();
        let mut snap = job.snapshot();
        while !snap.display.contains(needle) && !snap.done && start.elapsed() < timeout {
            thread::sleep(Duration::from_millis(20));
            snap = job.snapshot();
        }
        snap
    }

    #[test]
    fn spawn_echo_captures_stdout() {
        let job = spawn_pty("/bin/sh", &[], &[("PS1", ""), ("TERM", "xterm")], 80, 24);
        let ready = wait_contains(&job, "", Duration::from_secs(2));
        let _ = ready;
        thread::sleep(Duration::from_millis(80));
        job.write(b"printf 'hello-nook\\n'\r");
        let snap = wait_contains(&job, "hello-nook", Duration::from_secs(5));
        force_kill(&job);
        assert!(
            snap.display.contains("hello-nook"),
            "output was {:?}",
            snap.display
        );
    }

    #[test]
    fn cancel_stops_session() {
        let job = spawn_pty("/bin/sh", &[], &[("PS1", ""), ("TERM", "xterm")], 80, 24);
        thread::sleep(Duration::from_millis(80));
        job.write(b"sleep 8\r");
        thread::sleep(Duration::from_millis(80));
        force_kill(&job);
        let snap = wait_done(&job, Duration::from_secs(4));
        assert!(snap.done, "session should finish after cancel");
    }
}
