//! Lucide icons (v0.562) loaded from `assets/icons/*.svg`.
//! GPUI renders each SVG as an alpha mask tinted by `text_color`.

use crate::theme;
use gpui::{prelude::*, px, svg, SharedString};
use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "src/assets/icons"]
struct IconPack;

pub fn lucide(name: &'static str, size: f32) -> impl IntoElement {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .text_color(theme::TEXT)
}

pub struct Assets;

impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let key = path.strip_prefix("icons/").unwrap_or(path);
        Ok(IconPack::get(key).map(|file| file.data))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        if path.trim_matches('/') != "icons" {
            return Ok(Vec::new());
        }
        Ok(IconPack::iter()
            .map(|name| SharedString::from(name.into_owned()))
            .collect())
    }
}
