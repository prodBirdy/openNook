//! Unified search card: single-line query field + Spotlight / clipboard results.

use super::Island;
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, size, App, Bounds, ClipboardItem, Context, CursorStyle,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font,
    FontFeatures, FontStyle, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, SharedString, TextAlign, UTF16Selection, Window,
};
use nook_core::clipboard::{ClipboardKind, ClipboardRecord};
use nook_core::spotlight::{self, SearchHit};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

pub(super) const SEARCH_WIDTH: f32 = 520.0;
pub(super) const SEARCH_QUERY_H: f32 = 36.0;
pub(super) const SEARCH_ROW_H: f32 = 34.0;
pub(super) const SEARCH_MAX_ROWS: usize = 8;
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 20.0;

#[derive(Clone)]
pub(crate) enum SearchResult {
    Spotlight(SearchHit),
    Clipboard(ClipboardRecord),
}

impl SearchResult {
    fn title(&self) -> &str {
        match self {
            Self::Spotlight(hit) => &hit.display_name,
            Self::Clipboard(item) => &item.text,
        }
    }

    fn subtitle(&self) -> String {
        match self {
            Self::Spotlight(hit) => {
                if hit.is_app {
                    "Application".into()
                } else {
                    hit.path.clone()
                }
            }
            Self::Clipboard(item) => match item.kind {
                ClipboardKind::Image => "Clipboard image".into(),
                ClipboardKind::File => "Clipboard file".into(),
                ClipboardKind::Text => item
                    .app_bundle_id
                    .clone()
                    .unwrap_or_else(|| "Clipboard".into()),
            },
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Self::Spotlight(hit) if hit.is_app => "layout-grid",
            Self::Spotlight(_) => "files",
            Self::Clipboard(item) => match item.kind {
                ClipboardKind::Image => "image",
                ClipboardKind::File => "files",
                ClipboardKind::Text => "notebook",
            },
        }
    }
}

pub(crate) enum SearchEditorEvent {
    QueryChanged(String),
    Submit,
    Move(isize),
    ToggleClipboard,
    Dismiss,
}

impl EventEmitter<SearchEditorEvent> for SearchEditor {}

pub(crate) struct SearchEditor {
    text: String,
    anchor: usize,
    head: usize,
    marked_range: Option<Range<usize>>,
    focus: FocusHandle,
    bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    selecting: bool,
}

impl SearchEditor {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            anchor: 0,
            head: 0,
            marked_range: None,
            focus: cx.focus_handle(),
            bounds: Rc::new(RefCell::new(None)),
            selecting: false,
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

    fn set_caret(&mut self, i: usize) {
        let i = self.snap(i);
        self.anchor = i;
        self.head = i;
    }

