import { useState, useEffect, useCallback, useRef } from 'react';
import { IconPlayerPlay, IconPlayerPause, IconRefresh, IconPlus, IconTrash, IconClock } from '@tabler/icons-react';
import { registerWidget } from './WidgetRegistry';
import { CompactWidgetProps } from './WidgetTypes';
import { CompactWrapper } from '../island/CompactWrapper';

interface TimerInstance {
    id: string;
    name: string;
    duration: number; // Total duration in seconds
    remaining: number; // Remaining time in seconds
    isRunning: boolean;
    createdAt: number;
}

const TIMER_STORAGE_KEY = 'timer-instances';
const PRESETS = [
    { label: '5m', seconds: 5 * 60 },
    { label: '15m', seconds: 15 * 60 },
    { label: '25m', seconds: 25 * 60 },
    { label: '1h', seconds: 60 * 60 },
];

function useTimers() {
    const [timers, setTimers] = useState<TimerInstance[]>([]);
    const intervalRefs = useRef<Map<string, ReturnType<typeof setInterval>>>(new Map());

    // Load timers from storage
    useEffect(() => {
        const saved = localStorage.getItem(TIMER_STORAGE_KEY);
        if (saved) {
            try {
                const parsed = JSON.parse(saved) as TimerInstance[];
                // Don't auto-resume running timers on reload for simplicity
                setTimers(parsed.map(t => ({ ...t, isRunning: false })));
            } catch (e) {
                console.error('Failed to load timers:', e);
            }
        }
    }, []);

    // Save timers to storage (excluding running state which resets on reload)
    useEffect(() => {
        localStorage.setItem(TIMER_STORAGE_KEY, JSON.stringify(timers));
    }, [timers]);

    const addTimer = useCallback((name: string, durationSeconds: number) => {
        const id = `timer-${Date.now()}`;
        const newTimer: TimerInstance = {
            id,
            name: name || `Timer ${timers.length + 1}`,
            duration: durationSeconds,
            remaining: durationSeconds,
            isRunning: false,
            createdAt: Date.now()
        };
        setTimers(prev => [...prev, newTimer]);
        return id;
    }, [timers.length]);

    const removeTimer = useCallback((id: string) => {
        // Clear interval if running
        const interval = intervalRefs.current.get(id);
        if (interval) {
            clearInterval(interval);
            intervalRefs.current.delete(id);
        }
        setTimers(prev => prev.filter(t => t.id !== id));
    }, []);

    const toggleTimer = useCallback((id: string) => {
        setTimers(prev => prev.map(timer => {
            if (timer.id !== id) return timer;

            const newIsRunning = !timer.isRunning;

            if (newIsRunning) {
                // Start the timer
                const interval = setInterval(() => {
                    setTimers(current => current.map(t => {
                        if (t.id !== id || !t.isRunning) return t;
                        const newRemaining = t.remaining - 1;
                        if (newRemaining <= 0) {
                            // Timer complete
                            clearInterval(intervalRefs.current.get(id)!);
                            intervalRefs.current.delete(id);
                            // Play notification sound
                            try {
                                const audio = new Audio('/notification.mp3');
                                audio.play().catch(() => { });
                            } catch { }
                            return { ...t, remaining: 0, isRunning: false };
                        }
                        return { ...t, remaining: newRemaining };
                    }));
                }, 1000);
                intervalRefs.current.set(id, interval);
            } else {
                // Pause the timer
                const interval = intervalRefs.current.get(id);
                if (interval) {
                    clearInterval(interval);
                    intervalRefs.current.delete(id);
                }
            }

            return { ...timer, isRunning: newIsRunning };
        }));
    }, []);

    const resetTimer = useCallback((id: string) => {
        // Clear interval if running
        const interval = intervalRefs.current.get(id);
        if (interval) {
            clearInterval(interval);
            intervalRefs.current.delete(id);
        }
        setTimers(prev => prev.map(timer =>
            timer.id === id
                ? { ...timer, remaining: timer.duration, isRunning: false }
                : timer
        ));
    }, []);

    // Cleanup intervals on unmount
    useEffect(() => {
        return () => {
            intervalRefs.current.forEach(interval => clearInterval(interval));
        };
    }, []);

    return { timers, addTimer, removeTimer, toggleTimer, resetTimer };
}

function formatTime(seconds: number): string {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    if (h > 0) {
        return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
    }
    return `${m}:${s.toString().padStart(2, '0')}`;
}

