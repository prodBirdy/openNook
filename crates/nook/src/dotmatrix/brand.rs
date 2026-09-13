//! Agent logos as LED-cell silhouettes.
//!
//! Source marks live in `assets/agents/` (Lobe Icons MIT, Aider logo from
//! aider.chat, fx favicon from fx.sh). Occupancy is a 16×16 silhouette of
//! those marks, painted with the global [`super::material::LED`] material —
//! solid cells only, brand-colored, with a working shimmer.

use super::material::{self, LED};
use crate::theme;
use gpui::{IntoElement, Rgba};
use nook_core::agents::AgentKind;

/// 16×16 occupancy, row-major, 0–15. Must match [`LED`].resolution.
const MASK_N: usize = 16;
const MASK_CELLS: usize = MASK_N * MASK_N;

/// CIELAB ΔL* below this is invisible at icon size. Tiny LEDs need more punch
/// than body text.
const MIN_DELTA_L: f32 = 58.0;

/// Official mark color (including black-on-white brands).
pub fn brand_rgb(kind: AgentKind) -> u32 {
    match kind {
        AgentKind::Claude => 0xD9_77_57,
        AgentKind::Codex => 0xFF_FF_FF,
        AgentKind::OpenCode => 0xFF_FF_FF,
        AgentKind::Fx => 0xFF_FF_FF,
        AgentKind::Grok => 0xFF_FF_FF,
        AgentKind::Cursor => 0xFF_FF_FF,
        AgentKind::Aider => 0x14_B0_14,
        AgentKind::Gemini => 0x31_86_FF,
        AgentKind::Pi => 0xFF_FF_FF,
    }
}

#[allow(dead_code)]
pub fn led_color(kind: AgentKind) -> Rgba {
    led_color_on(kind, theme::ISLAND)
}

