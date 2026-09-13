//! Installed-app lookup via Launch Services.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(15);

struct Cache {
    at: Instant,
    hits: HashMap<String, bool>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            at: Instant::now(),
            hits: HashMap::new(),
        })
    })
}

/// True when Launch Services knows an app with this bundle identifier.
pub fn is_installed(bundle_id: &str) -> bool {
    let id = bundle_id.trim();
    if id.is_empty() {
        return false;
    }
    let mut cache = match cache().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if cache.at.elapsed() > TTL {
        cache.hits.clear();
        cache.at = Instant::now();
    }
    if let Some(hit) = cache.hits.get(id) {
        return *hit;
    }
    let installed = lookup(id);
    cache.hits.insert(id.to_string(), installed);
    installed
}

pub fn any_installed(ids: &[&str]) -> bool {
    ids.iter().copied().any(is_installed)
}

fn lookup(bundle_id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_lookup(bundle_id)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bundle_id;
        false
    }
}

#[cfg(target_os = "macos")]
fn macos_lookup(bundle_id: &str) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::*;
    use std::ffi::CString;

    let Ok(cstr) = CString::new(bundle_id.replace('\0', "")) else {
        return false;
    };
    objc2::rc::autoreleasepool(|_| unsafe {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return false;
        }
        let ns: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: cstr.as_ptr()];
        let url: *mut AnyObject = msg_send![ws, URLForApplicationWithBundleIdentifier: ns];
        !url.is_null()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_id_is_not_installed() {
        assert!(!is_installed(""));
        assert!(!is_installed("   "));
        assert!(!any_installed(&[]));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finder_is_installed_and_unknown_bundles_are_not() {
        assert!(is_installed("com.apple.finder"));
        assert!(!is_installed("com.example.definitely.not.an.app"));
    }
}
