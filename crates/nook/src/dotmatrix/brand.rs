//! Agent logos, brand colors, and a Magic UI GlyphMatrix fill masked to each
//! mark.
//!
//! Source marks live in `assets/agents/` (Lobe Icons MIT, Aider logo from
//! aider.chat, fx favicon from fx.sh). Occupancy is a 16×16 silhouette of
//! those marks. The live face is a 16×16 cell grid of mutating glyphs from
//! `"01·•+*/\\<>="` — same charset, 90 ms tick, 4 % mutation rate, and
//! bottom fade as [GlyphMatrix](https://magicui.design/docs/components/glyph-matrix)
//! — painted in the agent's color, only where the logo is opaque.

use super::engine::bloom_level;
use super::{alpha, paint_glow};
use crate::theme;
use gpui::{canvas, fill, point, prelude::*, px, Bounds, IntoElement, Pixels, Rgba, Window};
use nook_core::agents::AgentKind;

/// 16×16 occupancy, row-major, 0–15.
const MASK_N: usize = 16;
const MASK_CELLS: usize = MASK_N * MASK_N;

/// Magic UI GlyphMatrix defaults.
const GLYPHS: &str = "01·•+*/\\<>=";
const INTERVAL: f32 = 0.090;
const MUTATION_RATE: f32 = 0.04;
/// Milder than Magic UI's 0.6: a 28 px logo can't afford to lose its bottom half.
const FADE_BOTTOM: f32 = 0.18;
/// Skip cells the logo barely covers so the silhouette stays sharp.
const MASK_FLOOR: f32 = 0.25;
/// Occupied cells always paint this much of the brand color so the mark reads
/// even when the glyph is a single-pixel `·`.
const BODY_IDLE: f32 = 0.78;
const BODY_WORK: f32 = 0.62;
/// Diagonal sheen while an agent is working. Period in seconds; band is the
/// fraction of the (row+col) path that is lit at once.
const SHIMMER_PERIOD: f32 = 1.35;
const SHIMMER_BAND: f32 = 0.28;
/// CIELAB ΔL* below this is invisible at icon size. Tiny LEDs need more punch
/// than body text.
const MIN_DELTA_L: f32 = 58.0;

/// 3×3 pixel font for the charset, bit 0 = top-left, row-major.
/// Used when a cell is too small for shaped text.
const GLYPH_BITS: [u16; 11] = [
    0b111_101_111, // 0
    0b010_010_010, // 1
    0b000_010_000, // ·
    0b000_111_000, // •
    0b010_111_010, // +
    0b101_010_101, // *
    0b001_010_100, // /
    0b100_010_001, // \
    0b001_110_001, // <
    0b100_011_100, // >
    0b111_000_111, // =
];

/// Official mark color (including black-on-white brands).
pub fn brand_rgb(kind: AgentKind) -> u32 {
    match kind {
        AgentKind::Claude => 0xD9_77_57,
        AgentKind::Codex => 0x7A_9D_FF,
        AgentKind::OpenCode => 0xFF_FF_FF,
        AgentKind::Fx => 0xFF_FF_FF,
        AgentKind::Grok => 0xFF_FF_FF,
        AgentKind::Cursor => 0xFF_FF_FF,
        AgentKind::Aider => 0x14_B0_14,
        AgentKind::Gemini => 0x31_86_FF,
        AgentKind::Pi => 0xFF_FF_FF,
    }
}

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
    let z = (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883;
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
    let g = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
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

/// One cell per occupancy pixel so the 16×16 mark is not 2×2-pooled down.
fn glyph_grid(_size: f32) -> i32 {
    MASK_N as i32
}

fn hash_u32(a: u32, b: u32) -> u32 {
    let mut x = a.wrapping_add(0x9E37_79B9).wrapping_mul(0x85EB_CA6B) ^ b;
    x ^= x >> 16;
    x = x.wrapping_mul(0xC2B2_AE35);
    x ^ (x >> 16)
}

/// Independent per-cell generation: mean time between mutations is
/// `INTERVAL / MUTATION_RATE` (2.25 s), matching Magic UI's 4 % of cells
/// per 90 ms tick.
fn generation(seed: u32, i: u32, now: f32, working: bool) -> u32 {
    if !working {
        return 0;
    }
    let phase = (hash_u32(seed, i) as f32) / (u32::MAX as f32);
    let period = INTERVAL / MUTATION_RATE;
    ((now / period) + phase).floor() as u32
}

fn glyph_index(seed: u32, i: u32, gen: u32) -> usize {
    (hash_u32(seed ^ gen.wrapping_mul(0xA24B_AED5), i) as usize) % GLYPH_BITS.len()
}

fn glyph_alpha(seed: u32, i: u32, gen: u32, working: bool) -> f32 {
    let r = (hash_u32(seed.wrapping_mul(0x9E37_79B9) ^ gen, i.wrapping_add(1)) as f32)
        / (u32::MAX as f32);
    if working {
        0.35 + r * 0.55
    } else {
        0.20 + r * 0.25
    }
}

fn fade_row(row: i32, grid: i32) -> f32 {
    if grid <= 1 {
        return 1.0;
    }
    1.0 - (row as f32 / (grid - 1) as f32) * FADE_BOTTOM
}

/// Smooth diagonal highlight, 0 outside the band and 1 at its crest.
fn shimmer(row: i32, col: i32, grid: i32, now: f32) -> f32 {
    if grid <= 1 {
        return 0.0;
    }
    let path = (row + col) as f32 / ((grid - 1) * 2) as f32;
    let t = (now / SHIMMER_PERIOD).rem_euclid(1.0);
    let mut d = (path - t).abs();
    if d > 0.5 {
        d = 1.0 - d;
    }
    if d >= SHIMMER_BAND {
        return 0.0;
    }
    let x = 1.0 - d / SHIMMER_BAND;
    x * x * (3.0 - 2.0 * x)
}

/// Glyph-matrix face for one agent: logo mask × mutating charset × brand color.
pub fn element(
    kind: AgentKind,
    seed: u32,
    now: f32,
    working: bool,
    size: f32,
    on: Rgba,
) -> impl IntoElement {
    let tint = led_color_on(kind, on);
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            paint_brand(window, bounds, kind, seed, now, working, size, tint);
        },
    )
    .size(px(size))
    .flex_shrink_0()
}