    fn splice(&mut self, cx: &mut Context<Self>, range: Range<usize>, insert: &str) {
        let insert = insert.replace(['\n', '\r'], "");
        self.text.replace_range(range.clone(), &insert);
        let i = range.start + insert.len();
        self.anchor = i;
        self.head = i;
        self.marked_range = None;
        cx.emit(SearchEditorEvent::QueryChanged(self.text.clone()));
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let m = &event.keystroke.modifiers;
        let cmd = m.platform || m.secondary();
        let sel = self.selection();
        match key {
            "backspace" => {
                let range = if sel.is_empty() {
                    prev_boundary(&self.text, sel.start)..sel.end
                } else {
                    sel.clone()
                };
                self.splice(cx, range, "");
            }
            "delete" => {
                let range = if sel.is_empty() {
                    sel.start..next_boundary(&self.text, sel.end)
                } else {
                    sel.clone()
                };
                self.splice(cx, range, "");
            }
            "left" => {
                let to = if cmd {
                    0
                } else {
                    prev_boundary(&self.text, self.head)
                };
                if m.shift {
                    self.head = to;
                } else {
                    self.set_caret(to);
                }
            }
            "right" => {
                let to = if cmd {
                    self.text.len()
                } else {
                    next_boundary(&self.text, self.head)
                };
                if m.shift {
                    self.head = to;
                } else {
                    self.set_caret(to);
                }
            }
            "home" => {
                if m.shift {
                    self.head = 0;
                } else {
                    self.set_caret(0);
                }
            }
            "end" => {
                if m.shift {
                    self.head = self.text.len();
                } else {
                    self.set_caret(self.text.len());
                }
            }
            "up" => cx.emit(SearchEditorEvent::Move(-1)),
            "down" => cx.emit(SearchEditorEvent::Move(1)),
            "enter" => cx.emit(SearchEditorEvent::Submit),
            "tab" => cx.emit(SearchEditorEvent::ToggleClipboard),
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
            "escape" => {
                window.blur();
                cx.emit(SearchEditorEvent::Dismiss);
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Focusable for SearchEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for SearchEditor {
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
        let range = replacement_range
            .map(|r| utf16_to_utf8(&self.text, r.start)..utf16_to_utf8(&self.text, r.end))
            .or_else(|| {
                self.marked_range
                    .clone()
                    .map(|r| utf16_to_utf8(&self.text, r.start)..utf16_to_utf8(&self.text, r.end))
            })
            .unwrap_or_else(|| self.selection());
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
        let range = range_utf16
            .map(|r| utf16_to_utf8(&self.text, r.start)..utf16_to_utf8(&self.text, r.end))
            .unwrap_or_else(|| self.selection());
        let start = range.start;
        self.splice(cx, range, new_text);
        let inserted = new_text.replace(['\n', '\r'], "").len();
        self.marked_range = Some(utf8_to_utf16(&self.text, start)..utf8_to_utf16(&self.text, start + inserted));
        if let Some(sel) = new_selected_range {
            let base = utf8_to_utf16(&self.text, start);
            self.anchor = utf16_to_utf8(&self.text, base + sel.start);
            self.head = utf16_to_utf8(&self.text, base + sel.end);
        }
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(Bounds {
            origin: element_bounds.origin,
            size: size(px(1.), px(LINE_HEIGHT)),
        })
    }

    fn character_index_for_point(
        &mut self,
        _p: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(utf8_to_utf16(&self.text, self.head))
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

impl Render for SearchEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let focus = self.focus.clone();
        let bounds_cell = self.bounds.clone();
        let text = self.text.clone();
        let head = self.head;
        let sel = self.selection();
        div()
            .id("search-editor")
            .w_full()
            .h(px(SEARCH_QUERY_H))
            .flex()
            .items_center()
            .cursor(CursorStyle::IBeam)
            .track_focus(&focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.selecting = true;
                    window.focus(&this.focus.clone());
                    window.activate_window();
                    crate::platform::activate_app();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|_this, _: &MouseMoveEvent, _, _| {}))
            .on_mouse_up(
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
                        paint_query(bounds, &entity, &focus, &text, head, sel.clone(), window, cx);
                    },
                )
                .w_full()
                .h(px(SEARCH_QUERY_H)),
            )
    }
}