export function TimerWidget() {
    const { timers, addTimer, removeTimer, toggleTimer, resetTimer } = useTimers();
    const [showAddDialog, setShowAddDialog] = useState(false);
    const [customMinutes, setCustomMinutes] = useState('');
    const [timerName, setTimerName] = useState('');

    const handleAddTimer = (seconds: number) => {
        addTimer(timerName || '', seconds);
        setTimerName('');
        setCustomMinutes('');
        setShowAddDialog(false);
    };

    return (
        <div className="timer-widget apple-style" style={{ position: 'relative' }}>
            {showAddDialog ? (
                <div className="widget-overlay">
                    <div className="creation-form">
                        <div className="form-header">
                            <span className="form-title">New Timer</span>
                            <button type="button" className="close-button" onClick={() => setShowAddDialog(false)}>Cancel</button>
                        </div>
                        <input
                            value={timerName}
                            onChange={e => setTimerName(e.target.value)}
                            placeholder="Timer name (optional)"
                            className="form-input"
                            autoFocus
                        />
                        <div className="timer-presets">
                            {PRESETS.map(preset => (
                                <button
                                    key={preset.label}
                                    className="preset-button"
                                    onClick={() => handleAddTimer(preset.seconds)}
                                >
                                    {preset.label}
                                </button>
                            ))}
                        </div>
                        <div className="form-row">
                            <input
                                type="number"
                                value={customMinutes}
                                onChange={e => setCustomMinutes(e.target.value)}
                                placeholder="Custom (minutes)"
                                className="form-input"
                                min="1"
                            />
                            <button
                                className="submit-button"
                                onClick={() => handleAddTimer(parseInt(customMinutes) * 60)}
                                disabled={!customMinutes || parseInt(customMinutes) < 1}
                            >
                                Add
                            </button>
                        </div>
                    </div>
                </div>
            ) : (
                <>
                    <div className="widget-header">
                        <span className="widget-title">Timers</span>
                        <button
                            className="refresh-button"
                            onClick={(e) => { e.stopPropagation(); setShowAddDialog(true); }}
                        >
                            <IconPlus size={14} />
                        </button>
                    </div>

                    {timers.length === 0 ? (
                        <div className="no-events-message">No timers</div>
                    ) : (
                        <div className="timers-list">
                            {timers.map(timer => {
                                const progress = timer.duration > 0
                                    ? ((timer.duration - timer.remaining) / timer.duration) * 100
                                    : 0;

                                return (
                                    <div className={`timer-item ${timer.isRunning ? 'running' : ''} ${timer.remaining === 0 ? 'complete' : ''}`} key={timer.id}>
                                        <div className="timer-progress-ring">
                                            <svg viewBox="0 0 36 36">
                                                <circle
                                                    className="timer-ring-bg"
                                                    cx="18" cy="18" r="15.915"
                                                    fill="none"
                                                    strokeWidth="2"
                                                />
                                                <circle
                                                    className="timer-ring-progress"
                                                    cx="18" cy="18" r="15.915"
                                                    fill="none"
                                                    strokeWidth="2"
                                                    strokeDasharray={`${progress} 100`}
                                                    strokeLinecap="round"
                                                />
                                            </svg>
                                        </div>
                                        <div className="timer-content">
                                            <div className="timer-name">{timer.name}</div>
                                            <div className="timer-time">{formatTime(timer.remaining)}</div>
                                        </div>
                                        <div className="timer-controls">
                                            <button
                                                className="timer-control-btn"
                                                onClick={(e) => { e.stopPropagation(); toggleTimer(timer.id); }}
                                            >
                                                {timer.isRunning ? <IconPlayerPause size={16} /> : <IconPlayerPlay size={16} />}
                                            </button>
                                            <button
                                                className="timer-control-btn"
                                                onClick={(e) => { e.stopPropagation(); resetTimer(timer.id); }}
                                            >
                                                <IconRefresh size={16} />
                                            </button>
                                            <button
                                                className="timer-control-btn delete"
                                                onClick={(e) => { e.stopPropagation(); removeTimer(timer.id); }}
                                            >
                                                <IconTrash size={16} />
                                            </button>
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    )}
                </>
            )}
        </div>
    );
}

// Compact Timer Component
export function CompactTimer({ baseNotchWidth, isHovered, contentOpacity }: CompactWidgetProps) {
    const { timers, toggleTimer } = useTimers();

    // Show the first running timer, or the first timer if none running
    const activeTimer = timers.find(t => t.isRunning) || timers[0];

    if (!activeTimer) {
        return (
            <CompactWrapper
                id="timer-compact"
                baseNotchWidth={baseNotchWidth}
                isHovered={isHovered}
                contentOpacity={contentOpacity}
                left={
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <IconClock size={20} color="white" />
                        <span style={{ color: 'white', fontSize: 14 }}>No timers</span>
                    </div>
                }
            />
        );
    }

    return (
        <CompactWrapper
            id="timer-compact"
            baseNotchWidth={baseNotchWidth}
            isHovered={isHovered}
            contentOpacity={contentOpacity}
            left={
                <div
                    style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }}
                    onClick={(e) => { e.stopPropagation(); toggleTimer(activeTimer.id); }}
                >
                    <div className={`compact-timer-indicator ${activeTimer.isRunning ? 'pulsing' : ''}`}>
                        {activeTimer.isRunning ? <IconPlayerPause size={16} /> : <IconPlayerPlay size={16} />}
                    </div>
                    <span style={{ color: 'white', fontSize: 14, fontVariantNumeric: 'tabular-nums' }}>
                        {formatTime(activeTimer.remaining)}
                    </span>
                </div>
            }
            right={
                <span style={{ color: 'rgba(255,255,255,0.6)', fontSize: 12 }}>
                    {activeTimer.name}
                </span>
            }
        />
    );
}

// Register the timer widget
registerWidget({
    id: 'timer',
    name: 'Timer',
    description: 'Countdown timer with presets',
    icon: IconClock,
    ExpandedComponent: TimerWidget,
    CompactComponent: CompactTimer,
    defaultEnabled: false,
    category: 'productivity',
    minWidth: 260,
    hasCompactMode: true,
    compactPriority: 10
});
