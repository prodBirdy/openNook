import { useState, useEffect } from 'react';
import { useWidgets } from '../../context/WidgetContext';
import './Settings.css';


interface SettingsState {
    showMedia: boolean;
    baseWidth: number;
    baseHeight: number;
    liquidGlassMode: boolean;
}

export default function Settings() {
    const { widgets, enabledState, toggleWidget } = useWidgets();
    const [settings, setSettings] = useState<SettingsState>({
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
        updateSetting(key, !settings[key] as SettingsState[typeof key]);
    };

    // Group widgets by category
    const productivityWidgets = widgets.filter(w => w.category === 'productivity');
    const utilityWidgets = widgets.filter(w => w.category === 'utility');
    const mediaWidgets = widgets.filter(w => w.category === 'media');

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
                <div className="section-title">Media</div>
                <div className="settings-group">
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
                <div className="section-title">Widgets</div>
                <div className="settings-group">
                    {productivityWidgets.map(widget => (
                        <div className="setting-item" key={widget.id}>
                            <div className="setting-info">
                                <span className="setting-label">{widget.name}</span>
                                <span className="setting-desc">{widget.description}</span>
                            </div>
                            <label className="switch">
                                <input
                                    type="checkbox"
                                    checked={enabledState[widget.id] ?? widget.defaultEnabled}
                                    onChange={() => toggleWidget(widget.id)}
                                />
                                <span className="slider round"></span>
                            </label>
                        </div>
                    ))}
                    {mediaWidgets.map(widget => (
                        <div className="setting-item" key={widget.id}>
                            <div className="setting-info">
                                <span className="setting-label">{widget.name}</span>
                                <span className="setting-desc">{widget.description}</span>
                            </div>
                            <label className="switch">
                                <input
                                    type="checkbox"
                                    checked={enabledState[widget.id] ?? widget.defaultEnabled}
                                    onChange={() => toggleWidget(widget.id)}
                                />
                                <span className="slider round"></span>
                            </label>
                        </div>
                    ))}
                    {utilityWidgets.map(widget => (
                        <div className="setting-item" key={widget.id}>
                            <div className="setting-info">
                                <span className="setting-label">{widget.name}</span>
                                <span className="setting-desc">{widget.description}</span>
                            </div>
                            <label className="switch">
                                <input
                                    type="checkbox"
                                    checked={enabledState[widget.id] ?? widget.defaultEnabled}
                                    onChange={() => toggleWidget(widget.id)}
                                />
                                <span className="slider round"></span>
                            </label>
                        </div>
                    ))}
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
