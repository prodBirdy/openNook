//! Browser tab URL → YouTube thumbnail or site icon when MediaRemote
//! artwork is missing or generic.
//!
//! MediaRemote is still the metadata/control channel. Browsers often omit
//! useful artwork, so we read the front (or title-matching) tab via
//! AppleScript and resolve a picture from the URL.

use crate::utils::{base64_encode, read_response_limited};

/// Bundle IDs that can report a tab URL via AppleScript.
pub const BROWSER_BUNDLE_IDS: &[&str] = &[
    "com.apple.Safari",
    "com.apple.Safari.WebApp",
    "com.apple.WebKit.GPU",
    "com.google.Chrome",
    "com.google.Chrome.canary",
    "com.brave.Browser",
    "com.microsoft.edgemac",
    "company.thebrowser.Browser",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
];

/// Chrome-family and Safari bundle IDs that can report a tab URL.
pub fn is_browser(app_name: Option<&str>, bundle_id: Option<&str>) -> bool {
    applescript_app(app_name, bundle_id).is_some()
}

/// True for a browser app or one of its helpers (`com.google.Chrome.helper`).
pub fn is_browser_bundle(id: &str) -> bool {
    BROWSER_BUNDLE_IDS
        .iter()
        .any(|known| id == *known || id.starts_with(&format!("{known}.")))
}

/// `https://meet.google.com/xxx-xxxx-xxx` (and the same host with a longer code).
pub fn is_meet_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return false;
    }
    let host = parsed.host_str().unwrap_or("").trim_start_matches("www.");
    if !host.eq_ignore_ascii_case("meet.google.com") {
        return false;
    }
    let mut segs = match parsed.path_segments() {
        Some(s) => s,
        None => return false,
    };
    let code = segs.next().unwrap_or("");
    is_meet_code(code)
}

fn is_meet_code(code: &str) -> bool {
    let parts: Vec<&str> = code.split('-').collect();
    (2..=4).contains(&parts.len())
        && parts.iter().all(|p| {
            (3..=4).contains(&p.len()) && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeetTab {
    pub app: String,
    pub url: String,
}

pub fn youtube_video_id(input: &str) -> Option<String> {
    let url = reqwest::Url::parse(input).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?.trim_start_matches("www.");
    if host.eq_ignore_ascii_case("youtu.be") {
        return url.path_segments()?.next().and_then(valid_video_id);
    }
    if host.eq_ignore_ascii_case("youtube.com")
        || host.to_ascii_lowercase().ends_with(".youtube.com")
    {
        if url.path() == "/watch" {
            return url
                .query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.into_owned())
                .filter(|id| valid_video_id(id).is_some());
        }
        let mut segs = url.path_segments()?;
        let kind = segs.next()?;
        if matches!(kind, "shorts" | "embed" | "live" | "v") {
            return segs.next().and_then(valid_video_id);
        }
    }
    None
}

pub fn youtube_thumbnail_candidates(video_id: &str) -> Vec<String> {
    [
        "maxresdefault",
        "sddefault",
        "hqdefault",
        "mqdefault",
        "default",
    ]
    .into_iter()
    .map(|kind| format!("https://i.ytimg.com/vi/{video_id}/{kind}.jpg"))
    .collect()
}

fn valid_video_id(id: &str) -> Option<String> {
    let id = id.trim();
    if (11..=12).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Some(id.to_string())
    } else {
        None
    }
}

pub fn applescript_app<'a>(app_name: Option<&'a str>, bundle_id: Option<&str>) -> Option<&'a str> {
    if let Some(id) = bundle_id {
        let name = match id {
            "com.apple.Safari" | "com.apple.Safari.WebApp" | "com.apple.WebKit.GPU" => "Safari",
            "com.google.Chrome" => "Google Chrome",
            "com.google.Chrome.canary" => "Google Chrome Canary",
            "com.brave.Browser" => "Brave Browser",
            "com.microsoft.edgemac" => "Microsoft Edge",
            "company.thebrowser.Browser" => "Arc",
            "com.operasoftware.Opera" => "Opera",
            "com.vivaldi.Vivaldi" => "Vivaldi",
            _ => "",
        };
        if !name.is_empty() {
            return Some(name);
        }
    }
    match app_name? {
        "Safari" => Some("Safari"),
        "Chrome" => Some("Google Chrome"),
        "Brave" => Some("Brave Browser"),
        "Edge" => Some("Microsoft Edge"),
        "Browser" => Some("Arc"),
        other if other.contains("Chrome") => Some("Google Chrome"),
        _ => None,
    }
}

