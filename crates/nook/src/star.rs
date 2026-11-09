//! Asks once for a star on GitHub, after an hour of use: a card at the foot of the open island.
//! Use is counted a minute at a time while someone is at the Mac (input in the last five
//! minutes), kept in `data/star.json` so it adds up across launches. Starring or dismissing the
//! card ends it for good.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

pub const REPO: &str = "https://github.com/prodBirdy/openNook";
const AFTER_SECS: u64 = 60 * 60;
const TICK: Duration = Duration::from_secs(60);
/// Longer than this without input, and the minute doesn't count.
const IDLE_SECS: f64 = 5.0 * 60.0;

#[derive(Default, Serialize, Deserialize)]
struct State {
    /// Seconds of use so far.
    used: u64,
    /// The card was answered (starred or dismissed): never again.
    #[serde(default)]
    answered: bool,
}

fn path() -> PathBuf {
    crate::home::app_dir().join("data").join("star.json")
}

fn load() -> State {
    std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(state: &State) {
    let p = path();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let _ = std::fs::write(p, serde_json::to_string(state).unwrap_or_default());
}

/// Counts use and calls `ask` once the hour is up (at once, if it already is).
pub fn watch(ask: impl Fn() + Send + 'static) {
    std::thread::Builder::new().name("nook-star".into()).spawn(move || {
        let mut state = load();
        while !state.answered {
            if state.used >= AFTER_SECS {
                ask();
                return;
            }
            std::thread::sleep(TICK);
            // Read again: another copy of openNook, or the card, may have written since.
            state = load();
            if !state.answered && crate::platform::idle_secs() < IDLE_SECS {
                state.used += TICK.as_secs();
                save(&state);
            }
        }
    }).expect("star thread");
}

/// The card was starred or dismissed.
pub fn answered() {
    save(&State { answered: true, ..load() });
}
