//! In-card notes editor: a hand-rolled multi-line text field on GPUI's IME
//! plumbing (`EntityInputHandler` + `Window::handle_input`), since gpui 0.2
//! ships no ready-made text area. Printable keys flow through the platform IME
//! into [`EntityInputHandler::replace_text_in_range`]; editing keys are bound
//! here. Text is markdown source, saved (debounced) to the shared notes store.

use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, relative, size, App, Bounds, ClipboardItem, Context,
    CursorStyle, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, Font, FontFeatures, FontStyle, FontWeight, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, TextAlign, UTF16Selection,
    Window, WrappedLine,
};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const FONT_SIZE: f32 = 13.0;
const LINE_HEIGHT: f32 = 18.0;
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

/// Shaped lines from the last paint, kept for hit-testing and caret math.
struct ShapedNotes {
    lines: Vec<WrappedLine>,
    /// Byte offset where each source line starts.
    line_starts: Vec<usize>,
    text: String,
}

impl ShapedNotes {
    /// y offset of each source line's top, accounting for soft wraps.
    fn line_tops(&self) -> Vec<f32> {
        let mut tops = Vec::with_capacity(self.lines.len());
        let mut y = 0.0f32;
        for line in &self.lines {
            tops.push(y);
            y += f32::from(line.size(px(LINE_HEIGHT)).height);
        }
        tops
    }

    /// Which source line holds byte index `idx`, and the index within it.
    fn line_for_index(&self, idx: usize) -> (usize, usize) {
        for (ix, start) in self.line_starts.iter().enumerate().rev() {
            if idx >= *start {
                return (ix, idx - start);
            }
        }
        (0, idx)
    }

    /// Local pixel position of a byte index.
    fn position_for_index(&self, idx: usize) -> Option<Point<Pixels>> {
        let (line_ix, local) = self.line_for_index(idx);
        let top = self.line_tops().get(line_ix).copied().unwrap_or(0.0);
        let pos = self
            .lines
            .get(line_ix)?
            .position_for_index(local, px(LINE_HEIGHT))?;
        Some(point(pos.x, pos.y + px(top)))
    }

    /// Byte index closest to a local point.
    fn index_for_position(&self, p: Point<Pixels>) -> usize {
        let tops = self.line_tops();
        let y: f32 = p.y.into();
        let mut ix = tops.len().saturating_sub(1);
        for (i, top) in tops.iter().enumerate() {
            if y < *top {
                ix = i.saturating_sub(1);
                break;
            }
        }
        let Some(line) = self.lines.get(ix) else {
            return 0;
        };
        let local_y = px(y - tops.get(ix).copied().unwrap_or(0.0));
        let in_line = line
            .closest_index_for_position(point(p.x, local_y), px(LINE_HEIGHT))
            .unwrap_or_else(|e| e);
        self.line_starts.get(ix).copied().unwrap_or(0) + in_line
    }

    /// One rect per wrapped row the range touches.
    fn rects_for_range(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let mut rects = Vec::new();
        if range.end <= range.start {
            return rects;
        }
        let mut seg: Option<(usize, f32)> = None;
        let mut prev_end = range.start;
        let mut idx = range.start;
        while idx < range.end {
            let y = self.position_for_index(idx).map(|p| f32::from(p.y));
            match (seg, y) {
                (Some((s, sy)), Some(y)) if y == sy => {}
                (Some((s, sy)), _) => {
                    push_rect(
                        &mut rects,
                        self.position_for_index(s),
                        self.position_for_index(prev_end),
                        sy,
                    );
                    seg = None;
                    continue;
                }
                (None, Some(y)) => seg = Some((idx, y)),
                (None, None) => {}
            }
            prev_end = next_char(&self.text, idx);
            idx = prev_end;
        }
        if let Some((s, sy)) = seg {
            push_rect(
                &mut rects,
                self.position_for_index(s),
                self.position_for_index(range.end),
                sy,
            );
        }
        rects
    }
}

fn push_rect(
    out: &mut Vec<Bounds<Pixels>>,
    a: Option<Point<Pixels>>,
    b: Option<Point<Pixels>>,
    y: f32,
) {
    let (Some(a), Some(b)) = (a, b) else { return };
    let x0: f32 = a.x.into();
    let x1: f32 = b.x.into();
    out.push(Bounds {
        origin: point(a.x, px(y)),
        size: size(px((x1 - x0).max(3.0)), px(LINE_HEIGHT)),
    });
}