/// Fetch a YouTube thumb or site icon for the playing browser tab.
pub async fn resolve_artwork(
    app_name: Option<&str>,
    bundle_id: Option<&str>,
    title: Option<&str>,
) -> Option<String> {
    let app = applescript_app(app_name, bundle_id)?;
    let url = active_tab_url(app, title).await?;
    if let Some(id) = youtube_video_id(&url) {
        for candidate in youtube_thumbnail_candidates(&id) {
            if let Some(art) = fetch_image(&candidate, 2_000).await {
                return Some(art);
            }
        }
    }
    for candidate in favicon_candidates(&url) {
        if let Some(art) = fetch_image(&candidate, 80).await {
            return Some(art);
        }
    }
    None
}

fn favicon_candidates(page: &str) -> Vec<String> {
    let Ok(url) = reqwest::Url::parse(page) else {
        return Vec::new();
    };
    if url.scheme() != "https" {
        return Vec::new();
    }
    let Some(host) = url.host_str() else {
        return Vec::new();
    };
    if is_private_host(host) {
        return Vec::new();
    }
    vec![format!(
        "https://www.google.com/s2/favicons?sz=128&domain={host}"
    )]
}

fn is_private_host(host: &str) -> bool {
    let h = host.trim_end_matches('.');
    h.eq_ignore_ascii_case("localhost")
        || h.parse::<std::net::IpAddr>().is_ok_and(|ip| match ip {
            std::net::IpAddr::V4(v) => {
                v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified()
            }
            std::net::IpAddr::V6(v) => v.is_loopback() || v.is_unspecified(),
        })
}

async fn fetch_image(url: &str, min_bytes: usize) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?;
    if !matches!(host, "i.ytimg.com" | "www.google.com") {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    let response = client.get(parsed).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let is_image = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("image/"));
    if !is_image {
        return None;
    }
    let bytes = read_response_limited(response, 2 * 1024 * 1024)
        .await
        .ok()?;
    if bytes.len() < min_bytes {
        return None;
    }
    Some(base64_encode(&bytes))
}

#[cfg(target_os = "macos")]
async fn active_tab_url(app: &str, title: Option<&str>) -> Option<String> {
    let script = tab_script(app, title.unwrap_or(""));
    let stdout = run_osascript(&script).await?;
    let url = stdout.trim();
    if url.is_empty() || !url.starts_with("http") {
        return None;
    }
    Some(url.to_string())
}

#[cfg(not(target_os = "macos"))]
async fn active_tab_url(_app: &str, _title: Option<&str>) -> Option<String> {
    None
}

fn tab_script(app: &str, title: &str) -> String {
    let needle = applescript_escape(title.trim_end_matches(" - YouTube").trim());
    if app == "Safari" {
        format!(
            r#"tell application "Safari"
  if (count of windows) is 0 then return ""
  set needle to "{needle}"
  if needle is not "" then
    repeat with w in windows
      repeat with t in tabs of w
        try
          if (name of t) contains needle then return URL of t
        end try
      end repeat
    end repeat
  end if
  return URL of front document
end tell"#
        )
    } else {
        format!(
            r#"tell application "{app}"
  if (count of windows) is 0 then return ""
  set needle to "{needle}"
  if needle is not "" then
    repeat with w in windows
      repeat with t in tabs of w
        try
          if (title of t) contains needle then return URL of t
        end try
      end repeat
    end repeat
  end if
  return URL of active tab of front window
end tell"#
        )
    }
}

fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
async fn run_osascript(script: &str) -> Option<String> {
    use tokio::process::Command;
    let child = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(std::time::Duration::from_millis(1500), child)
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        log::debug!("browser tab osascript failed: {err}");
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

const MEET_JS_MUTE: &str = r#"(function(){var b=document.querySelector('[data-is-muted]')||document.querySelector('button[aria-label*="microphone" i]');if(!b)return 'missing';b.click();return b.getAttribute('data-is-muted')||'clicked';})()"#;
const MEET_JS_LEAVE: &str = r#"(function(){var b=document.querySelector('[aria-label*="Leave call" i]')||document.querySelector('button[aria-label*="leave" i]');if(!b)return 'missing';b.click();return 'clicked';})()"#;

fn meet_find_script(app: &str) -> String {
    if app == "Safari" {
        r#"tell application "Safari"
  if (count of windows) is 0 then return ""
  repeat with w in windows
    repeat with t in tabs of w
      try
        set u to URL of t
        if u contains "meet.google.com/" then return u
      end try
    end repeat
  end repeat
  return ""
end tell"#
            .into()
    } else {
        format!(
            r#"tell application "{app}"
  if (count of windows) is 0 then return ""
  repeat with w in windows
    repeat with t in tabs of w
      try
        set u to URL of t
        if u contains "meet.google.com/" then return u
      end try
    end repeat
  end repeat
  return ""
end tell"#
        )
    }
}

fn meet_activate_script(app: &str) -> String {
    if app == "Safari" {
        r#"tell application "Safari"
  repeat with w in windows
    repeat with t in tabs of w
      try
        if (URL of t) contains "meet.google.com/" then
          set current tab of w to t
          set index of w to 1
          activate
          return "ok"
        end if
      end try
    end repeat
  end repeat
  return ""
end tell"#
            .into()
    } else {
        format!(
            r#"tell application "{app}"
  repeat with w in windows
    set i to 0
    repeat with t in tabs of w
      set i to i + 1
      try
        if (URL of t) contains "meet.google.com/" then
          set active tab index of w to i
          set index of w to 1
          activate
          return "ok"
        end if
      end try
    end repeat
  end repeat
  return ""
end tell"#
        )
    }
}

fn meet_js_script(app: &str, js: &str) -> String {
    let escaped = applescript_escape(js);
    if app == "Safari" {
        format!(
            r#"tell application "Safari"
  repeat with w in windows
    repeat with t in tabs of w
      try
        if (URL of t) contains "meet.google.com/" then
          tell t to do JavaScript "{escaped}"
          return result as string
        end if
      end try
    end repeat
  end repeat
  return ""
end tell"#
        )
    } else {
        format!(
            r#"tell application "{app}"
  repeat with w in windows
    repeat with t in tabs of w
      try
        if (URL of t) contains "meet.google.com/" then
          tell t to execute javascript "{escaped}"
          return result as string
        end if
      end try
    end repeat
  end repeat
  return ""
end tell"#
        )
    }
}

fn meet_app_names() -> &'static [&'static str] {
    &[
        "Safari",
        "Google Chrome",
        "Google Chrome Canary",
        "Brave Browser",
        "Microsoft Edge",
        "Arc",
        "Opera",
        "Vivaldi",
    ]
}

