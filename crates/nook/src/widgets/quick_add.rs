//! One-line natural-language event / reminder field.
//!
//! GPUI 0.2 has no stock single-line input, so this lifts the
//! `EntityInputHandler` + `Window::handle_input` pattern from
//! [`super::notes_editor`]. Parse runs on each keystroke — no debounce
//! timer, no polling.

use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, size, App, Bounds, ClipboardItem, Context, CursorStyle,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font,
    FontFeatures, FontStyle, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point,
    Render, SharedString, TextAlign, UTF16Selection, Window, WrappedLine,
};
use nook_core::nl_parse::{self, EntryKind, ParsedEntry};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const FONT_SIZE: f32 = 12.0;
const LINE_HEIGHT: f32 = 16.0;

pub(crate) enum QuickAddEvent {
    Saved,
}

impl EventEmitter<QuickAddEvent> for QuickAdd {}

pub(crate) struct QuickAdd {
    text: String,
    anchor: usize,
    head: usize,
    marked_range: Option<Range<usize>>,
    focus: FocusHandle,
    layout: Rc<RefCell<Option<ShapedLine>>>,
    bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    default_kind: EntryKind,
    placeholder: SharedString,
    parsed: Option<ParsedEntry>,
    confirmed: Option<String>,
    saving: bool,
}

struct ShapedLine {
    line: Option<WrappedLine>,
    text: String,
}

impl QuickAdd {
    pub(crate) fn new(
        default_kind: EntryKind,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            text: String::new(),
            anchor: 0,
            head: 0,
            marked_range: None,
            focus: cx.focus_handle(),
            layout: Rc::new(RefCell::new(None)),
            bounds: Rc::new(RefCell::new(None)),
            default_kind,
            placeholder: placeholder.into(),
            parsed: None,
            confirmed: None,
            saving: false,
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

    fn reparse(&mut self) {
        self.confirmed = None;
        self.parsed = nl_parse::parse_as(&self.text, chrono::Local::now(), self.default_kind);
    }

    fn splice(&mut self, cx: &mut Context<Self>, range: Range<usize>, insertion: &str) {
        let start = self.snap(range.start);
        let end = self.snap(range.end).max(start);
        let normalized = insertion
            .replace('\n', " ")
            .replace('\r', " ")
            .chars()
            .filter(|c| *c != '\u{2028}' && *c != '\u{2029}')
            .collect::<String>();
        self.text.replace_range(start..end, &normalized);
        let caret = start + normalized.len();
        self.anchor = caret;
        self.head = caret;
        self.marked_range = None;
        self.reparse();
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.text.clear();
        self.anchor = 0;
        self.head = 0;
        self.marked_range = None;
        self.parsed = None;
        cx.notify();
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(entry) = self.parsed.clone() else {
            return;
        };
        self.saving = true;
        let title = entry.title.clone();
        cx.spawn(async move |this, cx| {
            let ok = cx
                .background_executor()
                .spawn(async move { save_entry(entry).await })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                if ok {
                    this.confirmed = Some(format!("Added · {title}"));
                    this.text.clear();
                    this.anchor = 0;
                    this.head = 0;
                    this.marked_range = None;
                    this.parsed = None;
                    cx.emit(QuickAddEvent::Saved);
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(Duration::from_secs(2))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            this.confirmed = None;
                            cx.notify();
                        });
                    })
                    .detach();
                }
                cx.notify();
            });
        })
        .detach();
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
                    self.prev_boundary(sel.start)..sel.end
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
            "left" => self.move_head(self.prev_boundary(self.head), m.shift),
            "right" => self.move_head(self.next_boundary(self.head), m.shift),
            "home" => self.move_head(0, m.shift),
            "end" => self.move_head(self.text.len(), m.shift),
            "enter" => self.commit(cx),
            "escape" => {
                self.clear(cx);
                window.blur();
            }
            "a" if cmd => {
                self.anchor = 0;
                self.head = self.text.len();
            }
            "c" if cmd => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.text[sel.clone()].to_string()));
            }
            "x" if cmd => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.text[sel.clone()].to_string()));
                self.splice(cx, sel, "");
            }
            "v" if cmd => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.splice(cx, sel, &text);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

async fn save_entry(entry: ParsedEntry) -> bool {
    let result = match entry.kind {
        EntryKind::Event => {
            nook_core::calendar::create_event(
                entry.title,
                entry.start,
                entry.end,
                entry.all_day,
                entry.location,
            )
            .await
        }
        EntryKind::Reminder => {
            nook_core::calendar::create_reminder(entry.title, Some(entry.start)).await
        }
    };
    match result {
        Ok(ok) => ok,
        Err(err) => {
            log::warn!("quick add failed: {err}");
            false
        }
    }
}

