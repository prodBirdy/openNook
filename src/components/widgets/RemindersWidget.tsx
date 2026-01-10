import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { IconRefresh, IconPlus, IconChecklist } from '@tabler/icons-react';
import { z } from 'zod';
import { format, isPast } from 'date-fns';
import { motion, AnimatePresence } from 'motion/react';
import { registerWidget } from './WidgetRegistry';
import { WidgetWrapper } from './WidgetWrapper';
import { WidgetAddDialog } from './WidgetAddDialog';
import { cn } from '@/lib/utils';

interface Reminder {
    id: string;
    title: string;
    due_date: number | null;
    priority: number;
    is_completed: boolean;
    list_name: string;
    list_color: string;
}

// Zod schema for reminder form
const reminderFormSchema = z.object({
    title: z.string().min(1, "Title is required"),
    dueDate: z.string().optional(),
});

type ReminderFormValues = z.infer<typeof reminderFormSchema>;

export function RemindersWidget() {
    const [reminders, setReminders] = useState<Reminder[]>([]);
    const [loading, setLoading] = useState(true);
    const [isRefreshing, setIsRefreshing] = useState(false);
    const [showAddDialog, setShowAddDialog] = useState(false);

    const fetchReminders = (force = false) => {
        if (force) setIsRefreshing(true);
        invoke<Reminder[]>('get_reminders', { forceRefresh: force })
            .then(data => setReminders(data))
            .catch(err => console.error(err))
            .finally(() => {
                setLoading(false);
                if (force) setIsRefreshing(false);
            });
    };

    useEffect(() => {
        invoke('request_calendar_access')
            .then(() => fetchReminders())
            .catch(console.error);
    }, []);

    const toggleReminder = (id: string) => {
        // Optimistic update
        setReminders(prev => prev.filter(r => r.id !== id));
        // In real app invoke complete_reminder
        invoke('complete_reminder', { id });
    };

    const handleCreateReminder = (data: ReminderFormValues) => {
        let dueDate = null;
        if (data.dueDate) {
            dueDate = new Date(data.dueDate).getTime() / 1000;
        }

        // Ensure we match the Rust argument names strictly (snake_case)
        return invoke('create_reminder', { title: data.title, dueDate: dueDate })
            .then(() => {
                console.log("Reminder created successfully");
                fetchReminders(true);
            })
            .catch(err => {
                console.error("Failed to create reminder:", err);
                throw err; // Re-throw to prevent dialog from closing
            });
    };

    if (loading && !isRefreshing && reminders.length === 0) return <div className="widget-placeholder">Loading...</div>;

    const headerActions = [
        <div key="actions" className="flex gap-1">
            <button
                className="bg-transparent border-none text-white/40 cursor-pointer p-1.5 rounded-full flex items-center justify-center transition-all duration-200 hover:bg-white/10 hover:text-white"
                onClick={(e) => { e.stopPropagation(); setShowAddDialog(true); }}
            >
                <IconPlus size={18} />
            </button>
            <button
                className={cn(
                    "bg-transparent border-none text-white/40 cursor-pointer p-1.5 rounded-full flex items-center justify-center transition-all duration-200 hover:bg-white/10 hover:text-white",
                    isRefreshing && "animate-spin text-primary"
                )}
                onClick={(e) => { e.stopPropagation(); fetchReminders(true); }}
            >
                <IconRefresh size={18} />
            </button>
        </div>
    ];

    // Get default date-time for the form (now)
    const getDefaultDateTime = () => {
        const now = new Date();
        now.setMinutes(now.getMinutes() - now.getTimezoneOffset());
        return now.toISOString().slice(0, 16);
    };

    const formatDueDate = (timestamp: number) => {
        const date = new Date(timestamp * 1000);
        const overdue = isPast(date);

        // If it's today, show only time, otherwise show date and time
        const isToday = new Date().toDateString() === date.toDateString();
        const formatStr = isToday ? 'h:mm a' : 'MMM d, h:mm a';

        return {
            text: (isToday ? 'Today, ' : '') + format(date, formatStr),
            overdue
        };
    };

    return (
        <WidgetWrapper
            title="Reminders"
            headerActions={headerActions}
            className="flex flex-col p-5 h-full box-border overflow-hidden"
        >
            <WidgetAddDialog
                open={showAddDialog}
                onOpenChange={setShowAddDialog}
                title="New Reminder"
                schema={reminderFormSchema}
                defaultValues={{ title: '', dueDate: getDefaultDateTime() }}
                onSubmit={handleCreateReminder}
                fields={[
                    {
                        name: 'title',
                        label: 'Title',
                        placeholder: 'Reminder title',
                        icon: <IconChecklist size={18} className="text-primary" />,
                        autoFocus: true,
                        required: true,
                    },
                    {
                        name: 'dueDate',
                        label: 'Due Date',
                        type: 'datetime-local',
                    },
                ]}
                submitLabel="Add Reminder"
            />

            {reminders.length === 0 ? (
                <div className="flex-1 flex flex-col items-center justify-center gap-3 text-white/30 text-sm">
                    <span>No reminders</span>
                    <button
                        className="bg-white/10 border-none px-4 py-2 rounded-[20px] text-white text-[13px] font-medium cursor-pointer transition-colors duration-200 hover:bg-white/20"
                        onClick={() => setShowAddDialog(true)}
                    >
                        Create Reminder
                    </button>
                </div>
            ) : (
                <div className="flex flex-col gap-1 flex-1 overflow-y-auto min-h-0 pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                    <AnimatePresence mode="popLayout" initial={false}>
                        {reminders.slice(0, 10).map(rem => (
                            <motion.div
                                layout
                                initial={{ opacity: 0, scale: 0.9, y: 20 }}
                                animate={{ opacity: 1, scale: 1, y: 0 }}
                                exit={{ opacity: 0, scale: 0.9, y: -20 }}
                                transition={{ type: "spring", bounce: 0.3, duration: 0.4 }}
                                className="group flex items-center gap-3 px-4 py-3 rounded-[20px] transition-colors duration-200 cursor-default border border-transparent hover:bg-white/5"
                                key={rem.id}
                            >
                                <div
                                    className="relative w-8 h-8 shrink-0 cursor-pointer rounded-full border-2 flex items-center justify-center transition-transform active:scale-95"
                                    style={{ borderColor: rem.list_color || 'var(--accent-color)' }}
                                    onClick={(e) => { e.stopPropagation(); toggleReminder(rem.id); }}
                                >
                                    <div
                                        className="w-4 h-4 rounded-full transition-opacity opacity-0 group-hover:opacity-40"
                                        style={{ backgroundColor: rem.list_color || 'var(--accent-color)' }}
                                    />
                                </div>
                                <div className="flex-1 flex flex-col justify-center overflow-hidden">
                                    <div className="text-[17px] font-medium text-white/95 truncate leading-tight flex flex-row justify-between">
                                        {rem.title}
                                        <div className="text-[13px] font-medium opacity-60" style={{ color: rem.list_color || 'var(--accent-color)' }}>
                                            {rem.list_name}
                                        </div>
                                    </div>
                                    <div className="flex items-center gap-2 mt-0.5">
                                        {rem.due_date && (() => {
                                            const { text, overdue } = formatDueDate(rem.due_date);
                                            return (
                                                <div className={cn("text-[13px] font-medium", overdue ? "text-red-400" : "text-white/40")}>
                                                    {text}
                                                </div>
                                            );
                                        })()}

                                    </div>
                                </div>
                            </motion.div>
                        ))}
                    </AnimatePresence>
                </div>
            )}
        </WidgetWrapper>
    );
}

// Register the reminders widget
registerWidget({
    id: 'reminders',
    name: 'Reminders',
    description: 'Show incomplete reminders',
    icon: IconChecklist,
    ExpandedComponent: RemindersWidget,
    defaultEnabled: false,
    category: 'productivity',
    minWidth: 260,
    hasCompactMode: false
});
