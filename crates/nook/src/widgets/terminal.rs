//! Termi-Notch expanded card: a real login-shell PTY.
//!
//! Keys and IME commits go to the child's stdin. The URL scheme / CLI /
//! Services paths never call into `nook_core::shell`.

use crate::island::ui::{label, nook_pane};
use crate::island::Island;
use crate::platform;
use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, size, App, Bounds, ClipboardItem, Context, CursorStyle,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font,
    FontFallbacks, FontFeatures, FontStyle, FontWeight, Hsla, KeyDownEvent, MouseButton,
    MouseDownEvent, Pixels, Point, Render, ScrollWheelEvent, SharedString, TextRun, UTF16Selection,
    UnderlineStyle, Window,
};
use nook_core::shell::{self, SessionHandle, SessionSnapshot, StyledRow};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_FONT_SIZE: f32 = 11.0;
/// Fallback metrics until the first paint measures the real font.
/// Mockup PTY rows are 11px / 14px.
const DEFAULT_LINE_HEIGHT: f32 = 14.0;
const DEFAULT_CHAR_WIDTH: f32 = 6.6;
/// Mockup caret is `#FFFFFF8C`, one cell wide.
const CURSOR_ALPHA: f32 = 0x8C as f32 / 255.0;
/// Fonts tried in order when `terminal_font` is empty or missing.
const FONT_STACK: [&str; 5] = [
    "SF Mono",
    "Menlo",
    "Monaco",
    "Courier New",
    "DejaVu Sans Mono",
];
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 18;
/// Restart / exit chip row, only painted after the shell ends.
const TERMINAL_HEADER_H: f32 = 20.0;
const START_ICON: f32 = 16.0;
const START_GAP: f32 = 10.0;
const START_BTN_RADIUS: f32 = 20.0;
const START_BTN_PAD_X: f32 = 16.0;
const START_BTN_PAD_Y: f32 = 8.0;
const EXIT_BTN: f32 = 28.0;
const EXIT_BTN_RADIUS: f32 = 14.0;
const EXIT_PLAY: f32 = 16.0;
const EXIT_CHIP_H: f32 = 20.0;
const EXIT_CHIP_RADIUS: f32 = 10.0;
const EXIT_CHIP_PAD_X: f32 = 8.0;
const EXIT_HEADER_GAP: f32 = 8.0;

/// Height of the Term pane itself. Matches the mockup Nook row (128)
/// minus the expanded bottom pad that `terminal_card` applies.
pub(crate) fn terminal_pane_min_height() -> f32 {
    theme::NOOK_BODY - theme::EXPANDED_PAD
}

pub(crate) enum TerminalEvent {
    State { running: bool, exit: Option<i32> },
    Focus(bool),
}

impl EventEmitter<TerminalEvent> for TerminalView {}

pub(crate) struct TerminalView {
    shell: String,
    session: Option<SessionHandle>,
    session_gen: u64,
    display: String,
    styled: Vec<StyledRow>,
    cursor_row: u16,
    cursor_col: u16,
    cursor_hidden: bool,
    running: bool,
    exit: Option<i32>,
    focus: FocusHandle,
    want_focus: bool,
    marked: String,
    wheel_accum: f32,
    bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    /// Font family override from settings (empty = built-in stack).
    font_family: String,
    font_size: f32,
    /// Family that actually exists on this machine, picked from the setting
    /// and `FONT_STACK`. GPUI silently substitutes a proportional UI font for
    /// an unknown family, which wrecks column alignment, so never guess.
    resolved_family: Option<String>,
    /// Measured from the font at paint time.
    char_w: f32,
    line_h: f32,
    cols: u16,
    rows: u16,
    ever_started: bool,
    copied_at: Option<Instant>,
}