impl Focusable for QuickAdd {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn utf8_to_utf16(text: &str, utf8: usize) -> usize {
    text[..utf8.min(text.len())].encode_utf16().count()
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

impl EntityInputHandler for QuickAdd {
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
        Some(self.text.get(start..end)?.to_string())
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
        self.splice(cx, range, new_text);
        let inserted = new_text.replace('\n', " ").replace('\r', " ");
        self.marked_range = Some(
            utf8_to_utf16(&self.text, start)..utf8_to_utf16(&self.text, start + inserted.len()),
        );
        if let Some(sel) = new_selected_range {
            let base = utf8_to_utf16(&self.text, start);
            self.anchor = utf16_to_utf8(&self.text, base + sel.start);
            self.head = utf16_to_utf8(&self.text, base + sel.end);
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
        let x = shaped
            .as_ref()
            .and_then(|s| s.line.as_ref())
            .and_then(|line| line.position_for_index(idx, px(LINE_HEIGHT)))
            .map(|p| p.x)
            .unwrap_or(px(0.));
        Some(Bounds {
            origin: point(element_bounds.origin.x + x, element_bounds.origin.y),
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
        let idx = shaped
            .as_ref()
            .and_then(|s| s.line.as_ref())
            .and_then(|line| line.closest_index_for_position(local, px(LINE_HEIGHT)).ok())
            .unwrap_or(self.text.len());
        Some(utf8_to_utf16(&self.text, self.snap(idx)))
    }
}

fn editor_font() -> Font {
    Font {
        family: "SF Pro".into(),
        features: FontFeatures(Arc::new(Vec::new())),
        fallbacks: None,
        weight: FontWeight::MEDIUM,
        style: FontStyle::Normal,
    }
}

fn paint_field(
    bounds: Bounds<Pixels>,
    entity: &Entity<QuickAdd>,
    focus: &FocusHandle,
    layout_cell: &Rc<RefCell<Option<ShapedLine>>>,
    window: &mut Window,
    cx: &mut App,
) {
    let (text, sel, focused, placeholder) = {
        let editor = entity.read(cx);
        (
            editor.text.clone(),
            editor.selection(),
            focus.is_focused(window),
            editor.placeholder.clone(),
        )
    };

    let display = if text.is_empty() {
        placeholder.to_string()
    } else {
        text.clone()
    };
    let color = if text.is_empty() {
        theme::TEXT_MUTED
    } else {
        theme::TEXT
    };
    let run = gpui::TextRun {
        len: display.len(),
        font: editor_font(),
        color: color.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window
        .text_system()
        .shape_text(display.into(), px(FONT_SIZE), &[run], None, None)
        .ok()
        .and_then(|lines| lines.into_iter().next());

    if focused && !text.is_empty() {
        if let Some(line) = shaped.as_ref() {
            if let (Some(a), Some(b)) = (
                line.position_for_index(sel.start, px(LINE_HEIGHT)),
                line.position_for_index(sel.end, px(LINE_HEIGHT)),
            ) {
                let x0: f32 = a.x.into();
                let x1: f32 = b.x.into();
                let highlight = Bounds {
                    origin: point(bounds.origin.x + a.x, bounds.origin.y),
                    size: size(px((x1 - x0).max(1.5)), px(LINE_HEIGHT)),
                };
                window.paint_quad(gpui::fill(highlight, theme::FILL_SECONDARY).corner_radii(px(2.)));
            }
            if let Some(caret) = line.position_for_index(entity.read(cx).head, px(LINE_HEIGHT)) {
                let caret_bounds = Bounds {
                    origin: point(bounds.origin.x + caret.x, bounds.origin.y),
                    size: size(px(1.5), px(LINE_HEIGHT)),
                };
                window.paint_quad(gpui::fill(caret_bounds, theme::LABEL));
            }
        }
    }

    if let Some(line) = shaped.as_ref() {
        let _ = line.paint(bounds.origin, px(LINE_HEIGHT), TextAlign::Left, None, window, cx);
    }

    *layout_cell.borrow_mut() = Some(ShapedLine {
        line: shaped,
        text,
    });
    window.handle_input(focus, ElementInputHandler::new(bounds, entity.clone()), cx);
}

impl Render for QuickAdd {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let focus = self.focus.clone();
        let layout_cell = self.layout.clone();
        let bounds_cell = self.bounds.clone();
        let chip = self
            .confirmed
            .clone()
            .or_else(|| self.parsed.as_ref().map(|e| format!("→ {}", e.preview_label())));

        div()
            .id("quick-add")
            .w_full()
            .flex()
            .flex_col()
            .gap(px(4.))
            .flex_shrink_0()
            .child(
                div()
                    .id("quick-add-field")
                    .w_full()
                    .h(px(22.))
                    .px(px(8.))
                    .rounded(px(8.))
                    .bg(theme::FILL)
                    .flex()
                    .items_center()
                    .cursor(CursorStyle::IBeam)
                    .track_focus(&focus)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.set_caret(this.text.len());
                            window.focus(&this.focus.clone());
                            window.activate_window();
                            crate::platform::activate_app();
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(Self::handle_key))
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                *bounds_cell.borrow_mut() = Some(bounds);
                            },
                            move |bounds, _, window, cx| {
                                paint_field(bounds, &entity, &focus, &layout_cell, window, cx);
                            },
                        )
                        .w_full()
                        .h(px(LINE_HEIGHT)),
                    ),
            )
            .when_some(chip, |d, text| {
                d.child(
                    div()
                        .id("quick-add-chip")
                        .w_full()
                        .px(px(8.))
                        .text_size(px(10.))
                        .line_height(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if self.confirmed.is_some() {
                            theme::accent()
                        } else {
                            theme::SECONDARY_LABEL
                        })
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(text),
                )
            })
    }
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
}
