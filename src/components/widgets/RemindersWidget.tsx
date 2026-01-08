import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { IconRefresh, IconPlus, IconChecklist } from '@tabler/icons-react';
import { z } from 'zod';
import { registerWidget } from './WidgetRegistry';
import { WidgetWrapper } from './WidgetWrapper';
import { WidgetAddDialog } from './WidgetAddDialog';

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

        console.log("Creating reminder:", { title: data.title, dueDate });

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
        <div key="actions" style={{ display: 'flex', gap: 4 }}>
            <button
                className="icon-button"
                onClick={(e) => { e.stopPropagation(); setShowAddDialog(true); }}
            >
                <IconPlus size={18} />
            </button>
            <button
                className={`icon-button ${isRefreshing ? 'spinning' : ''}`}
                onClick={(e) => { e.stopPropagation(); fetchReminders(true); }}
            >
                <IconRefresh size={18} />
            </button>
        </div>
    ]

    // Get default date-time for the form (now)
    const getDefaultDateTime = () => {
        const now = new Date();
        now.setMinutes(now.getMinutes() - now.getTimezoneOffset());
        return now.toISOString().slice(0, 16);
    };

    return (
        <WidgetWrapper title="Reminders" headerActions={headerActions} className="reminders-widget" >
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
                <div className="no-events-message">No reminders</div>
            ) : (
                <div className="reminders-list" >
                    {reminders.slice(0, 10).map(rem => (
                        <div className="reminder-item-modern" key={rem.id}>
                            <div
                                className="reminder-checkbox-circle"
                                style={{ borderColor: rem.list_color || 'var(--accent-color)' }}
                                onClick={(e) => { e.stopPropagation(); toggleReminder(rem.id); }}
                            >
                                <div className="reminder-checkbox-inner" style={{ backgroundColor: rem.list_color || 'var(--accent-color)' }} />
                            </div>
                            <div className="reminder-content">
                                <div className="reminder-title">{rem.title}</div>
                                <div className="reminder-list-name" style={{ color: rem.list_color || 'var(--accent-color)' }}>{rem.list_name}</div>
                            </div>
                        </div>
                    ))}
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
