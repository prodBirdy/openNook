//! Files mods follow (`tail`, `watchFile`) and the mod folders themselves, polled. Polling
//! sees editors' atomic renames and log rotation the same way, and a few stats a tick cost
//! nothing.

use crate::engine::Msg;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const TICK: Duration = Duration::from_millis(250);
/// A tail reads at most this much per tick; a burst past it resumes on the next tick.
const TAIL_CHUNK: u64 = 1 << 20;

pub enum Kind {
    Tail { offset: Option<u64>, partial: Vec<u8> },
    Watch { seen: Option<Option<(u64, SystemTime)>> },
}

pub struct Sub {
    pub name: String,
    pub load: u64,
    pub path: PathBuf,
    pub kind: Kind,
}

#[derive(Default)]
pub struct Watches {
    pub subs: HashMap<u64, Sub>,
    next: u64,
}

impl Watches {
    pub fn add(&mut self, sub: Sub) -> u64 {
        self.next += 1;
        self.subs.insert(self.next, sub);
        self.next
    }

    pub fn drop_load(&mut self, name: &str, load: u64) {
        self.subs.retain(|_, s| s.name != name || s.load != load);
    }
}

pub fn spawn(watches: Arc<Mutex<Watches>>, mods: PathBuf, tx: Sender<Msg>) {
    std::thread::Builder::new().name("nook-mods-watch".into()).spawn(move || {
        let mut stamps = folder_stamps(&mods);
        loop {
            std::thread::sleep(TICK);
            let now = folder_stamps(&mods);
            if now != stamps {
                stamps = now;
                if tx.send(Msg::Reload).is_err() {
                    return;
                }
            }
            let mut events = Vec::new();
            for (id, sub) in watches.lock().unwrap().subs.iter_mut() {
                poll(*id, sub, &mut events);
            }
            for e in events {
                if tx.send(e).is_err() {
                    return;
                }
            }
        }
    }).expect("watch thread");
}

fn folder_stamps(mods: &Path) -> Vec<(String, Option<SystemTime>)> {
    crate::files::scan(mods).into_iter().map(|f| (f.name, crate::files::stamp(&f.dir))).collect()
}

fn identity(path: &Path) -> Option<(u64, SystemTime)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

fn poll(id: u64, sub: &mut Sub, events: &mut Vec<Msg>) {
    let event = |payload: Value| Msg::Event { name: sub.name.clone(), load: sub.load, sub: id, payload };
    match &mut sub.kind {
        Kind::Watch { seen } => {
            let now = identity(&sub.path);
            if seen.as_ref() != Some(&now) {
                *seen = Some(now);
                let text = std::fs::read(&sub.path).ok().map(|b| String::from_utf8_lossy(&b).into_owned());
                events.push(event(text.map_or(Value::Null, Value::from)));
            }
        }
        Kind::Tail { offset, partial } => {
            let Ok(mut file) = std::fs::File::open(&sub.path) else { return };
            let Ok(len) = file.metadata().map(|m| m.len()) else { return };
            let start = match *offset {
                // Not there when subscribed: everything it has is new.
                None => 0,
                // Shorter than we've read: rotated or truncated, so it starts over.
                Some(o) if len < o => {
                    partial.clear();
                    0
                }
                Some(o) => o,
            };
            if len <= start {
                *offset = Some(len);
                return;
            }
            let take = (len - start).min(TAIL_CHUNK);
            let mut chunk = Vec::with_capacity(take as usize);
            if file.seek(SeekFrom::Start(start)).is_err() || file.take(take).read_to_end(&mut chunk).is_err() {
                return;
            }
            *offset = Some(start + chunk.len() as u64);
            partial.extend_from_slice(&chunk);
            while let Some(i) = partial.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = partial.drain(..=i).collect();
                let line = String::from_utf8_lossy(&line[..line.len() - 1]).trim_end_matches('\r').to_owned();
                events.push(event(Value::from(line)));
            }
        }
    }
}

/// Where a new tail starts: the file's end, or where its last `n` lines begin. `None` when the
/// file doesn't exist yet.
pub fn tail_start(path: &Path, n: usize) -> Option<u64> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    Some(back_lines(&mut file, len, n))
}

fn back_lines(file: &mut std::fs::File, len: u64, n: usize) -> u64 {
    if n == 0 {
        return len;
    }
    let from = len.saturating_sub(64 << 10);
    let mut tail = Vec::new();
    if file.seek(SeekFrom::Start(from)).is_err() || file.read_to_end(&mut tail).is_err() {
        return len;
    }
    // Skip the final newline, then count back n more.
    let mut seen = 0;
    for (i, &b) in tail.iter().enumerate().rev().skip(1) {
        if b == b'\n' {
            seen += 1;
            if seen == n {
                return from + i as u64 + 1;
            }
        }
    }
    from
}
