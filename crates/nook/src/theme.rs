use gpui::{linear_color_stop, linear_gradient, Background, FontWeight, Rgba};

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

/// Black band, in points, at slider midpoint (`0.5`). That is the approved fall.
const GLASS_VEIL_BAND_AT_DEFAULT: f32 = 48.0;
const GLASS_GRADIENT_MID: f32 = 0.5;
/// The bottom edge always keeps some material, even with the slider at full.
const GLASS_VEIL_HOLD_CAP: f32 = 0.82;
/// Black alpha at the bottom edge when Increase Contrast is on.
const GLASS_VEIL_CONTRAST_FLOOR: f32 = 0.72;
/// Points past the compact pill over which the veil fades in.
const GLASS_VEIL_REVEAL_SPAN: f32 = 96.0;

/// Tallest compact pill, including the hover chin. The fall stays off at or
/// below this height.
pub fn compact_glass_ceiling(notch_h: f32) -> f32 {
    let notch = if notch_h.is_finite() {
        notch_h.max(NOTCH_MIN_H)
    } else {
        NOTCH_MIN_H
    };
    notch + COMPACT_HOVER_CHIN + COMPACT_HEIGHT_OVERFLOW
}

/// How far a full resisted stretch opens the veil. `1` finishes the fall
/// while the pill is still resisting, so the scroll itself is the transition.
pub const GLASS_PULL_REVEAL: f32 = 1.0;

/// Fraction of the stretch at which the system material is fully present.
/// The veil keeps opening after this, over a material that is already there.
const GLASS_PULL_MATERIAL_AT: f32 = 0.35;

fn pull_t(pull: f32, pull_max: f32) -> f32 {
    if !pull.is_finite() || !pull_max.is_finite() || pull <= 0.0 || pull_max <= 0.0 {
        return 0.0;
    }
    (pull / pull_max).clamp(0.0, 1.0)
}

/// Opacity of the system material during a down-swipe. It arrives early so
/// the opening veil has glass behind it instead of a faded scrim.
pub fn glass_pull_material(pull: f32, pull_max: f32) -> f32 {
    (pull_t(pull, pull_max) / GLASS_PULL_MATERIAL_AT).clamp(0.0, 1.0)
}

/// Veil open amount for the slow down-swipe. Eased so the glass is obvious
/// well before the commit, and complete at a full stretch.
pub fn glass_pull_reveal(pull: f32, pull_max: f32) -> f32 {
    let t = pull_t(pull, pull_max);
    let eased = 1.0 - (1.0 - t) * (1.0 - t);
    eased * GLASS_PULL_REVEAL
}

/// `0` on a compact pill, `1` once the sheet has opened past it.
pub fn glass_veil_reveal(height: f32, ceiling: f32) -> f32 {
    if !height.is_finite() || !ceiling.is_finite() {
        return 0.0;
    }
    ((height - ceiling) / GLASS_VEIL_REVEAL_SPAN).clamp(0.0, 1.0)
}

