import { useEffect, useState, useMemo } from 'react';
import { motion } from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import { IconRefresh, IconPlus, IconCalendar, IconMapPin } from '@tabler/icons-react';
import { z } from 'zod';
import { registerWidget } from './WidgetRegistry';
import { WidgetWrapper } from './WidgetWrapper';
import { WidgetAddDialog } from './WidgetAddDialog';
import { cn } from '@/lib/utils';

interface CalendarEvent {
    id: string;
    title: string;
    start_date: number; // Timestamp in seconds
    end_date: number;
    location: string | null;
    is_all_day: boolean;
    color: string;
}

// Zod schema for calendar event form
const eventFormSchema = z.object({
    title: z.string().min(1, "Title is required"),
    location: z.string().optional(),
    start: z.string().min(1, "Start date is required"),
    end: z.string().min(1, "End date is required"),
});

type EventFormValues = z.infer<typeof eventFormSchema>;

export function CalendarWidget() {
    const [events, setEvents] = useState<CalendarEvent[]>([]);
    const [loading, setLoading] = useState(true);
    const [permission, setPermission] = useState(true);
    const [selectedDate, setSelectedDate] = useState(new Date());
    const [isRefreshing, setIsRefreshing] = useState(false);
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

    // Get default date-time for the form
    const getDefaultDateTime = () => {
        const now = new Date();
        now.setMinutes(now.getMinutes() - now.getTimezoneOffset());
        return now.toISOString().slice(0, 16);
    };

    const getDefaultEndDateTime = () => {
        const later = new Date(Date.now() + 3600000);
        later.setMinutes(later.getMinutes() - later.getTimezoneOffset());
        return later.toISOString().slice(0, 16);
    };

    const handleCreateEvent = async (data: EventFormValues) => {
        const startDate = new Date(data.start);
        const endDate = new Date(data.end);

        const startTs = startDate.getTime() / 1000;
        const endTs = endDate.getTime() / 1000;

        await invoke('create_calendar_event', {
            title: data.title,
            startDate: startTs,
            endDate: endTs,
            isAllDay: false,
            location: data.location || null
        });

        fetchEvents(true);
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
        <div key="actions" style={{ display: 'flex', gap: 4 }}>
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
        <WidgetWrapper
            title="Calendar"
            headerActions={headerActions}
            className="flex flex-col h-full box-border overflow-hidden p-5"
            initial={{ opacity: 0, scale: 0.9, filter: "blur(10px)" }}
            animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
            exit={{ opacity: 0, scale: 0.95, filter: "blur(10px)" }}
            transition={{ type: "spring", stiffness: 300, damping: 25 }}
        >
            <WidgetAddDialog
                open={showAddDialog}
                onOpenChange={setShowAddDialog}
                title="New Event"
                schema={eventFormSchema}
                defaultValues={{
                    title: '',
                    location: '',
                    start: getDefaultDateTime(),
                    end: getDefaultEndDateTime(),
                }}
                onSubmit={handleCreateEvent}
                fields={[
                    {
                        name: 'title',
                        label: 'Title',
                        placeholder: 'Event title',
                        icon: <IconCalendar size={18} className="text-primary" />,
                        autoFocus: true,
                        required: true,
                    },
                    {
                        name: 'location',
                        label: 'Location',
                        placeholder: 'Location (optional)',
                        icon: <IconMapPin size={18} className="text-muted-foreground" />,
                    },
                    {
                        name: 'start',
                        label: 'Start',
                        type: 'datetime-local',
                        required: true,
                    },
                    {
                        name: 'end',
                        label: 'End',
                        type: 'datetime-local',
                        required: true,
                    },
                ]}
                submitLabel="Add Event"
            />



            {/* Scroller Container */}
            <motion.div
                layout
                transition={{ type: "spring", stiffness: 400, damping: 30 }}
                className="relative z-10 mb-2 bg-transparent rounded-xl overflow-hidden h-[32px] mb-1"
            >
                {/* Horizontal Day Scroller */}
                <motion.div
                    layout
                    className="flex gap-1 items-center h-full pb-0 overflow-hidden gap-1"
                >
                    {days.map((date, i) => {
                        const isSelected = date.toDateString() === selectedDate.toDateString();
                        const isToday = date.toDateString() === new Date().toDateString();
                        const hasEvent = events.some(e => new Date(e.start_date * 1000).toDateString() === date.toDateString());

                        return (
                            <motion.div
                                layout
                                key={i}
                                transition={{ type: "spring", stiffness: 400, damping: 30 }}
                                className={cn(
                                    "flex flex-col items-center justify-center cursor-pointer flex-1",
                                    // Base State Styles
                                    "bg-white/10 opacity-60 h-[24px] min-w-[20px] rounded-[6px] p-0 m-0 hover:bg-white/20 hover:opacity-100",
                                    isSelected && "opacity-100! bg-[var(--accent-color)]! text-white h-[28px] shadow-sm scale-110",
                                    hasEvent && !isSelected && "bg-[#ff3b30] opacity-100!",
                                    isToday && "bg-[var(--accent-color)] brightness-75 opacity-100! h-[24px]"
                                )}
                                onClick={(e) => { e.stopPropagation(); setSelectedDate(date); }}
                            >
                                <motion.span
                                    layout
                                    className="text-[7px] mb-0 leading-none text-white/70"
                                >
                                    {date.toLocaleDateString('en-US', { weekday: 'short' }).charAt(0)}
                                </motion.span>
                                <motion.span
                                    layout
                                    className="text-[9px] leading-none text-white font-medium"
                                >
                                    {date.getDate()}
                                </motion.span>
                            </motion.div>
                        );
                    })}
                </motion.div>
            </motion.div>

            {/* Events List */}
            <div className={cn(
                "flex-1 overflow-y-auto pr-1 flex flex-col transition-opacity duration-200 opacity-100 pointer-events-auto"
            )}>
                {filteredEvents.length === 0 ? (
                    <div className="h-full flex items-center justify-center text-white/40 text-[14px] font-medium">No events</div>
                ) : (
                    <div className="flex flex-col gap-3">
                        {filteredEvents.map((event, i) => (
                            <div
                                className="flex items-center py-2 border-b border-white/5 last:border-0 cursor-pointer"
                                key={i}
                                onClick={(e) => { e.stopPropagation(); handleEventClick(event); }}
                            >
                                <div className="flex flex-col items-end w-12 text-right pr-2 shrink-0">
                                    {event.is_all_day ? (
                                        <span className="text-[10px] uppercase text-white/60 font-semibold">All Day</span>
                                    ) : (
                                        <>
                                            <span className="text-[13px] font-medium text-white/90">{formatTime(event.start_date)}</span>
                                        </>
                                    )}
                                </div>
                                <div className="w-[3px] h-8 rounded-sm mr-3 shrink-0 bg-[var(--accent-color)]" />
                                <div className="flex flex-col justify-center overflow-hidden">
                                    <div className="text-[14px] font-semibold">{event.title}</div>
                                    {event.location && <div className="text-[11px] text-white/50 mt-[1px]">{event.location}</div>}
                                </div>
                            </div>
                        ))}
                    </div>
                )}
            </div>
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