impl TerminalView {
    pub(crate) fn new(shell: String, cx: &mut Context<Self>) -> Self {
        let this = Self {
            shell,
            session: None,
            session_gen: 0,
            display: String::new(),
            styled: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
            cursor_hidden: false,
            running: false,
            exit: None,
            focus: cx.focus_handle(),
            want_focus: false,
            marked: String::new(),
            wheel_accum: 0.0,
            bounds: Rc::new(RefCell::new(None)),
            font_family: String::new(),
            font_size: DEFAULT_FONT_SIZE,
            resolved_family: None,
            char_w: DEFAULT_CHAR_WIDTH,
            line_h: DEFAULT_LINE_HEIGHT,
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            ever_started: false,
            copied_at: None,
        };
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, cx| {
                if !this.ever_started {
                    cx.emit(TerminalEvent::State {
                        running: false,
                        exit: None,
                    });
                }
            });
        })
        .detach();
        this
    }

    pub(crate) fn is_started(&self) -> bool {
        self.ever_started
    }

    pub(crate) fn shutdown(&mut self) {
        self.session_gen = self.session_gen.wrapping_add(1);
        if let Some(session) = self.session.take() {
            session.cancel();
        }
        self.running = false;
        self.marked.clear();
    }

    pub(crate) fn restart(&mut self, shell: String, cx: &mut Context<Self>) {
        self.shell = shell;
        self.start_session(cx);
        self.want_focus = true;
        cx.notify();
    }

    fn start_session(&mut self, cx: &mut Context<Self>) {
        self.ever_started = true;
        self.shutdown();
        self.session_gen = self.session_gen.wrapping_add(1);
        let gen = self.session_gen;
        self.display.clear();
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.exit = None;
        self.running = true;
        let handle = shell::spawn_login_pty(&self.shell, self.cols, self.rows);
        self.session = Some(handle.clone());
        cx.emit(TerminalEvent::State {
            running: true,
            exit: None,
        });
        Self::spawn_watch(handle, gen, cx);
        cx.notify();
    }

    fn spawn_watch(handle: SessionHandle, gen: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            let snap = cx
                .background_executor()
                .spawn({
                    let handle = handle.clone();
                    async move { handle.wait_update().await }
                })
                .await;
            let done = this
                .update(cx, |this, cx| {
                    if this.session_gen != gen {
                        return true;
                    }
                    this.apply_snapshot(&snap);
                    cx.emit(TerminalEvent::State {
                        running: !snap.done,
                        exit: snap.exit,
                    });
                    cx.notify();
                    snap.done
                })
                .unwrap_or(true);
            if done {
                break;
            }
        })
        .detach();
    }

    fn apply_snapshot(&mut self, snap: &SessionSnapshot) {
        self.display = snap.display.clone();
        self.styled = snap.styled.clone();
        self.cursor_row = snap.cursor_row;
        self.cursor_col = snap.cursor_col;
        self.cursor_hidden = snap.cursor_hidden;
        self.running = !snap.done;
        self.exit = snap.exit;
        if snap.done {
            self.session = None;
        }
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        if let Some(session) = &self.session {
            session.write(bytes);
        }
    }

    fn scroll_lines(&mut self, lines: i32) {
        if let Some(session) = &self.session {
            session.scroll_lines(lines);
        }
    }

    /// Apply font settings; a change re-measures on the next paint and
    /// re-flows the PTY size.
    pub(crate) fn set_font(&mut self, family: &str, size: f32) {
        let size = if size.is_finite() && size >= 6.0 {
            size.min(32.0)
        } else {
            DEFAULT_FONT_SIZE
        };
        if self.font_family != family || (self.font_size - size).abs() > f32::EPSILON {
            self.font_family = family.to_string();
            self.font_size = size;
            self.resolved_family = None;
        }
    }

    /// Pick the first installed family from the setting + built-in stack.
    fn resolve_family(&mut self, window: &Window) {
        if self.resolved_family.is_some() {
            return;
        }
        let installed = installed_families(window);
        let wanted = self.font_family.trim();
        let pick = std::iter::once(wanted)
            .filter(|w| !w.is_empty())
            .chain(FONT_STACK.iter().copied())
            .find(|name| installed.iter().any(|f| f.eq_ignore_ascii_case(name)))
            .unwrap_or("Menlo");
        if !wanted.is_empty() && !pick.eq_ignore_ascii_case(wanted) {
            log::warn!("terminal font {wanted:?} not installed, using {pick}");
        }
        self.resolved_family = Some(pick.to_string());
    }

    fn font(&self, bold: bool, italic: bool) -> Font {
        let family = self
            .resolved_family
            .clone()
            .unwrap_or_else(|| "Menlo".to_string());
        let stack: Vec<String> = FONT_STACK
            .iter()
            .filter(|f| !f.eq_ignore_ascii_case(&family))
            .map(|f| f.to_string())
            .collect();
        Font {
            family: family.into(),
            features: FontFeatures(Arc::new(Vec::new())),
            fallbacks: Some(FontFallbacks::from_fonts(stack)),
            weight: if bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            },
            style: if italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
        }
    }

    fn write_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let payload = text.replace("\r\n", "\r").replace('\n', "\r");
        self.write_bytes(payload.as_bytes());
    }

    fn resize_to_bounds(&mut self, bounds: Bounds<Pixels>) {
        let width: f32 = bounds.size.width.into();
        let height: f32 = bounds.size.height.into();
        let cols = ((width / self.char_w).floor() as u16).clamp(8, 240);
        let rows = ((height / self.line_h).floor() as u16).clamp(4, 80);
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        if let Some(session) = &self.session {
            session.resize(cols, rows);
        }
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let key = ks.key.as_str();
        let m = &ks.modifiers;
        let cmd = m.secondary();

        if cmd && key == "c" {
            cx.write_to_clipboard(ClipboardItem::new_string(self.visible_screen()));
            self.copied_at = Some(Instant::now());
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let _ = this.update(cx, |this, cx| {
                    this.copied_at = None;
                    cx.notify();
                });
            })
            .detach();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if cmd && key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.write_text(&text);
            }
            cx.stop_propagation();
            return;
        }
        if cmd && key == "k" {
            self.write_bytes(&[0x0c]);
            cx.stop_propagation();
            return;
        }
        if cmd {
            return;
        }

        if let Some(bytes) = key_to_pty(ks) {
            self.write_bytes(&bytes);
            cx.stop_propagation();
            cx.notify();
            return;
        }

        // Printable keys normally arrive via the IME path
        // (`EntityInputHandler::replace_text_in_range`), but that round-trip
        // is not reliable for this panel window, so fall back to the
        // keystroke's own character. Skipped while composing (marked text)
        // so dead keys / CJK input still go through the IME. Stopping
        // propagation here also keeps the platform from inserting it twice.
        if !m.control && !m.alt && !m.platform && !m.function && self.marked.is_empty() {
            if let Some(ch) = ks.key_char.as_deref() {
                if !ch.is_empty() && !ch.chars().any(char::is_control) {
                    self.write_text(ch);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
            }
        }

        let _ = window;
    }

    fn visible_screen(&self) -> String {
        if !self.styled.is_empty() {
            return self
                .styled
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|span| span.text.as_str())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
        let lines: Vec<&str> = self.display.lines().collect();
        let start = lines.len().saturating_sub(self.rows as usize);
        lines[start..].join("\n")
    }
}

