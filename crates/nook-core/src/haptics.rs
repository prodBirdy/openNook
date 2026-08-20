use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HapticPattern {
    Generic,
    Alignment,
    LevelChange,
    Light,
    #[default]
    Medium,
    Heavy,
    Selection,
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HapticConfig {
    pub pattern: HapticPattern,
    #[serde(default = "default_intensity")]
    pub intensity: f64,
}

fn default_intensity() -> f64 {
    0.6
}

impl Default for HapticConfig {
    fn default() -> Self {
        Self {
            pattern: HapticPattern::Medium,
            intensity: 0.6,
        }
    }
}

pub fn trigger(config: Option<HapticConfig>) {
    let config = config.unwrap_or_default();
    #[cfg(not(target_os = "macos"))]
    let _ = config;

    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::*;

        let manager: *mut AnyObject = msg_send![class!(NSHapticFeedbackManager), defaultPerformer];

        match config.pattern {
            HapticPattern::Generic | HapticPattern::Medium => {
                let _: () =
                    msg_send![manager, performFeedbackPattern: 0_i64, performanceTime: 1_i64];
            }
            HapticPattern::Alignment | HapticPattern::Light => {
                let _: () =
                    msg_send![manager, performFeedbackPattern: 1_i64, performanceTime: 1_i64];
            }
            HapticPattern::LevelChange | HapticPattern::Heavy => {
                let _: () =
                    msg_send![manager, performFeedbackPattern: 2_i64, performanceTime: 1_i64];
            }
            HapticPattern::Selection => {
                let _: () =
                    msg_send![manager, performFeedbackPattern: 1_i64, performanceTime: 0_i64];
            }
            HapticPattern::Success => {
                let _: () =
                    msg_send![manager, performFeedbackPattern: 1_i64, performanceTime: 1_i64];
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _: () =
                    msg_send![manager, performFeedbackPattern: 0_i64, performanceTime: 1_i64];
            }
            HapticPattern::Error => {
                for i in 0..3 {
                    let _: () =
                        msg_send![manager, performFeedbackPattern: 2_i64, performanceTime: 1_i64];
                    if i < 2 {
                        std::thread::sleep(std::time::Duration::from_millis(40));
                    }
                }
            }
        }
    }
}
