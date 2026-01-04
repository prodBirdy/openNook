import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

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

    useEffect(() => {
        invoke<Reminder[]>('get_reminders')
            .then(data => setReminders(data))
            .catch(err => console.error(err))
            .finally(() => setLoading(false));
    }, []);

    const toggleReminder = (id: string) => {
        // Optimistic update
        setReminders(prev => prev.filter(r => r.id !== id));
        // In real app invoke complete_reminder
        // invoke('complete_reminder', { id });
    };

    if (loading) return <div className="widget-placeholder">Loading Reminders...</div>;
    if (reminders.length === 0) return <div className="widget-placeholder">No reminders</div>;

    return (
        <div className="reminders-widget">
            <div className="widget-header">
                <span className="widget-title">Reminders</span>
            </div>
            <div className="reminders-list">
                {reminders.slice(0, 5).map(rem => (
                    <div className="reminder-item" key={rem.id}>
                        <div
                            className="reminder-checkbox"
                            style={{ borderColor: rem.list_color || '#ff3b30' }}
                            onClick={() => toggleReminder(rem.id)}
                        />
                        <div className="reminder-title">{rem.title}</div>
                    </div>
                ))}
            </div>
        </div>
    );
}
