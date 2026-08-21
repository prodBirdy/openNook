use gpui::{hsla, rgb, FontWeight, Hsla, Rgba};

/// Opaque island fill. Live Activities compact/expanded presentations use a
/// black background; we keep that role without cloning Apple chrome.
pub const ISLAND: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// Regular-material fallback when people turn on translucency. More opaque
/// than a demo glass so labels stay legible over wallpaper.
pub const ISLAND_GLASS: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.82,
};

/// Semantic dark-overlay roles (HIG Color: label / fill / separator).
pub const LABEL: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
pub const SECONDARY_LABEL: Rgba = Rgba {
    r: 0.92,
    g: 0.92,
    b: 0.96,
    a: 0.60,
};
pub const TERTIARY_LABEL: Rgba = Rgba {
    r: 0.92,
    g: 0.92,
    b: 0.96,
    a: 0.30,
};
pub const TEXT: Rgba = LABEL;
pub const TEXT_MUTED: Rgba = SECONDARY_LABEL;
#[allow(dead_code)]
pub const TEXT_FAINT: Rgba = TERTIARY_LABEL;

pub const FILL: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.10,
};
pub const FILL_SECONDARY: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.16,
};
pub const FILL_TERTIARY: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.08,
};
#[allow(dead_code)]
pub const SURFACE: Rgba = FILL;
#[allow(dead_code)]
pub const SURFACE_HOVER: Rgba = FILL_SECONDARY;

#[allow(dead_code)]
pub const SEPARATOR: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.22,
};
#[allow(dead_code)]
pub const DIVIDER: Rgba = SEPARATOR;

/// Dark-mode systemBlue / systemRed / systemGreen reference values.
pub const ACCENT: Rgba = Rgba {
    r: 0.039,
    g: 0.518,
    b: 1.0,
    a: 1.0,
};
pub const ACCENT_FALLBACK: Rgba = ACCENT;

/// The accent the user picked in System Settings → Appearance, falling back to
/// systemBlue off-macOS or when AppKit reports a color we cannot read.
pub fn accent() -> Rgba {
    match crate::platform::accent_color() {
        Some((r, g, b)) => Rgba { r, g, b, a: 1.0 },
        None => ACCENT_FALLBACK,
    }
}
pub const DESTRUCTIVE: Rgba = Rgba {
    r: 1.0,
    g: 0.271,
    b: 0.227,
    a: 1.0,
};
#[allow(dead_code)]
pub const SUCCESS: Rgba = Rgba {
    r: 0.188,
    g: 0.820,
    b: 0.345,
    a: 1.0,
};

pub const WINDOW_BG: Rgba = Rgba {
    r: 0.110,
    g: 0.110,
    b: 0.118,
    a: 1.0,
};
/// Settings window fill. Slightly transparent so macOS `Blurred` chrome reads
/// as dark glass; opaque enough that Linux (no vibrancy) stays legible.
pub const SETTINGS_GLASS: Rgba = Rgba {
    r: 0.110,
    g: 0.110,
    b: 0.118,
    a: 0.86,
};
pub const GROUPED_BG: Rgba = Rgba {
    r: 0.173,
    g: 0.173,
    b: 0.180,
    a: 1.0,
};
/// Module list well — a touch darker than grouped rows.
pub const SETTINGS_WELL: Rgba = Rgba {
    r: 0.086,
    g: 0.086,
    b: 0.094,
    a: 0.92,
};

/// Compact island matches the hardware notch; expanded uses a larger bottom
/// radius so inner cards can sit concentrically (outer − inset).
pub const COMPACT_RADIUS: f32 = 18.0;
pub const EXPANDED_RADIUS: f32 = 36.0;
/// React `WidgetWrapper`: `rounded-[28px]`.
pub const WIDGET_RADIUS: f32 = 28.0;
pub const INNER_RADIUS: f32 = 10.0;
pub const CONTROL_RADIUS: f32 = 8.0;
pub const CONTENT_INSET: f32 = 12.0;
/// React expanded pane `p-5` (files tab still uses this).
pub const EXPANDED_PAD: f32 = 20.0;
/// Nook tab body: one row under the notch, matching the capsule layout.
pub const NOOK_BODY: f32 = 112.0;
pub const NOOK_INSET: f32 = 16.0;
pub const EXPANDED_MAX_WIDTH: f32 = 780.0;
/// React widgets row `gap-4`.
pub const WIDGET_GAP: f32 = 16.0;
/// React `WidgetWrapper` padding (`1rem`).
pub const WIDGET_PAD: f32 = 16.0;
/// How far a row highlight bleeds back out of the card's content margin. Also
/// the concentric gap that sets the row's own corner radius.
pub const ROW_INSET: f32 = 6.0;
/// React widget row chips: `rounded-[20px]`.
pub const ROW_RADIUS: f32 = 20.0;

/// A macOS built-in text style: point size, line height, and the two weights
/// the platform pairs with it.
#[derive(Clone, Copy)]
pub struct Text {
    pub size: f32,
    pub leading: f32,
    pub weight: FontWeight,
    /// The weight to use when the text carries the emphasis in its group.
    pub emphasized: FontWeight,
}

/// macOS built-in text styles, verbatim from HIG › Typography › Specifications
/// › "macOS built-in text styles". macOS has no Dynamic Type, so the sizes are
/// fixed; the platform default is 13 pt and the legible minimum is 10 pt, so
/// nothing here goes below Footnote.
pub const TITLE_2: Text = Text {
    size: 17.0,
    leading: 22.0,
    weight: FontWeight::NORMAL,
    emphasized: FontWeight::BOLD,
};
pub const TITLE_3: Text = Text {
    size: 15.0,
    leading: 20.0,
    weight: FontWeight::NORMAL,
    emphasized: FontWeight::SEMIBOLD,
};
pub const BODY: Text = Text {
    size: 13.0,
    leading: 16.0,
    weight: FontWeight::NORMAL,
    emphasized: FontWeight::SEMIBOLD,
};
pub const CALLOUT: Text = Text {
    size: 12.0,
    leading: 15.0,
    weight: FontWeight::NORMAL,
    emphasized: FontWeight::SEMIBOLD,
};
pub const SUBHEADLINE: Text = Text {
    size: 11.0,
    leading: 14.0,
    weight: FontWeight::NORMAL,
    emphasized: FontWeight::SEMIBOLD,
};
#[allow(dead_code)]
pub const FOOTNOTE: Text = Text {
    size: 10.0,
    leading: 13.0,
    weight: FontWeight::NORMAL,
    emphasized: FontWeight::SEMIBOLD,
};

/// Compact Live Activity face — album chip, mode icons, timer ring, loader.
pub const COMPACT_FACE: f32 = 26.0;

/// HIG › Accessibility › Buttons gives macOS a 28×28 pt recommended hit target
/// (20×20 pt minimum). Interactive rows and controls hold this floor even when
/// their visible artwork is smaller.
pub const HIT_MIN: f32 = 28.0;

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