fn paint_brand(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    kind: AgentKind,
    seed: u32,
    now: f32,
    working: bool,
    size: f32,
    tint: Rgba,
) {
    let grid = glyph_grid(size);
    let cell = size / grid as f32;
    let ox: f32 = bounds.origin.x.into();
    let oy: f32 = bounds.origin.y.into();
    let bw: f32 = bounds.size.width.into();
    let bh: f32 = bounds.size.height.into();
    let ox = ox + (bw - size).max(0.0) * 0.5;
    let oy = oy + (bh - size).max(0.0) * 0.5;
    let map = mask(kind);

    let mut cells = Vec::with_capacity((grid * grid) as usize);
    let body = if working { BODY_WORK } else { BODY_IDLE };
    for row in 0..grid {
        let fade = fade_row(row, grid);
        for col in 0..grid {
            let occ = mask_cell(map, col, row, grid);
            if occ < MASK_FLOOR {
                continue;
            }
            let i = (row * grid + col) as u32;
            let gen = generation(seed, i, now, working);
            let glyph_a = (occ * fade * glyph_alpha(seed, i, gen, working)).clamp(0.0, 1.0);
            let body_a = if working {
                let sheen = shimmer(row, col, grid, now);
                let sparkle = glyph_a * 0.18;
                (occ * fade * (BODY_WORK + (1.0 - BODY_WORK) * sheen) + sparkle).clamp(0.0, 1.0)
            } else {
                (occ * fade * body).clamp(0.0, 1.0)
            };
            let x = ox + col as f32 * cell;
            let y = oy + row as f32 * cell;
            cells.push((
                x,
                y,
                body_a,
                glyph_a,
                glyph_index(seed, i, gen),
                bloom_level(glyph_a.max(body_a)),
            ));
        }
    }

    for &(x, y, _, _, _, level) in &cells {
        if level <= 0.0 {
            continue;
        }
        paint_glow(window, x, y, cell * 0.85, level, tint);
    }
    let gap = if cell < 3.0 {
        (cell * 0.08).min(0.25)
    } else {
        (cell * 0.12).min(0.6)
    };
    let body_s = (cell - gap).max(0.75);
    let radius = px((body_s * 0.22).min(0.9));
    for &(x, y, body_a, _, _, _) in &cells {
        let dx = x + gap * 0.5;
        let dy = y + gap * 0.5;
        window.paint_quad(
            fill(
                Bounds::from_corners(
                    point(px(dx), px(dy)),
                    point(px(dx + body_s), px(dy + body_s)),
                ),
                alpha(tint, body_a),
            )
            .corner_radii(radius),
        );
    }
    if cell >= 3.0 {
        for &(x, y, _, glyph_a, gi, _) in &cells {
            paint_mini_glyph(window, x, y, cell, gi, tint, glyph_a);
        }
    }
}

