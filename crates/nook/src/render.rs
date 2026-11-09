//! Mods' element trees as Rune elements. Keys are namespaced per mod (`m/<mod>/<id>`): Rune's
//! keys are one namespace, and a mod's ids shouldn't collide with another's or the island's.

use crate::fields::Fields;
use crate::images::Images;
use crate::island::Island;
use crate::theme::Theme;
use nook_mods::Mods;
use nook_mods::node::{Dir, Enter, Kind, Len, Node, Place, Style};
use rune::tok::{Family, TextStyle};
use rune::{Align, Color, Cursor, El, LoopSpec, ScrollSpec, Spring, SurfaceFit, TextEdit, Transition, col, edit_access, el, icon, row, text, text_input};
use std::path::PathBuf;
use serde_json::json;
use std::path::Path;

pub struct Rcx<'a> {
    pub name: &'a str,
    pub dir: &'a Path,
    pub theme: &'a Theme,
    pub images: Option<&'a Images>,
    pub mods: Option<&'a Mods>,
    pub fields: Option<&'a Fields>,
    /// Asks for the keyboard: a field has just been focused.
    pub request_keys: Option<&'a dyn Fn()>,
    /// Scroll viewports this render drew, for the island to route swipes to.
    pub scrollers: Option<&'a std::cell::RefCell<Vec<Scroller>>>,
}

/// A mod's scroll viewport as drawn: its key, its content's (`<key>#content`), and what the
/// island needs to scroll it.
#[derive(Clone, Debug, PartialEq)]
pub struct Scroller {
    pub key: String,
    pub stick_bottom: bool,
    /// The viewport's own top and bottom padding, which scrolls with the content.
    pub pad: f32,
}

impl Scroller {
    pub fn content_key(&self) -> String {
        format!("{}#content", self.key)
    }
}

const DEFAULT_ICON: f32 = 16.0;

pub fn node(n: &Node, r: &Rcx) -> El {
    let s = &n.s;
    let mut e = match n.t {
        Kind::Box if s.scroll => scroller(n, s, r),
        Kind::Box => {
            let mut b = if s.dir == Some(Dir::Row) { row() } else { col() };
            for c in &n.c {
                b = b.child(node(c, r));
            }
            b
        }
        Kind::Text => text_el(n.v.as_deref().unwrap_or_default(), s, r.theme),
        Kind::Icon => icon_el(n.v.as_deref().unwrap_or_default(), s, r),
        Kind::Image => image_el(n.v.as_deref().unwrap_or_default(), s, r),
        Kind::Shader => {
            let path = shader_path(n.v.as_deref().unwrap_or_default(), r.dir);
            let mark = crate::shader::mark(r.name, &path, n.k.as_deref(), s.params.as_deref().unwrap_or_default(), s.fps.unwrap_or(30.0));
            rune::effect(mark)
        }
        Kind::Input => input_el(n, s, r),
    };
    if let Some(k) = &n.k {
        e = e.key(format!("m/{}/{k}", r.name));
    } else if n.t == Kind::Input {
        // Focus follows the key; an index path would lose it when the page changes around it.
        e = e.key(format!("m/{}/#input-{}", r.name, n.v.as_deref().unwrap_or_default()));
    }
    e = style(e, s, r.theme, n.t);
    handlers(e, n, r)
}

/// A scroll viewport over the children, laid out in a content box of their own (the box the
/// island measures to know how far it can go). The children's gap and alignment are the
/// content's.
fn scroller(n: &Node, s: &Style, r: &Rcx) -> El {
    let key = format!("m/{}/{}", r.name, n.k.as_deref().unwrap_or("scroll"));
    let mut content = if s.dir == Some(Dir::Row) { row() } else { col() }.w_full();
    if let Some(g) = s.gap {
        content = content.gap(g);
    }
    if let Some(p) = s.items {
        content = place_items(content, p);
    }
    if let Some(p) = s.justify {
        content = place_justify(content, p);
    }
    for c in &n.c {
        content = content.child(node(c, r));
    }
    let scroller = Scroller { key, stick_bottom: s.stick_bottom, pad: s.p.map_or(0.0, |p| p[0] + p[2]) };
    let content = content.key(scroller.content_key());
    if let Some(list) = r.scrollers {
        list.borrow_mut().push(scroller);
    }
    col().scroll(ScrollSpec { stick_bottom: s.stick_bottom }).scroll_edge_fade(14.0).child(content)
}

