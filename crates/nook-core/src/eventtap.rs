//! Accessibility permission helpers for widget controls.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    Unsupported,
}

pub fn accessibility_status() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        if crate::notifications::ax_trusted(false) {
            PermissionStatus::Granted
        } else {
            PermissionStatus::Denied
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionStatus::Unsupported
    }
}

pub fn request_accessibility() -> bool {
    crate::notifications::ax_trusted(true)
}

pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    {
        let _ = open::that(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        );
    }
}
