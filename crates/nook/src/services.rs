//! Live data for mods, from nook-core and the OS, published into the mods' store. Each source
//! has a thread that parks between changes; mods re-render only when what they read moves.

use base64::Engine;
use nook_core::agents::{AgentSession, AgentStatus};
use nook_core::models::RepeatMode;
use nook_core::NowPlayingData;
use nook_mods::Store;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Starts the parts of nook-core mods read. Not `nook_core::init`: that also installs the
/// system-HUD replacement, which hides macOS's volume HUD until a mod draws one.
pub fn start_core() {
    let _ = nook_core::runtime();
    if let Err(e) = nook_core::database::init_db() {
        log::warn!("database unavailable ({e}); settings won't persist");
    } else {
        nook_core::settings::load_from_db();
    }
    nook_core::audio::init_audio_state();
    #[cfg(target_os = "macos")]
    let _ = nook_core::audio::prime_media_apps();
}

/// The sources every island uses. Agents start on demand (`start_agents`): scanning processes
/// every two seconds is wasted when no mod reads `agents()`.
pub fn start(store: Arc<Store>) {
    let s = store.clone();
    spawn("nook-media", move || {
        let mut anchor = None;
        loop {
        let np = nook_core::runtime().block_on(nook_core::audio::get_now_playing());
        anchor = next_anchor(anchor, &np, now_ms());
        s.publish("media", media_json(&np, anchor));
        let cadence = if np.is_playing { Duration::from_secs(1) } else { Duration::from_secs(5) };
        nook_core::runtime().block_on(nook_core::audio::wait_media_or_timeout(cadence));
        }
    });
    spawn("nook-apps", move || loop {
        store.publish("apps", Value::Array(crate::platform::running_apps()));
        std::thread::sleep(Duration::from_secs(2));
    });
}

/// Asks for the Mac's location (reduced accuracy) and publishes it as `location`. Must run on
/// the main thread: CoreLocation answers there.
pub fn request_location(store: Arc<Store>) {
    let fix = nook_core::location::begin_request();
    spawn("nook-location", move || {
        // An unbundled binary (cargo run) gets no answer at all: CoreLocation needs the app's
        // Info.plist. Don't leave mods waiting forever.
        let answer = nook_core::runtime().block_on(async { tokio::time::timeout(Duration::from_secs(15), fix).await });
        let value = match answer.unwrap_or(Ok(Err("Location didn't answer. It needs openNook.app (scripts/bundle.sh); or give the mod a city.".into()))) {
            Ok(Ok((lat, lon))) => {
                let name = nook_core::runtime().block_on(nook_core::location::begin_reverse_geocode(lat, lon)).ok().flatten();
                json!({ "latitude": lat, "longitude": lon, "name": name })
            }
            Ok(Err(e)) => json!({ "error": e }),
            Err(_) => json!({ "error": "Location request ended without an answer." }),
        };
        match value.get("error") {
            Some(e) => log::info!("location: {e}"),
            None => log::info!("location: found"),
        }
        store.publish("location", value);
    });
}

