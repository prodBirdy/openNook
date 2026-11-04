//! Mod folders on disk: manifests, error and log files, storage, and the shipped mods.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// `mod.json`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Manifest {
    pub title: Option<String>,
    /// A built-in icon name, "./file.svg", or "app:<bundle id>" (that app's icon, in colour),
    /// shown on the island's tab for this mod.
    pub icon: Option<String>,
    pub description: Option<String>,
    pub props: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct Found {
    pub name: String,
    pub dir: PathBuf,
}

/// Mod names are folder names: lowercase letters, digits and dashes.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Every mod folder with a main.js.
pub fn scan(mods: &Path) -> Vec<Found> {
    let mut found: Vec<Found> = std::fs::read_dir(mods).into_iter().flatten().flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let dir = e.path();
            (valid_name(&name) && dir.join("main.js").is_file()).then_some(Found { name, dir })
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    match std::fs::read_to_string(dir.join("mod.json")) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("mod.json: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
        Err(e) => Err(format!("mod.json: {e}")),
    }
}

/// Newest modification time of the files in a mod's folder, to notice edits.
pub fn stamp(dir: &Path) -> Option<SystemTime> {
    fn walk(dir: &Path, depth: u8, newest: &mut Option<SystemTime>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                if depth < 4 {
                    walk(&path, depth + 1, newest);
                }
            } else if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
                *newest = Some(newest.map_or(t, |n: SystemTime| n.max(t)));
            }
        }
    }
    let mut newest = None;
    walk(dir, 0, &mut newest);
    newest
}

/// `mods/<name>.error`, beside the folder: present exactly while the mod is broken.
pub fn set_error(mods: &Path, name: &str, error: Option<&str>) {
    let path = mods.join(format!("{name}.error"));
    match error {
        Some(e) => {
            let _ = write_if_changed(&path, format!("{e}\n"));
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

const LOG_LIMIT: u64 = 256 << 10;

/// Appends to `logs/<name>.log`, starting over past 256 KiB.
pub fn log(logs: &Path, name: &str, level: &str, text: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(logs);
    let path = logs.join(format!("{name}.log"));
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_LIMIT) {
        let _ = std::fs::rename(&path, logs.join(format!("{name}.log.old")));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{secs} {level} {text}");
    }
}

/// A mod's localStorage, kept in `data/<name>.json`.
pub struct Storage {
    path: PathBuf,
    pub items: BTreeMap<String, String>,
}

impl Storage {
    pub fn open(data: &Path, name: &str) -> Self {
        let path = data.join(format!("{name}.json"));
        let items = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Self { path, items }
    }

    pub fn set(&mut self, key: String, value: Option<String>) {
        match value {
            Some(v) => self.items.insert(key, v),
            None => self.items.remove(&key),
        };
        self.save();
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.save();
    }

    fn save(&self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = write_atomic(&self.path, serde_json::to_string(&self.items).unwrap_or_default().as_bytes());
    }
}

pub fn write_if_changed(path: &Path, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    let content = content.as_ref();
    if std::fs::read(path).ok().as_deref() != Some(content) {
        write_atomic(path, content)?;
    }
    Ok(())
}

pub fn write_atomic(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

// Shipped mods. openNook owns them: each start replaces them whole with the binary's copy.
// A shipped mod someone edited is moved to `<name>-edited` first, so no edit is lost (the
// approach herdr-app takes, Apache-2.0).

/// File path inside a mod to content fingerprint.
type Files = BTreeMap<String, String>;
/// Shipped mods as installed, by name (`installed.json`).
type Installed = BTreeMap<String, Files>;

/// FNV-1a: stable across builds, unlike std's hasher.
fn fingerprint(bytes: &[u8]) -> String {
    let h = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
    format!("{h:016x}")
}

/// Exactly as installed: every file unchanged and none added.
fn as_shipped(dir: &Path, files: &Files) -> bool {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, dir, &mut found);
    found.len() == files.len()
        && found.iter().all(|f| files.contains_key(f))
        && files.iter().all(|(path, want)| std::fs::read(dir.join(path)).is_ok_and(|b| fingerprint(&b) == *want))
}

fn edited_name(mods: &Path, name: &str) -> String {
    (1..).map(|i| if i == 1 { format!("{name}-edited") } else { format!("{name}-edited-{i}") })
        .find(|n| !mods.join(n).exists())
        .unwrap()
}

/// Installs or updates the shipped mods (`files` are `("<mod>/<path>", bytes)`). Returns the
/// clones made of edited shipped mods.
pub fn sync_shipped(mods: &Path, installed_path: &Path, files: &[(&str, &[u8])]) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(mods)?;
    let text = std::fs::read_to_string(installed_path).unwrap_or_default();
    let mut installed: Installed = serde_json::from_str(&text).unwrap_or_default();
    let names: std::collections::BTreeSet<&str> = files.iter().filter_map(|(p, _)| p.split('/').next()).collect();
    let mut clones = Vec::new();

    let dropped: Vec<String> = installed.keys().filter(|n| !names.contains(n.as_str())).cloned().collect();
    for name in dropped {
        let target = mods.join(&name);
        if installed.get(&name).is_some_and(|was| target.exists() && as_shipped(&target, was)) {
            std::fs::remove_dir_all(&target)?;
        }
        installed.remove(&name);
    }
    for name in names {
        let target = mods.join(name);
        let prefix = format!("{name}/");
        let shipped: Files = files.iter()
            .filter_map(|(p, bytes)| Some((p.strip_prefix(&prefix)?.to_owned(), fingerprint(bytes))))
            .collect();
        if installed.get(name) == Some(&shipped) && target.exists() && as_shipped(&target, &shipped) {
            continue;
        }
        if target.exists() {
            if as_shipped(&target, installed.get(name).unwrap_or(&shipped)) {
                std::fs::remove_dir_all(&target)?;
            } else {
                let clone = edited_name(mods, name);
                std::fs::rename(&target, mods.join(&clone))?;
                clones.push(clone);
            }
        }
        for (path, bytes) in files.iter().filter(|(p, _)| p.starts_with(&prefix)) {
            let file = mods.join(path);
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(&file, bytes)?;
        }
        installed.insert(name.to_owned(), shipped);
    }
    write_if_changed(installed_path, serde_json::to_string_pretty(&installed).unwrap() + "\n")?;
    Ok(clones)
}

/// Expands a leading `~/`.
pub fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home.to_path_buf(),
        None => PathBuf::from(path),
    }
}