fn key_to_pty(ks: &gpui::Keystroke) -> Option<Vec<u8>> {
    let key = ks.key.as_str();
    let m = &ks.modifiers;
    if m.control && !m.alt && !m.platform {
        let byte = match key {
            "a" => 0x01,
            "b" => 0x02,
            "c" => 0x03,
            "d" => 0x04,
            "e" => 0x05,
            "f" => 0x06,
            "g" => 0x07,
            "h" => 0x08,
            "i" | "tab" => 0x09,
            "k" => 0x0b,
            "l" => 0x0c,
            "n" => 0x0e,
            "p" => 0x10,
            "r" => 0x12,
            "u" => 0x15,
            "w" => 0x17,
            "z" => 0x1a,
            "[" | "escape" => 0x1b,
            _ => return None,
        };
        return Some(vec![byte]);
    }
    if m.alt || m.platform || m.control {
        return None;
    }
    Some(match key {
        "enter" => b"\r".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" => b"\x1b".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" | "page-up" => b"\x1b[5~".to_vec(),
        "pagedown" | "page-down" => b"\x1b[6~".to_vec(),
        _ => return None,
    })
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let text = &self.marked;
        let start = utf16_to_utf8(text, range.start);
        let end = utf16_to_utf8(text, range.end);
        adjusted_range.replace(utf8_to_utf16(text, start)..utf8_to_utf16(text, end));
        Some(text.get(start..end).unwrap_or("").to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let len = utf8_to_utf16(&self.marked, self.marked.len());
        Some(UTF16Selection {
            range: len..len,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        if self.marked.is_empty() {
            None
        } else {
            Some(0..utf8_to_utf16(&self.marked, self.marked.len()))
        }
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked.clear();
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked.clear();
        self.write_text(text);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = new_text.to_string();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let origin = element_bounds.origin
            + point(
                px(self.cursor_col as f32 * self.char_w),
                px(self.cursor_row as f32 * self.line_h),
            );
        Some(Bounds {
            origin,
            size: size(px(self.char_w.max(1.0)), px(self.line_h)),
        })
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(utf8_to_utf16(&self.marked, self.marked.len()))
    }
}

fn utf8_to_utf16(text: &str, utf8: usize) -> usize {
    let utf8 = utf8.min(text.len());
    text[..utf8].encode_utf16().count()
}

fn utf16_to_utf8(text: &str, utf16: usize) -> usize {
    let mut seen = 0;
    for (i, c) in text.char_indices() {
        if seen >= utf16 {
            return i;
        }
        seen += c.len_utf16();
    }
    text.len()
}

fn paint_terminal(
    bounds: Bounds<Pixels>,
    entity: &Entity<TerminalView>,
    focus: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    // Measure the cell from the configured font, then size the PTY.
    let (probe_font, font_size) = entity.update(cx, |term, _| {
        term.resolve_family(window);
        (term.font(false, false), term.font_size)
    });
    let (char_w, line_h) = measure_cell(&probe_font, font_size, window);
    entity.update(cx, |term, _| {
        term.char_w = char_w;
        term.line_h = line_h;
        term.resize_to_bounds(bounds);
    });

    let (styled, cursor_row, cursor_col, cursor_hidden, marked, focused, running) = {
        let term = entity.read(cx);
        (
            term.styled.clone(),
            term.cursor_row,
            term.cursor_col,
            term.cursor_hidden,
            term.marked.clone(),
            focus.is_focused(window),
            term.running,
        )
    };

    let default_fg: Hsla = theme::LABEL.into();
    let height: f32 = bounds.size.height.into();
    let max_rows = ((height / line_h).floor() as usize).max(1);
    for (ix, row) in styled.iter().enumerate().take(max_rows) {
        if row.is_empty() {
            continue;
        }
        let origin = point(bounds.origin.x, bounds.origin.y + px(ix as f32 * line_h));
        let mut text = String::new();
        let mut runs: Vec<TextRun> = Vec::with_capacity(row.len());
        let term = entity.read(cx);
        for span in row {
            let color = span.fg.map(rgb3).unwrap_or(default_fg);
            runs.push(TextRun {
                len: span.text.len(),
                font: term.font(span.bold, span.italic),
                color,
                background_color: span.bg.map(rgb3),
                underline: span.underline.then(|| UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(color),
                    wavy: false,
                }),
                strikethrough: None,
            });
            text.push_str(&span.text);
        }
        let line =
            window
                .text_system()
                .shape_line(SharedString::from(text), px(font_size), &runs, None);
        let _ = line.paint(origin, px(line_h), window, cx);
    }

    if running && !cursor_hidden && (cursor_row as usize) < max_rows {
        let caret_origin = bounds.origin
            + point(
                px(cursor_col as f32 * char_w),
                px(cursor_row as f32 * line_h),
            );
        let caret = Bounds {
            origin: caret_origin,
            size: size(px(char_w.max(1.0)), px(line_h)),
        };
        let mut c: Hsla = theme::LABEL.into();
        c.a = CURSOR_ALPHA;
        if focused {
            window.paint_quad(gpui::fill(caret, c));
        } else {
            window.paint_quad(gpui::outline(caret, theme::LABEL, gpui::BorderStyle::Solid));
        }
        if focused && !marked.is_empty() {
            let run = TextRun {
                len: marked.len(),
                font: probe_font,
                color: theme::accent().into(),
                background_color: None,
                underline: Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: None,
                    wavy: false,
                }),
                strikethrough: None,
            };
            let line = window.text_system().shape_line(
                SharedString::from(marked),
                px(font_size),
                &[run],
                None,
            );
            let _ = line.paint(caret_origin, px(line_h), window, cx);
        }
    }

    window.handle_input(focus, ElementInputHandler::new(bounds, entity.clone()), cx);
}

