use log;
use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct CalendarEvent {
    pub id: String,
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
    use objc2_foundation::{MainThreadMarker, NSCalendar, NSCalendarUnit, NSDate};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;
    use tokio::sync::oneshot::Sender;

    // Flags to prevent duplicate concurrent access requests
    static ACCESS_REQUEST_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
    static ACCESS_ALREADY_REQUESTED: AtomicBool = AtomicBool::new(false);

    // Wrapper to force Sync implementation for EKEventStore
    // EKEventStore is generally thread-safe on macOS
    #[derive(Clone)]
    struct SyncEventStore(Retained<EKEventStore>);
    unsafe impl Sync for SyncEventStore {}
    unsafe impl Send for SyncEventStore {}

    use std::sync::Mutex;
    use std::time::{Duration, SystemTime};

    // Cache generic struct
    struct Cache<T> {
        data: T,
        last_fetched: SystemTime,
    }

    impl<T> Cache<T> {
        fn new(data: T) -> Self {
            Self {
                data,
                last_fetched: SystemTime::now(),
            }
        }

        fn is_valid(&self, duration: Duration) -> bool {
            SystemTime::now()
                .duration_since(self.last_fetched)
                .map(|d| d < duration)
                .unwrap_or(false)
        }
    }

    // Static caches
    static EVENTS_CACHE: OnceLock<Mutex<Cache<Vec<CalendarEvent>>>> = OnceLock::new();
    static REMINDERS_CACHE: OnceLock<Mutex<Cache<Vec<Reminder>>>> = OnceLock::new();

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
        // If already requested, return immediately
        if ACCESS_ALREADY_REQUESTED.load(Ordering::SeqCst) {
            return Ok(true);
        }

        // Prevent concurrent requests
        if ACCESS_REQUEST_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(true);
        }

        let result = request_access_inner().await;

        // Mark as requested regardless of outcome
        ACCESS_ALREADY_REQUESTED.store(true, Ordering::SeqCst);
        ACCESS_REQUEST_IN_PROGRESS.store(false, Ordering::SeqCst);

        result
    }

    async fn request_access_inner() -> Result<bool, String> {
        let store = get_store().ok_or("Failed to initialize EventStore")?;

        let status_events =
            unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
        let status_reminders =
            unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Reminder) };

        log::debug!(
            "Calendar authorization status: {:?}, Reminders status: {:?}",
            status_events,
            status_reminders
        );

        // Check Events - only request if NotDetermined
        if status_events == EKAuthorizationStatus::NotDetermined {
            log::info!("Requesting Calendar Access...");
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
            match rx.await {
                Ok(granted) => log::info!("Calendar access granted: {}", granted),
                Err(_) => log::warn!("Calendar access request cancelled"),
            }
        } else {
            log::debug!("Calendar access already determined: {:?}", status_events);
        }

        // Check Reminders - only request if NotDetermined
        if status_reminders == EKAuthorizationStatus::NotDetermined {
            log::info!("Requesting Reminders Access...");
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
            match rx.await {
                Ok(granted) => log::info!("Reminders access granted: {}", granted),
                Err(_) => log::warn!("Reminders access request cancelled"),
            }
        } else {
            log::debug!(
                "Reminders access already determined: {:?}",
                status_reminders
            );
        }

        Ok(true)
    }

    pub fn get_events(days_ahead: i64, force_refresh: bool) -> Vec<CalendarEvent> {
        // Check cache first
        if !force_refresh {
            if let Some(cache_mutex) = EVENTS_CACHE.get() {
                if let Ok(cache) = cache_mutex.lock() {
                    if cache.is_valid(Duration::from_secs(600)) {
                        // 10 minutes
                        return cache.data.clone();
                    }
                }
            }
        }

        log::debug!("Fetching fresh calendar events...");

        let mut events_list = Vec::new();
        let store = match get_store() {
            Some(s) => &s.0,
            None => return events_list,
        };

        let now = NSDate::date();
        let end = NSDate::dateWithTimeIntervalSinceNow((days_ahead * 24 * 60 * 60) as f64);

        // Create a predicate for events in the date range
        let predicate =
            unsafe { store.predicateForEventsWithStartDate_endDate_calendars(&now, &end, None) };

        // Fetch events matching the predicate
        let events = unsafe { store.eventsMatchingPredicate(&predicate) };

        // Convert each EKEvent to our CalendarEvent struct
        for event in events.iter() {
            // title() returns Retained<NSString> or Option<Retained<NSString>>
            // We'll handle both cases
            let title: String = {
                let title_ns = unsafe { event.title() };
                title_ns.to_string()
            };

            // startDate() returns Retained<NSDate>
            let start_ts: f64 = {
                let date = unsafe { event.startDate() };
                date.timeIntervalSince1970()
            };

            // endDate() returns Retained<NSDate>
            let end_ts: f64 = {
                let date = unsafe { event.endDate() };
                date.timeIntervalSince1970()
            };

            // location() returns Option<Retained<NSString>>
            let location: Option<String> = {
                let loc = unsafe { event.location() };
                loc.map(|s| s.to_string())
            };

            // eventIdentifier() returns Option<Retained<NSString>>
            let id: String = {
                unsafe { event.eventIdentifier() }
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            };

            let is_all_day = unsafe { event.isAllDay() };

            // Use default color for now
            let color = "#34c759".to_string();

            events_list.push(CalendarEvent {
                id,
                title,
                start_date: start_ts,
                end_date: end_ts,
                location,
                is_all_day,
                color,
            });
        }

        // Sort by start date
        events_list.sort_by(|a, b| a.start_date.partial_cmp(&b.start_date).unwrap());

        // Update cache
        let cache_mutex = EVENTS_CACHE.get_or_init(|| Mutex::new(Cache::new(Vec::new())));
        if let Ok(mut cache) = cache_mutex.lock() {
            *cache = Cache::new(events_list.clone());
        }

        events_list
    }

    pub async fn get_reminders(force_refresh: bool) -> Vec<Reminder> {
        // Check cache first
        if !force_refresh {
            if let Some(cache_mutex) = REMINDERS_CACHE.get() {
                if let Ok(cache) = cache_mutex.lock() {
                    if cache.is_valid(Duration::from_secs(600)) {
                        // 10 minutes
                        return cache.data.clone();
                    }
                }
            }
        }

        log::debug!("Fetching fresh reminders...");

        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<Reminder>>();

        {
            let store = match get_store() {
                Some(s) => &s.0,
                None => return Vec::new(),
            };

            // Create a predicate for incomplete reminders in all calendars
            let predicate = unsafe { store.predicateForRemindersInCalendars(None) };

            // Set up async channel for completion callback
            // Wrap in Mutex because the block is invoked as Fn (could be called multiple times conceptually, though here only once)
            let tx = std::sync::Mutex::new(Some(tx));

            // Create completion block
            let handler = block2::RcBlock::new(
                move |reminders_ptr: *mut objc2_foundation::NSArray<
                    objc2_event_kit::EKReminder,
                >| {
                    let mut results = Vec::new();

                    if !reminders_ptr.is_null() {
                        let reminders = unsafe { &*reminders_ptr };

                        for reminder in reminders.iter() {
                            // Get reminder properties
                            let title: String = {
                                let title_ns = unsafe { reminder.title() };
                                title_ns.to_string()
                            };

                            // Skip completed reminders
                            let is_completed = unsafe { reminder.isCompleted() };
                            if is_completed {
                                continue;
                            }

                            // Get calendar item identifier as ID
                            let id: String = {
                                let id_ns = unsafe { reminder.calendarItemIdentifier() };
                                id_ns.to_string()
                            };

                            // Get priority (0 = none, 1-4 = high, 5 = medium, 6-9 = low)
                            let priority = unsafe { reminder.priority() } as i32;

                            // Due date - reminders use dueDateComponents
                            let due_date: Option<f64> = unsafe {
                                reminder.dueDateComponents().and_then(|components| {
                                    let calendar = NSCalendar::currentCalendar();
                                    calendar
                                        .dateFromComponents(&components)
                                        .map(|date| date.timeIntervalSince1970())
                                })
                            };

                            // Get calendar info
                            let (list_name, list_color) = {
                                match unsafe { reminder.calendar() } {
                                    Some(cal) => {
                                        let name = unsafe { cal.title() }.to_string();

                                        // Extract color from calendar using Core Graphics C API
                                        let color = unsafe {
                                            use objc2::msg_send;
                                            use std::ffi::c_void;

                                            // CGColorRef is a C type, not an Objective-C object
                                            type CGColorRef = *const c_void;

                                            // External C functions from Core Graphics
                                            extern "C" {
                                                fn CGColorGetNumberOfComponents(
                                                    color: CGColorRef,
                                                ) -> usize;
                                                fn CGColorGetComponents(
                                                    color: CGColorRef,
                                                ) -> *const f64;
                                            }

                                            // Get CGColor from calendar (this returns a CGColorRef)
                                            let cg_color: CGColorRef = msg_send![&cal, CGColor];

                                            if !cg_color.is_null() {
                                                // Use Core Graphics C functions
                                                let num_components =
                                                    CGColorGetNumberOfComponents(cg_color);

                                                if num_components >= 3 {
                                                    let components_ptr =
                                                        CGColorGetComponents(cg_color);

                                                    if !components_ptr.is_null() {
                                                        let components = std::slice::from_raw_parts(
                                                            components_ptr,
                                                            num_components,
                                                        );

                                                        // Convert RGB components (0.0-1.0) to hex
                                                        let r = (components[0] * 255.0) as u8;
                                                        let g = (components[1] * 255.0) as u8;
                                                        let b = (components[2] * 255.0) as u8;

                                                        format!("#{:02x}{:02x}{:02x}", r, g, b)
                                                    } else {
                                                        "#0a84ff".to_string() // Default blue
                                                    }
                                                } else {
                                                    "#0a84ff".to_string()
                                                }
                                            } else {
                                                "#0a84ff".to_string()
                                            }
                                        };

                                        (name, color)
                                    }
                                    None => ("Unknown".to_string(), "#0a84ff".to_string()),
                                }
                            };

                            results.push(Reminder {
                                id,
                                title,
                                due_date,
                                priority,
                                is_completed,
                                list_name,
                                list_color,
                            });
                        }
                    }

                    if let Ok(mut tx_guard) = tx.lock() {
                        if let Some(tx) = tx_guard.take() {
                            let _ = tx.send(results);
                        }
                    }
                },
            );

            // Fetch reminders asynchronously
            unsafe {
                let block_ref = &*handler;
                store.fetchRemindersMatchingPredicate_completion(&predicate, block_ref);
            }
        }

        // Wait for completion
        match rx.await {
            Ok(results) => {
                // Update cache
                let cache_mutex =
                    REMINDERS_CACHE.get_or_init(|| Mutex::new(Cache::new(Vec::new())));
                if let Ok(mut cache) = cache_mutex.lock() {
                    *cache = Cache::new(results.clone());
                }
                results
            }
            Err(_) => {
                log::warn!("Reminders fetch timed out or cancelled");
                Vec::new()
            }
        }
    }

    pub async fn complete_reminder(id: String) -> Result<bool, String> {
        let store = match get_store() {
            Some(s) => &s.0,
            None => return Err("Failed to access event store".to_string()),
        };

        // We need to fetch the specific reminder to modify it
        // EKEventStore calendarItemWithIdentifier:
        let ns_id = objc2_foundation::NSString::from_str(&id);
        let item = unsafe { store.calendarItemWithIdentifier(&ns_id) };

        if let Some(item) = item {
            // Check if it is a reminder (EKReminder inherits from EKCalendarItem)
            // We can try to cast or check class. For now, we assume ID is correct.
            let reminder_ptr: *const objc2_event_kit::EKCalendarItem =
                objc2::rc::Retained::as_ptr(&item);
            let reminder: &objc2_event_kit::EKReminder =
                unsafe { &*(reminder_ptr as *const objc2_event_kit::EKReminder) };

            unsafe {
                reminder.setCompleted(true);
                let _ = store.saveReminder_commit_error(reminder, true);
            }

            // Invalidate cache
            if let Some(cache_mutex) = REMINDERS_CACHE.get() {
                if let Ok(mut cache) = cache_mutex.lock() {
                    // Remove the item from cache immediately for responsiveness
                    cache.data.retain(|r| r.id != id);
                }
            }

            Ok(true)
        } else {
            Err("Reminder not found".to_string())
        }
    }

    pub async fn create_reminder(title: String, due_date: Option<f64>) -> Result<bool, String> {
        let store = match get_store() {
            Some(s) => &s.0,
            None => return Err("Failed to access event store".to_string()),
        };

        // Get default calendar for reminders
        let default_calendar = unsafe { store.defaultCalendarForNewReminders() };

        if let Some(calendar) = default_calendar {
            // Create new reminder
            let reminder = unsafe { objc2_event_kit::EKReminder::reminderWithEventStore(store) };

            unsafe {
                let ns_title = objc2_foundation::NSString::from_str(&title);
                reminder.setTitle(Some(&ns_title));
                reminder.setCalendar(Some(&calendar));

                // Set due date if provided
                if let Some(ts) = due_date {
                    let ns_date = objc2_foundation::NSDate::dateWithTimeIntervalSince1970(ts);

                    // We need to convert NSDate to NSDateComponents for EKReminder
                    // EKReminder uses dueDateComponents rather than a simple NSDate
                    let calendar_app = NSCalendar::currentCalendar();
                    let unit_flags = NSCalendarUnit::Year
                        | NSCalendarUnit::Month
                        | NSCalendarUnit::Day
                        | NSCalendarUnit::Hour
                        | NSCalendarUnit::Minute;

                    let components = calendar_app.components_fromDate(unit_flags, &ns_date);
                    reminder.setDueDateComponents(Some(&components));
                }

                // Save
                let _ = store.saveReminder_commit_error(&reminder, true);
            }

            // Invalidate cache
            if let Some(cache_mutex) = REMINDERS_CACHE.get() {
                if let Ok(mut cache) = cache_mutex.lock() {
                    // We don't have the full object to add to cache easily without fetching, so just clear it or invalidate
                    // For simplicity, let's just clear for now so next fetch gets it.
                    // Or better, we can re-fetch?
                    // Let's just invalidate/remove all to force refresh or just let the user's refresh button handle it if needed.
                    // Actually, better to just invalidate effectively.
                    cache.data.clear();
                    // Wait, clearing might show empty list. Maybe better to leave it stale until refresh?
                    // The user asked for "Add reminder", assume they want to see it.
                    // So we should probably return the new list or let the frontend trigger a refresh.
                    // The frontend stores local state too.
                }
            }

            Ok(true)
        } else {
            Err("No default calendar found for reminders".to_string())
        }
    }

    pub async fn create_event(
        title: String,
        start_date: f64,
        end_date: f64,
        is_all_day: bool,
        location: Option<String>,
    ) -> Result<bool, String> {
        let store = match get_store() {
            Some(s) => &s.0,
            None => return Err("Failed to access event store".to_string()),
        };

        // Get default calendar for new events
        let default_calendar = unsafe { store.defaultCalendarForNewEvents() };

        if let Some(calendar) = default_calendar {
            let event = unsafe { objc2_event_kit::EKEvent::eventWithEventStore(store) };

            unsafe {
                let ns_title = objc2_foundation::NSString::from_str(&title);
                event.setTitle(Some(&ns_title));
                event.setCalendar(Some(&calendar));

                let start = objc2_foundation::NSDate::dateWithTimeIntervalSince1970(start_date);
                event.setStartDate(Some(&start));

                let end = objc2_foundation::NSDate::dateWithTimeIntervalSince1970(end_date);
                event.setEndDate(Some(&end));

                event.setAllDay(is_all_day);

                if let Some(loc) = location {
                    let ns_loc = objc2_foundation::NSString::from_str(&loc);
                    event.setLocation(Some(&ns_loc));
                }

                // EKSpan::ThisEvent is usually 0
                let _ = store.saveEvent_span_commit_error(
                    &event,
                    objc2_event_kit::EKSpan::ThisEvent,
                    true,
                );
            }

            // Invalidate cache
            if let Some(cache_mutex) = EVENTS_CACHE.get() {
                if let Ok(mut cache) = cache_mutex.lock() {
                    cache.data.clear();
                }
            }

            Ok(true)
        } else {
            Err("No default calendar found for events".to_string())
        }
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
pub async fn get_upcoming_events(
    force_refresh: Option<bool>,
) -> Result<Vec<CalendarEvent>, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(macos::get_events(7, force_refresh.unwrap_or(false)))
    }
    #[cfg(not(target_os = "macos"))]
    Ok(vec![])
}

