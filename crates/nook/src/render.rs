//! Mods' element trees as Rune elements. Keys are namespaced per mod (`m/<mod>/<id>`): Rune's
//! keys are one namespace, and a mod's ids shouldn't collide with another's or the island's.

use crate::images::Images;
use crate::island::Island;
use crate::theme::Theme;
use nook_mods::Mods;
use nook_mods::node::{Dir, Enter, Kind, Len, Node, Place, Style};
use rune::tok::{Family, TextStyle};
use rune::{Align, Color, Cursor, El, LoopSpec, Spring, SurfaceFit, Transition, col, el, icon, row, text};
use std::path::PathBuf;
use serde_json::json;
use std::path::Path;

pub struct Rcx<'a> {
    pub name: &'a str,
    pub dir: &'a Path,
    pub theme: &'a Theme,
    pub images: Option<&'a Images>,
    pub mods: Option<&'a Mods>,
}

const DEFAULT_ICON: f32 = 16.0;

pub fn node(n: &Node, r: &Rcx) -> El {
    let s = &n.s;
    let mut e = match n.t {
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
    };
    if let Some(k) = &n.k {
        e = e.key(format!("m/{}/{k}", r.name));
    }
    e = style(e, s, r.theme, n.t);
    handlers(e, n, r)
}

/// A mod's shader file: relative to its folder.
pub fn shader_path(src: &str, dir: &Path) -> PathBuf {
    if Path::new(src).is_absolute() { PathBuf::from(src) } else { dir.join(src.trim_start_matches("./")) }
}

fn text_el(content: &str, s: &Style, theme: &Theme) -> El {
    let mut font = TextStyle::new(s.text_size.unwrap_or(13.0), s.weight.unwrap_or(400.0));
    if s.mono {
        font = font.family(Family::Mono);
    }
    if s.tnum {
        font.features = "'tnum' 1";
    }
    let mut t = text(content).font(font).color(theme.color(s.color.as_deref().unwrap_or("fg")));
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
    e
}
