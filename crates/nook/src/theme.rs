use gpui::{hsla, rgb, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Hsla, Rgba};

/// Export UI face (`export.html`: `font-[Inter,system-ui,sans-serif]`).
pub const UI_FONT: &str = "Inter";
/// Export mono face (`Roboto Mono`) — timers, terminal fallback.
pub const MONO_FONT: &str = "Roboto Mono";

pub fn ui_font(weight: FontWeight) -> Font {
    Font {
        family: UI_FONT.into(),
        features: FontFeatures::default(),
        fallbacks: Some(FontFallbacks::from_fonts(vec![
            "system-ui".into(),
            "SF Pro".into(),
            ".AppleSystemUIFont".into(),
        ])),
        weight,
        style: FontStyle::Normal,
    }
}

pub fn mono_font(weight: FontWeight) -> Font {
    Font {
        family: MONO_FONT.into(),
        features: FontFeatures::default(),
        fallbacks: Some(FontFallbacks::from_fonts(vec![
            "JetBrains Mono".into(),
            "SF Mono".into(),
            "Menlo".into(),
            "DejaVu Sans Mono".into(),
        ])),
        weight,
        style: FontStyle::Normal,
    }
}

/// Opaque island fill. Live Activities compact/expanded presentations use a
/// black background; we keep that role without cloning Apple chrome.
pub const ISLAND: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// Painted fallback when native `NSGlassEffectView` / HUD vibrancy is not
/// available. More opaque than a demo glass so labels stay legible.
pub const ISLAND_GLASS: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.82,
};

pub fn rgba_from_u32(rgb: u32, a: f32) -> Rgba {
    Rgba {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a,
    }
}

/// Solid island fill from Settings, or the default black.
pub fn island_fill(color: Option<u32>) -> Rgba {
    match color {
        Some(rgb) => rgba_from_u32(rgb, 1.0),
        None => ISLAND,
    }
}

/// Glass fallback fill from Settings. Same hue, 82% opaque like `ISLAND_GLASS`.
pub fn island_fill_glass(color: Option<u32>) -> Rgba {
    match color {
        Some(rgb) => rgba_from_u32(rgb, 0.82),
        None => ISLAND_GLASS,
    }
}

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

pub const SEPARATOR: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.22,
};
#[allow(dead_code)]
pub const HAIRLINE: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.05,
};
#[allow(dead_code)]
pub const SCRIM: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.60,
};
pub const DISABLED_OPACITY: f32 = 0.45;

/// Dark-mode systemBlue / systemRed / systemGreen / systemYellow reference
/// values (HIG Color: documented numbers are design-time only).
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

/// Replace only the alpha channel of `c`.
pub fn with_alpha(c: Rgba, a: f32) -> Rgba {
    Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a,
    }
}

/// Secondary label; bumped when Accessibility › Increase Contrast is on.
pub fn secondary_label() -> Rgba {
    if crate::platform::increase_contrast() {
        with_alpha(SECONDARY_LABEL, 0.85)
    } else {
        SECONDARY_LABEL
    }
}

/// Tertiary label; bumped when Accessibility › Increase Contrast is on.
pub fn tertiary_label() -> Rgba {
    if crate::platform::increase_contrast() {
        with_alpha(TERTIARY_LABEL, 0.55)
    } else {
        TERTIARY_LABEL
    }
}
pub const DESTRUCTIVE: Rgba = Rgba {
    r: 1.0,
    g: 0.271,
    b: 0.227,
    a: 1.0,
};
/// Dark-mode systemOrange for low-battery (not yet critical) compact faces.
pub const SYSTEM_ORANGE: Rgba = Rgba {
    r: 1.0,
    g: 0.584,
    b: 0.0,
    a: 1.0,
};
/// Dark-mode systemYellow — display brightness and Low Power Mode.
pub const SYSTEM_YELLOW: Rgba = Rgba {
    r: 1.0,
    g: 0.839,
    b: 0.039,
    a: 1.0,
};
/// systemOrange — muted meeting (Zoom verified).
pub const WARNING: Rgba = Rgba {
    r: 1.0,
    g: 0.624,
    b: 0.039,
    a: 1.0,
};
/// Dark-mode systemGreen — charging battery.
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

