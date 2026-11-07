use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start_date: f64, // Timestamp
    /// EventKit `endDate`, Unix seconds. Missing in older serialized events.
    #[serde(default)]
    pub end: Option<f64>,
    pub location: Option<String>,
    pub is_all_day: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Reminder {
    pub id: String,
    pub title: String,
    pub due_date: Option<f64>,
    pub is_completed: bool,
    pub list_color: String,
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::rc::Retained;
    use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEventStore};
    use objc2_foundation::{MainThreadMarker, NSCalendar, NSCalendarUnit, NSDate};
    use std::sync::OnceLock;
    use tokio::sync::oneshot::Sender;

    // Serialize system prompts; recheck OS authorization after each completed request.
    static ACCESS_REQUEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

    const CACHE_TTL: Duration = Duration::from_secs(30);

    fn authorized_for(entity: EKEntityType) -> Option<bool> {
        let status = unsafe { EKEventStore::authorizationStatusForEntityType(entity) };
        match status {
            EKAuthorizationStatus::FullAccess | EKAuthorizationStatus::WriteOnly => Some(true),
            EKAuthorizationStatus::Denied | EKAuthorizationStatus::Restricted => Some(false),
            EKAuthorizationStatus::NotDetermined => None,
            _ => None,
        }
    }

    pub fn calendar_authorized() -> Option<bool> {
        authorized_for(EKEntityType::Event)
    }

    pub fn reminders_authorized() -> Option<bool> {
        authorized_for(EKEntityType::Reminder)
    }

    pub fn init_store() {
        if EVENT_STORE.get().is_some() {
            return;
        }
        let Some(_mtm) = MainThreadMarker::new() else {
            log::warn!("EventKit store init skipped: not on the main thread");
            return;
        };
        let store = unsafe { EKEventStore::new() };
        let _ = EVENT_STORE.set(SyncEventStore(store));
    }

    fn get_store() -> Option<&'static SyncEventStore> {
        EVENT_STORE.get()
    }

    pub async fn request_access(reminders: bool) -> Result<bool, String> {
        let _request = ACCESS_REQUEST.lock().await;
        request_access_inner(reminders).await
    }

    async fn request_access_inner(reminders: bool) -> Result<bool, String> {
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
                    request_entity_access(&store.0, EKEntityType::Event, &handler);
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
        if reminders && status_reminders == EKAuthorizationStatus::NotDetermined {
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
                    request_entity_access(&store.0, EKEntityType::Reminder, &handler);
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

    /// macOS 14+ `requestFullAccessTo*`; falls back to the deprecated
    /// `requestAccessToEntityType:completion:` on older systems.
    unsafe fn request_entity_access(
        store: &EKEventStore,
        entity: EKEntityType,
        handler: &block2::Block<dyn Fn(objc2::runtime::Bool, *mut objc2_foundation::NSError)>,
    ) {
        use objc2::{msg_send, sel};

        let block_ptr = handler as *const block2::Block<_> as *mut block2::Block<_>;
        let modern = if entity == EKEntityType::Event {
            sel!(requestFullAccessToEventsWithCompletion:)
        } else {
            sel!(requestFullAccessToRemindersWithCompletion:)
        };
        let responds: bool = msg_send![store, respondsToSelector: modern];
        if responds {
            if entity == EKEntityType::Event {
                let _: () = msg_send![store, requestFullAccessToEventsWithCompletion: block_ptr];
            } else {
                let _: () = msg_send![store, requestFullAccessToRemindersWithCompletion: block_ptr];
            }
        } else {
            #[allow(deprecated)]
            store.requestAccessToEntityType_completion(entity, block_ptr);
        }
    }

    pub fn get_events(days_ahead: i64, force_refresh: bool) -> Vec<CalendarEvent> {
        // Check cache first
        if !force_refresh {
            if let Some(cache_mutex) = EVENTS_CACHE.get() {
                if let Ok(cache) = cache_mutex.lock() {
                    if cache.is_valid(CACHE_TTL) {
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

            // startDate() / endDate() return Retained<NSDate>
            let start_ts: f64 = {
                let date = unsafe { event.startDate() };
                date.timeIntervalSince1970()
            };
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

            events_list.push(CalendarEvent {
                id,
                title,
                start_date: start_ts,
                end: Some(end_ts),
                location,
                is_all_day,
            });
        }

        // Sort by start date
        events_list.sort_by(|a, b| {
            a.start_date
                .partial_cmp(&b.start_date)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

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
                    if cache.is_valid(CACHE_TTL) {
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

                            // Due date - reminders use dueDateComponents
                            let due_date: Option<f64> = unsafe {
                                reminder.dueDateComponents().and_then(|components| {
                                    let calendar = NSCalendar::currentCalendar();
                                    calendar
                                        .dateFromComponents(&components)
                                        .map(|date| date.timeIntervalSince1970())
                                })
                            };

                            // List color from the EventKit calendar.
                            let list_color = {
                                match unsafe { reminder.calendar() } {
                                    Some(cal) => {
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
                                                        // SAFETY: CGColor components buffer is
                                                        // valid for `num_components` CGFloats.
                                                        let components = crate::ffi::slice(
                                                            components_ptr,
                                                            num_components,
                                                        );

                                                        if components.len() >= 3 {
                                                            let r = (components[0] * 255.0) as u8;
                                                            let g = (components[1] * 255.0) as u8;
                                                            let b = (components[2] * 255.0) as u8;

                                                            format!("#{:02x}{:02x}{:02x}", r, g, b)
                                                        } else {
                                                            "#0a84ff".to_string()
                                                        }
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

                                        color
                                    }
                                    None => "#0a84ff".to_string(),
                                }
                            };

                            results.push(Reminder {
                                id,
                                title,
                                due_date,
                                is_completed,
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
            let reminder = item
                .downcast::<objc2_event_kit::EKReminder>()
                .map_err(|_| "calendar item is not a reminder".to_string())?;

            unsafe {
                reminder.setCompleted(true);
                store
                    .saveReminder_commit_error(&reminder, true)
                    .map_err(|e| e.to_string())?;
            }

            // Invalidate cache
            if let Some(cache_mutex) = REMINDERS_CACHE.get() {
                if let Ok(mut cache) = cache_mutex.lock() {
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
                store
                    .saveReminder_commit_error(&reminder, true)
                    .map_err(|e| e.to_string())?;
            }

            // Invalidate cache so the next fetch picks up the new reminder.
            if let Some(cache_mutex) = REMINDERS_CACHE.get() {
                if let Ok(mut cache) = cache_mutex.lock() {
                    cache.data.clear();
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
                store
                    .saveEvent_span_commit_error(&event, objc2_event_kit::EKSpan::ThisEvent, true)
                    .map_err(|e| e.to_string())?;
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

/// Create the EventKit store on the main thread. Safe to call more than once.
pub fn init_store() {
    #[cfg(target_os = "macos")]
    macos::init_store();
}

/// `Some(true)` granted, `Some(false)` denied or restricted, `None` if
/// undetermined or EventKit is unavailable.
pub fn calendar_authorized() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        macos::calendar_authorized()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Same as [`calendar_authorized`] for the Reminders entity.
pub fn reminders_authorized() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        macos::reminders_authorized()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub async fn request_calendar_access() -> Result<bool, String> {
    request_calendar_access_with(crate::settings::get_app_settings().show_reminders).await
}

pub async fn request_calendar_access_with(reminders: bool) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        macos::request_access(reminders).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = reminders;
        Ok(true)
    }
}

pub async fn get_upcoming_events(
    force_refresh: Option<bool>,
) -> Result<Vec<CalendarEvent>, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(macos::get_events(7, force_refresh.unwrap_or(false)))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = force_refresh;
        Ok(vec![])
    }
}

pub async fn get_reminders(force_refresh: Option<bool>) -> Result<Vec<Reminder>, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(macos::get_reminders(force_refresh.unwrap_or(false)).await)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = force_refresh;
        Ok(vec![])
    }
}

pub async fn complete_reminder(id: String) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        macos::complete_reminder(id).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = id;
        Ok(true)
    }
}

pub async fn create_reminder(title: String, due_date: Option<f64>) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        macos::create_reminder(title, due_date).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = title;
        let _ = due_date;
        Ok(true)
    }
}

pub async fn open_calendar_app() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open")
            .arg("-a")
            .arg("Calendar")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        open::that("outlookcal:").map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        // Try to open common calendar URL or let xdg-open find default
        // "webcal:" might be handled? or just calendar:
        open::that("calendar:")
            .or_else(|_| open::that("gnome-calendar"))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn open_reminders_app() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open")
            .arg("-a")
            .arg("Reminders")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        open::that("ms-to-do:").map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        open::that("todo:")
            .or_else(|_| open::that("gnome-todo"))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn open_privacy_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        open::that("x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars")
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        open::that("ms-settings:privacy-calendar").map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        open::that("help:privacy").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Alias used by the quick-add UI.
pub async fn create_event(
    title: String,
    start_date: f64,
    end_date: f64,
    is_all_day: bool,
    location: Option<String>,
) -> Result<bool, String> {
    create_calendar_event(title, start_date, end_date, is_all_day, location).await
}

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
    {
        let _ = title;
        let _ = start_date;
        let _ = end_date;
        let _ = is_all_day;
        let _ = location;
        Ok(true)
    }
}

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

        std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = _id;
        let _ = date;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CalendarEvent;

    #[test]
    fn calendar_event_end_defaults_when_missing() {
        let json = r#"{"id":"1","title":"Design sync","start_date":0,"location":null,"is_all_day":false}"#;
        let event: CalendarEvent = serde_json::from_str(json).expect("legacy event");
        assert_eq!(event.end, None);
        assert_eq!(event.title, "Design sync");
    }
}