/// Installed font families, fetched once — CoreText enumeration is slow.
fn installed_families(window: &Window) -> &'static [String] {
    static FAMILIES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FAMILIES.get_or_init(|| window.text_system().all_font_names())
}

/// Snap the PTY 16-color blues/greens onto the mockup prompt
/// (`#0A84FF` path, `#30D158` ❯). Truecolor spans pass through.
fn map_ansi_fg(c: [u8; 3]) -> [u8; 3] {
    match c {
        [0x57, 0xc7, 0xff] | [0x7f, 0xd7, 0xff] => [0x0a, 0x84, 0xff],
        [0x5a, 0xf7, 0x8e] | [0x7d, 0xff, 0xa6] => [0x30, 0xd1, 0x58],
        other => other,
    }
}

fn rgb3(c: [u8; 3]) -> Hsla {
    let c = map_ansi_fg(c);
    gpui::rgb(((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32).into()
}

/// Advance width of one cell and the mockup 11/14 line height, both from
/// the actual shaped font so `cols`/`rows` line up with what is drawn.
fn measure_cell(font: &Font, font_size: f32, window: &mut Window) -> (f32, f32) {
    let run = TextRun {
        len: 1,
        font: font.clone(),
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line =
        window
            .text_system()
            .shape_line(SharedString::from("M"), px(font_size), &[run], None);
    let w: f32 = line.width.into();
    let char_w = if w > 1.0 { w } else { DEFAULT_CHAR_WIDTH };
    let line_h = (font_size * DEFAULT_LINE_HEIGHT / DEFAULT_FONT_SIZE).round();
    (char_w, line_h)
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.want_focus && self.ever_started {
            self.want_focus = false;
            window.focus(&self.focus);
            window.activate_window();
            platform::activate_app();
            cx.emit(TerminalEvent::Focus(true));
        }
        let entity = cx.entity();
        let focus = self.focus.clone();
        let bounds_cell = self.bounds.clone();
        let copied = self
            .copied_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(1));
        div()
            .id("nook-pty")
            .relative()
            .w_full()
            .flex_1()
            .min_h_0()
            .min_w(px(0.))
            .overflow_hidden()
            .cursor(CursorStyle::IBeam)
            .track_focus(&focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.want_focus = false;
                    window.focus(&this.focus.clone());
                    window.activate_window();
                    platform::activate_app();
                    cx.emit(TerminalEvent::Focus(true));
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(Self::handle_key))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let line_h = this.line_h;
                let delta = event.delta.pixel_delta(px(line_h));
                let dx: f32 = delta.x.into();
                let dy: f32 = delta.y.into();
                if dx.abs() > dy.abs() {
                    return;
                }
                this.wheel_accum += dy;
                let lines = (this.wheel_accum / line_h).trunc();
                if lines != 0.0 {
                    this.wheel_accum -= lines * line_h;
                    // Wheel up (positive y) reveals older output.
                    this.scroll_lines(lines as i32);
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    move |bounds, _, _| {
                        *bounds_cell.borrow_mut() = Some(bounds);
                    },
                    move |bounds, _, window, cx| {
                        paint_terminal(bounds, &entity, &focus, window, cx);
                    },
                )
                .w_full()
                .h_full(),
            )
            .when(copied, |d| {
                d.child(div().absolute().top(px(4.)).right(px(8.)).child(label(
                    "Copied screen",
                    theme::FOOTNOTE,
                    true,
                )))
            })
    }
}