/// Blocking osascript. Safe inside the Core Tokio runtime (no nested `block_on`).
pub fn run_osascript_blocking(script: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .ok()?;
        if !output.status.success() {
            log::debug!(
                "osascript failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = script;
        None
    }
}

pub fn find_meet_tab_blocking() -> Option<MeetTab> {
    for app in meet_app_names() {
        if let Some(url) = run_osascript_blocking(&meet_find_script(app)) {
            if is_meet_url(&url) {
                return Some(MeetTab {
                    app: (*app).to_string(),
                    url,
                });
            }
        }
    }
    None
}

pub async fn find_meet_tab() -> Option<MeetTab> {
    #[cfg(target_os = "macos")]
    {
        for app in meet_app_names() {
            if let Some(url) = run_osascript(&meet_find_script(app)).await {
                if is_meet_url(&url) {
                    return Some(MeetTab {
                        app: (*app).to_string(),
                        url,
                    });
                }
            }
        }
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn activate_meet_tab_blocking() -> bool {
    if let Some(tab) = find_meet_tab_blocking() {
        return run_osascript_blocking(&meet_activate_script(&tab.app))
            .is_some_and(|s| s == "ok");
    }
    false
}

pub async fn activate_meet_tab() -> bool {
    #[cfg(target_os = "macos")]
    {
        if let Some(tab) = find_meet_tab().await {
            return run_osascript(&meet_activate_script(&tab.app))
                .await
                .is_some_and(|s| s == "ok");
        }
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Opt-in: Chrome/Safari "Allow JavaScript from Apple Events".
pub fn meet_click_mute_js() -> Option<String> {
    let tab = find_meet_tab_blocking()?;
    let out = run_osascript_blocking(&meet_js_script(&tab.app, MEET_JS_MUTE))?;
    if out == "missing" || out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn meet_click_leave_js() -> bool {
    let Some(tab) = find_meet_tab_blocking() else {
        return false;
    };
    run_osascript_blocking(&meet_js_script(&tab.app, MEET_JS_LEAVE))
        .is_some_and(|s| s == "clicked")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_watch_and_short_urls() {
        assert_eq!(
            youtube_video_id("https://www.youtube.com/watch?v=dQw4w9wgGcQ").as_deref(),
            Some("dQw4w9wgGcQ")
        );
        assert_eq!(
            youtube_video_id("https://youtu.be/dQw4w9wgGcQ").as_deref(),
            Some("dQw4w9wgGcQ")
        );
        assert_eq!(
            youtube_video_id("https://www.youtube.com/shorts/dQw4w9wgGcQ").as_deref(),
            Some("dQw4w9wgGcQ")
        );
        assert_eq!(
            youtube_video_id("https://music.youtube.com/watch?v=dQw4w9wgGcQ&list=x").as_deref(),
            Some("dQw4w9wgGcQ")
        );
        assert!(youtube_video_id("https://example.com/watch?v=dQw4w9wgGcQ").is_none());
        assert!(youtube_video_id("https://youtube.com.evil.example/watch?v=dQw4w9wgGcQ").is_none());
    }

    #[test]
    fn thumbnails_are_https_ytimg() {
        let urls = youtube_thumbnail_candidates("dQw4w9wgGcQ");
        assert!(urls[0].starts_with("https://i.ytimg.com/vi/dQw4w9wgGcQ/"));
        assert!(urls.iter().any(|u| u.ends_with("hqdefault.jpg")));
    }

    #[test]
    fn favicon_only_https_public_hosts() {
        let urls = favicon_candidates("https://open.spotify.com/track/1");
        assert_eq!(urls.len(), 1);
        assert!(urls[0].starts_with("https://www.google.com/s2/favicons?"));
        assert!(urls[0].contains("domain=open.spotify.com"));
        assert!(favicon_candidates("http://example.com/").is_empty());
        assert!(favicon_candidates("https://127.0.0.1/").is_empty());
        assert!(favicon_candidates("https://localhost/x").is_empty());
    }

    #[test]
    fn chrome_and_safari_are_browsers() {
        assert!(is_browser(Some("Chrome"), Some("com.google.Chrome")));
        assert!(is_browser(Some("Safari"), None));
        assert!(!is_browser(Some("Spotify"), Some("com.spotify.client")));
        assert!(!is_browser(None, None));
        assert!(is_browser_bundle("com.google.Chrome"));
        assert!(is_browser_bundle("com.google.Chrome.helper"));
        assert!(!is_browser_bundle("us.zoom.xos"));
    }

    #[test]
    fn meet_urls_require_the_code_path() {
        assert!(is_meet_url("https://meet.google.com/abc-defg-hij"));
        assert!(is_meet_url("https://www.meet.google.com/aaa-bbbb-ccc"));
        assert!(!is_meet_url("https://meet.google.com/landing"));
        assert!(!is_meet_url("https://meet.google.com/"));
        assert!(!is_meet_url("https://zoom.us/j/123"));
        assert!(!is_meet_url("https://evil.example/meet.google.com/abc-defg-hij"));
    }
}
