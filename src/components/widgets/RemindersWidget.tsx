import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { IconRefresh, IconPlus, IconChecklist } from '@tabler/icons-react';
import { registerWidget } from './WidgetRegistry';

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
        fetchReminders();
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

        invoke('create_reminder', { title, dueDate })
            .then(() => {
                setShowAddDialog(false);
                fetchReminders(true);
            })
            .catch(console.error);
    };

    if (loading && !isRefreshing && reminders.length === 0) return <div className="widget-placeholder">Loading...</div>;

    return (
        <div className="reminders-widget apple-style" style={{ position: 'relative' }}>
            {showAddDialog ? (
                <div className="widget-overlay">
                    <form onSubmit={handleCreateReminder} className="creation-form">
                        <div className="form-header">
                            <span className="form-title">New Reminder</span>
                            <button type="button" className="close-button" onClick={() => setShowAddDialog(false)}>Cancel</button>
                        </div>
                        <input name="title" placeholder="Title" required className="form-input" autoFocus />
                        <div className="form-row">
                            <label>Due</label>
                            <input
                                name="due_date"
                                type="datetime-local"
                                className="form-input"
                                defaultValue={new Date().toISOString().slice(0, 16)}
                            />
                        </div>
                        <button type="submit" className="submit-button">Add</button>
                    </form>
                </div>
            ) : (
                <>
                    <div className="widget-header">
                        <span className="widget-title">Reminders</span>
                        <div style={{ display: 'flex', gap: 4 }}>
                            <button
                                className="refresh-button"
                                onClick={(e) => { e.stopPropagation(); setShowAddDialog(true); }}
                            >
                                <IconPlus size={14} />
                            </button>
                            <button
                                className={`refresh-button ${isRefreshing ? 'spinning' : ''}`}
                                onClick={(e) => { e.stopPropagation(); fetchReminders(true); }}
                            >
                                <IconRefresh size={14} />
                            </button>
                        </div>
                    </div>

                    {reminders.length === 0 ? (
                        <div className="no-events-message">No reminders</div>
                    ) : (
                        <div className="reminders-list">
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
        </div>
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