pub(crate) fn terminal_card(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    let view = island.ensure_terminal(cx);
    let (family, size) = (
        island.settings.terminal_font.clone(),
        island.settings.terminal_font_size,
    );
    view.update(cx, |v, _| v.set_font(&family, size));
    let started = view.read(cx).is_started();
    let running = island.shell_running;
    let chip = exit_chip(island);
    if !started {
        return nook_pane("nook-terminal")
            .w_full()
            .px(px(theme::EXPANDED_PAD))
            .pb(px(theme::EXPANDED_PAD))
            .child(
                div()
                    .id("term-start-empty")
                    .flex_1()
                    .w_full()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(START_GAP))
                    .child(crate::icons::lucide_color(
                        "terminal",
                        START_ICON,
                        theme::LABEL,
                    ))
                    .child(
                        div()
                            .id("term-start")
                            .flex_shrink_0()
                            .px(px(START_BTN_PAD_X))
                            .py(px(START_BTN_PAD_Y))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(START_BTN_RADIUS))
                            .bg(theme::FILL)
                            .hover(|s| s.bg(theme::FILL_SECONDARY))
                            .active(|s| s.opacity(0.85))
                            .cursor(CursorStyle::PointingHand)
                            .child(label("Start Shell", theme::BODY, true))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.restart_terminal(window, cx);
                                }),
                            ),
                    ),
            )
            .into_any_element();
    }
    nook_pane("nook-terminal")
        .relative()
        .w_full()
        .px(px(theme::EXPANDED_PAD))
        .pb(px(theme::EXPANDED_PAD))
        .child(view)
        .when(!running, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(0.))
                    .right(px(0.))
                    .h(px(EXIT_BTN.max(TERMINAL_HEADER_H)))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(EXIT_HEADER_GAP))
                    .child(
                        div()
                            .id("term-run")
                            .size(px(EXIT_BTN))
                            .rounded(px(EXIT_BTN_RADIUS))
                            .bg(theme::FILL)
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(theme::FILL_SECONDARY))
                            .active(|s| s.opacity(0.85))
                            .cursor(CursorStyle::PointingHand)
                            .child(crate::icons::lucide_color("play", EXIT_PLAY, theme::LABEL))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.restart_terminal(window, cx);
                                }),
                            ),
                    )
                    .when_some(chip, |d, chip| d.child(chip)),
            )
        })
        .into_any_element()
}

