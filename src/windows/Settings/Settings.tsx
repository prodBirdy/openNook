import { useState, useEffect } from 'react';
import './Settings.css';


interface SettingsState {
    showCalendar: boolean;
    showReminders: boolean;
    showMedia: boolean;
    baseWidth: number;
    baseHeight: number;
    liquidGlassMode: boolean;
}

export default function Settings() {
    const [settings, setSettings] = useState<SettingsState>({
        showCalendar: false,
        showReminders: false,
        showMedia: true,
        baseWidth: 160,
        baseHeight: 38,
        liquidGlassMode: false,
    });

    useEffect(() => {
        const saved = localStorage.getItem('app-settings');
        if (saved) {
            try {
                const parsed = JSON.parse(saved);
                // Ensure defaults for new settings
                setSettings({
                    showCalendar: false,
                    showReminders: false,
                    showMedia: true,
                    baseWidth: 160,
                    baseHeight: 38,
                    liquidGlassMode: false,
                    ...parsed
                });
            } catch (e) {
                console.error("Failed to parse settings", e);
            }
        }
    }, []);

    const updateSetting = <K extends keyof SettingsState>(key: K, value: SettingsState[K]) => {
        setSettings(prev => {
            const next = { ...prev, [key]: value };
            localStorage.setItem('app-settings', JSON.stringify(next));
            // Dispatch event for other windows to update
            window.dispatchEvent(new StorageEvent('storage', {
                key: 'app-settings',
                newValue: JSON.stringify(next)
            }));
            return next;
        });
    };

    const toggleSetting = (key: keyof SettingsState) => {
        updateSetting(key, !settings[key]);
    };

    return (
        <div className="settings-window">
            <div className="settings-header">
                <h1>Settings</h1>
            </div>

            <div className="settings-section">
                <div className="section-title">Common Dimensions (Idle)</div>
                <div className="settings-group">
                    <div className="setting-item slider-item">
                        <div className="setting-info">
                            <span className="setting-label">Base Width</span>
                            <span className="setting-desc">{settings.baseWidth}px</span>
                        </div>
                        <input
                            type="range"
                            min="160"
                            max={window.screen.width}
                            value={settings.baseWidth}
                            onChange={(e) => updateSetting('baseWidth', parseInt(e.target.value))}
                            className="settings-slider"
                        />
                    </div>
                    <div className="setting-item slider-item">
                        <div className="setting-info">
                            <span className="setting-label">Base Height</span>
                            <span className="setting-desc">{settings.baseHeight}px</span>
                        </div>
                        <input
                            type="range"
                            min="38"
                            max={window.screen.height}
                            value={settings.baseHeight}
                            onChange={(e) => updateSetting('baseHeight', parseInt(e.target.value))}
                            className="settings-slider"
                        />
                    </div>
                </div>
            </div>

            <div className="settings-section">
                <div className="section-title">Appearance</div>
                <div className="settings-group">
                    <div className="setting-item">
                        <div className="setting-info">
                            <span className="setting-label">Liquid Glass Mode</span>
                            <span className="setting-desc">Experimental translucency effect</span>
                        </div>
                        <label className="switch">
                            <input
                                type="checkbox"
                                checked={settings.liquidGlassMode}
                                onChange={() => toggleSetting('liquidGlassMode')}
                            />
                            <span className="slider round"></span>
                        </label>
                    </div>
                </div>
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
