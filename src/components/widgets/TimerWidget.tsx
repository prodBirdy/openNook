import { useState } from 'react';
import { IconPlayerPlay, IconPlayerPause, IconRefresh, IconPlus, IconTrash, IconClock } from '@tabler/icons-react';
import { registerWidget } from './WidgetRegistry';
import { CompactWidgetProps } from './WidgetTypes';
import { CompactWrapper } from '../island/CompactWrapper';
import { useTimerContext } from '../../context/TimerContext';
import { WidgetWrapper } from './WidgetWrapper';
import { WidgetAddDialog } from './WidgetAddDialog';

const PRESETS = [
    { label: '5m', seconds: 5 * 60 },
    { label: '15m', seconds: 15 * 60 },
    { label: '25m', seconds: 25 * 60 },
    { label: '1h', seconds: 60 * 60 },
];

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
    const { timers, addTimer, removeTimer, toggleTimer, resetTimer } = useTimerContext();
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
        <WidgetWrapper
            title="Timers"
            className="timer-widget"
            headerActions={[
                !showAddDialog && (
                    <button
                        key="add"
                        className="icon-button"
                        onClick={(e) => { e.stopPropagation(); setShowAddDialog(true); }}
                    >
                        <IconPlus size={18} />
                    </button>
                )
            ].filter(Boolean)}
        >
            {showAddDialog ? (
                <WidgetAddDialog
                    title="New Timer"
                    onClose={() => setShowAddDialog(false)}
                    onSubmit={(e) => {
                        e.preventDefault();
                        handleAddTimer(customMinutes ? parseInt(customMinutes) * 60 : 0);
                    }}
                    submitLabel="Start Timer"
                    submitDisabled={!customMinutes || parseInt(customMinutes) < 1}
                    mainInput={{
                        value: timerName,
                        onChange: e => setTimerName(e.target.value),
                        placeholder: "Timer Label",
                        icon: <IconClock size={18} color="var(--accent-color)" />
                    }}
                >

                    <div className="timer-presets">
                        {PRESETS.map(preset => (
                            <button
                                key={preset.label}
                                type="button"
                                className="preset-button"
                                onClick={() => handleAddTimer(preset.seconds)}
                            >
                                {preset.label}
                            </button>
                        ))}
                    </div>

                    <div className="form-row">
                        <label style={{ minWidth: '60px' }}>Duration</label>
                        <input
                            type="number"
                            value={customMinutes}
                            onChange={e => setCustomMinutes(e.target.value)}
                            placeholder="Minutes"
                            className="form-input"
                            min="1"
                            style={{ flex: 1 }}
                        />
                    </div>
                </WidgetAddDialog>
            ) : timers.length === 0 ? (
                <div className="empty-state">
                    <span>No active timers</span>
                    <button className="text-button" onClick={() => setShowAddDialog(true)}>Create Timer</button>
                </div>
            ) : (
                <div className="timers-list minimal-list" style={{ overflowY: 'auto' }}>
                    {timers.map(timer => {
                        const progress = timer.duration > 0
                            ? ((timer.duration - timer.remaining) / timer.duration) * 100
                            : 0;

                        return (
                            <div className={`timer-item-minimal ${timer.remaining === 0 ? 'finished' : ''}`} key={timer.id}>
                                <div
                                    className="timer-ring-container"
                                    onClick={(e) => { e.stopPropagation(); toggleTimer(timer.id); }}
                                >
                                    <svg viewBox="0 0 44 44" className="ring-svg">
                                        <circle
                                            className="ring-bg"
                                            cx="22" cy="22" r="20"
                                            fill="none"
                                            strokeWidth="3"
                                        />
                                        <circle
                                            className="ring-progress"
                                            cx="22" cy="22" r="20"
                                            fill="none"
                                            strokeWidth="3"
                                            strokeDasharray={`${(progress / 100) * (2 * Math.PI * 20)} ${2 * Math.PI * 20}`}
                                            strokeLinecap="round"
                                        />
                                    </svg>
                                    <div className="ring-icon-overlay">
                                        {timer.isRunning ? (
                                            <IconPlayerPause size={16} fill="currentColor" className="control-icon" />
                                        ) : (
                                            <IconPlayerPlay size={16} fill="currentColor" className="control-icon offset" />
                                        )}
                                    </div>
                                </div>

                                <div className="timer-info">
                                    <div className="time-display">{formatTime(timer.remaining)}</div>
                                    {timer.name && <div className="timer-label">{timer.name}</div>}
                                </div>

                                <div className="timer-actions-minimal">
                                    <button
                                        className="action-btn-minimal"
                                        onClick={(e) => { e.stopPropagation(); resetTimer(timer.id); }}
                                        title="Reset"
                                    >
                                        <IconRefresh size={18} />
                                    </button>
                                    <button
                                        className="action-btn-minimal destructive"
                                        onClick={(e) => { e.stopPropagation(); removeTimer(timer.id); }}
                                        title="Delete"
                                    >
                                        <IconTrash size={18} />
                                    </button>
                                </div>
                            </div>
                        );
                    })}
                </div>
            )}
        </WidgetWrapper>
    );
}

// Compact Timer Component
export function CompactTimer({ baseNotchWidth, isHovered, contentOpacity }: CompactWidgetProps) {
    const { timers, toggleTimer } = useTimerContext();

    // Prioritize showing a completed timer so the user sees it immediately
    const completedTimer = timers.find(t => t.remaining === 0 && t.duration > 0);
    const activeTimer = completedTimer || timers.find(t => t.isRunning) || timers[0];

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

    const calcProgress = activeTimer.duration > 0
        ? ((activeTimer.duration - activeTimer.remaining) / activeTimer.duration) * 100
        : 0;
    const progress = Math.min(100, Math.max(0, Number.isFinite(calcProgress) ? calcProgress : 0));

    const radius = 5;
    const circumference = 2 * Math.PI * radius;
    const strokeLength = (progress / 100) * circumference;

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

                    <div className="timer-ring-container" style={{ width: 24, height: 24 }}>
                        <svg viewBox="0 0 12 12" className="ring-svg">
                            <circle
                                className="ring-bg"
                                cx="6" cy="6" r="5"
                                fill="none"
                                strokeWidth="2"
                            />
                            <circle
                                className="ring-progress"
                                cx="6" cy="6" r="5"
                                fill="none"
                                strokeWidth="2"
                                strokeDasharray={`${strokeLength} ${circumference}`}
                                strokeLinecap="round"
                            />
                        </svg>
                    </div>
                </div>
            }
            right={
                <span style={{ color: 'white', fontSize: 14, fontVariantNumeric: 'tabular-nums' }}>
                    {formatTime(activeTimer.remaining)}
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
