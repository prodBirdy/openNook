//! On-demand LAN sharing: LocalSend send. AirDrop is handled by the island.
//!
//! Send-only LocalSend never holds a socket at idle. Discovery binds for a
//! short window, then every socket is dropped.

pub mod localsend;

pub use localsend::{
    DeviceInfo, FileMeta, PrepareUploadResponse, TransferProgress, PROTOCOL_VERSION,
};

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Persisted sharing preferences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShareSettings {
    #[serde(default = "default_device_alias")]
    pub device_alias: String,
    /// Optional PIN appended as `?pin=` on prepare-upload.
    #[serde(default)]
    pub localsend_pin: String,
}

pub fn default_device_alias() -> String {
    "openNook".into()
}

impl Default for ShareSettings {
    fn default() -> Self {
        Self {
            device_alias: default_device_alias(),
            localsend_pin: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShareKind {
    #[default]
    Idle,
    LocalSend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SharePhase {
    #[default]
    Idle,
    Discovering,
    Picking,
    Transferring,
    Done,
    Failed,
}

/// Island-facing session. Progress is pushed from a spawn; nothing polls.
#[derive(Debug, Clone, Default)]
pub struct ShareSession {
    pub kind: ShareKind,
    pub phase: SharePhase,
    pub peers: Vec<DeviceInfo>,
    pub progress: f32,
    pub status: String,
    pub hud: Option<String>,
    pub error: Option<String>,
    pub gen: u64,
    pub paths: Vec<std::path::PathBuf>,
    /// When [`SharePhase::Failed`] was entered; used by [`Self::maybe_reset_failure`].
    #[doc(hidden)]
    pub failed_at: Option<Instant>,
}

/// Map a raw LocalSend / network error to a short island-facing message.
pub fn friendly_share_error(err: &str) -> String {
    match err {
        "Couldn't reach device" | "Timed out" | "Device not found" | "Transfer failed" => {
            return err.into();
        }
        _ => {}
    }
    let lower = err.to_ascii_lowercase();
    if lower.contains("connection refused")
        || lower.contains("network is unreachable")
        || lower.contains("no route to host")
        || lower.contains("host unreachable")
    {
        "Couldn't reach device".into()
    } else if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("deadline has elapsed")
    {
        "Timed out".into()
    } else if lower.contains("not found")
        || lower.contains("no devices")
        || lower.contains("nxdomain")
        || lower.contains("name or service not known")
    {
        "Device not found".into()
    } else {
        "Transfer failed".into()
    }
}

impl ShareSession {
    pub fn is_live(&self) -> bool {
        !matches!(self.phase, SharePhase::Idle) || self.hud.is_some()
    }

    pub fn shows_picker(&self) -> bool {
        matches!(self.phase, SharePhase::Discovering | SharePhase::Picking)
    }

    /// Enter [`SharePhase::Failed`] with a user-facing error and start the
    /// 4 s auto-clear timer (call [`Self::maybe_reset_failure`] from the UI tick).
    pub fn mark_failed(&mut self, err: impl AsRef<str>) {
        self.phase = SharePhase::Failed;
        self.error = Some(friendly_share_error(err.as_ref()));
        self.failed_at = Some(Instant::now());
        self.hud = None;
    }

    /// Clear a stale failure after 4 s, mirroring the Done auto-reset path.
    /// Returns `true` when the session was reset.
    pub fn maybe_reset_failure(&mut self) -> bool {
        let Some(at) = self.failed_at else {
            return false;
        };
        if !matches!(self.phase, SharePhase::Failed) {
            return false;
        }
        if at.elapsed() < Duration::from_secs(4) {
            return false;
        }
        let gen = self.gen;
        *self = ShareSession {
            gen,
            ..ShareSession::default()
        };
        true
    }

    pub fn compact_label(&self) -> String {
        if let Some(hud) = &self.hud {
            return hud.clone();
        }
        if let Some(err) = &self.error {
            return friendly_share_error(err);
        }
        if !self.status.is_empty() {
            return self.status.clone();
        }
        match self.phase {
            SharePhase::Discovering => "Looking…".into(),
            SharePhase::Picking => "Choose device".into(),
            SharePhase::Transferring => format!("{:.0}%", self.progress * 100.0),
            SharePhase::Done => "Sent".into(),
            SharePhase::Failed => "Transfer failed".into(),
            SharePhase::Idle => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_is_stable_and_unknown_keys_are_ignored() {
        let parsed: ShareSettings = serde_json::from_str(
            r#"{"device_alias":"openNook","legacy_host":"public","unused_flag":true}"#,
        )
        .unwrap();
        assert_eq!(parsed, ShareSettings::default());
        assert_eq!(parsed.device_alias, "openNook");
        assert!(parsed.localsend_pin.is_empty());
    }

    #[test]
    fn session_picker_and_live_flags() {
        let mut session = ShareSession::default();
        assert!(!session.is_live());
        assert!(!session.shows_picker());
        session.phase = SharePhase::Discovering;
        assert!(session.is_live());
        assert!(session.shows_picker());
        session.phase = SharePhase::Idle;
        session.hud = Some("Sent".into());
        assert!(session.is_live());
        assert_eq!(session.compact_label(), "Sent");
    }

    #[test]
    fn friendly_share_error_maps_common_failures() {
        assert_eq!(
            friendly_share_error("tcp connect: Connection refused"),
            "Couldn't reach device"
        );
        assert_eq!(friendly_share_error("request timed out"), "Timed out");
        assert_eq!(
            friendly_share_error("peer not found on LAN"),
            "Device not found"
        );
        assert_eq!(
            friendly_share_error("ssl handshake blew up"),
            "Transfer failed"
        );
    }

    #[test]
    fn mark_failed_stores_friendly_error() {
        let mut session = ShareSession::default();
        session.mark_failed("connection refused while dialing");
        assert_eq!(session.phase, SharePhase::Failed);
        assert_eq!(session.error.as_deref(), Some("Couldn't reach device"));
        assert!(session.failed_at.is_some());
        assert!(!session.maybe_reset_failure());
    }
}