/// Brand color pushed in CIELAB so its L* clears the island fill. Hue (a*, b*)
/// stays put: a black Grok mark becomes a light neutral, terracotta stays warm.
pub fn led_color_on(kind: AgentKind, on: Rgba) -> Rgba {
    visible_on(theme::rgba_from_u32(brand_rgb(kind), 1.0), on)
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn lab_f(t: f32) -> f32 {
    const E: f32 = 216.0 / 24389.0;
    const K: f32 = 24389.0 / 27.0;
    if t > E {
        t.cbrt()
    } else {
        (K * t + 16.0) / 116.0
    }
}

fn lab_f_inv(t: f32) -> f32 {
    const D: f32 = 6.0 / 29.0;
    if t > D {
        t * t * t
    } else {
        3.0 * D * D * (t - 4.0 / 29.0)
    }
}

fn to_lab(c: Rgba) -> (f32, f32, f32) {
    let r = srgb_to_linear(c.r.clamp(0.0, 1.0));
    let g = srgb_to_linear(c.g.clamp(0.0, 1.0));
    let b = srgb_to_linear(c.b.clamp(0.0, 1.0));
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = (0.0193339 * r + 0.119_192 * g + 0.9503041 * b) / 1.08883;
    let fx = lab_f(x);
    let fy = lab_f(y);
    let fz = lab_f(z);
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

fn from_lab(l: f32, a: f32, b: f32) -> Rgba {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let x = lab_f_inv(fx) * 0.95047;
    let y = lab_f_inv(fy);
    let z = lab_f_inv(fz) * 1.08883;
    let r = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let g = -0.969_266 * x + 1.8760108 * y + 0.0415560 * z;
    let bl = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;
    Rgba {
        r: linear_to_srgb(r).clamp(0.0, 1.0),
        g: linear_to_srgb(g).clamp(0.0, 1.0),
        b: linear_to_srgb(bl).clamp(0.0, 1.0),
        a: 1.0,
    }
}

fn visible_on(fg: Rgba, bg: Rgba) -> Rgba {
    let (fl, fa, fb) = to_lab(fg);
    let (bl, _, _) = to_lab(bg);
    if (fl - bl).abs() >= MIN_DELTA_L {
        return fg;
    }
    let target = if bl < 50.0 {
        (bl + MIN_DELTA_L).min(96.0)
    } else {
        (bl - MIN_DELTA_L).max(8.0)
    };
    from_lab(target, fa, fb)
}

fn mask(kind: AgentKind) -> &'static [u8; MASK_CELLS] {
    match kind {
        AgentKind::Claude => &CLAUDE,
        AgentKind::Codex => &CODEX,
        AgentKind::OpenCode => &OPENCODE,
        AgentKind::Fx => &FX,
        AgentKind::Grok => &GROK,
        AgentKind::Cursor => &CURSOR,
        AgentKind::Aider => &AIDER,
        AgentKind::Gemini => &GEMINI,
        AgentKind::Pi => &PI,
    }
}

/// Max occupancy in the mask rectangle that one glyph cell covers.
fn mask_cell(map: &[u8; MASK_CELLS], col: i32, row: i32, grid: i32) -> f32 {
    if grid <= 0 || col < 0 || row < 0 || col >= grid || row >= grid {
        return 0.0;
    }
    let grid = grid as usize;
    let x0 = (col as usize * MASK_N) / grid;
    let x1 = (((col as usize + 1) * MASK_N) / grid)
        .max(x0 + 1)
        .min(MASK_N);
    let y0 = (row as usize * MASK_N) / grid;
    let y1 = (((row as usize + 1) * MASK_N) / grid)
        .max(y0 + 1)
        .min(MASK_N);
    let mut m = 0u8;
    for y in y0..y1 {
        for x in x0..x1 {
            m = m.max(map[y * MASK_N + x]);
        }
    }
    m as f32 / 15.0
}

/// LED face for one agent: logo mask × global [`LED`] material × brand color.
/// `lite` skips glow while expand/collapse is springing.
pub fn element(
    kind: AgentKind,
    _seed: u32,
    now: f32,
    working: bool,
    size: f32,
    on: Rgba,
    lite: bool,
) -> impl IntoElement {
    debug_assert_eq!(MASK_N as i32, LED.resolution);
    let tint = led_color_on(kind, on);
    let map = mask(kind);
    let grid = LED.grid();
    material::element(
        size,
        LED,
        LED.grid(),
        tint,
        now,
        working,
        lite,
        move |col, row| mask_cell(map, col, row, grid),
    )
}

// Silhouettes traced from the saved marks in `assets/agents/`.

const CLAUDE: [u8; MASK_CELLS] = [
    0, 0, 0, 0, 15, 15, 0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 15, 15, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 0, 15, 15, 0, 0, 15, 15, 0, 0, 0, 15, 15, 0, 0, 15, 15, 0, 15,
    15, 0, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 0,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 0, 15, 15, 15, 15,
    15, 15, 15, 15, 0, 0, 0, 0, 0, 15, 0, 0, 15, 0, 15, 15, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0,
    15, 15, 0, 15, 15, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 15, 15, 0, 0, 15, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 0, 0,
];

// OpenAI blossom / open-eye (hollow hex), not the old Codex square.
const CODEX: [u8; MASK_CELLS] = [
    0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, // ......####......
    0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, // ....########....
    0, 0, 0, 15, 15, 15, 0, 15, 15, 0, 15, 15, 15, 0, 0, 0, // ...###.##.###...
    0, 0, 15, 15, 0, 15, 15, 0, 0, 15, 15, 0, 15, 15, 0, 0, // ..##.##..##.##..
    0, 15, 15, 0, 15, 15, 0, 0, 0, 0, 15, 15, 0, 15, 15, 0, // .##.##....##.##.
    0, 15, 15, 0, 15, 0, 0, 0, 0, 0, 0, 15, 0, 15, 15, 0, // .##.#......#.##.
    15, 15, 0, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 0, 15, 15, // ##.##......##.##
    15, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 15, // #.##........##.#
    15, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 15, // #.##........##.#
    15, 15, 0, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 0, 15, 15, // ##.##......##.##
    0, 15, 15, 0, 15, 0, 0, 0, 0, 0, 0, 15, 0, 15, 15, 0, // .##.#......#.##.
    0, 15, 15, 0, 15, 15, 0, 0, 0, 0, 15, 15, 0, 15, 15, 0, // .##.##....##.##.
    0, 0, 15, 15, 0, 15, 15, 0, 0, 15, 15, 0, 15, 15, 0, 0, // ..##.##..##.##..
    0, 0, 0, 15, 15, 15, 0, 15, 15, 0, 15, 15, 15, 0, 0, 0, // ...###.##.###...
    0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, // ....########....
    0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, // ......####......
];

const OPENCODE: [u8; MASK_CELLS] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15,
    0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0,
    15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15,
    0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0,
    15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15,
    15, 15, 15, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const GROK: [u8; MASK_CELLS] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 0, 0, 0, 15, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15,
    0, 0, 15, 0, 0, 0, 0, 0, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0,
    0, 0, 0, 15, 15, 15, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 15, 15,
    0, 0, 0, 0, 0, 15, 0, 0, 15, 15, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 15, 0, 0, 0, 15, 15, 0, 0, 0,
    0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 15, 15,
    0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 15, 15, 0, 15, 15, 15, 15, 15,
    15, 15, 15, 0, 0, 0, 0, 0, 15, 0, 0, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const CURSOR: [u8; MASK_CELLS] = [
    0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 0, 0,
    0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 15,
    15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 15,
    15, 0, 0, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 15, 15, 15, 15, 15, 15, 15,
    0, 0, 0, 0, 15, 15, 15, 0, 0, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 15,
    15, 15, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15,
    15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 15, 0, 0, 0, 0, 15, 15,
    15, 15, 15, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0,
];

const GEMINI: [u8; MASK_CELLS] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15,
    15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15,
    15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const FX: [u8; MASK_CELLS] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15, 15, 15, 0, 0, 0, 0, 15, 15, 15, 15, 0,
    15, 15, 15, 0, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0,
    15, 15, 0, 0, 0, 0, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0,
    0, 0, 0, 15, 15, 0, 0, 0, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 15, 15, 0, 0,
    15, 15, 0, 0, 0, 0, 0, 15, 15, 0, 0, 15, 15, 15, 0, 0, 0, 15, 15, 0, 0, 0, 0, 15, 15, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const AIDER: [u8; MASK_CELLS] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 15, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15,
    15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 0,
    0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 15, 0,
    0, 0, 0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const PI: [u8; MASK_CELLS] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15,
    0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0,
    0, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 0, 0, 0, 15,
    15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15,
    15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0,
    0, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15,
    15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dotmatrix::material::MASK_FLOOR;

    const KINDS: [AgentKind; 9] = [
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::OpenCode,
        AgentKind::Fx,
        AgentKind::Grok,
        AgentKind::Cursor,
        AgentKind::Aider,
        AgentKind::Gemini,
        AgentKind::Pi,
    ];

    fn lum(c: Rgba) -> f32 {
        to_lab(c).0
    }

    #[test]
    fn every_logo_has_a_silhouette() {
        for kind in KINDS {
            let lit = mask(kind).iter().filter(|&&v| v >= 8).count();
            assert!(lit >= 20, "{kind:?} only {lit} lit cells");
        }
    }

    #[test]
    fn gemini_is_a_plus_not_a_fill() {
        let m = mask(AgentKind::Gemini);
        // Top-left cell (row 0, col 0) stays empty — a filled blob would light it.
        assert_eq!(m[0], 0);
        assert_eq!(m[15], 0);
        assert!(m[7 * MASK_N + 7] >= 8);
    }

    #[test]
    fn opencode_keeps_a_hollow_window() {
        let m = mask(AgentKind::OpenCode);
        assert!(m[7 * MASK_N + 1] >= 8);
        assert_eq!(m[7 * MASK_N + 7], 0);
    }

    #[test]
    fn codex_is_the_openai_open_eye() {
        let m = mask(AgentKind::Codex);
        assert_eq!(m[7 * MASK_N + 7], 0);
        assert_eq!(m[8 * MASK_N + 8], 0);
        assert!(m[7] >= 8);
        assert!(m[15 * MASK_N + 8] >= 8);
        assert!(m[7 * MASK_N] >= 8);
        assert!(m[8 * MASK_N + 15] >= 8);
    }

    fn bbox(kind: AgentKind) -> (usize, usize, usize, usize) {
        let m = mask(kind);
        let mut x0 = MASK_N;
        let mut y0 = MASK_N;
        let mut x1 = 0;
        let mut y1 = 0;
        for y in 0..MASK_N {
            for x in 0..MASK_N {
                if m[y * MASK_N + x] >= 8 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        (x0, y0, x1, y1)
    }

    #[test]
    fn pi_mark_matches_grok_inset() {
        let (_, _, gx1, gy1) = bbox(AgentKind::Grok);
        let (px0, py0, px1, py1) = bbox(AgentKind::Pi);
        assert!(
            px0 >= 1 && py0 >= 1,
            "pi should not touch the top/left edge"
        );
        assert!(
            px1 <= 15 && py1 <= 15,
            "pi should not touch the bottom/right edge"
        );
        let grok_span = gx1.max(gy1);
        let pi_span = (px1 - px0).max(py1 - py0);
        assert!(
            pi_span <= grok_span + 2,
            "pi span {pi_span} vs grok {grok_span}"
        );
    }

    #[test]
    fn brand_colors_read_on_black() {
        for kind in KINDS {
            let l = lum(led_color(kind));
            assert!(
                l >= MIN_DELTA_L - 1.0,
                "{kind:?} L* {l:.1} too dark on the island"
            );
        }
    }

    #[test]
    fn dark_lab_lifts_off_black() {
        let black = theme::rgba_from_u32(0x000000, 1.0);
        let lifted = visible_on(black, theme::ISLAND);
        assert!(lum(lifted) >= MIN_DELTA_L - 1.0);
        let (l, a, b) = to_lab(lifted);
        assert!(
            a.abs() < 4.0 && b.abs() < 4.0,
            "black stays neutral {l} {a} {b}"
        );
    }

    #[test]
    fn white_stays_white_on_black() {
        let white = theme::rgba_from_u32(0xFFFFFF, 1.0);
        let out = visible_on(white, theme::ISLAND);
        assert!((out.r - 1.0).abs() < 0.02);
        assert!((out.g - 1.0).abs() < 0.02);
        assert!((out.b - 1.0).abs() < 0.02);
    }

    #[test]
    fn claude_keeps_terracotta_on_black() {
        let c = led_color_on(AgentKind::Claude, theme::ISLAND);
        let (_, a, b) = to_lab(c);
        assert!(a > 20.0, "red axis {a}");
        assert!(b > 20.0, "yellow axis {b}");
    }

    #[test]
    fn white_logo_darkens_on_a_light_island() {
        let white = theme::rgba_from_u32(0xFFFFFF, 1.0);
        let paper = theme::rgba_from_u32(0xF5F5F5, 1.0);
        let out = visible_on(white, paper);
        assert!(lum(out) < 50.0, "L* {}", lum(out));
    }

    #[test]
    fn mask_floor_drops_empty_cells() {
        let m = mask(AgentKind::Gemini);
        assert_eq!(mask_cell(m, 0, 0, 8), 0.0);
        assert!(mask_cell(m, 4, 4, 8) >= MASK_FLOOR);
    }

    #[test]
    fn agent_faces_use_global_led_resolution() {
        assert_eq!(MASK_N as i32, LED.resolution);
        assert_eq!(LED.grid(), 16);
    }
}
