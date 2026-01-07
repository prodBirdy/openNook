import { useState, useEffect } from 'react';
import { IconPlayerPlay, IconPlayerStop, IconPlus, IconTrash, IconActivity, IconBriefcase, IconCode, IconBook, IconMoon, IconCpu, IconBulb, IconHeadphones, IconCoffee, IconDeviceLaptop, IconWriting } from '@tabler/icons-react';
import { registerWidget } from './WidgetRegistry';
import { CompactWidgetProps } from './WidgetTypes';
import { CompactWrapper } from '../island/CompactWrapper';
import { useSessionContext } from '../../context/SessionContext';
import { WidgetWrapper } from './WidgetWrapper';
import { WidgetAddDialog } from './WidgetAddDialog';

const AVAILABLE_ICONS = [
    { name: 'activity', Icon: IconActivity, label: 'Default' },
    { name: 'code', Icon: IconCode, label: 'Code' },
    { name: 'briefcase', Icon: IconBriefcase, label: 'Work' },
    { name: 'book', Icon: IconBook, label: 'Study' },
    { name: 'writing', Icon: IconWriting, label: 'Write' },
    { name: 'moon', Icon: IconMoon, label: 'Rest' },
    { name: 'bulb', Icon: IconBulb, label: 'Focus' },
    { name: 'cpu', Icon: IconCpu, label: 'Deep' },
    { name: 'headphones', Icon: IconHeadphones, label: 'Listen' },
    { name: 'coffee', Icon: IconCoffee, label: 'Break' },
    { name: 'laptop', Icon: IconDeviceLaptop, label: 'Meeting' },
];

function formatElapsed(seconds: number): string {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    if (h > 0) {
        return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
    }
    return `${m}:${s.toString().padStart(2, '0')}`;
}

function getSessionIcon(session: { name: string, icon?: string }) {
    if (session.icon) {
        const found = AVAILABLE_ICONS.find(i => i.name === session.icon);
        if (found) return found.Icon;
    }

    // Fallback to heuristic
    const n = session.name.toLowerCase();
    if (n.includes('code') || n.includes('dev') || n.includes('program')) return IconCode;
    if (n.includes('work') || n.includes('job') || n.includes('busines')) return IconBriefcase;
    if (n.includes('read') || n.includes('book') || n.includes('study')) return IconBook;
    if (n.includes('sleep') || n.includes('rest') || n.includes('nap')) return IconMoon;
    return IconActivity; // Default
}

