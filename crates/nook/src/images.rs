//! Image sources mods name: URLs, files, data: URLs, "app:<bundle id>" and "nook:art/<id>"
//! (the playing track's artwork, held here rather than sent through JSON every second).

use base64::Engine;
use rune::{Image, ImageCache};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Decoded images kept by source; past this the oldest half goes.
const MAX_IMAGES: usize = 96;
/// Remote pictures: at most this big a download, decoded to at most this many pixels a side.
const MAX_DOWNLOAD: u64 = 16 << 20;
const MAX_REMOTE_SIDE: u32 = 1280;
/// Some sites send nothing (or a page) to a client they don't know.
const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
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
    /// A download finished: the island's poll renders again to pick it up.
    arrived: Arc<AtomicBool>,
    waker: rune::Waker,
}

/// Remote pictures by URL. Rune fetches no URLs on native: they're downloaded here, one thread
/// each, and decoded to RGBA. A failed one stays failed (shown as the empty frame) until the
/// app restarts.
enum Remote {
    Loading,
    Ready(u32, u32, Arc<[u8]>),
    Failed,
}

static REMOTE: OnceLock<Mutex<HashMap<String, Remote>>> = OnceLock::new();

fn remote_map() -> &'static Mutex<HashMap<String, Remote>> {
    REMOTE.get_or_init(Default::default)
}

fn fetch_remote(url: &str) -> Remote {
    match download(url).and_then(|bytes| decode(&bytes)) {
        Ok((w, h, rgba)) => Remote::Ready(w, h, rgba.into()),
        Err(e) => {
            log::warn!("image {url}: {e}");
            Remote::Failed
        }
    }
}

/// Downloads these pictures now, in parallel, for a still (`opennook shot`) that can't wait
/// for them the way the island does.
pub fn preload(urls: &[String]) {
    let threads: Vec<_> = urls.iter().filter(|u| !remote_map().lock().unwrap().contains_key(*u)).cloned()
        .map(|url| std::thread::spawn(move || {
            let state = fetch_remote(&url);
            remote_map().lock().unwrap().insert(url, state);
        }))
        .collect();
    for t in threads {
        let _ = t.join();
    }
}

impl Images {
    pub fn new(waker: rune::Waker) -> Self {
        Self { cache: ImageCache::new(waker.clone()), loaded: Mutex::default(), arrived: Arc::default(), waker }
    }

    /// Whether a remote picture has arrived since the last call.
    pub fn take_arrived(&self) -> bool {
        self.arrived.swap(false, Ordering::Relaxed)
    }

    /// A URL's picture once downloaded and decoded; None meanwhile (and when it can't be had).
    fn remote(&self, url: &str) -> Option<Image> {
        let mut remote = remote_map().lock().unwrap();
        match remote.get(url) {
            Some(Remote::Ready(w, h, rgba)) => return Some(self.cache.rgba(*w, *h, rgba.clone())),
            Some(_) => return None,
            None => {}
        }
        remote.insert(url.to_owned(), Remote::Loading);
        let (url, arrived, waker) = (url.to_owned(), self.arrived.clone(), self.waker.clone());
        std::thread::spawn(move || {
            let state = fetch_remote(&url);
            remote_map().lock().unwrap().insert(url, state);
            arrived.store(true, Ordering::Relaxed);
            waker();
        });
        None
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
            return self.remote(src);
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

fn download(url: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let client = reqwest::blocking::Client::builder().user_agent(BROWSER_UA).timeout(std::time::Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let response = client.get(url).header("Accept", "image/avif,image/webp,image/png,image/jpeg,image/*;q=0.8").send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let mut bytes = Vec::new();
    response.take(MAX_DOWNLOAD).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// Straight RGBA at most `MAX_REMOTE_SIDE` a side: PNG, JPEG, GIF, WebP and the rest the
/// `image` crate reads, else whatever macOS reads (AVIF, HEIC), through `sips`.
fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let img = match image::load_from_memory(bytes) {
        Ok(img) => img,
        Err(e) => image::load_from_memory(&transcode(bytes).ok_or(format!("{e}"))?).map_err(|e| e.to_string())?,
    };
    let img = if img.width() > MAX_REMOTE_SIDE || img.height() > MAX_REMOTE_SIDE { img.thumbnail(MAX_REMOTE_SIDE, MAX_REMOTE_SIDE) } else { img };
    let rgba = img.to_rgba8();
    Ok((rgba.width(), rgba.height(), rgba.into_raw()))
}

#[cfg(target_os = "macos")]
fn transcode(bytes: &[u8]) -> Option<Vec<u8>> {
    let dir = std::env::temp_dir();
    let stem = format!("opennook-img-{}-{:x}", std::process::id(), bytes.len() ^ bytes.first().copied().unwrap_or(0) as usize);
    let (input, output) = (dir.join(&stem), dir.join(format!("{stem}.png")));
    std::fs::write(&input, bytes).ok()?;
    let ok = std::process::Command::new("/usr/bin/sips").args(["-s", "format", "png"]).arg(&input).arg("--out").arg(&output)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
    let png = if ok { std::fs::read(&output).ok() } else { None };
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
    png
}

#[cfg(not(target_os = "macos"))]
fn transcode(_: &[u8]) -> Option<Vec<u8>> {
    None
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_else(|| "/".into())
}
