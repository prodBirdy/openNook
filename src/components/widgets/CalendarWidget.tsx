import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface CalendarEvent {
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

    useEffect(() => {
        // First check/request permission
        invoke<boolean>('request_calendar_access')
            .then(granted => {
                setPermission(granted);
                if (granted) {
                    return invoke<CalendarEvent[]>('get_upcoming_events');
                }
                return [];
            })
            .then(data => {
                // Sort by date
                setEvents(data.sort((a, b) => a.start_date - b.start_date));
            })
            .catch(err => console.error("Calendar error:", err))
            .finally(() => setLoading(false));
    }, []);

    const formatTime = (ts: number) => {
        // ts is in seconds? usually JS uses ms.
        // Backend usually sends seconds for unix timestamp or ms?
        // NSDate is seconds since 2001, but we usually convert to unix seconds in backend.
        // Let's assume seconds for now (standard unix) -> ms for JS
        return new Date(ts * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    };

    const formatDate = (ts: number) => {
        const date = new Date(ts * 1000);
        const today = new Date();
        const tomorrow = new Date(today);
        tomorrow.setDate(today.getDate() + 1);

        if (date.toDateString() === today.toDateString()) return 'Today';
        if (date.toDateString() === tomorrow.toDateString()) return 'Tomorrow';
        return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
    };

    if (loading) return <div className="widget-placeholder">Loading Calendar...</div>;
    if (!permission) return <div className="widget-placeholder">Access Denied</div>;
    if (events.length === 0) return <div className="widget-placeholder">No upcoming events</div>;

    return (
        <div className="calendar-widget">
            <div className="widget-header">
                <span className="widget-title">Calendar</span>
            </div>
            <div className="events-list">
                {events.slice(0, 3).map((event, i) => (
                    <div className="event-item" key={i}>
                        <div className="event-time-badge" style={{ backgroundColor: event.color || '#34c759' }}>
                            <div className="event-date-text">{formatDate(event.start_date)}</div>
                            <div className="event-time-text">
                                {event.is_all_day ? 'All Day' : formatTime(event.start_date)}
                            </div>
                        </div>
                        <div className="event-details">
                            <div className="event-title">{event.title}</div>
                            {event.location && <div className="event-location">{event.location}</div>}
                        </div>
                    </div>
                ))}
            </div>
        </div>
    );
}