fn paint_mini_glyph(
    window: &mut Window,
    x: f32,
    y: f32,
    cell: f32,
    glyph: usize,
    tint: Rgba,
    a: f32,
) {
    let bits = GLYPH_BITS[glyph.min(GLYPH_BITS.len() - 1)];
    let inset = (cell * 0.08).max(0.1);
    let inner = (cell - inset * 2.0).max(1.0);
    let px_s = inner / 3.0;
    let gap = (px_s * 0.18).min(0.4);
    let dot = (px_s - gap).max(0.55);
    let radius = px((dot * 0.35).min(0.8));
    let color = alpha(tint, a);
    for r in 0..3 {
        for c in 0..3 {
            if bits & (1 << (r * 3 + c)) == 0 {
                continue;
            }
            let dx = x + inset + c as f32 * px_s + gap * 0.5;
            let dy = y + inset + r as f32 * px_s + gap * 0.5;
            window.paint_quad(
                fill(
                    Bounds::from_corners(point(px(dx), px(dy)), point(px(dx + dot), px(dy + dot))),
                    color,
                )
                .corner_radii(radius),
            );
        }
    }
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

const CODEX: [u8; MASK_CELLS] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 0,
    0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 0, 0,
    15, 15, 0, 0, 15, 15, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 15, 15, 0, 15, 0, 0, 0, 0, 0, 0, 15,
    15, 15, 15, 0, 0, 15, 15, 0, 0, 15, 15, 0, 0, 0, 0, 15, 15, 15, 15, 0, 0, 15, 15, 15, 0, 0, 0,
    0, 0, 0, 15, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 0, 0,
    15, 15, 15, 0, 0, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0,
    15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
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
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0,
    0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0,
    0, 0, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 0, 0, 0, 0,
    0, 0, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0,
    0, 0, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0,
    0, 0, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0, 0, 0, 0,
    0, 0, 15, 15, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0,
    0, 0, 15, 15, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0,
    0, 0, 15, 15, 15, 15, 15, 15, 0, 0, 0, 15, 15, 15, 15, 0,
    0, 0, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0,
    0, 0, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0,
    0, 0, 15, 15, 15, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

#[cfg(test)]
mod tests {
    use super::*;

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
    fn charset_matches_magic_ui() {
        assert_eq!(GLYPHS.chars().count(), GLYPH_BITS.len());
        assert_eq!(GLYPHS, "01·•+*/\\<>=");
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
        assert_eq!(m[0], 0);
        assert_eq!(m[15], 0);
        assert!(m[7 * MASK_N + 7] >= 8);
        assert_eq!(m[0 * MASK_N + 0], 0);
    }

    #[test]
    fn opencode_keeps_a_hollow_window() {
        let m = mask(AgentKind::OpenCode);
        assert!(m[7 * MASK_N + 1] >= 8);
        assert_eq!(m[7 * MASK_N + 7], 0);
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
        assert!(px0 >= 1 && py0 >= 1, "pi should not touch the top/left edge");
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
    fn idle_glyphs_do_not_mutate() {
        for i in 0..16u32 {
            let a = generation(11, i, 0.2, false);
            let b = generation(11, i, 4.0, false);
            assert_eq!(a, b);
            assert_eq!(glyph_index(11, i, a), glyph_index(11, i, b));
        }
    }

    #[test]
    fn working_glyphs_mutate() {
        let changed = (0..64u32).any(|i| {
            generation(3, i, 0.1, true) != generation(3, i, 5.0, true)
                || glyph_index(3, i, generation(3, i, 0.1, true))
                    != glyph_index(3, i, generation(3, i, 5.0, true))
        });
        assert!(changed);
    }

    #[test]
    fn working_shimmer_sweeps() {
        let a = shimmer(0, 0, 16, 0.0);
        let b = shimmer(8, 8, 16, 0.0);
        let c = shimmer(0, 0, 16, SHIMMER_PERIOD * 0.5);
        assert!(a > 0.7, "crest starts top-left {a}");
        assert!(b < 0.2, "opposite corner is dark {b}");
        assert!(c < a, "half a period later the crest has moved {c} vs {a}");
        let later = shimmer(8, 8, 16, SHIMMER_PERIOD * 0.5);
        assert!(later > b, "mid-logo lights up mid-sweep {later}");
    }

    #[test]
    fn fade_darkens_the_bottom() {
        assert!((fade_row(0, 8) - 1.0).abs() < f32::EPSILON);
        assert!(fade_row(7, 8) < fade_row(0, 8));
        assert!((fade_row(7, 8) - (1.0 - FADE_BOTTOM)).abs() < 0.02);
    }

    #[test]
    fn compact_face_uses_the_full_mask() {
        assert_eq!(glyph_grid(theme::COMPACT_FACE), MASK_N as i32);
        assert_eq!(glyph_grid(16.0), MASK_N as i32);
        assert_eq!(glyph_grid(64.0), MASK_N as i32);
    }
}
