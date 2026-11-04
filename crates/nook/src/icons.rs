//! Icons by name: the shipped set (lucide, and agents' logos as `agent-<name>`) and SVG files
//! from a mod's folder. Rune draws a subset of SVG whose paint attributes live on each element,
//! so a root `<svg>`'s stroke, fill and caps are copied onto its children.

use rune::Icon;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

pub fn names() -> impl Iterator<Item = &'static str> {
    ICON_FILES.iter().map(|(n, _)| *n)
}

/// A mod's own picture ("./icon.png"): drawn in colour as it is, like an app's icon, where
/// an icon file ("./x.svg") is a glyph tinted to the theme.
pub fn is_picture(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (name.starts_with("./") || name.starts_with('/')) && [".png", ".jpg", ".jpeg", ".webp"].iter().any(|e| lower.ends_with(e))
}

/// Icons stay for the process: a bounded set (shipped ones and the files mods name).
static CACHE: OnceLock<Mutex<HashMap<String, Option<&'static Icon>>>> = OnceLock::new();
const MAX_FILE_ICONS: usize = 512;

/// A shipped icon, or "./x.svg" relative to `dir`. None when it doesn't exist.
pub fn get(name: &str, dir: &Path) -> Option<&'static Icon> {
    let key = if name.starts_with("./") || name.starts_with('/') { dir.join(name).to_string_lossy().into_owned() } else { name.to_owned() };
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap().get(&key) {
        return *hit;
    }
    let svg = if name.starts_with("./") || name.starts_with('/') {
        if cache.lock().unwrap().len() > MAX_FILE_ICONS + ICON_FILES.len() {
            return None;
        }
        std::fs::read_to_string(&key).ok()
    } else {
        ICON_FILES.iter().find(|(n, _)| *n == name).map(|(_, s)| s.to_string())
    };
    let icon = svg.and_then(|s| convert(&s)).map(|(view, body)| {
        &*Box::leak(Box::new(Icon { name: Box::leak(key.clone().into_boxed_str()), view, body: Box::leak(body.into_boxed_str()) }))
    });
    cache.lock().unwrap().insert(key, icon);
    icon
}

const PAINT: [&str; 6] = ["fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "fill-rule"];
const SHAPES: [&str; 7] = ["path", "circle", "rect", "line", "ellipse", "polyline", "polygon"];

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!(" {name}=\"");
    let i = tag.find(&needle)? + needle.len();
    Some(&tag[i..i + tag[i..].find('"')?])
}

/// `(viewBox size, body)` from a whole SVG document.
fn convert(svg: &str) -> Option<(f32, String)> {
    let start = svg.find("<svg")?;
    let open_end = start + svg[start..].find('>')?;
    let root = svg[start..open_end].replace('\n', " ");
    let view = attr(&root, "viewBox")
        .and_then(|v| {
            let n: Vec<f32> = v.split([' ', ',']).filter_map(|x| x.parse().ok()).collect();
            (n.len() == 4).then(|| n[2].max(n[3]))
        })
        .or_else(|| attr(&root, "width").and_then(|w| w.trim_end_matches("px").parse().ok()))
        .unwrap_or(24.0);
    let close = svg.rfind("</svg>").unwrap_or(svg.len());
    let inner = &svg[open_end + 1..close];
    let inherited: Vec<(&str, &str)> = PAINT.iter().filter_map(|a| Some((*a, attr(&root, a)?))).collect();
    // Polylines and polygons become paths; rune draws path, rect, circle, ellipse and line.
    let mut body = String::new();
    let mut rest = inner;
    // Only shapes: text between tags (a <title>) would run into the next tag, and rune splits
    // the body on '>'.
    while let Some(i) = rest.find('<') {
        rest = &rest[i..];
        let end = rest.find('>').map_or(rest.len(), |e| e + 1);
        let tag = &rest[..end];
        let name = tag.trim_start_matches('<').split(|c: char| c.is_whitespace() || c == '/' || c == '>').next().unwrap_or("");
        if SHAPES.contains(&name) {
            let mut t = tag.trim_end_matches('>').trim_end_matches('/').to_owned();
            if let Some(points) = attr(tag, "points").filter(|_| matches!(name, "polyline" | "polygon")) {
                let close = if name == "polygon" { "Z" } else { "" };
                t = format!("<path d=\"M{points}{close}\"");
            }
            for (a, v) in &inherited {
                if attr(&t, a).is_none() {
                    t.push_str(&format!(" {a}=\"{v}\""));
                }
            }
            body.push_str(&t);
            body.push_str("/>");
        }
        rest = &rest[end..];
    }
    (!body.is_empty()).then_some((view, body))
}

#[cfg(test)]
mod tests {
    #[test]
    fn root_paint_moves_onto_children() {
        let (view, body) = super::convert(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="2"><path d="M20 6 9 17l-5-5"/><polyline points="1 2 3 4"/></svg>"##).unwrap();
        assert_eq!(view, 24.0);
        assert_eq!(body, r##"<path d="M20 6 9 17l-5-5" fill="none" stroke="#fff" stroke-width="2"/><path d="M1 2 3 4" fill="none" stroke="#fff" stroke-width="2"/>"##);
    }

    #[test]
    fn every_shipped_icon_converts() {
        for (name, svg) in super::ICON_FILES {
            let (_, body) = super::convert(svg).unwrap_or_else(|| panic!("{name}"));
            assert!(!body.is_empty(), "{name} has no shapes");
        }
    }
}

