import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { IconRefresh, IconPlus, IconChecklist } from '@tabler/icons-react';
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

    const handleCreateReminder = (e: React.FormEvent) => {
        e.preventDefault();
        const form = e.target as HTMLFormElement;
        const formData = new FormData(form);
        const title = formData.get('title') as string;
        const dueDateStr = formData.get('due_date') as string;

        let dueDate = null;
        if (dueDateStr) {
            dueDate = new Date(dueDateStr).getTime() / 1000;
        }

        console.log("Creating reminder:", { title, dueDate });

        // Ensure we match the Rust argument names strictly (snake_case)
        invoke('create_reminder', { title, dueDate: dueDate })
            .then(() => {
                console.log("Reminder created successfully");
                setShowAddDialog(false);
                fetchReminders(true);
            })
            .catch(err => {
                console.error("Failed to create reminder:", err);
                alert(`Failed to create reminder: ${err}`);
            });
    };

    if (loading && !isRefreshing && reminders.length === 0) return <div className="widget-placeholder">Loading...</div>;

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
                onClick={(e) => { e.stopPropagation(); fetchReminders(true); }}
            >
                <IconRefresh size={18} />
            </button>
        </div>
    ]


    return (
        <WidgetWrapper title="Reminders" headerActions={headerActions} className="reminders-widget" >
            {showAddDialog ? (
                <WidgetAddDialog
                    title="New Reminder"
                    onClose={() => setShowAddDialog(false)}
                    onSubmit={handleCreateReminder}
                    submitLabel="Add"
                    mainInput={{
                        name: "title",
                        placeholder: "Title",
                        required: true,
                        icon: <IconChecklist size={18} color="var(--accent-color)" />
                    }}
                >
                    <div className="form-row">
                        <label>Due</label>
                        <input
                            name="due_date"
                            type="datetime-local"
                            className="form-input"
                            defaultValue={new Date().toISOString().slice(0, 16)}
                        />
                    </div>
                </WidgetAddDialog>
            ) : (
                <>

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
                </>
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