pub fn start_agents(store: Arc<Store>) {
    spawn("nook-agents", move || loop {
        let agents: Vec<Value> = nook_core::agents::snapshot().iter().map(agent_json).collect();
        store.publish("agents", Value::Array(agents));
        std::thread::sleep(nook_core::agents::poll_interval());
    });
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name(name.into()).spawn(f).expect("service thread");
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Playback position as `(elapsed seconds, at ms)`. Kept while playback follows the clock,
/// so polling doesn't change the published value every second; moves on a seek, a pause or
/// a new track.
pub fn next_anchor(prev: Option<(f64, u64)>, np: &NowPlayingData, now: u64) -> Option<(f64, u64)> {
    let elapsed = np.elapsed_time?;
    if np.is_playing {
        if let Some((e, at)) = prev.filter(|(_, at)| *at > 0) {
            let projected = e + (now.saturating_sub(at)) as f64 / 1000.0;
            if (projected - elapsed).abs() < 1.5 {
                return prev;
            }
        }
        Some((elapsed, now))
    } else {
        // Paused: the position holds; `at` 0 says "don't advance".
        Some((elapsed.floor(), 0))
    }
}

pub fn media_json(np: &NowPlayingData, anchor: Option<(f64, u64)>) -> Value {
    if np.title.as_deref().is_none_or(str::is_empty) {
        return Value::Null;
    }
    let bytes = np.artwork_base64.as_deref()
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b.trim()).ok())
        .filter(|b| !b.is_empty());
    let colors = bytes.as_deref().and_then(colors_of);
    let artwork = bytes.map(crate::images::put_art);
    json!({
        "title": np.title,
        "artist": np.artist,
        "album": np.album,
        "duration": np.duration,
        "elapsed": anchor.map(|a| a.0),
        "sampledAt": anchor.map_or(0, |a| a.1),
        "playing": np.is_playing,
        "app": np.app_name,
        "bundleId": np.bundle_id,
        "artwork": artwork,
        "colors": colors,
        "shuffle": np.shuffle,
        "repeat": np.repeat.map(|r| match r { RepeatMode::Off => "off", RepeatMode::One => "one", RepeatMode::All => "all" }),
    })
}

/// The cover's colours, worked out once per cover (the poll repeats the same one every second).
fn colors_of(bytes: &[u8]) -> Option<[String; 3]> {
    static LAST: std::sync::Mutex<Option<(u64, Option<[String; 3]>)>> = std::sync::Mutex::new(None);
    let key = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
    let mut last = LAST.lock().unwrap();
    if let Some((_, c)) = last.as_ref().filter(|(k, _)| *k == key) {
        return c.clone();
    }
    let c = crate::palette::of(bytes);
    *last = Some((key, c.clone()));
    c
}

pub fn agent_json(a: &AgentSession) -> Value {
    let kind = a.kind.label();
    json!({
        "kind": kind,
        "pid": a.pid,
        "title": a.title(),
        "project": a.project,
        "cwd": a.cwd,
        "status": match a.status { AgentStatus::Working => "working", AgentStatus::Waiting => "waiting" },
        "sessionId": a.session_id,
        "model": a.model,
        "icon": agent_icon(kind),
    })
}

/// The agent's logo, or a terminal for agents without one rune can draw.
fn agent_icon(kind: &str) -> String {
    let name = format!("agent-{}", kind.to_lowercase());
    if crate::icons::get(&name, std::path::Path::new("/")).is_some() { name } else { "terminal".into() }
}

/// Media and agent commands mods `run`. Returns false for a command this doesn't know.
pub fn command(command: &str, args: &Value) -> bool {
    use nook_core::audio;
    type Command = fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>;
    let media = |f: Command| {
        nook_core::runtime().spawn(async move {
            if let Err(e) = f().await {
                log::warn!("media command: {e}");
            }
            audio::note_media_event();
        });
    };
    match command {
        "media.toggle" | "media.play" | "media.pause" => media(|| Box::pin(audio::media_play_pause())),
        "media.next" => media(|| Box::pin(audio::media_next_track())),
        "media.previous" => media(|| Box::pin(audio::media_previous_track())),
        "media.shuffle" => media(|| Box::pin(audio::media_toggle_shuffle())),
        "media.repeat" => media(|| Box::pin(audio::media_toggle_repeat())),
        "media.seek" => {
            let seconds = args.get("seconds").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
            nook_core::runtime().spawn(async move {
                let _ = audio::media_seek(seconds).await;
                audio::note_media_event();
            });
        }
        "agents.focus" => {
            let pid = args.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32;
            std::thread::spawn(move || nook_core::agents::focus(pid));
        }
        "agents.reveal" => {
            let cwd = args.get("cwd").and_then(Value::as_str).unwrap_or_default().to_owned();
            std::thread::spawn(move || nook_core::agents::reveal(&cwd));
        }
        "open" => {
            let target = args.get("target").and_then(Value::as_str).unwrap_or_default().to_owned();
            if !target.is_empty() {
                std::thread::spawn(move || open::that_detached(target));
            }
        }
        _ => return false,
    }
    true
}
