//! Three colours from a track's artwork, as 1.x picked them: the most common 4-bit buckets of
//! a 32×32 thumbnail, skipping near-black and near-white, at least a little apart.

use image::GenericImageView;
use std::collections::HashMap;

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn shift(c: [f32; 3], amount: f32) -> [f32; 3] {
    [(c[0] + amount).clamp(0.0, 1.0), (c[1] + amount * 0.5).clamp(0.0, 1.0), (c[2] + amount * 0.75).clamp(0.0, 1.0)]
}

fn hex(c: [f32; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8)
}

pub fn of(bytes: &[u8]) -> Option<[String; 3]> {
    let img = image::load_from_memory(bytes).ok()?.thumbnail(32, 32);
    let mut buckets: HashMap<(u8, u8, u8), u32> = HashMap::new();
    let (mut sum, mut n) = ([0f32; 3], 0f32);
    for (_, _, px) in img.pixels() {
        let [r, g, b, a] = px.0;
        if a < 200 {
            continue;
        }
        sum = [sum[0] + r as f32, sum[1] + g as f32, sum[2] + b as f32];
        n += 1.0;
        let brightness = (r as u16 + g as u16 + b as u16) / 3;
        if (16..=240).contains(&brightness) {
            *buckets.entry((r >> 4, g >> 4, b >> 4)).or_insert(0) += 1;
        }
    }
    if n == 0.0 {
        return None;
    }
    let mut ranked: Vec<_> = buckets.into_iter().collect();
    ranked.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let mut picked: Vec<[f32; 3]> = Vec::new();
    for ((r, g, b), _) in ranked {
        let c = [(r as f32 + 0.5) * 16.0 / 255.0, (g as f32 + 0.5) * 16.0 / 255.0, (b as f32 + 0.5) * 16.0 / 255.0];
        if picked.iter().all(|p| dist(*p, c) > 0.18) {
            picked.push(c);
        }
        if picked.len() == 3 {
            break;
        }
    }
    let base = picked.first().copied().unwrap_or([sum[0] / n / 255.0, sum[1] / n / 255.0, sum[2] / n / 255.0]);
    while picked.len() < 3 {
        picked.push(shift(base, 0.08 * picked.len() as f32));
    }
    Some([hex(picked[0]), hex(picked[1]), hex(picked[2])])
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_two_colour_cover_gives_both() {
        let mut img = image::RgbaImage::new(32, 32);
        for (x, _, px) in img.enumerate_pixels_mut() {
            *px = if x < 20 { image::Rgba([220, 40, 60, 255]) } else { image::Rgba([30, 90, 220, 255]) };
        }
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let p = super::of(&png).unwrap();
        assert!(p[0].starts_with("#d") || p[0].starts_with("#e"), "{p:?}");
        assert!(p.iter().any(|c| c.ends_with("d8") || c.ends_with("e8")), "{p:?}");
    }
}