fn exit_chip(island: &Island) -> Option<impl IntoElement> {
    if island.shell_running {
        return None;
    }
    let code = island.shell_exit?;
    let ok = code == 0;
    Some(
        div()
            .px(px(EXIT_CHIP_PAD_X))
            .h(px(EXIT_CHIP_H))
            .rounded(px(EXIT_CHIP_RADIUS))
            .bg(if ok {
                gpui::Rgba {
                    a: 0.2,
                    ..theme::SUCCESS
                }
            } else {
                gpui::Rgba {
                    a: 0.2,
                    ..theme::DESTRUCTIVE
                }
            })
            .flex()
            .items_center()
            .child(
                div()
                    .text_size(px(theme::SUBHEADLINE.size))
                    .line_height(px(theme::SUBHEADLINE.leading))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if ok {
                        theme::SUCCESS
                    } else {
                        theme::DESTRUCTIVE
                    })
                    .child(format!("exit {code}")),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_empty_matches_mockup() {
        assert_eq!(START_ICON, 16.0);
        assert_eq!(START_GAP, 10.0);
        assert_eq!(START_BTN_RADIUS, 20.0);
        assert_eq!(START_BTN_PAD_X, 16.0);
        assert_eq!(START_BTN_PAD_Y, 8.0);
        assert_eq!(DEFAULT_FONT_SIZE, 11.0);
        assert_eq!(DEFAULT_LINE_HEIGHT, 14.0);
        assert_eq!(
            terminal_pane_min_height(),
            theme::NOOK_BODY - theme::EXPANDED_PAD
        );
        assert!((CURSOR_ALPHA - 0x8C as f32 / 255.0).abs() < f32::EPSILON);
    }

    #[test]
    fn exited_header_matches_mockup() {
        assert_eq!(EXIT_BTN, 28.0);
        assert_eq!(EXIT_BTN_RADIUS, 14.0);
        assert_eq!(EXIT_PLAY, 16.0);
        assert_eq!(EXIT_CHIP_H, 20.0);
        assert_eq!(EXIT_CHIP_RADIUS, 10.0);
        assert_eq!(EXIT_CHIP_PAD_X, 8.0);
        assert_eq!(EXIT_HEADER_GAP, 8.0);
        assert_eq!(TERMINAL_HEADER_H, 20.0);
    }

    #[test]
    fn prompt_palette_matches_mockup() {
        assert_eq!(map_ansi_fg([0x57, 0xc7, 0xff]), [0x0a, 0x84, 0xff]);
        assert_eq!(map_ansi_fg([0x7f, 0xd7, 0xff]), [0x0a, 0x84, 0xff]);
        assert_eq!(map_ansi_fg([0x5a, 0xf7, 0x8e]), [0x30, 0xd1, 0x58]);
        assert_eq!(map_ansi_fg([0x7d, 0xff, 0xa6]), [0x30, 0xd1, 0x58]);
        assert_eq!(map_ansi_fg([0xf1, 0xf1, 0xf0]), [0xf1, 0xf1, 0xf0]);
    }
}
