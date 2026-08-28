//! Mirror island-local timers into Notification Center.
//!
//! `UNUserNotificationCenter` + `UNTimeIntervalNotificationTrigger`: usernoted
//! delivers the alert even if the overlay is occluded or the process exits.

pub fn request_authorization() {
    #[cfg(target_os = "macos")]
    macos::request_authorization();
}

pub fn schedule_island_timer(id: u64, remaining: u32, title: &str) {
    if remaining == 0 {
        cancel_island_timer(id);
        return;
    }
    request_authorization();
    #[cfg(target_os = "macos")]
    macos::schedule(id, remaining, title);
    #[cfg(not(target_os = "macos"))]
    let _ = title;
}

pub fn cancel_island_timer(id: u64) {
    #[cfg(target_os = "macos")]
    macos::cancel(id);
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
        unsafe {
            center.requestAuthorizationWithOptions_completionHandler(options, &handler);
        }
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
        unsafe {
            content.setTitle(&NSString::from_str(&heading));
            content.setBody(&NSString::from_str("Time is up"));
            content.setSound(Some(&UNNotificationSound::defaultSound()));
        }
        let trigger = unsafe {
            UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(
                remaining.max(1) as f64,
                false,
            )
        };
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
        unsafe {
            center.addNotificationRequest_withCompletionHandler(&request, Some(&handler));
        }
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
}
