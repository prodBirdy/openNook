//! Image sources mods name: URLs, files, data: URLs, "app:<bundle id>" and "nook:art/<id>"
//! (the playing track's artwork, held here rather than sent through JSON every second).

use base64::Engine;
use rune::{Image, ImageCache};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

/// Decoded images kept by source; past this the oldest half goes.
const MAX_IMAGES: usize = 96;
const MAX_ART: usize = 8;

/// Artwork bytes by id, newest last.
type Art = Vec<(String, Arc<[u8]>)>;
static ART: OnceLock<Mutex<Art>> = OnceLock::new();

/// Keeps artwork bytes and returns the source mods use for them.
pub fn put_art(bytes: Vec<u8>) -> String {
    let id = format!("{:016x}", bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3)));
    let art = ART.get_or_init(Default::default);
    let mut art = art.lock().unwrap();
    if !art.iter().any(|(k, _)| *k == id) {
        art.push((id.clone(), bytes.into()));
        if art.len() > MAX_ART {
            art.remove(0);
        }
    }
    format!("nook:art/{id}")
}

fn art(id: &str) -> Option<Arc<[u8]>> {
    ART.get()?.lock().unwrap().iter().find(|(k, _)| k == id).map(|(_, b)| b.clone())
}

/// Images by source with the clock tick each was last used at, and the clock.
type Loaded = (HashMap<String, (u64, Image)>, u64);

pub struct Images {
    cache: ImageCache,
    loaded: Mutex<Loaded>,
}

impl Images {
    pub fn new(waker: rune::Waker) -> Self {
        Self { cache: ImageCache::new(waker), loaded: Mutex::default() }
    }

    /// The image for a source, loading it on first use; None when the source is unusable.
    pub fn get(&self, src: &str, dir: &Path, size_pt: f32) -> Option<Image> {
        // A relative file is the mod's own: two mods' "./icon.png" are different pictures.
        let relative = !src.contains(':') && !src.starts_with('/') && !src.starts_with("~/");
        let key = if relative { format!("{}@{size_pt}", dir.join(src).display()) } else { format!("{src}@{size_pt}") };
        let mut guard = self.loaded.lock().unwrap();
        let (map, clock) = &mut *guard;
        *clock += 1;
        if let Some((used, img)) = map.get_mut(&key) {
            *used = *clock;
            return Some(img.clone());
        }
        let img = self.load(src, dir, size_pt)?;
        if map.len() >= MAX_IMAGES {
            let mut ages: Vec<u64> = map.values().map(|(u, _)| *u).collect();
            ages.sort_unstable();
            let cut = ages[ages.len() / 2];
            map.retain(|_, (u, _)| *u > cut);
        }
        map.insert(key, (*clock, img.clone()));
        Some(img)
    }

    fn load(&self, src: &str, dir: &Path, size_pt: f32) -> Option<Image> {
        let scale = 2.0;
        if let Some(id) = src.strip_prefix("nook:art/") {
            return Some(self.cache.load(art(id)?));
        }
        if let Some(rest) = src.strip_prefix("data:") {
            let (_, data) = rest.split_once(";base64,")?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).ok()?;
            return Some(self.cache.load(bytes));
        }
        if let Some(bundle) = src.strip_prefix("app:") {
            // The light variant: the island is black.
            if let Some((w, h, rgba)) = crate::platform::app_icon_light(bundle, (size_pt * scale).ceil().max(16.0) as u32) {
                return Some(self.cache.rgba(w, h, rgba));
            }
            let path = crate::platform::app_path(bundle)?;
            return Some(self.cache.app_icon_scaled(path, size_pt, scale));
        }
        if src.starts_with("https://") || src.starts_with("http://") {
            return Some(self.cache.url(src, size_pt, scale));
        }
        let path = if let Some(rest) = src.strip_prefix("~/") {
            dirs_home().join(rest)
        } else if Path::new(src).is_absolute() {
            src.into()
        } else {
            dir.join(src)
        };
        Some(self.cache.file(path, size_pt, scale))
    }
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_else(|| "/".into())
}