#[tauri::command]
pub async fn get_reminders(force_refresh: Option<bool>) -> Result<Vec<Reminder>, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(macos::get_reminders(force_refresh.unwrap_or(false)).await)
    }
    #[cfg(not(target_os = "macos"))]
    Ok(vec![])
}

#[tauri::command]
pub async fn complete_reminder(id: String) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        macos::complete_reminder(id).await
    }
    #[cfg(not(target_os = "macos"))]
    Ok(true)
}

#[tauri::command]
pub async fn create_reminder(title: String, due_date: Option<f64>) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        macos::create_reminder(title, due_date).await
    }
    #[cfg(not(target_os = "macos"))]
    Ok(true)
}

#[tauri::command]
pub async fn open_calendar_app() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-a")
            .arg("Calendar")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        // Try to open Windows Calendar or Outlook
        std::process::Command::new("explorer")
            .arg("outlookcal:")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        // Try to open standard calendar app via xdg-open
        // We might not have a specific calendar URL scheme, so we try opening a calendar file or just generic logic?
        // Actually, just spawning gnome-calendar or similar if present, or just nothing for now as 'xdg-open' needs a URL.
        // A safe bet is trying to run common calendar apps or just return Ok.
        // Let's try xdg-open with a calendar scheme if it exists, or just log.
        // "webcal:" or "calendar:" might work on some DEs.
        std::process::Command::new("xdg-open")
            .arg("calendar:")
            .spawn()
            .or_else(|_| std::process::Command::new("gnome-calendar").spawn())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn open_reminders_app() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-a")
            .arg("Reminders")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("ms-to-do:")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        // Try to open common todo apps
        std::process::Command::new("xdg-open")
            .arg("todo:") // unlikely to work but consistent
            .spawn()
            .or_else(|_| std::process::Command::new("gnome-todo").spawn())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn open_privacy_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("ms-settings:privacy-calendar")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg("help:privacy") // Very generic/wrong, but Linux settings are DE specific.
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn create_calendar_event(
    title: String,
    start_date: f64,
    end_date: f64,
    is_all_day: bool,
    location: Option<String>,
) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        macos::create_event(title, start_date, end_date, is_all_day, location).await
    }
    #[cfg(not(target_os = "macos"))]
    Ok(true)
}

#[tauri::command]
pub async fn open_calendar_event(_id: String, date: f64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSCalendar, NSCalendarUnit, NSDate};

        // Get date components
        // Get date components
        let ns_date = NSDate::dateWithTimeIntervalSince1970(date);
        let calendar = NSCalendar::currentCalendar();
        // Add Hour and Minute to flags
        let unit_flags = NSCalendarUnit::Year
            | NSCalendarUnit::Month
            | NSCalendarUnit::Day
            | NSCalendarUnit::Hour
            | NSCalendarUnit::Minute;

        let components = calendar.components_fromDate(unit_flags, &ns_date);

        let year = components.year();
        let month = components.month();
        let day = components.day();
        let hour = components.hour();
        let minute = components.minute();

        let script = format!(
            r#"
            tell application "Calendar"
                activate
                switch view to day view
                set targetDate to current date
                set year of targetDate to {}
                set month of targetDate to {}
                set day of targetDate to {}
                set time of targetDate to ({} * 3600 + {} * 60)
                switch view to targetDate
            end tell
            "#,
            year, month, day, hour, minute
        );

        log::debug!(
            "Opening/Switching Calendar to: {}/{}/{} {}:{}",
            year,
            month,
            day,
            hour,
            minute
        );

        std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
