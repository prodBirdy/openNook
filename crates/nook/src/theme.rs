use gpui::{hsla, rgb, Hsla, Rgba};

pub const ISLAND: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
pub const ISLAND_GLASS: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.65,
};
pub const TEXT: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
pub const TEXT_MUTED: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.6,
};
pub const TEXT_FAINT: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.4,
};
pub const SURFACE: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.10,
};
pub const SURFACE_HOVER: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.20,
};
pub const DIVIDER: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.08,
};
pub const ACCENT_FALLBACK: Rgba = Rgba {
    r: 0.04,
    g: 0.52,
    b: 1.0,
    a: 1.0,
};

pub const COMPACT_RADIUS: f32 = 18.0;
pub const EXPANDED_RADIUS: f32 = 18.0;
pub const WIDGET_RADIUS: f32 = 16.0;
pub const INNER_RADIUS: f32 = 12.0;

pub fn parse_hex(hex: &str) -> Hsla {
    let hex = hex.trim_start_matches('#');
    if hex.len() >= 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        ) {
            return rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32).into();
        }
    }
    hsla(0.58, 1.0, 0.52, 1.0)
}
