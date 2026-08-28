//! Optional Focus hook via the Shortcuts CLI.
//!
//! macOS has no public API to set a Focus mode. The supported route is a
//! user-authored shortcut that openNook runs on pomodoro phase edges. Fire
//! and forget — never block the island tick.

pub fn run_shortcut_detached(name: Option<&str>) {
    let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
        return;
    };
    let name = name.to_string();
    crate::runtime().spawn(async move {
        if let Err(err) = run_shortcut(&name).await {
            log::debug!("focus shortcut '{name}': {err}");
        }
    });
}

pub async fn run_shortcut(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        macos::run_shortcut(name).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
        Err("Shortcuts are only available on macOS".into())
    }
}

pub async fn list_shortcuts() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        macos::list_shortcuts().await
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// Cycle None → first listed name → next → None.
pub fn cycle_shortcut(current: Option<&str>, listed: &[String]) -> Option<String> {
    if listed.is_empty() {
        return None;
    }
    let Some(current) = current.map(str::trim).filter(|name| !name.is_empty()) else {
        return Some(listed[0].clone());
    };
    match listed.iter().position(|name| name == current) {
        Some(i) if i + 1 < listed.len() => Some(listed[i + 1].clone()),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
mod macos {
    pub async fn list_shortcuts() -> Vec<String> {
        let output = tokio::process::Command::new("/usr/bin/shortcuts")
            .arg("list")
            .output()
            .await;
        match output {
            Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect(),
            Ok(output) => {
                log::debug!(
                    "shortcuts list failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                Vec::new()
            }
            Err(err) => {
                log::debug!("shortcuts list: {err}");
                Vec::new()
            }
        }
    }

    pub async fn run_shortcut(name: &str) -> Result<(), String> {
        let output = tokio::process::Command::new("/usr/bin/shortcuts")
            .arg("run")
            .arg(name)
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|err| err.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_shortcut_walks_the_list_then_clears() {
        let listed = vec!["Focus Work".into(), "Focus Break".into()];
        assert_eq!(
            cycle_shortcut(None, &listed).as_deref(),
            Some("Focus Work")
        );
        assert_eq!(
            cycle_shortcut(Some("Focus Work"), &listed).as_deref(),
            Some("Focus Break")
        );
        assert_eq!(cycle_shortcut(Some("Focus Break"), &listed), None);
        assert_eq!(cycle_shortcut(Some("missing"), &listed), None);
        assert_eq!(cycle_shortcut(None, &[]), None);
    }
}
