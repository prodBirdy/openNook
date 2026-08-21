//! Scratchpad notes Nook pane.
//!
//! Preview renders the note as markdown (pulldown-cmark mapped to styled
//! divs); the pencil toggle swaps in [`NotesEditor`] for raw-markdown editing.
//! Inline styles flow at span granularity, so a paragraph with mixed bold /
//! italic runs wraps between spans rather than mid-span.

use crate::island::ui::{nook_empty, nook_icon_btn, nook_pane, scroll_body};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, relative, Context, CursorStyle, FontWeight, MouseButton, MouseDownEvent,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

const BODY_SIZE: f32 = 12.0;
const MONO_FAMILY: &str = "SF Mono";

pub(crate) fn notes_card(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    let editing = island.notes_editing;
    let toggle = nook_icon_btn(
        if editing { "eye" } else { "pencil" },
        "notes-toggle",
        cx,
        |this, _, window, cx| {
            if this.notes_editing {
                this.close_notes_editor(cx);
            } else {
                this.begin_notes_edit(window, cx);
            }
        },
    );
    let body = if editing {
        let editor = island
            .notes_editor
            .clone()
            .expect("notes editor exists while editing");
        div()
            .w_full()
            .min_h(relative(1.))
            .flex()
            .flex_col()
            .child(editor)
            .into_any_element()
    } else {
        preview_body(island.notes.trim().is_empty(), island.notes.clone(), cx).into_any_element()
    };
    nook_pane("nook-notes")
        .relative()
        .w_full()
        .child(scroll_body("notes-scroll", body))
        .child(div().absolute().top(px(0.)).right(px(0.)).child(toggle))
}

fn preview_body(empty: bool, notes: String, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("notes-body")
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.begin_notes_edit(window, cx);
            }),
        )
        .when(empty, |d| {
            d.child(nook_empty("notebook", "Click to add notes"))
        })
        .when(!empty, |d| d.child(markdown_preview(&notes)))
}

/// One inline run with its active style flags.
struct Span {
    text: String,
    strong: bool,
    em: bool,
    strike: bool,
    code: bool,
    link: bool,
}

#[derive(Default, Clone, Copy)]
struct Flags {
    strong: bool,
    em: bool,
    strike: bool,
    link: bool,
}

impl Span {
    fn text(text: String, flags: Flags) -> Self {
        Self {
            text,
            strong: flags.strong,
            em: flags.em,
            strike: flags.strike,
            code: false,
            link: flags.link,
        }
    }

    fn code(text: String) -> Self {
        Self {
            text,
            strong: false,
            em: false,
            strike: false,
            code: true,
            link: false,
        }
    }

    fn div(&self) -> gpui::Div {
        let mut d = div();
        if self.code {
            d = d
                .font_family(MONO_FAMILY)
                .text_size(px(BODY_SIZE - 1.0))
                .px_1()
                .rounded(px(3.))
                .bg(theme::FILL_TERTIARY)
                .text_color(theme::TEXT_MUTED);
        } else {
            d = d.text_size(px(BODY_SIZE));
            if self.link {
                d = d.text_color(theme::accent());
            }
            if self.strong {
                d = d.font_weight(FontWeight::SEMIBOLD);
            }
            if self.em {
                d = d.italic();
            }
            if self.strike {
                d = d.line_through();
            }
        }
        d.child(self.text.clone())
    }
}

/// Inline runs laid out as wrapping baseline-aligned spans.
fn inline_row(spans: &[Span], muted: bool) -> gpui::Div {
    let mut row = div().flex().flex_wrap().items_baseline().gap_x(px(4.));
    row = if muted {
        row.text_color(theme::TEXT_MUTED).italic()
    } else {
        row.text_color(theme::TEXT)
    };
    for span in spans {
        row = row.child(span.div());
    }
    row
}

fn push_inline(
    heading_open: bool,
    item_open: bool,
    span: Span,
    spans: &mut Vec<Span>,
    item_spans: &mut Vec<Span>,
    heading_text: &mut String,
) {
    if heading_open {
        // Heading text is concatenated below, so style flags are dropped.
        heading_text.push_str(&span.text);
    } else if item_open {
        item_spans.push(span);
    } else {
        spans.push(span);
    }
}

