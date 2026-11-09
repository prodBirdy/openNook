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
    /// A text field over an `InputState`; `v` is the state's id.
    Input,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Handlers {
    #[serde(default)]
    pub click: Option<u64>,
    #[serde(default)]
    pub hover: Option<u64>,
    /// Keys while this element, or a text field inside it, has the keyboard.
    #[serde(default)]
    pub key: Option<u64>,
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
    /// A vertical scroll viewport over the children (needs a bounded height and an id).
    #[serde(default)]
    pub scroll: bool,
    /// Scroll viewports: open at the bottom and follow it as content grows (a chat).
    #[serde(default)]
    pub stick_bottom: bool,
    /// Text fields: shown while empty.
    pub placeholder: Option<String>,
    pub placeholder_color: Option<String>,
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

/// How the island itself looks: layout.json's `island.style`, and a mod's `style()` over it
/// while that mod owns the island. Colors are theme tokens or hex; unset fields fall through.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct IslandStyle {
    /// The body's color (default: the `island` token).
    pub fill: Option<String>,
    /// Top and bottom colors, in place of `fill`.
    pub gradient: Option<(String, String)>,
    pub border: Option<Stroke>,
    /// A soft light around the island.
    pub glow: Option<Glow>,
    /// Bottom corners of the closed island (default 14) and the open one (default 36).
    pub compact_radius: Option<f32>,
    pub expanded_radius: Option<f32>,
    /// The concave fillets joining the flat top to the screen edge (default 14, 0 for none).
    pub wings: Option<f32>,
    /// The open island's drop shadow (default true).
    pub shadow: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub width: f32,
    pub color: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Glow {
    pub color: String,
    #[serde(default = "Glow::default_radius")]
    pub radius: f32,
}

impl Glow {
    fn default_radius() -> f32 {
        18.0
    }
}

impl IslandStyle {
    /// This style with `base` filling what it leaves unset.
    pub fn over(&self, base: &IslandStyle) -> IslandStyle {
        IslandStyle {
            // A fill replaces the base's gradient and a gradient the base's fill.
            fill: self.fill.clone().or_else(|| if self.gradient.is_some() { None } else { base.fill.clone() }),
            gradient: self.gradient.clone().or_else(|| if self.fill.is_some() { None } else { base.gradient.clone() }),
            border: self.border.clone().or_else(|| base.border.clone()),
            glow: self.glow.clone().or_else(|| base.glow.clone()),
            compact_radius: self.compact_radius.or(base.compact_radius),
            expanded_radius: self.expanded_radius.or(base.expanded_radius),
            wings: self.wings.or(base.wings),
            shadow: self.shadow.or(base.shadow),
        }
    }
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