/// `(hold, floor)` for the black veil.
///
/// `amount` is the settings slider (`0` bare glass, `0.5` the 48pt band,
/// `1` the longest black cap). `hold` is the fraction of `height` that stays
/// opaque black. `floor` is the black alpha at the bottom edge.
pub fn glass_veil_curve(height: f32, amount: f32, increase_contrast: bool) -> (f32, f32) {
    let amount = if amount.is_finite() {
        amount.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let floor = if increase_contrast {
        GLASS_VEIL_CONTRAST_FLOOR
    } else {
        0.0
    };
    if amount <= f32::EPSILON || !height.is_finite() || height <= 1.0 {
        return (0.0, floor);
    }
    let band = GLASS_VEIL_BAND_AT_DEFAULT * (amount / GLASS_GRADIENT_MID);
    let hold = (band / height).clamp(0.0, GLASS_VEIL_HOLD_CAP);
    (hold, floor)
}

fn black_fall(hold: f32, start: Rgba, end: Rgba) -> Background {
    // 180°: first stop is the top edge, second stop is the bottom edge.
    linear_gradient(
        180.0,
        linear_color_stop(start, hold),
        linear_color_stop(end, 1.0),
    )
}

/// Black alphas `(top, bottom)` painted over the material.
///
/// Compact (`open` 0) is solid black. As the sheet opens, the bottom clears
/// so the material comes in. Slider `amount` 0 clears the whole sheet.
pub fn glass_veil_alphas(open: f32, amount: f32, floor: f32) -> (f32, f32) {
    let open = if open.is_finite() {
        open.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let amount = if amount.is_finite() {
        amount.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let floor = if floor.is_finite() {
        floor.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let end = floor + (1.0 - floor) * (1.0 - open);
    if amount <= f32::EPSILON {
        (end, end)
    } else {
        (1.0, end)
    }
}

/// Veil painted over live Liquid Glass. `open` is 0 on a compact pill and 1
/// on a fully open sheet. Solid black at 0; black through the top band, then
/// the system material, as it opens.
pub fn island_glass_veil(height: f32, open: f32, amount: f32) -> Background {
    let open = if open.is_finite() {
        open.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (hold, floor) = glass_veil_curve(height, amount, crate::platform::increase_contrast());
    let (start_a, end_a) = glass_veil_alphas(open, amount, floor);
    if (start_a - end_a).abs() < 0.004 {
        return with_alpha(ISLAND, start_a).into();
    }
    black_fall(hold, with_alpha(ISLAND, start_a), with_alpha(ISLAND, end_a))
}

/// Same fall where no system material view is behind the fill. Compact stays
/// the opaque island color so the open end never punches a hole.
pub fn island_glass_fallback_veil(
    color: Option<u32>,
    height: f32,
    open: f32,
    amount: f32,
) -> Background {
    let solid = island_fill(color);
    let glass = island_fill_glass(color);
    let open = if open.is_finite() {
        open.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (hold, floor) = glass_veil_curve(height, amount, crate::platform::increase_contrast());
    let (start_a, end_a) = glass_veil_alphas(open, amount, floor);
    let paint = |alpha: f32| {
        let t = (1.0 - alpha).clamp(0.0, 1.0);
        Rgba {
            r: solid.r + (glass.r - solid.r) * t,
            g: solid.g + (glass.g - solid.g) * t,
            b: solid.b + (glass.b - solid.b) * t,
            a: solid.a + (glass.a - solid.a) * t,
        }
    };
    if (start_a - end_a).abs() < 0.004 {
        return paint(start_a).into();
    }
    black_fall(hold, paint(start_a), paint(end_a))
}

/// Peak alpha of the Liquid Glass edge highlight at the bottom rim.
pub const GLASS_RIM_ALPHA: f32 = 0.38;
/// Fraction of the island height the rim stays dark before it fades in.
pub const GLASS_RIM_HOLD: f32 = 0.35;

/// Hairline edge highlight stroked around the Liquid Glass island: dark along
/// the notch edge, catching light toward the rounded bottom (mockup eYApc /
/// w31KY).
pub fn island_glass_rim() -> Background {
    let peak = if crate::platform::increase_contrast() {
        (GLASS_RIM_ALPHA * 1.5).min(1.0)
    } else {
        GLASS_RIM_ALPHA
    };
    linear_gradient(
        180.0,
        linear_color_stop(with_alpha(LABEL, 0.0), GLASS_RIM_HOLD),
        linear_color_stop(with_alpha(LABEL, peak), 1.0),
    )
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

pub const SEPARATOR: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.22,
};
#[allow(dead_code)]
pub const DIVIDER: Rgba = SEPARATOR;
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
/// React `WidgetWrapper`: `rounded-[28px]`.
pub const WIDGET_RADIUS: f32 = 28.0;
pub const INNER_RADIUS: f32 = 10.0;
pub const CONTROL_RADIUS: f32 = 8.0;
pub const CONTENT_INSET: f32 = 12.0;
/// React expanded pane `p-5` (files tab still uses this).
pub const EXPANDED_PAD: f32 = 20.0;
/// Nook tab body: one row under the notch, matching the capsule layout.
pub const NOOK_BODY: f32 = 128.0;
/// Mockup Nook row padding `0 20 20 20` — lines panes up with the tab bar.
pub const NOOK_INSET: f32 = 20.0;
/// Nominal minimum cell; the grid uses `Island::nook_cell_width()`.
pub const NOOK_CELL: f32 = 56.0;
/// Gap between Nook panes (mockup row `gap: 20`, no divider rule).
pub const NOOK_DIVIDER: f32 = 20.0;
/// Vertical gap between Nook rows: 1 px rule + CONTENT_INSET margin each side.
pub const NOOK_ROW_GAP: f32 = 25.0;
/// Expanded island width; the Nook row holds `TOTAL_CELLS` cells at `nook_cell_width()`.
pub const EXPANDED_MAX_WIDTH: f32 = 1120.0;
/// Bottom widget picker strip while editing the Nook row (icons + labels + actions).
pub const WIDGET_EDIT_PICKER_H: f32 = 78.0;
/// React widgets row `gap-4`.
#[allow(dead_code)]
pub const WIDGET_GAP: f32 = 16.0;
pub const NOTCH_MIN_H: f32 = 32.0;
pub const SCREEN_MARGIN: f32 = 40.0;
pub const LOCKUP_MAX_WIDTH: f32 = 420.0;
pub const RECORDER_BODY: f32 = 260.0;
/// Compact hover chin / flank extras.
pub const COMPACT_HOVER_EXTRA: f32 = 88.0;
pub const COMPACT_HUD_EXTRA: f32 = 120.0;
pub const COMPACT_LIVE_EXTRA: f32 = 72.0;
pub const COMPACT_HOVER_CHIN: f32 = 11.0;
/// React `WidgetWrapper` padding (`1rem`).
pub const WIDGET_PAD: f32 = 16.0;
/// How far a row highlight bleeds back out of the card's content margin. Also
/// the concentric gap that sets the row's own corner radius.
#[allow(dead_code)]
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
pub const COMPACT_FACE: f32 = 20.0;
/// Small inline badge next to a compact face (e.g. High Alert sun).
pub const COMPACT_BADGE: f32 = 12.0;
/// Small inline glyph next to text.
#[allow(dead_code)]
pub const GLYPH_SM: f32 = 16.0;
pub const TRACK_H: f32 = 4.0;
pub const TRACK_RADIUS: f32 = 2.0;
/// Inset from the compact capsule edge to the leading/trailing glyph.
/// Past the 14pt corner so a 20pt face does not sit on the curve.
#[cfg(test)]
pub const COMPACT_INSET: f32 = 8.0;
/// Expanded Nook Mirror circle. Fills `NOOK_BODY` minus the pane inset.
#[cfg(test)]
pub const MIRROR_FACE: f32 = 112.0;

/// HIG › Accessibility › Buttons gives macOS a 28×28 pt recommended hit target
/// (20×20 pt minimum). Interactive rows and controls hold this floor even when
/// their visible artwork is smaller.
pub const HIT_MIN: f32 = 28.0;