export function SessionWidget() {
    const { sessions, startSession, stopSession, resumeSession, removeSession, getElapsedTime } = useSessionContext();
    const [showAddDialog, setShowAddDialog] = useState(false);
    const [sessionName, setSessionName] = useState('');
    const [selectedIcon, setSelectedIcon] = useState('activity');

    const activeSessions = sessions.filter(s => s.isActive);
    const completedSessions = sessions.filter(s => !s.isActive).slice(0, 5);

    const handleStartSession = () => {
        startSession(sessionName, selectedIcon);
        setSessionName('');
        setSelectedIcon('activity');
        setShowAddDialog(false);
    };

    return (
        <WidgetWrapper
            title="Sessions"
            className="session-widget"
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
                    title="New Session"
                    onClose={() => setShowAddDialog(false)}
                    onSubmit={(e) => { e.preventDefault(); handleStartSession(); }}
                    submitLabel="Start Session"
                    submitIcon={<IconPlayerPlay size={16} />}
                    mainInput={{
                        value: sessionName,
                        onChange: e => setSessionName(e.target.value),
                        placeholder: "Session name (e.g., Deep Work)",
                        icon: (() => {
                            const SelectedIcon = AVAILABLE_ICONS.find(i => i.name === selectedIcon)?.Icon || IconActivity;
                            return <SelectedIcon size={18} color="var(--accent-color)" />;
                        })()
                    }}
                >

                    <div style={{ marginBottom: 4 }}>
                        <div style={{
                            display: 'flex',
                            gap: 8,
                            overflowX: 'auto',
                            padding: '4px 0',
                            msOverflowStyle: 'none',
                            scrollbarWidth: 'none'
                        }}>
                            {AVAILABLE_ICONS.map(({ name, Icon }) => (
                                <div
                                    key={name}
                                    style={{
                                        width: 32,
                                        height: 32,
                                        minWidth: 32,
                                        borderRadius: '50%',
                                        background: selectedIcon === name ? 'var(--accent-color)' : 'rgba(255,255,255,0.05)',
                                        display: 'flex',
                                        alignItems: 'center',
                                        justifyContent: 'center',
                                        cursor: 'pointer',
                                        border: selectedIcon === name ? '2px solid white' : '1px solid transparent',
                                        transition: 'all 0.2s'
                                    }}
                                    onClick={() => setSelectedIcon(name)}
                                >
                                    <Icon size={16} color="white" />
                                </div>
                            ))}
                        </div>
                    </div>
                </WidgetAddDialog>
            ) : (
                <>
                    {/* Active Sessions */}
                    {activeSessions.length > 0 && (
                        <div className="timers-list minimal-list" style={{ overflowY: 'auto' }}>
                            {activeSessions.map(session => (
                                <div className="timer-item-minimal" key={session.id}>

                                    <div className="timer-icon">
                                        {(() => {
                                            const Icon = getSessionIcon(session);
                                            return <Icon size={24} />;
                                        })()}
                                    </div>
                                    <div className='timer-info'>
                                        <div className='time-display'>{formatElapsed(getElapsedTime(session))}</div>
                                        <div className='timer-label' >{session.name || 'Focus Session'}</div>
                                    </div>

                                    <div className="timer-actions-minimal">
                                        <button
                                            className="action-btn-minimal destructive"
                                            onClick={(e) => { e.stopPropagation(); stopSession(session.id); }}
                                            title="Stop Session"
                                        >
                                            <IconPlayerStop size={18} />
                                        </button>
                                    </div>
                                </div>
                            ))}
                        </div>
                    )}

                    {completedSessions.length > 0 && (
                        <div className="timers-list minimal-list" style={{ overflowY: 'auto', marginTop: activeSessions.length > 0 ? 12 : 0 }}>
                            {activeSessions.length > 0 && <div className="section-label">Recent</div>}
                            {completedSessions.map(session => (
                                <div
                                    className="timer-item-minimal"
                                    key={session.id}
                                    onClick={(e) => { e.stopPropagation(); resumeSession(session.id); }}
                                    style={{ cursor: 'pointer' }}
                                >
                                    <div className="timer-icon">
                                        {(() => {
                                            const Icon = getSessionIcon(session);
                                            return <Icon size={24} />;
                                        })()}
                                    </div>
                                    <div className="timer-info">
                                        <div className="time-display">{formatElapsed(getElapsedTime(session))}</div>
                                        <div className="timer-label">{session.name || 'Focus Session'}</div>
                                    </div>

                                    <div className="timer-actions-minimal">
                                        <button
                                            className="action-btn-minimal"
                                            onClick={(e) => { e.stopPropagation(); resumeSession(session.id); }}
                                            title="Resume Session"
                                        >
                                            <IconPlayerPlay size={18} />
                                        </button>
                                        <button
                                            className="action-btn-minimal destructive"
                                            onClick={(e) => { e.stopPropagation(); removeSession(session.id); }}
                                            title="Delete Entry"
                                        >
                                            <IconTrash size={18} />
                                        </button>
                                    </div>
                                </div>
                            ))}
                        </div>
                    )}

                    {activeSessions.length === 0 && completedSessions.length === 0 && (
                        <div className="empty-state">
                            <span>No sessions</span>
                            <button className="text-button" onClick={() => setShowAddDialog(true)}>Start Session</button>
                        </div>
                    )}
                </>
            )}
        </WidgetWrapper>
    );
}

// Compact Session Component
export function CompactSession({ baseNotchWidth, isHovered, contentOpacity }: CompactWidgetProps) {
    const { sessions, stopSession, getElapsedTime } = useSessionContext();
    const [, forceUpdate] = useState(0);

    // Force re-render every second for elapsed time
    useEffect(() => {
        const hasActive = sessions.some(s => s.isActive);
        if (!hasActive) return;
        const interval = setInterval(() => forceUpdate(n => n + 1), 1000);
        return () => clearInterval(interval);
    }, [sessions]);

    const activeSession = sessions.find(s => s.isActive);

    if (!activeSession) {
        return (
            <CompactWrapper
                id="session-compact"
                baseNotchWidth={baseNotchWidth}
                isHovered={isHovered}
                contentOpacity={contentOpacity}
                left={
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <IconActivity size={20} color="white" />
                    </div>
                }
            />
        );
    }

    return (
        <CompactWrapper
            id="session-compact"
            baseNotchWidth={baseNotchWidth}
            isHovered={isHovered}
            contentOpacity={contentOpacity}
            left={
                <div
                    onClick={(e) => { e.stopPropagation(); stopSession(activeSession.id); }}
                >
                    <div className="compact-session-icon">
                        {(() => {
                            const Icon = getSessionIcon(activeSession);
                            return <Icon size={14} />;
                        })()}
                    </div>
                </div>
            }
            right={
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span style={{ color: 'white', fontSize: 14, fontVariantNumeric: 'tabular-nums' }}>
                        {formatElapsed(getElapsedTime(activeSession))}
                    </span>
                </div>
            }
        />
    );
}

// Register the session widget
registerWidget({
    id: 'session',
    name: 'Sessions',
    description: 'Track work sessions',
    icon: IconActivity,
    ExpandedComponent: SessionWidget,
    CompactComponent: CompactSession,
    defaultEnabled: false,
    category: 'productivity',
    minWidth: 260,
    hasCompactMode: true,
    compactPriority: 15
});
