use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HapticPattern {
    #[default]
    Medium,
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HapticConfig {
    pub pattern: HapticPattern,
}

pub fn trigger(config: Option<HapticConfig>) {
    let config = config.unwrap_or_default();
    #[cfg(not(target_os = "macos"))]
    let _ = config;

    #[cfg(target_os = "macos")]
    {
        // Multi-tap patterns sleep between strikes; never do that on the UI thread.
        match config.pattern {
            HapticPattern::Success => {
                let _ = std::thread::Builder::new()
                    .name("nook-haptic".into())
                    .spawn(move || fire(config));
            }
            HapticPattern::Medium => fire(config),
        }
    }
}

#[cfg(target_os = "macos")]
fn fire(config: HapticConfig) {
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::*;

        let manager: *mut AnyObject = msg_send![class!(NSHapticFeedbackManager), defaultPerformer];

        match config.pattern {
            HapticPattern::Medium => {
                let _: () =
                    msg_send![manager, performFeedbackPattern: 0_i64, performanceTime: 1_i64];
            }
            HapticPattern::Success => {
                let _: () =
                    msg_send![manager, performFeedbackPattern: 1_i64, performanceTime: 1_i64];
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _: () =
                    msg_send![manager, performFeedbackPattern: 0_i64, performanceTime: 1_i64];
            }
        }
    }
}
