use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct CalendarEvent {
    pub title: String,
    pub start_date: f64, // Timestamp
    pub end_date: f64,   // Timestamp
    pub location: Option<String>,
    pub is_all_day: bool,
    pub color: String,
}

#[derive(Serialize, Clone)]
pub struct Reminder {
    pub id: String,
    pub title: String,
    pub due_date: Option<f64>,
    pub priority: i32,
    pub is_completed: bool,
    pub list_name: String,
    pub list_color: String,
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::rc::Retained;
    use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEventStore};
    use objc2_foundation::{MainThreadMarker, NSDate};
    use std::sync::OnceLock;
    use tokio::sync::oneshot::Sender;

    // Wrapper to force Sync implementation for EKEventStore
    // EKEventStore is generally thread-safe on macOS
    #[derive(Clone)]
    struct SyncEventStore(Retained<EKEventStore>);
    unsafe impl Sync for SyncEventStore {}
    unsafe impl Send for SyncEventStore {}

    // Static store reference
    static EVENT_STORE: OnceLock<SyncEventStore> = OnceLock::new();

    fn get_store() -> Option<&'static SyncEventStore> {
        EVENT_STORE.get_or_init(|| {
            // EKEventStore initialization
            // We'll trust that we call this from command which is on some thread?
            // But to be safe with MTM:
            let _mtm = unsafe { MainThreadMarker::new_unchecked() };
            let store = unsafe { EKEventStore::new() };
            SyncEventStore(store)
        });
        EVENT_STORE.get()
    }

    pub async fn request_access() -> Result<bool, String> {
        let store = get_store().ok_or("Failed to initialize EventStore")?;

        let status_events =
            unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
        let status_reminders =
            unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Reminder) };

        // Check Events
        if status_events == EKAuthorizationStatus::NotDetermined {
            println!("Requesting Calendar Access...");
            let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
            let tx: std::sync::Mutex<Option<Sender<bool>>> = std::sync::Mutex::new(Some(tx));

            {
                let handler = block2::RcBlock::new(
                    move |granted: objc2::runtime::Bool, _err: *mut objc2_foundation::NSError| {
                        if let Ok(mut tx_guard) = tx.lock() {
                            if let Some(tx) = tx_guard.take() {
                                let _ = tx.send(granted.as_bool());
                            }
                        }
                    },
                );

                unsafe {
                    let block_ref = &*handler;
                    let block_ptr = block_ref as *const block2::Block<_> as *mut block2::Block<_>;
                    #[allow(deprecated)]
                    store
                        .0
                        .requestAccessToEntityType_completion(EKEntityType::Event, block_ptr);
                }
            }

            // Wait for user response
            let _ = rx.await;
        }

        // Check Reminders
        if status_reminders == EKAuthorizationStatus::NotDetermined {
            println!("Requesting Reminders Access...");
            let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
            let tx: std::sync::Mutex<Option<Sender<bool>>> = std::sync::Mutex::new(Some(tx));

            {
                let handler = block2::RcBlock::new(
                    move |granted: objc2::runtime::Bool, _err: *mut objc2_foundation::NSError| {
                        if let Ok(mut tx_guard) = tx.lock() {
                            if let Some(tx) = tx_guard.take() {
                                let _ = tx.send(granted.as_bool());
                            }
                        }
                    },
                );

                unsafe {
                    let block_ref = &*handler;
                    let block_ptr = block_ref as *const block2::Block<_> as *mut block2::Block<_>;
                    #[allow(deprecated)]
                    store
                        .0
                        .requestAccessToEntityType_completion(EKEntityType::Reminder, block_ptr);
                }
            }
            let _ = rx.await;
        }

        Ok(true)
    }

    pub fn get_events(days_ahead: i64) -> Vec<CalendarEvent> {
        let events_list = Vec::new();
        let _store = match get_store() {
            Some(s) => &s.0,
            None => return events_list,
        };

        // NSDate methods are safe in recent objc2 versions if they don't take pointers
        // But checking docs, NSDate::date() is often safe.
        // If the previous error said "unnecessary unsafe block", then they are safe.
        let _now = NSDate::date();
        let _end = NSDate::dateWithTimeIntervalSinceNow((days_ahead * 24 * 60 * 60) as f64);

        // Logic to fetch events will go here when we have verified bindings

        events_list
    }
}

// Public commands

#[tauri::command]
pub async fn request_calendar_access() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        macos::request_access().await
    }
    #[cfg(not(target_os = "macos"))]
    Ok(true)
}

#[tauri::command]
pub async fn get_upcoming_events() -> Result<Vec<CalendarEvent>, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(macos::get_events(7))
    }
    #[cfg(not(target_os = "macos"))]
    Ok(vec![])
}

#[tauri::command]
pub async fn get_reminders() -> Result<Vec<Reminder>, String> {
    // Placeholder
    Ok(vec![])
}
