//! Wake signal for the island's main UI loop.
//!
//! The island parks on [`wait_or_timeout`] instead of a fixed 80 ms idle
//! timer. Mouse monitors, settings writes, and other push sources call
//! [`poke`]; a ~1 s backstop covers anything that only publishes atomics.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Notify;

static WAKE: OnceLock<Notify> = OnceLock::new();
static PENDING: AtomicBool = AtomicBool::new(false);

fn wake() -> &'static Notify {
    WAKE.get_or_init(Notify::new)
}

/// Unpark the island UI loop (or remember the poke if it is between waits).
pub fn poke() {
    PENDING.store(true, Ordering::Release);
    wake().notify_waiters();
}

/// Park until a [`poke`] or `timeout`.
///
/// Subscribe before consuming the pending flag so a poke that lands in
/// between cannot be missed.
pub async fn wait_or_timeout(timeout: Duration) {
    let notified = wake().notified();
    tokio::pin!(notified);
    if PENDING.swap(false, Ordering::AcqRel) {
        return;
    }
    tokio::select! {
        _ = notified => {
            let _ = PENDING.swap(false, Ordering::AcqRel);
        }
        _ = tokio::time::sleep(timeout) => {}
    }
}
