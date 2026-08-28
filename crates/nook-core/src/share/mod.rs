//! On-demand LAN sharing: LocalSend send + drop-a-link uploads.
//!
//! Send-only LocalSend never holds a socket at idle. Discovery binds for a
//! short window, then every socket is dropped. Receive mode is a settings
//! flag only in this release (default off) — no listener is started.

pub mod localsend;
pub mod upload;

pub use localsend::{
    DeviceInfo, FileMeta, PrepareUploadResponse, TransferProgress, PROTOCOL_VERSION,
};
pub use upload::{LinkBackend, LinkUpload, UploadResult};

use serde::{Deserialize, Serialize};

/// How a finished drop-a-link upload should be hosted.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LinkBackendKind {
    /// Community host at 0x0.st. Public-by-URL, 512 MiB cap, 30–365 day retention.
    #[default]
    ZeroXZero,
    /// HTTP PUT + Basic auth. The configured base URL must be publicly readable.
    WebDav,
    /// S3-compatible PUT. Permanent links need a public bucket or CloudFront;
    /// otherwise a 7-day presigned GET is issued.
    S3,
}

impl LinkBackendKind {
    pub const ALL: [Self; 3] = [Self::ZeroXZero, Self::WebDav, Self::S3];

    pub fn caption(self) -> &'static str {
        match self {
            Self::ZeroXZero => "0x0.st",
            Self::WebDav => "WebDAV",
            Self::S3 => "S3",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::ZeroXZero => Self::WebDav,
            Self::WebDav => Self::S3,
            Self::S3 => Self::ZeroXZero,
        }
    }
}

/// Persisted sharing preferences. Secrets are stripped on macOS serialize
/// and stored in the Keychain instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShareSettings {
    #[serde(default = "default_device_alias")]
    pub device_alias: String,
    /// Opt-in LocalSend receive. Default off; this release does not bind a
    /// receive server even when the flag is on (phase 2).
    #[serde(default)]
    pub localsend_receive: bool,
    /// Optional PIN appended as `?pin=` on prepare-upload.
    #[serde(default)]
    pub localsend_pin: String,
    #[serde(default)]
    pub link_backend: LinkBackendKind,
    #[serde(default)]
    pub webdav_url: String,
    #[serde(default)]
    pub webdav_username: String,
    #[serde(default, skip_serializing)]
    pub webdav_password: String,
    #[serde(default)]
    pub s3_bucket: String,
    #[serde(default)]
    pub s3_region: String,
    #[serde(default)]
    pub s3_endpoint: String,
    #[serde(default, skip_serializing)]
    pub s3_access_key: String,
    #[serde(default, skip_serializing)]
    pub s3_secret_key: String,
    /// Public base for permanent S3 links (`https://cdn.example/`). Empty
    /// falls back to a 7-day presigned GET.
    #[serde(default)]
    pub s3_public_base: String,
}

pub fn default_device_alias() -> String {
    "openNook".into()
}

impl Default for ShareSettings {
    fn default() -> Self {
        Self {
            device_alias: default_device_alias(),
            localsend_receive: false,
            localsend_pin: String::new(),
            link_backend: LinkBackendKind::default(),
            webdav_url: String::new(),
            webdav_username: String::new(),
            webdav_password: String::new(),
            s3_bucket: String::new(),
            s3_region: String::new(),
            s3_endpoint: String::new(),
            s3_access_key: String::new(),
            s3_secret_key: String::new(),
            s3_public_base: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShareKind {
    #[default]
    Idle,
    LocalSend,
    Link,
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
}

impl ShareSession {
    pub fn is_live(&self) -> bool {
        !matches!(self.phase, SharePhase::Idle) || self.hud.is_some()
    }

    pub fn shows_picker(&self) -> bool {
        matches!(self.phase, SharePhase::Discovering | SharePhase::Picking)
    }

    pub fn compact_label(&self) -> String {
        if let Some(hud) = &self.hud {
            return hud.clone();
        }
        if let Some(err) = &self.error {
            return err.clone();
        }
        if !self.status.is_empty() {
            return self.status.clone();
        }
        match self.phase {
            SharePhase::Discovering => "Looking…".into(),
            SharePhase::Picking => "Choose device".into(),
            SharePhase::Transferring => format!("{:.0}%", self.progress * 100.0),
            SharePhase::Done => "Sent".into(),
            SharePhase::Failed => "Failed".into(),
            SharePhase::Idle => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_defaults_off_and_alias_is_stable() {
        let parsed: ShareSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, ShareSettings::default());
        assert!(!parsed.localsend_receive);
        assert_eq!(parsed.device_alias, "openNook");
        assert_eq!(parsed.link_backend, LinkBackendKind::ZeroXZero);
    }

    #[test]
    fn secrets_are_omitted_from_json() {
        let mut settings = ShareSettings::default();
        settings.webdav_password = "dav-secret".into();
        settings.s3_access_key = "AKIA".into();
        settings.s3_secret_key = "s3-secret".into();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("dav-secret"));
        assert!(!json.contains("s3-secret"));
        assert!(!json.contains("webdav_password"));
        assert!(!json.contains("s3_access_key"));
        assert!(!json.contains("s3_secret_key"));
    }

    #[test]
    fn backend_cycle_visits_every_host() {
        assert_eq!(LinkBackendKind::ZeroXZero.caption(), "0x0.st");
        assert_eq!(LinkBackendKind::ZeroXZero.next(), LinkBackendKind::WebDav);
        assert_eq!(LinkBackendKind::WebDav.next(), LinkBackendKind::S3);
        assert_eq!(LinkBackendKind::S3.next(), LinkBackendKind::ZeroXZero);
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
        session.hud = Some("Link copied".into());
        assert!(session.is_live());
        assert_eq!(session.compact_label(), "Link copied");
    }
}
