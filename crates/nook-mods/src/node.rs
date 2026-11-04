//! The element trees mods render, as `nook/ui` serializes them. Plain data: the app turns
//! them into Rune elements.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub t: Kind,
    /// The mod's own id for the element (`.id()`), so changes animate instead of replacing it.
    #[serde(default)]
    pub k: Option<String>,
    /// Text content, icon name or image source.
    #[serde(default)]
    pub v: Option<String>,
    #[serde(default)]
    pub s: Style,
    #[serde(default)]
    pub on: Handlers,
    #[serde(default)]
    pub c: Vec<Node>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Box,
    Text,
    Icon,
    Image,
    /// A Metal fragment shader from the mod's folder, animated.
    Shader,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Handlers {
    #[serde(default)]
    pub click: Option<u64>,
    #[serde(default)]
    pub hover: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Len {
    Pt(f32),
    Named(Fill),
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Fill {
    Full,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Row,
    Col,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    Start,
    Center,
    End,
    Between,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Enter {
    Fade,
    Scale,
    Blur,
    Slide,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub dir: Option<Dir>,
    pub w: Option<Len>,
    pub h: Option<Len>,
    pub min_w: Option<f32>,
    pub max_w: Option<f32>,
    pub min_h: Option<f32>,
    pub max_h: Option<f32>,
    pub grow: Option<f32>,
    pub shrink: Option<f32>,
    /// Padding: top, right, bottom, left.
    pub p: Option<[f32; 4]>,
    pub gap: Option<f32>,
    pub items: Option<Place>,
    pub justify: Option<Place>,
    pub bg: Option<String>,
    /// Painted over `bg`, top to bottom.
    pub gradient: Option<(String, String)>,
    pub rounded: Option<f32>,
    pub border: Option<(f32, String)>,
    pub opacity: Option<f32>,
    pub shadow: Option<(f32, f32, String)>,
    #[serde(default)]
    pub clip: bool,
    pub hover_bg: Option<String>,
    pub cursor: Option<String>,
    pub text_size: Option<f32>,
    pub color: Option<String>,
    pub weight: Option<f32>,
    #[serde(default)]
    pub mono: bool,
    #[serde(default)]
    pub tnum: bool,
    #[serde(default)]
    pub truncate: bool,
    pub lines: Option<usize>,
    pub align: Option<Place>,
    pub spring: Option<(f32, f32)>,
    pub enter: Option<Enter>,
    pub pulse: Option<f32>,
    pub spin: Option<f32>,
    pub scale: Option<f32>,
    pub offset: Option<(f32, f32)>,
    /// Absolutely placed inside the parent: left, top, right, bottom insets (null = free).
    pub pin: Option<[Option<f32>; 4]>,
    /// Shader elements: up to 8 numbers the shader reads as `P(0)`…`P(7)`.
    pub params: Option<Vec<f32>>,
    /// Shader elements: frames a second (default 30; 0 draws one still frame).
    pub fps: Option<f32>,
}

/// What a mod shows in the closed island: beside the camera, left and right.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Activity {
    pub leading: Option<Node>,
    pub trailing: Option<Node>,
    /// Shown under the camera while the closed island is hovered (a title, a time).
    #[serde(default)]
    pub detail: Option<Node>,
    /// Higher wins the compact island when layout.json doesn't order the mods.
    #[serde(default)]
    pub priority: f64,
    /// Width of each side, in points; the island picks one when absent.
    #[serde(default)]
    pub width: Option<f32>,
}

impl Node {
    /// Every node in the tree, depth first (for checks and tests).
    pub fn walk(&self, f: &mut impl FnMut(&Node)) {
        f(self);
        for c in &self.c {
            c.walk(f);
        }
    }
}