fn paint_query(
    bounds: Bounds<Pixels>,
    entity: &Entity<SearchEditor>,
    focus: &FocusHandle,
    text: &str,
    head: usize,
    sel: Range<usize>,
    window: &mut Window,
    cx: &mut App,
) {
    let display = if text.is_empty() {
        "Search apps, files, clipboard…"
    } else {
        text
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
    if !text.is_empty() && sel.end > sel.start {
        let origin = bounds.origin;
        window.paint_quad(
            gpui::fill(
                Bounds {
                    origin,
                    size: size(px(8.), px(LINE_HEIGHT)),
                },
                theme::FILL_SECONDARY,
            )
            .corner_radii(px(2.)),
        );
        let _ = sel;
    }
    if let Ok(lines) = window.text_system().shape_text(
        display.to_string().into(),
        px(FONT_SIZE),
        &[run],
        Some(bounds.size.width),
        None,
    ) {
        if let Some(line) = lines.first() {
            let origin = point(bounds.origin.x, bounds.origin.y + px(8.));
            let _ = line.paint(origin, px(LINE_HEIGHT), TextAlign::Left, None, window, cx);
            if focus.is_focused(window) && !text.is_empty() {
                if let Some(caret) = line.position_for_index(head.min(text.len()), px(LINE_HEIGHT)) {
                    window.paint_quad(gpui::fill(
                        Bounds {
                            origin: point(origin.x + caret.x, origin.y),
                            size: size(px(1.5), px(LINE_HEIGHT)),
                        },
                        theme::LABEL,
                    ));
                }
            }
        }
    }
    window.handle_input(focus, ElementInputHandler::new(bounds, entity.clone()), cx);
}

impl Island {
    pub(crate) fn search_body_height(&self) -> f32 {
        let rows = self.search_results.len().min(SEARCH_MAX_ROWS).max(1) as f32;
        theme::EXPANDED_PAD + SEARCH_QUERY_H + 8.0 + rows * SEARCH_ROW_H + theme::EXPANDED_PAD
    }

    pub(crate) fn open_search(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if !self.settings.search.enabled {
            return;
        }
        if self.search_open {
            self.close_search(cx);
            return;
        }
        crate::platform::remember_frontmost();
        self.search_open = true;
        self.expanded = true;
        self.search_query.clear();
        self.search_results.clear();
        self.search_selected = 0;
        self.search_clipboard_only = false;
        let editor = cx.new(SearchEditor::new);
        self.search_sub = Some(cx.subscribe(&editor, |this, editor, event, cx| {
            this.on_search_event(editor, event, cx);
        }));
        if let Some(window) = window {
            window.focus(&editor.focus_handle(cx));
            window.activate_window();
        }
        crate::platform::activate_app();
        crate::platform::make_island_key();
        self.search_editor = Some(editor);
        self.refresh_search_results(cx);
        cx.notify();
    }

    pub(crate) fn close_search(&mut self, cx: &mut Context<Self>) {
        if !self.search_open && self.search_editor.is_none() {
            return;
        }
        self.search_open = false;
        self.search_sub = None;
        self.search_editor = None;
        self.search_results.clear();
        self.search_query.clear();
        if self.expanded && !self.hovered && !self.notes_editing && !self.mirror_on {
            self.expanded = false;
        }
        crate::platform::restore_frontmost();
        cx.notify();
    }

    fn on_search_event(
        &mut self,
        _editor: Entity<SearchEditor>,
        event: &SearchEditorEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SearchEditorEvent::QueryChanged(q) => {
                self.search_query = q.clone();
                let parsed = spotlight::parse_search_query(q);
                if parsed.clipboard_only {
                    self.search_clipboard_only = true;
                }
                self.schedule_search(cx);
            }
            SearchEditorEvent::Submit => self.activate_selected(cx),
            SearchEditorEvent::Move(delta) => {
                if self.search_results.is_empty() {
                    return;
                }
                let len = self.search_results.len() as isize;
                let next = (self.search_selected as isize + delta).rem_euclid(len);
                self.search_selected = next as usize;
                cx.notify();
            }
            SearchEditorEvent::ToggleClipboard => {
                self.search_clipboard_only = !self.search_clipboard_only;
                self.schedule_search(cx);
            }
            SearchEditorEvent::Dismiss => self.close_search(cx),
        }
    }

    fn schedule_search(&mut self, cx: &mut Context<Self>) {
        self.search_gen = self.search_gen.wrapping_add(1);
        let gen = self.search_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(spotlight::DEBOUNCE_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.search_gen != gen || !this.search_open {
                    return;
                }
                this.refresh_search_results(cx);
            });
        })
        .detach();
    }

    fn refresh_search_results(&mut self, cx: &mut Context<Self>) {
        let parsed = spotlight::parse_search_query(&self.search_query);
        let clipboard_only = self.search_clipboard_only || parsed.clipboard_only;
        let term = parsed.term;
        let history_on = self.settings.search.clipboard_history;
        let clips = if history_on {
            nook_core::clipboard::search(&term, 20)
        } else {
            Vec::new()
        };
        if clipboard_only {
            self.search_results = clips.into_iter().map(SearchResult::Clipboard).collect();
            self.search_selected = 0;
            cx.notify();
            return;
        }
        self.search_loading = true;
        let gen = spotlight::cancel_prior_query();
        let term_q = term.clone();
        cx.spawn(async move |this, cx| {
            let hits = cx
                .background_executor()
                .spawn(async move { spotlight::query(&term_q, gen) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.search_open || spotlight::current_query_gen() != gen {
                    return;
                }
                this.search_loading = false;
                let mut merged = Vec::new();
                for hit in hits {
                    merged.push(SearchResult::Spotlight(hit));
                }
                for clip in clips {
                    merged.push(SearchResult::Clipboard(clip));
                }
                this.search_results = merged;
                this.search_selected = 0;
                cx.notify();
            });
        })
        .detach();
    }

    fn activate_selected(&mut self, cx: &mut Context<Self>) {
        let Some(result) = self.search_results.get(self.search_selected).cloned() else {
            return;
        };
        match result {
            SearchResult::Spotlight(hit) => {
                crate::platform::launch_path(&hit.path);
                self.close_search(cx);
            }
            SearchResult::Clipboard(item) => {
                nook_core::clipboard::write_text(&item.text);
                let auto = self.settings.search.auto_paste && crate::platform::accessibility_trusted();
                self.close_search(cx);
                if auto {
                    let _ = crate::platform::auto_paste_cmd_v();
                }
            }
        }
    }

    pub(super) fn render_search(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.search_editor.clone();
        let selected = self.search_selected;
        let clipboard_only = self.search_clipboard_only;
        let results = self.search_results.clone();
        let empty = results.is_empty();
        div()
            .flex()
            .flex_col()
            .size_full()
            .px(px(theme::EXPANDED_PAD))
            .pb(px(theme::EXPANDED_PAD))
            .gap(px(8.))
            .child(
                div()
                    .w_full()
                    .h(px(SEARCH_QUERY_H))
                    .px(px(10.))
                    .rounded(px(theme::INNER_RADIUS))
                    .bg(theme::FILL)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(lucide_color("search", 16.0, theme::SECONDARY_LABEL))
                    .child(if let Some(editor) = editor {
                        editor.into_any_element()
                    } else {
                        div().flex_1().into_any_element()
                    })
                    .when(clipboard_only, |d| {
                        d.child(
                            div()
                                .px(px(6.))
                                .py(px(2.))
                                .rounded(px(6.))
                                .bg(theme::FILL_SECONDARY)
                                .child(
                                    div()
                                        .text_size(px(theme::FOOTNOTE.size))
                                        .text_color(theme::SECONDARY_LABEL)
                                        .child("Clipboard"),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(if empty {
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(px(theme::SUBHEADLINE.size))
                                    .text_color(theme::TERTIARY_LABEL)
                                    .child(if self.search_loading {
                                        "Searching…"
                                    } else if clipboard_only {
                                        "No clipboard matches"
                                    } else if self.search_query.trim().is_empty() {
                                        "Type to search · ; for clipboard"
                                    } else {
                                        "No results"
                                    }),
                            )
                            .into_any_element()
                    } else {
                        let mut list = div().flex().flex_col().w_full();
                        for (i, result) in results.iter().take(SEARCH_MAX_ROWS).enumerate() {
                            list = list.child(self.search_row(i, result, i == selected, cx));
                        }
                        list.into_any_element()
                    }),
            )
    }

    fn search_row(
        &self,
        index: usize,
        result: &SearchResult,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let title = SharedString::from(truncate(result.title(), 48));
        let subtitle = SharedString::from(truncate(&result.subtitle(), 56));
        let icon = result.icon();
        div()
            .id(SharedString::from(format!("search-row-{index}")))
            .w_full()
            .h(px(SEARCH_ROW_H))
            .px(px(8.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor(CursorStyle::PointingHand)
            .when(selected, |d| d.bg(theme::FILL_SECONDARY))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.search_selected = index;
                    this.activate_selected(cx);
                }),
            )
            .child(lucide_color(icon, 16.0, theme::SECONDARY_LABEL))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(theme::BODY.size))
                            .text_color(theme::LABEL)
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(theme::FOOTNOTE.size))
                            .text_color(theme::TERTIARY_LABEL)
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(subtitle),
                    ),
            )
    }
}

fn truncate(s: &str, max: usize) -> String {
    let mut chars = s.chars();
    let take: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{take}…")
    } else {
        take
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

fn prev_boundary(text: &str, i: usize) -> usize {
    if i == 0 {
        return 0;
    }
    let mut j = i - 1;
    while j > 0 && !text.is_char_boundary(j) {
        j -= 1;
    }
    j
}

fn next_boundary(text: &str, i: usize) -> usize {
    if i >= text.len() {
        return text.len();
    }
    let mut j = i + 1;
    while j < text.len() && !text.is_char_boundary(j) {
        j += 1;
    }
    j
}
