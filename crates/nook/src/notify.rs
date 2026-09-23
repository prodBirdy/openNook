//! Mirror island-local timers into Notification Center.
//!
//! `UNUserNotificationCenter` + `UNTimeIntervalNotificationTrigger`: usernoted
//! delivers the alert even if the overlay is occluded or the process exits.

pub fn request_authorization() {
    #[cfg(target_os = "macos")]
    if macos::available() {
        macos::request_authorization();
    }
}

pub fn schedule_island_timer(id: u64, remaining: u32, title: &str) {
    if remaining == 0 {
        cancel_island_timer(id);
        return;
    }
    #[cfg(target_os = "macos")]
    if macos::available() {
        request_authorization();
        macos::schedule(id, remaining, title);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = title;
}

pub fn cancel_island_timer(id: u64) {
    #[cfg(target_os = "macos")]
    if macos::available() {
        macos::cancel(id);
    }
}

pub fn identifier(id: u64) -> String {
    format!("nook.island.timer.{id}")
}

#[cfg(target_os = "macos")]
mod macos {
    use super::identifier;
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSArray, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNNotificationSound, UNTimeIntervalNotificationTrigger, UNUserNotificationCenter,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    static ASKED: AtomicBool = AtomicBool::new(false);
    static LOGGED_SKIP: AtomicBool = AtomicBool::new(false);

    /// `UNUserNotificationCenter` throws `NSInternalInconsistencyException`
    /// (`bundleProxyForCurrentProcess is nil`) from inside `dispatch_once`
    /// when this process is not an app bundle. `@try` cannot catch that, so
    /// `cargo run` must not touch the class at all.
    pub fn available() -> bool {
        let ok = unsafe {
            use objc2::runtime::AnyObject;
            use objc2::{class, msg_send};
            let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
            if bundle.is_null() {
                false
            } else {
                let ident: *mut AnyObject = msg_send![bundle, bundleIdentifier];
                !ident.is_null()
            }
        };
        if !ok && !LOGGED_SKIP.swap(true, Ordering::Relaxed) {
            log::info!("timer notifications skipped: process has no bundle identifier");
        }
        ok
    }

    pub fn request_authorization() {
        if ASKED.swap(true, Ordering::SeqCst) {
            return;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let options = UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound;
        let handler = RcBlock::new(move |granted: Bool, error: *mut NSError| {
            if !error.is_null() {
                let err = unsafe { &*error };
                log::debug!("notification auth error: {err:?}");
            } else {
                log::debug!("notification auth granted={}", granted.as_bool());
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(options, &handler);
    }

    pub fn schedule(id: u64, remaining: u32, title: &str) {
        let ident = identifier(id);
        cancel(id);
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let content = UNMutableNotificationContent::new();
        let heading = if title.trim().is_empty() {
            "Timer".to_string()
        } else {
            title.to_string()
        };
        content.setTitle(&NSString::from_str(&heading));
        content.setBody(&NSString::from_str("Time is up"));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let trigger = UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(
            remaining.max(1) as f64,
            false,
        );
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&ident),
            &content,
            Some(&trigger),
        );
        let handler = RcBlock::new(move |error: *mut NSError| {
            if !error.is_null() {
                let err = unsafe { &*error };
                log::debug!("schedule timer notification: {err:?}");
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&handler));
    }

    pub fn cancel(id: u64) {
        let ident = identifier(id);
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let ids = NSArray::from_slice(&[NSString::from_str(&ident).as_ref()]);
        center.removePendingNotificationRequestsWithIdentifiers(&ids);
        center.removeDeliveredNotificationsWithIdentifiers(&ids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_is_stable_per_timer() {
        assert_eq!(identifier(3), "nook.island.timer.3");
        assert_ne!(identifier(1), identifier(2));
    }

    /// `cargo test` / `cargo run` are not app bundles. Scheduling must return
    /// instead of aborting on the UserNotifications exception.
    #[test]
    fn schedule_does_not_abort_without_a_bundle() {
        schedule_island_timer(7, 30, "regression");
        cancel_island_timer(7);
        schedule_island_timer(7, 0, "clears");
    }
}