/// Rendered markdown blocks, stacked vertically.
fn markdown_preview(src: &str) -> gpui::Div {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);

    let mut col = div().flex().flex_col().gap(px(6.)).w_full();
    let mut spans: Vec<Span> = Vec::new();
    let mut flags = Flags::default();
    // 0 = bullet list, n = next number of an ordered list.
    let mut lists: Vec<u64> = Vec::new();
    let mut item_marker: Option<String> = None;
    let mut item_spans: Vec<Span> = Vec::new();
    let mut quote_depth = 0usize;
    let mut heading_level: Option<HeadingLevel> = None;
    let mut heading_text = String::new();
    let mut code = String::new();
    let mut in_code = false;

    for event in Parser::new_ext(src, opts) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading_level = Some(level);
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                let level = heading_level.take().unwrap_or(HeadingLevel::H3);
                let (size, weight) = match level {
                    HeadingLevel::H1 => (17.0, FontWeight::BOLD),
                    HeadingLevel::H2 => (15.0, FontWeight::SEMIBOLD),
                    HeadingLevel::H3 => (13.0, FontWeight::SEMIBOLD),
                    _ => (12.0, FontWeight::SEMIBOLD),
                };
                let text = std::mem::take(&mut heading_text);
                col = col.child(
                    div()
                        .text_size(px(size))
                        .line_height(px((size * 1.25).ceil()))
                        .font_weight(weight)
                        .text_color(theme::TEXT)
                        .child(text),
                );
            }
            Event::Start(Tag::Item) => {
                item_marker = Some(match lists.last_mut() {
                    Some(n @ 1..) => {
                        let label = format!("{n}.");
                        *n += 1;
                        label
                    }
                    _ => "•".into(),
                });
                item_spans.clear();
            }
            Event::End(TagEnd::Item) => {
                let marker = item_marker.take().unwrap_or_else(|| "•".into());
                let body = std::mem::take(&mut item_spans);
                col = col.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(BODY_SIZE))
                                .text_color(theme::TEXT_MUTED)
                                .min_w(px(14.))
                                .child(marker),
                        )
                        .child(inline_row(&body, false)),
                );
            }
            Event::Start(Tag::BlockQuote(_)) => quote_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => quote_depth = quote_depth.saturating_sub(1),
            Event::Start(Tag::List(start)) => lists.push(start.unwrap_or(0)),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::CodeBlock(_)) => {
                in_code = true;
                code.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                if !code.trim().is_empty() {
                    col = col.child(
                        div()
                            .w_full()
                            .p_2()
                            .rounded(px(theme::INNER_RADIUS))
                            .bg(theme::FILL_TERTIARY)
                            .font_family(MONO_FAMILY)
                            .text_size(px(BODY_SIZE - 1.0))
                            .text_color(theme::TEXT_MUTED)
                            .child(code.trim_end().to_string()),
                    );
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if item_marker.is_none() && !spans.is_empty() {
                    let taken = std::mem::take(&mut spans);
                    col = col.child(inline_row(&taken, quote_depth > 0));
                }
            }
            Event::Rule => {
                col = col.child(div().w_full().h(px(1.)).bg(theme::SEPARATOR));
            }
            Event::Text(t) if in_code => code.push_str(&t),
            Event::Text(t) => push_inline(
                heading_level.is_some(),
                item_marker.is_some(),
                Span::text(t.to_string(), flags),
                &mut spans,
                &mut item_spans,
                &mut heading_text,
            ),
            Event::Code(t) => push_inline(
                heading_level.is_some(),
                item_marker.is_some(),
                Span::code(t.to_string()),
                &mut spans,
                &mut item_spans,
                &mut heading_text,
            ),
            Event::SoftBreak | Event::HardBreak => push_inline(
                heading_level.is_some(),
                item_marker.is_some(),
                Span::text(" ".into(), flags),
                &mut spans,
                &mut item_spans,
                &mut heading_text,
            ),
            Event::Html(h) => push_inline(
                heading_level.is_some(),
                item_marker.is_some(),
                Span::text(h.to_string(), flags),
                &mut spans,
                &mut item_spans,
                &mut heading_text,
            ),
            Event::Start(Tag::Emphasis) => flags.em = true,
            Event::End(TagEnd::Emphasis) => flags.em = false,
            Event::Start(Tag::Strong) => flags.strong = true,
            Event::End(TagEnd::Strong) => flags.strong = false,
            Event::Start(Tag::Strikethrough) => flags.strike = true,
            Event::End(TagEnd::Strikethrough) => flags.strike = false,
            Event::Start(Tag::Link { .. }) => flags.link = true,
            Event::End(TagEnd::Link) => flags.link = false,
            _ => {}
        }
    }
    if !spans.is_empty() {
        let taken = std::mem::take(&mut spans);
        col = col.child(inline_row(&taken, quote_depth > 0));
    }
    col
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_inline_text_is_retained() {
        let mut spans = Vec::new();
        let mut item_spans = Vec::new();
        let mut heading = String::new();
        push_inline(
            true,
            false,
            Span::text("Heading".into(), Flags::default()),
            &mut spans,
            &mut item_spans,
            &mut heading,
        );
        assert_eq!(heading, "Heading");
    }
}