fn next_char(text: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < text.len() && !text.is_char_boundary(j) {
        j += 1;
    }
    j.min(text.len())
}

fn utf8_to_utf16(text: &str, utf8: usize) -> usize {
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

fn resolved_replacement_range(
    text: &str,
    replacement_utf16: Option<Range<usize>>,
    marked_utf16: Option<Range<usize>>,
    selection: Range<usize>,
) -> Range<usize> {
    replacement_utf16
        .or(marked_utf16)
        .map(|r| utf16_to_utf8(text, r.start)..utf16_to_utf8(text, r.end))
        .unwrap_or(selection)
}

fn composition_selection(
    text: &str,
    marked_start_utf8: usize,
    selected_utf16: Range<usize>,
) -> Range<usize> {
    let start_utf16 = utf8_to_utf16(text, marked_start_utf8);
    utf16_to_utf8(text, start_utf16 + selected_utf16.start)
        ..utf16_to_utf8(text, start_utf16 + selected_utf16.end)
}

pub(crate) enum NotesEditorEvent {
    Dismiss,
}

impl EventEmitter<NotesEditorEvent> for NotesEditor {}

pub(crate) struct NotesEditor {
    text: String,
    anchor: usize,
    head: usize,
    marked_range: Option<Range<usize>>,
    focus: FocusHandle,
    layout: Rc<RefCell<Option<ShapedNotes>>>,
    bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    last_save: Instant,
    dirty: bool,
    save_scheduled: bool,
    selecting: bool,
    content_height: f32,
}

impl NotesEditor {
    pub(crate) fn new(text: String, cx: &mut Context<Self>) -> Self {
        let head = text.len();
        let content_height = text.lines().count().max(1) as f32 * LINE_HEIGHT;
        Self {
            text,
            anchor: head,
            head,
            marked_range: None,
            focus: cx.focus_handle(),
            layout: Rc::new(RefCell::new(None)),
            bounds: Rc::new(RefCell::new(None)),
            last_save: Instant::now(),
            dirty: false,
            save_scheduled: false,
            selecting: false,
            content_height,
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Write pending edits through to the notes store.
    pub(crate) fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.save_scheduled = false;
        self.last_save = Instant::now();
        if let Err(err) = nook_core::notes::save_notes(self.text.clone()) {
            log::warn!("save notes: {err}");
        }
    }

    fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    fn snap(&self, i: usize) -> usize {
        let mut i = i.min(self.text.len());
        while i > 0 && !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn set_caret(&mut self, at: usize) {
        let at = self.snap(at);
        self.anchor = at;
        self.head = at;
    }

    fn move_head(&mut self, to: usize, extend: bool) {
        let to = self.snap(to);
        self.head = to;
        if !extend {
            self.anchor = to;
        }
    }

    fn prev_boundary(&self, from: usize) -> usize {
        self.text[..from]
            .chars()
            .next_back()
            .map(|c| from - c.len_utf8())
            .unwrap_or(0)
    }

    fn next_boundary(&self, from: usize) -> usize {
        self.text[from..]
            .chars()
            .next()
            .map(|c| from + c.len_utf8())
            .unwrap_or(from)
    }

    fn word_at(text: &str, idx: usize) -> bool {
        text[idx..]
            .chars()
            .next()
            .map(|c| c.is_alphanumeric() || c == '_')
            .unwrap_or(false)
    }

    fn prev_word(&self, mut i: usize) -> usize {
        while i > 0 && !Self::word_at(&self.text, self.prev_boundary(i)) {
            i = self.prev_boundary(i);
        }
        while i > 0 && Self::word_at(&self.text, self.prev_boundary(i)) {
            i = self.prev_boundary(i);
        }
        i
    }

    fn next_word(&self, mut i: usize) -> usize {
        let len = self.text.len();
        while i < len && Self::word_at(&self.text, i) {
            i = self.next_boundary(i);
        }
        while i < len && !Self::word_at(&self.text, i) {
            i = self.next_boundary(i);
        }
        i
    }

    fn line_start(&self, from: usize) -> usize {
        self.text[..from].rfind('\n').map_or(0, |i| i + 1)
    }

    fn line_end(&self, from: usize) -> usize {
        self.text[from..]
            .find('\n')
            .map_or(self.text.len(), |i| from + i)
    }

    fn splice(&mut self, cx: &mut Context<Self>, range: Range<usize>, insertion: &str) {
        let start = self.snap(range.start);
        let end = self.snap(range.end).max(start);
        let normalized = insertion.replace("\r\n", "\n").replace('\r', "\n");
        self.text.replace_range(start..end, &normalized);
        let caret = start + normalized.len();
        self.anchor = caret;
        self.head = caret;
        self.marked_range = None;
        self.schedule_save(cx);
        cx.notify();
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        if self.last_save.elapsed() >= SAVE_DEBOUNCE {
            self.flush();
            return;
        }
        if self.save_scheduled {
            return;
        }
        self.save_scheduled = true;
        let wait = SAVE_DEBOUNCE.saturating_sub(self.last_save.elapsed());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |this, _| this.flush());
        })
        .detach();
    }

    fn line_move(&mut self, down: bool, extend: bool) {
        let target_x = match self.layout.borrow().as_ref() {
            Some(shaped) => shaped.position_for_index(self.head).map(|p| p.x),
            None => None,
        };
        let Some(target_x) = target_x else {
            // No layout yet: fall back to whole-line hops over the raw text.
            let to = if down {
                self.line_end(self.head).min(self.text.len())
            } else {
                self.line_start(self.head)
            };
            self.move_head(to, extend);
            return;
        };
        let probe_y = {
            let shaped = self.layout.borrow();
            let pos = shaped
                .as_ref()
                .and_then(|s| s.position_for_index(self.head));
            let y: f32 = pos.map(|p| f32::from(p.y)).unwrap_or(0.0);
            if down {
                y + LINE_HEIGHT * 1.5
            } else {
                y - LINE_HEIGHT * 0.5
            }
        };
        let idx = {
            let shaped = self.layout.borrow();
            shaped
                .as_ref()
                .map(|s| s.index_for_position(point(target_x, px(probe_y))))
                .unwrap_or(self.head)
        };
        self.move_head(idx, extend);
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let key = ks.key.as_str();
        let m = &ks.modifiers;
        let cmd = m.platform || m.control;
        let sel = self.selection();
        match key {
            "backspace" => {
                let range = if sel.is_empty() {
                    let to = if m.alt || cmd {
                        self.prev_word(sel.start)
                    } else {
                        self.prev_boundary(sel.start)
                    };
                    to..sel.end
                } else {
                    sel.clone()
                };
                self.splice(cx, range, "");
            }
            "delete" => {
                let range = if sel.is_empty() {
                    sel.start..self.next_boundary(sel.end)
                } else {
                    sel.clone()
                };
                self.splice(cx, range, "");
            }
            "left" => {
                let to = if cmd {
                    self.line_start(self.head)
                } else if m.alt {
                    self.prev_word(self.head)
                } else {
                    self.prev_boundary(self.head)
                };
                self.move_head(to, m.shift);
            }
            "right" => {
                let to = if cmd {
                    self.line_end(self.head)
                } else if m.alt {
                    self.next_word(self.head)
                } else {
                    self.next_boundary(self.head)
                };
                self.move_head(to, m.shift);
            }
            "up" => self.line_move(false, m.shift),
            "down" => self.line_move(true, m.shift),
            "home" => self.move_head(self.line_start(self.head), m.shift),
            "end" => self.move_head(self.line_end(self.head), m.shift),
            "enter" => self.splice(cx, sel, "\n"),
            "tab" => self.splice(cx, sel, "  "),
            "a" if cmd => {
                self.anchor = 0;
                self.head = self.text.len();
            }
            "c" if cmd => {
                let text = self.text[sel.clone()].to_string();
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            "x" if cmd => {
                let text = self.text[sel.clone()].to_string();
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.splice(cx, sel, "");
            }
            "v" if cmd => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.splice(cx, sel, &text);
                }
            }
            "escape" => {
                self.flush();
                window.blur();
                cx.emit(NotesEditorEvent::Dismiss);
            }
            _ => return, // printable keys fall through to the IME path
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Focusable for NotesEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Bridge to the platform IME. Ranges cross this boundary as UTF-16; the
/// editor stores UTF-8 offsets internally and converts at the edge.
impl EntityInputHandler for NotesEditor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let start = utf16_to_utf8(&self.text, range_utf16.start);
        let end = utf16_to_utf8(&self.text, range_utf16.end);
        adjusted_range.replace(utf8_to_utf16(&self.text, start)..utf8_to_utf16(&self.text, end));
        Some(self.text[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let sel = self.selection();
        Some(UTF16Selection {
            range: utf8_to_utf16(&self.text, sel.start)..utf8_to_utf16(&self.text, sel.end),
            reversed: self.head < self.anchor,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = resolved_replacement_range(
            &self.text,
            replacement_range,
            self.marked_range.clone(),
            self.selection(),
        );
        self.splice(cx, range, text);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = resolved_replacement_range(
            &self.text,
            range_utf16,
            self.marked_range.clone(),
            self.selection(),
        );
        let start = range.start;
        let inserted_len = new_text.replace("\r\n", "\n").replace('\r', "\n").len();
        self.splice(cx, range, new_text);
        self.marked_range =
            Some(utf8_to_utf16(&self.text, start)..utf8_to_utf16(&self.text, start + inserted_len));
        if let Some(sel) = new_selected_range {
            let sel = composition_selection(&self.text, start, sel);
            self.anchor = sel.start;
            self.head = sel.end;
        }
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let idx = utf16_to_utf8(&self.text, range_utf16.start);
        let shaped = self.layout.borrow();
        let pos = shaped.as_ref()?.position_for_index(idx)?;
        Some(Bounds {
            origin: element_bounds.origin + pos,
            size: size(px(1.), px(LINE_HEIGHT)),
        })
    }

    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let b = (*self.bounds.borrow())?;
        let local = point(p.x - b.origin.x, p.y - b.origin.y);
        let shaped = self.layout.borrow();
        let idx = shaped.as_ref()?.index_for_position(local);
        Some(utf8_to_utf16(&self.text, self.snap(idx)))
    }
}

fn editor_font() -> Font {
    Font {
        family: "SF Pro".into(),
        features: FontFeatures(Arc::new(Vec::new())),
        fallbacks: None,
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    }
}

fn paint_editor(
    bounds: Bounds<Pixels>,
    entity: &Entity<NotesEditor>,
    focus: &FocusHandle,
    layout_cell: &Rc<RefCell<Option<ShapedNotes>>>,
    window: &mut Window,
    cx: &mut App,
) {
    let (text, sel, marked, focused) = {
        let editor = entity.read(cx);
        (
            editor.text.clone(),
            editor.selection(),
            editor.marked_range.clone(),
            focus.is_focused(window),
        )
    };

    let run = gpui::TextRun {
        len: text.len(),
        font: editor_font(),
        color: theme::TEXT.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let wrap_width = (f32::from(bounds.size.width) > 8.0).then_some(bounds.size.width);
    let shaped_lines = window
        .text_system()
        .shape_text(text.clone().into(), px(FONT_SIZE), &[run], wrap_width, None)
        .map(|lines| lines.into_iter().collect::<Vec<_>>())
        .unwrap_or_default();

    let mut line_starts = Vec::with_capacity(shaped_lines.len());
    let mut offset = 0usize;
    for segment in text.split('\n') {
        line_starts.push(offset);
        offset += segment.len() + 1;
    }
    let shaped = ShapedNotes {
        lines: shaped_lines,
        line_starts,
        text: text.clone(),
    };

    if focused {
        for mut r in shaped.rects_for_range(sel.clone()) {
            r.origin = bounds.origin + r.origin;
            window.paint_quad(gpui::fill(r, theme::FILL_SECONDARY).corner_radii(px(2.)));
        }
        if let Some(marked) = marked {
            let m_start = utf16_to_utf8(&text, marked.start);
            let m_end = utf16_to_utf8(&text, marked.end);
            for mut r in shaped.rects_for_range(m_start..m_end) {
                r.origin = bounds.origin + r.origin;
                let underline = Bounds {
                    origin: point(r.origin.x, r.origin.y + r.size.height - px(2.)),
                    size: size(r.size.width, px(1.5)),
                };
                window.paint_quad(gpui::fill(underline, theme::accent()));
            }
        }
        if let Some(caret) = shaped.position_for_index(entity.read(cx).head) {
            let caret_bounds = Bounds {
                origin: bounds.origin + caret,
                size: size(px(1.5), px(LINE_HEIGHT)),
            };
            window.paint_quad(gpui::fill(caret_bounds, theme::LABEL));
        }
    }

    let tops = shaped.line_tops();
    if text.is_empty() {
        paint_placeholder(bounds, window, cx);
    } else {
        for (ix, line) in shaped.lines.iter().enumerate() {
            let origin = point(bounds.origin.x, bounds.origin.y + px(tops[ix]));
            let _ = line.paint(origin, px(LINE_HEIGHT), TextAlign::Left, None, window, cx);
        }
    }

    let content_height = tops.last().copied().unwrap_or(0.0)
        + shaped
            .lines
            .last()
            .map(|line| f32::from(line.size(px(LINE_HEIGHT)).height))
            .unwrap_or(LINE_HEIGHT);
    if wrap_width.is_some() {
        entity.update(cx, |editor, cx| {
            if (editor.content_height - content_height).abs() > 0.5 {
                editor.content_height = content_height;
                cx.notify();
            }
        });
    }

    *layout_cell.borrow_mut() = Some(shaped);
    window.handle_input(focus, ElementInputHandler::new(bounds, entity.clone()), cx);
}

fn paint_placeholder(bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let placeholder = "Write markdown…";
    let run = gpui::TextRun {
        len: placeholder.len(),
        font: editor_font(),
        color: theme::TEXT_MUTED.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    if let Ok(lines) = window.text_system().shape_text(
        placeholder.into(),
        px(FONT_SIZE),
        &[run],
        Some(bounds.size.width),
        None,
    ) {
        if let Some(line) = lines.first() {
            let _ = line.paint(
                bounds.origin,
                px(LINE_HEIGHT),
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }
}

impl Render for NotesEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let focus = self.focus.clone();
        let layout_cell = self.layout.clone();
        let bounds_cell = self.bounds.clone();
        let min_h = self.content_height.max(LINE_HEIGHT);
        div()
            .id("notes-editor")
            .w_full()
            .min_h(relative(1.))
            .flex()
            .flex_col()
            .cursor(CursorStyle::IBeam)
            .track_focus(&focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.selecting = true;
                    let idx = hit_index(this, event.position);
                    if event.modifiers.shift {
                        this.head = this.snap(idx);
                    } else {
                        this.set_caret(idx);
                    }
                    window.focus(&this.focus.clone());
                    // Re-key the nonactivating panel so typing works even if
                    // another window took key status since the editor opened.
                    window.activate_window();
                    crate::platform::activate_app();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if !this.selecting {
                    return;
                }
                this.head = this.snap(hit_index(this, event.position));
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.selecting = false;
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.selecting = false;
                }),
            )
            .on_key_down(cx.listener(Self::handle_key))
            .child(
                canvas(
                    move |bounds, _, _| {
                        *bounds_cell.borrow_mut() = Some(bounds);
                    },
                    move |bounds, _, window, cx| {
                        paint_editor(bounds, &entity, &focus, &layout_cell, window, cx);
                    },
                )
                .w_full()
                .flex_1()
                .min_h(px(min_h)),
            )
    }
}

fn hit_index(editor: &NotesEditor, position: Point<Pixels>) -> usize {
    let Some(bounds) = *editor.bounds.borrow() else {
        return editor.text.len();
    };
    let local = point(position.x - bounds.origin.x, position.y - bounds.origin.y);
    editor
        .layout
        .borrow()
        .as_ref()
        .map(|shaped| shaped.index_for_position(local))
        .unwrap_or(editor.text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ime_replaces_the_active_marked_range() {
        let text = "beforeかなafter";
        let marked = utf8_to_utf16(text, 6)..utf8_to_utf16(text, 12);
        assert_eq!(
            resolved_replacement_range(text, None, Some(marked), text.len()..text.len()),
            6..12
        );
    }

    #[test]
    fn ime_selection_is_relative_to_inserted_mark() {
        let text = "before漢字after";
        assert_eq!(composition_selection(text, 6, 1..2), 9..12);
    }
}