/// A mod's shader file: relative to its folder.
pub fn shader_path(src: &str, dir: &Path) -> PathBuf {
    if Path::new(src).is_absolute() { PathBuf::from(src) } else { dir.join(src.trim_start_matches("./")) }
}

fn font_of(s: &Style) -> TextStyle {
    let mut font = TextStyle::new(s.text_size.unwrap_or(13.0), s.weight.unwrap_or(400.0));
    if s.mono {
        font = font.family(Family::Mono);
    }
    if s.tnum {
        font.features = "'tnum' 1";
    }
    font
}

fn text_el(content: &str, s: &Style, theme: &Theme) -> El {
    let mut t = text(content).font(font_of(s)).color(theme.color(s.color.as_deref().unwrap_or("fg")));
    if s.truncate {
        t = t.truncate();
    }
    if let Some(n) = s.lines {
        t = t.max_lines(n);
    }
    match s.align {
        Some(Place::Center) => t.align(Align::Center),
        Some(Place::End) => t.align(Align::End),
        _ => t,
    }
}

/// A mod's text field: single line, its text in `Island::fields`. Return submits; Escape leaves
/// the field. A render tells the mod what changed since the last (Rune edits the text itself).
fn input_el(n: &Node, s: &Style, r: &Rcx) -> El {
    let id: u64 = n.v.as_deref().and_then(|v| v.parse().ok()).unwrap_or(0);
    let field = r.fields.and_then(|f| f.get(id));
    if let (Some(fields), Some(mods)) = (r.fields, r.mods) && fields.sync(id, mods) && let Some(ask) = r.request_keys {
        ask();
    }
    let blank = TextEdit::new("");
    let mut e = text_input(field.map_or(&blank, |f| &f.edit), font_of(s)).single_line()
        .color(r.theme.color(s.color.as_deref().unwrap_or("fg")))
        .selection_color(r.theme.color("#0a84ff66"));
    if let Some(p) = &s.placeholder {
        e = e.placeholder(p.clone(), r.theme.color(s.placeholder_color.as_deref().unwrap_or("faint")));
    }
    let Some(mods) = r.mods.filter(|_| field.is_some()) else { return e };
    let (mods, name) = (mods.clone(), r.name.to_owned());
    e.on_edit(edit_access(move |s: &mut Island| s.fields.edit_mut(id)))
        .on_submit(move |_: &mut Island, _| mods.input_event(&name, id, "submit", json!({})))
        .on_input_key(|s: &mut Island, k: &rune::KeyEvent, tx| {
            if k.key != rune::Key::Escape {
                return false;
            }
            tx.blur();
            s.field_left(tx);
            true
        })
}

fn size_of(len: Option<Len>, default: f32) -> f32 {
    match len {
        Some(Len::Pt(v)) => v,
        _ => default,
    }
}

fn icon_el(name: &str, s: &Style, r: &Rcx) -> El {
    let size = size_of(s.w, DEFAULT_ICON).min(size_of(s.h, DEFAULT_ICON));
    match crate::icons::get(name, r.dir) {
        Some(i) => icon(i, size).color(r.theme.color(s.color.as_deref().unwrap_or("fg"))),
        // An unknown icon shows as a magenta square, so it's noticed.
        None => el().size(size, size).radius(3.0).fill(Color { r: 1.0, g: 0.0, b: 1.0, a: 0.8 }),
    }
}

fn image_el(src: &str, s: &Style, r: &Rcx) -> El {
    let size = size_of(s.w, 32.0).max(size_of(s.h, 32.0));
    let base = el().fill(r.theme.color("raised"));
    match r.images.and_then(|i| i.get(src, r.dir, size)) {
        Some(img) => base.pixels_fit(img.pixels(), SurfaceFit::Fill),
        None => base,
    }
}

fn place_items(e: El, p: Place) -> El {
    match p {
        Place::Start => e.items_start(),
        Place::Center => e.items_center(),
        Place::End => e.items_end(),
        Place::Between => e,
    }
}

fn place_justify(e: El, p: Place) -> El {
    match p {
        Place::Start => e.justify_start(),
        Place::Center => e.justify_center(),
        Place::End => e.justify_end(),
        Place::Between => e.justify_between(),
    }
}