/// Idle compact wraps the camera housing by this much so the hardware sits
/// inside the island instead of sitting on the painted edge.
pub const IDLE_NOTCH_OVERFLOW: f32 = 1.0;
/// Extra height on every compact rest state so the bottom rim clears the
/// housing after anti-aliasing.
pub const COMPACT_HEIGHT_OVERFLOW: f32 = 1.0;
/// Bottom-corner radius of the compact island. The camera housing is a
/// rounded rect, not a capsule — half-height rounding ate the 1px wrap.
pub const COMPACT_RADIUS: f32 = 14.0;
pub const EXPANDED_RADIUS: f32 = 36.0;
/// Extra space on each side of the hardware notch in Liquid Glass mode so
/// compact content (album art, visualizer) does not sit against the camera.
/// Painted mode hides the notch inside the pill and must stay at 0.
pub const GLASS_NOTCH_GAP: f32 = 14.0;
/// Compact Live Activity width from the 0923 export (`w-[318px]`).
pub const COMPACT_LIVE_W: f32 = 318.0;
/// Expanded tab row: `p-[14px_20px_10px_20px]` + 36px tab = 60.
/// `188 − 128` against the export island (`h-[188px]` / `h-[128px]` row).
pub const EXPANDED_TAB_H: f32 = 60.0;
pub const EXPANDED_TAB_PAD_X: f32 = 20.0;
pub const EXPANDED_TAB_PAD_TOP: f32 = 14.0;
pub const EXPANDED_TAB_PAD_BOTTOM: f32 = 10.0;
/// Selected Nook/Tray/Terminal chip (`bg-[#FFFFFF29]`).
pub const TAB_ACTIVE: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.16,
};
pub const TAB_RADIUS: f32 = 12.0;
pub const TAB_PAD_X: f32 = 12.0;
pub const TAB_PAD_Y: f32 = 6.0;
pub const TAB_GAP: f32 = 6.0;
/// Compact album chip (`w-[22px]`, `rounded-[6px]`, `#FFFFFF1F` hairline).
pub const COMPACT_ART_BORDER: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.12,
};
/// Resting compact waveform (`#FF7A4D`).
pub const COMPACT_WAVE: Rgba = Rgba {
    r: 1.0,
    g: 0.478,
    b: 0.302,
    a: 1.0,
};
/// Tray / drop accent (`#0A84FF`).
pub const DROP_ACCENT: Rgba = ACCENT;
pub const DROP_FILL: Rgba = Rgba {
    r: 0.039,
    g: 0.518,
    b: 1.0,
    a: 0.10,
};
pub const DROP_RADIUS: f32 = 14.0;
#[allow(dead_code)]
pub const WIDGET_RADIUS: f32 = 28.0;
pub const INNER_RADIUS: f32 = 10.0;
pub const CONTROL_RADIUS: f32 = 8.0;
pub const CONTENT_INSET: f32 = 12.0;
/// React expanded pane `p-5` (files tab still uses this).
pub const EXPANDED_PAD: f32 = 20.0;
/// Nook / Tray body row from the export (`h-[128px]`, pad lives inside).
pub const NOOK_BODY: f32 = 128.0;
/// Expanded body inset (`p-[0px_20px_20px_20px]`).
pub const NOOK_INSET: f32 = 20.0;
/// Nominal minimum cell; the grid uses `Island::nook_cell_width()`.
pub const NOOK_CELL: f32 = 56.0;
/// Export Nook row gap (`gap-[20px]`).
pub const NOOK_DIVIDER: f32 = 20.0;
/// Vertical gap between Nook rows: 1 px rule + CONTENT_INSET margin each side.
pub const NOOK_ROW_GAP: f32 = 25.0;
/// Expanded island width from the 0923 export (`w-[780px]`).
pub const EXPANDED_MAX_WIDTH: f32 = 780.0;
/// Bottom widget picker strip while editing the Nook row (icons + labels + actions).
pub const WIDGET_EDIT_PICKER_H: f32 = 78.0;
pub const NOTCH_MIN_H: f32 = 32.0;
pub const SCREEN_MARGIN: f32 = 40.0;
pub const LOCKUP_MAX_WIDTH: f32 = 420.0;
pub const RECORDER_BODY: f32 = 260.0;
/// Compact hover chin / flank extras.
pub const COMPACT_HOVER_EXTRA: f32 = 88.0;
pub const COMPACT_HUD_EXTRA: f32 = 120.0;
pub const COMPACT_LIVE_EXTRA: f32 = 72.0;
pub const COMPACT_HOVER_CHIN: f32 = 11.0;
#[allow(dead_code)]
pub const WIDGET_PAD: f32 = 16.0;
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
pub const FOOTNOTE: Text = Text {
    size: 10.0,
    leading: 13.0,
    weight: FontWeight::NORMAL,
    emphasized: FontWeight::SEMIBOLD,
};
#[allow(dead_code)]
pub const DISPLAY: Text = Text {
    size: 32.0,
    leading: 36.0,
    weight: FontWeight::BOLD,
    emphasized: FontWeight::BOLD,
};

/// Compact Live Activity face — lucide glyphs, HUD marks, avatars, file thumbs.
/// Export album art is 22×22.
pub const COMPACT_FACE: f32 = 22.0;
/// Small inline badge next to a compact face (e.g. High Alert sun).
pub const COMPACT_BADGE: f32 = 12.0;
/// Small inline glyph next to text.
#[allow(dead_code)]
pub const GLYPH_SM: f32 = 16.0;
pub const TRACK_H: f32 = 4.0;
pub const TRACK_RADIUS: f32 = 2.0;
/// Inset from the compact capsule edge (`p-[0px_9px]`).
pub const COMPACT_INSET: f32 = 9.0;
/// Expanded Nook Mirror circle (`w-[100px]`).
pub const MIRROR_FACE: f32 = 100.0;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_0923_chrome_tokens() {
        assert_eq!(COMPACT_LIVE_W, 318.0);
        assert_eq!(COMPACT_RADIUS, 14.0);
        assert_eq!(COMPACT_INSET, 9.0);
        assert_eq!(COMPACT_FACE, 22.0);
        assert_eq!(EXPANDED_MAX_WIDTH, 780.0);
        assert_eq!(EXPANDED_RADIUS, 36.0);
        assert_eq!(EXPANDED_TAB_H, 60.0);
        assert_eq!(NOOK_BODY, 128.0);
        assert_eq!(EXPANDED_TAB_H + NOOK_BODY, 188.0);
        assert_eq!(NOOK_INSET, 20.0);
        assert_eq!(TAB_RADIUS, 12.0);
        assert_eq!(MIRROR_FACE, 100.0);
        assert_eq!(UI_FONT, "Inter");
        assert_eq!(MONO_FONT, "Roboto Mono");
    }
}
