import { useEffect, useState, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { IconRefresh, IconPlus, IconCalendar } from '@tabler/icons-react';
import { registerWidget } from './WidgetRegistry';
import { WidgetWrapper } from './WidgetWrapper';
import { WidgetAddDialog } from './WidgetAddDialog';

interface CalendarEvent {
    id: string;
    title: string;
    start_date: number; // Timestamp in seconds
    end_date: number;
    location: string | null;
    is_all_day: boolean;
    color: string;
}

export function CalendarWidget() {
    const [events, setEvents] = useState<CalendarEvent[]>([]);
    const [loading, setLoading] = useState(true);
    const [permission, setPermission] = useState(true);
    const [selectedDate, setSelectedDate] = useState(new Date());
    const [isRefreshing, setIsRefreshing] = useState(false);
    const [isScrollerHovered, setIsScrollerHovered] = useState(false);
    const [showAddDialog, setShowAddDialog] = useState(false);

    // Generate days for the scroller (today + next 14 days)
    const days = useMemo(() => {
        const d = [];
        const today = new Date();
        for (let i = 0; i < 7; i++) {
            const date = new Date(today);
            date.setDate(today.getDate() + i);
            d.push(date);
        }
        return d;
    }, []);

    const fetchEvents = (force = false) => {
        if (force) setIsRefreshing(true);
        invoke<boolean>('request_calendar_access')
            .then(granted => {
                setPermission(granted);
                if (granted) {
                    return invoke<CalendarEvent[]>('get_upcoming_events', { forceRefresh: force });
                }
                return [];
            })
            .then(data => {
                setEvents(data.sort((a, b) => a.start_date - b.start_date));
            })
            .catch(err => console.error("Calendar error:", err))
            .finally(() => {
                setLoading(false);
                if (force) setIsRefreshing(false);
            });
    };

    useEffect(() => {
        fetchEvents();
    }, []);

    const filteredEvents = useMemo(() => {
        return events.filter(event => {
            const eventDate = new Date(event.start_date * 1000);
            return eventDate.toDateString() === selectedDate.toDateString();
        });
    }, [events, selectedDate]);

    const formatTime = (ts: number) => {
        return new Date(ts * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    };

    const openPrivacySettings = () => {
        invoke('open_privacy_settings').catch(console.error);
    };

    const handleCreateEvent = (e: React.FormEvent) => {
        e.preventDefault();
        const form = e.target as HTMLFormElement;
        const formData = new FormData(form);
        const title = formData.get('title') as string;
        const location = formData.get('location') as string;
        const isAllDay = formData.get('isAllDay') === 'on';
        const startStr = formData.get('start') as string;
        const endStr = formData.get('end') as string;

        // Parse local time strings to timestamps
        // Input type="datetime-local" returns "YYYY-MM-DDTHH:mm"
        const startDate = new Date(startStr);
        const endDate = new Date(endStr);

        const startTs = startDate.getTime() / 1000;
        const endTs = endDate.getTime() / 1000;

        invoke('create_calendar_event', {
            title,
            startDate: startTs,
            endDate: endTs,
            isAllDay,
            location: location || null
        })
            .then(() => {
                setShowAddDialog(false);
                fetchEvents(true);
            })
            .catch(console.error);
    };

    const handleEventClick = (event: CalendarEvent) => {
        invoke('open_calendar_event', { id: event.id, date: event.start_date }).catch(console.error);
    };

    if (loading && !isRefreshing && events.length === 0) return <div className="widget-placeholder">Loading...</div>;
    if (!permission) return (
        <div className="widget-placeholder" onClick={openPrivacySettings} style={{ cursor: 'pointer' }}>
            Access denied
        </div>
    );

    const headerActions = [
        <div style={{ display: 'flex', gap: 4 }}>
            <button
                className="icon-button"
                onClick={(e) => { e.stopPropagation(); setShowAddDialog(true); }}
            >
                <IconPlus size={18} />
            </button>
            <button
                className={`icon-button ${isRefreshing ? 'spinning' : ''}`}
                onClick={(e) => { e.stopPropagation(); fetchEvents(true); }}
            >
                <IconRefresh size={18} />
            </button>
        </div>
    ];

    return (
        <WidgetWrapper title="Calendar" headerActions={headerActions} className="calendar-widget" >
            {showAddDialog ? (
                <WidgetAddDialog
                    title="New Event"
                    onClose={() => setShowAddDialog(false)}
                    onSubmit={handleCreateEvent}
                    submitLabel="Add"
                    mainInput={{
                        name: "title",
                        placeholder: "Title",
                        required: true,
                        icon: <IconCalendar size={18} color="var(--accent-color)" />
                    }}
                >
                    <input name="location" placeholder="Location" className="form-input" />
                    <div className="form-row">
                        <label>Start</label>
                        <input
                            name="start"
                            type="datetime-local"
                            required
                            className="form-input"
                            defaultValue={new Date().toISOString().slice(0, 16)}
                        />
                    </div>
                    <div className="form-row">
                        <label>End</label>
                        <input
                            name="end"
                            type="datetime-local"
                            required
                            className="form-input"
                            defaultValue={new Date(Date.now() + 3600000).toISOString().slice(0, 16)}
                        />
                    </div>
                    <div className="form-row checkbox-row">
                        <label htmlFor="isAllDay">All Day</label>
                        <input name="isAllDay" id="isAllDay" type="checkbox" />
                    </div>
                </WidgetAddDialog>
            ) : (
                <>
                    {/* Scroller Container with Hover Logic */}
                    <div
                        className={`scroller-container ${isScrollerHovered ? 'expanded' : 'compact'}`}
                        onMouseEnter={() => setIsScrollerHovered(true)}
                        onMouseLeave={() => setIsScrollerHovered(false)}
                        onWheel={(e) => {
                            if (isScrollerHovered) e.stopPropagation();
                        }}
                    >
                        {/* Horizontal Day Scroller */}
                        <div className="calendar-day-scroller">
                            {days.map((date, i) => {
                                const isSelected = date.toDateString() === selectedDate.toDateString();
                                const isToday = date.toDateString() === new Date().toDateString();
                                return (
                                    <div
                                        key={i}
                                        className={`day-item ${isSelected ? 'selected' : ''} ${isToday ? 'today' : ''} ${events.some(e => new Date(e.start_date * 1000).toDateString() === date.toDateString()) ? 'has-event' : ''}`}
                                        onClick={(e) => { e.stopPropagation(); setSelectedDate(date); }}
                                    >
                                        <span className="day-name">{date.toLocaleDateString('en-US', { weekday: 'short' }).charAt(0)}</span>
                                        <span className="day-number">{date.getDate()}</span>
                                        {events.some(e => new Date(e.start_date * 1000).toDateString() === date.toDateString()) && (
                                            <div className="event-dot" />
                                        )}
                                    </div>
                                );
                            })}
                        </div>
                    </div>

                    {/* Events List */}
                    <div className={`events-list-container ${isScrollerHovered ? 'dimmed' : ''}`}>
                        {filteredEvents.length === 0 ? (
                            <div className="no-events-message">No events</div>
                        ) : (
                            <div className="events-list">
                                {filteredEvents.map((event, i) => (
                                    <div
                                        className="event-item-modern"
                                        key={i}
                                        onClick={(e) => { e.stopPropagation(); handleEventClick(event); }}
                                        style={{ cursor: 'pointer' }}
                                    >
                                        <div className="event-time-column">
                                            {event.is_all_day ? (
                                                <span className="all-day-label">All Day</span>
                                            ) : (
                                                <>
                                                    <span className="event-start-time">{formatTime(event.start_date)}</span>
                                                    {/* <span className="event-duration">1h</span> */}
                                                </>
                                            )}
                                        </div>
                                        <div className="event-color-bar" style={{ backgroundColor: 'var(--accent-color)' }} />
                                        <div className="event-details-modern">
                                            <div className="event-title">{event.title}</div>
                                            {event.location && <div className="event-location">{event.location}</div>}
                                        </div>
                                    </div>
                                ))}
                            </div>
                        )}
                    </div>
                </>
            )}

        </WidgetWrapper>

    );
}

// Register the calendar widget
registerWidget({
    id: 'calendar',
    name: 'Calendar',
    description: 'Show upcoming events',
    icon: IconCalendar,
    ExpandedComponent: CalendarWidget,
    defaultEnabled: false,
    category: 'productivity',
    minWidth: 280,
    hasCompactMode: false
});