fn style(mut e: El, s: &Style, theme: &Theme, kind: Kind) -> El {
    // Icons take their size from icon(); everything else from w/h.
    if kind != Kind::Icon {
        e = match s.w {
            Some(Len::Pt(v)) => e.w(v),
            Some(Len::Named(_)) => e.w_full(),
            None => e,
        };
        e = match s.h {
            Some(Len::Pt(v)) => e.h(v),
            Some(Len::Named(_)) => e.h_full(),
            None => e,
        };
    }
    if let Some(v) = s.min_w {
        e = e.min_w(v);
    }
    if let Some(v) = s.max_w {
        e = e.max_w(v);
    }
    if let Some(v) = s.min_h {
        e = e.min_h(v);
    }
    if let Some(v) = s.max_h {
        e = e.max_h(v);
    }
    if let Some(g) = s.grow {
        e = e.grow(g);
    }
    if s.shrink.is_some_and(|v| v > 0.0) {
        e = e.shrink();
    }
    if let Some([t, r, b, l]) = s.p {
        e = e.pt(t).pr(r).pb(b).pl(l);
    }
    if let Some(g) = s.gap {
        e = e.gap(g);
    }
    if let Some(p) = s.items {
        e = place_items(e, p);
    }
    if let Some(p) = s.justify {
        e = place_justify(e, p);
    }
    if let Some(c) = &s.bg {
        e = e.fill(theme.color(c));
    }
    if let Some((top, bottom)) = &s.gradient {
        e = e.gradient(theme.color(top), theme.color(bottom));
    }
    let radius = s.rounded.map(|r| r.min(999.0));
    if let Some(r) = radius {
        e = e.radius(r);
    }
    if let Some((w, c)) = &s.border {
        e = e.border(*w, theme.color(c));
    }
    if let Some(o) = s.opacity {
        e = e.opacity(o.clamp(0.0, 1.0));
    }
    if let Some((y, blur, c)) = &s.shadow {
        e = e.shadow(*y, *blur, theme.color(c));
    }
    if s.clip {
        e = e.clip_rounded(radius.unwrap_or(0.0));
    }
    if let Some(c) = &s.hover_bg {
        e = e.hover(theme.color(c));
    }
    if s.cursor.as_deref() == Some("pointer") {
        e = e.cursor(Cursor::Pointer);
    }
    if let Some((d, b)) = s.spring {
        e = e.motion(Spring::new(d.clamp(0.05, 3.0), b.clamp(-0.9, 0.9)));
    }
    if let Some(kind) = s.enter {
        e = e.transition(match kind {
            Enter::Fade => Transition::opacity(),
            Enter::Scale => Transition::scale(0.85),
            Enter::Blur => Transition::blur_replace(),
            Enter::Slide => Transition::slide(),
        });
    }
    if let Some(period) = s.pulse {
        e = e.looping(LoopSpec::opacity(period.max(0.2) as f64, 0.0, 0.3, 1.0));
    }
    if let Some(period) = s.spin {
        e = e.looping(LoopSpec::rotation(period.max(0.2) as f64));
    }
    if let Some(k) = s.scale {
        e = e.scale(k);
    }
    if let Some((x, y)) = s.offset {
        e = e.offset(x, y);
    }
    if let Some([l, t, r, b]) = s.pin {
        e = e.pin(l, t, r, b);
    }
    e
}

fn handlers(mut e: El, n: &Node, r: &Rcx) -> El {
    let Some(mods) = r.mods else { return e };
    if let Some(id) = n.on.click {
        let (mods, name) = (mods.clone(), r.name.to_owned());
        e = e.on_press(move |_: &mut Island| mods.dispatch(&name, id, json!({})));
    }
    if let Some(id) = n.on.hover {
        let (mods, name) = (mods.clone(), r.name.to_owned());
        e = e.on_hover_change(move |_: &mut Island, hovered, _| mods.dispatch(&name, id, json!(hovered)));
    }
    if let Some(id) = n.on.key {
        let (mods, name) = (mods.clone(), r.name.to_owned());
        // The mod answers on its own thread, too late to say whether it took the key: it never
        // does, and the key goes on as if unheard.
        e = e.on_key(move |_: &mut Island, k: &rune::KeyEvent, _| {
            mods.dispatch(&name, id, crate::fields::key_json(k));
            false
        });
    }
    e
}
