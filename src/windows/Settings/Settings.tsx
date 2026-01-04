import { useState, useEffect } from 'react';
import './Settings.css';

interface SettingsState {
    showCalendar: boolean;
    showReminders: boolean;
    showMedia: boolean;
}

export default function Settings() {
    const [settings, setSettings] = useState<SettingsState>({
        showCalendar: false,
        showReminders: false,
        showMedia: true,
    });

    useEffect(() => {
        const saved = localStorage.getItem('app-settings');
        if (saved) {
            try {
                setSettings(JSON.parse(saved));
            } catch (e) {
                console.error("Failed to parse settings", e);
            }
        }
    }, []);

    const toggleSetting = (key: keyof SettingsState) => {
        setSettings(prev => {
            const next = { ...prev, [key]: !prev[key] };
            localStorage.setItem('app-settings', JSON.stringify(next));
            // Dispatch event for other windows to update
            window.dispatchEvent(new StorageEvent('storage', {
                key: 'app-settings',
                newValue: JSON.stringify(next)
            }));
            return next;
        });
    };

    return (
        <div className="settings-window">
            <div className="settings-header">
                <h1>Settings</h1>
            </div>

            <div className="settings-section">
                <div className="section-title">Widgets</div>
                <div className="settings-group">
                    <div className="setting-item">
                        <div className="setting-info">
                            <span className="setting-label">Calendar</span>
                            <span className="setting-desc">Show upcoming events in expanded view</span>
                        </div>
                        <label className="switch">
                            <input
                                type="checkbox"
                                checked={settings.showCalendar}
                                onChange={() => toggleSetting('showCalendar')}
                            />
                            <span className="slider round"></span>
                        </label>
                    </div>

                    <div className="setting-item">
                        <div className="setting-info">
                            <span className="setting-label">Reminders</span>
                            <span className="setting-desc">Show incomplete reminders</span>
                        </div>
                        <label className="switch">
                            <input
                                type="checkbox"
                                checked={settings.showReminders}
                                onChange={() => toggleSetting('showReminders')}
                            />
                            <span className="slider round"></span>
                        </label>
                    </div>

                    <div className="setting-item">
                        <div className="setting-info">
                            <span className="setting-label">Media Controls</span>
                            <span className="setting-desc">Show now playing info</span>
                        </div>
                        <label className="switch">
                            <input
                                type="checkbox"
                                checked={settings.showMedia}
                                onChange={() => toggleSetting('showMedia')}
                            />
                            <span className="slider round"></span>
                        </label>
                    </div>
                </div>
            </div>

            <div className="settings-section">
                <div className="section-title">General</div>
                <div className="settings-group">
                    <div className="setting-item">
                        <div className="setting-info">
                            <span className="setting-label">Launch at Login</span>
                        </div>
                        <label className="switch">
                            <input type="checkbox" disabled />
                            <span className="slider round"></span>
                        </label>
                    </div>
                </div>
            </div>
        </div>
    );
}
