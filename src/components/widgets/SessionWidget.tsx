import { useState, useEffect, useCallback, useRef } from 'react';
import { IconPlayerPlay, IconPlayerStop, IconPlus, IconTrash, IconActivity } from '@tabler/icons-react';
import { registerWidget } from './WidgetRegistry';
import { CompactWidgetProps } from './WidgetTypes';
import { CompactWrapper } from '../island/CompactWrapper';

interface Session {
    id: string;
    name: string;
    startTime: number; // Unix timestamp in ms
    endTime: number | null; // null if still running
    isActive: boolean;
}

const SESSION_STORAGE_KEY = 'session-instances';

function useSessions() {
    const [sessions, setSessions] = useState<Session[]>([]);
    const [, forceUpdate] = useState(0);
    const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

    // Load sessions from storage
    useEffect(() => {
        const saved = localStorage.getItem(SESSION_STORAGE_KEY);
        if (saved) {
            try {
                setSessions(JSON.parse(saved));
            } catch (e) {
                console.error('Failed to load sessions:', e);
            }
        }
    }, []);

    // Save sessions to storage
    useEffect(() => {
        localStorage.setItem(SESSION_STORAGE_KEY, JSON.stringify(sessions));
    }, [sessions]);

    // Force re-render every second for elapsed time updates
    useEffect(() => {
        const hasActive = sessions.some(s => s.isActive);
        if (hasActive && !intervalRef.current) {
            intervalRef.current = setInterval(() => forceUpdate(n => n + 1), 1000);
        } else if (!hasActive && intervalRef.current) {
            clearInterval(intervalRef.current);
            intervalRef.current = null;
        }
        return () => {
            if (intervalRef.current) {
                clearInterval(intervalRef.current);
            }
        };
    }, [sessions]);

    const startSession = useCallback((name: string) => {
        const id = `session-${Date.now()}`;
        const newSession: Session = {
            id,
            name: name || 'Work Session',
            startTime: Date.now(),
            endTime: null,
            isActive: true
        };
        setSessions(prev => [newSession, ...prev]);
        return id;
    }, []);

    const stopSession = useCallback((id: string) => {
        setSessions(prev => prev.map(session =>
            session.id === id
                ? { ...session, endTime: Date.now(), isActive: false }
                : session
        ));
    }, []);

    const removeSession = useCallback((id: string) => {
        setSessions(prev => prev.filter(s => s.id !== id));
    }, []);

    const getElapsedTime = useCallback((session: Session): number => {
        const end = session.endTime || Date.now();
        return Math.floor((end - session.startTime) / 1000);
    }, []);

    return { sessions, startSession, stopSession, removeSession, getElapsedTime };
}

function formatElapsed(seconds: number): string {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    if (h > 0) {
        return `${h}h ${m}m`;
    }
    if (m > 0) {
        return `${m}m ${s}s`;
    }
    return `${s}s`;
}

function formatTimestamp(ts: number): string {
    return new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

export function SessionWidget() {
    const { sessions, startSession, stopSession, removeSession, getElapsedTime } = useSessions();
    const [showAddDialog, setShowAddDialog] = useState(false);
    const [sessionName, setSessionName] = useState('');

    const activeSessions = sessions.filter(s => s.isActive);
    const completedSessions = sessions.filter(s => !s.isActive).slice(0, 5);

    const handleStartSession = () => {
        startSession(sessionName);
        setSessionName('');
        setShowAddDialog(false);
    };

    return (
        <div className="session-widget apple-style" style={{ position: 'relative' }}>
            {showAddDialog ? (
                <div className="widget-overlay">
                    <form onSubmit={(e) => { e.preventDefault(); handleStartSession(); }} className="creation-form">
                        <div className="form-header">
                            <span className="form-title">Start Session</span>
                            <button type="button" className="close-button" onClick={() => setShowAddDialog(false)}>Cancel</button>
                        </div>
                        <input
                            value={sessionName}
                            onChange={e => setSessionName(e.target.value)}
                            placeholder="Session name (e.g., Deep Work)"
                            className="form-input"
                            autoFocus
                        />
                        <button type="submit" className="submit-button">
                            <IconPlayerPlay size={16} style={{ marginRight: 4 }} />
                            Start
                        </button>
                    </form>
                </div>
            ) : (
                <>
                    <div className="widget-header">
                        <span className="widget-title">Sessions</span>
                        <button
                            className="refresh-button"
                            onClick={(e) => { e.stopPropagation(); setShowAddDialog(true); }}
                        >
                            <IconPlus size={14} />
                        </button>
                    </div>

                    {/* Active Sessions */}
                    {activeSessions.length > 0 && (
                        <div className="sessions-section active">
                            {activeSessions.map(session => (
                                <div className="session-item active" key={session.id}>
                                    <div className="session-indicator recording" />
                                    <div className="session-content">
                                        <div className="session-name">{session.name}</div>
                                        <div className="session-elapsed">{formatElapsed(getElapsedTime(session))}</div>
                                    </div>
                                    <button
                                        className="session-stop-btn"
                                        onClick={(e) => { e.stopPropagation(); stopSession(session.id); }}
                                    >
                                        <IconPlayerStop size={16} />
                                    </button>
                                </div>
                            ))}
                        </div>
                    )}

                    {/* Session History */}
                    {completedSessions.length > 0 && (
                        <div className="sessions-section history">
                            <div className="section-label">Recent</div>
                            {completedSessions.map(session => (
                                <div className="session-item completed" key={session.id}>
                                    <div className="session-content">
                                        <div className="session-name">{session.name}</div>
                                        <div className="session-meta">
                                            <span>{formatTimestamp(session.startTime)}</span>
                                            <span className="session-duration">{formatElapsed(getElapsedTime(session))}</span>
                                        </div>
                                    </div>
                                    <button
                                        className="session-delete-btn"
                                        onClick={(e) => { e.stopPropagation(); removeSession(session.id); }}
                                    >
                                        <IconTrash size={14} />
                                    </button>
                                </div>
                            ))}
                        </div>
                    )}

                    {activeSessions.length === 0 && completedSessions.length === 0 && (
                        <div className="no-events-message">No sessions yet</div>
                    )}
                </>
            )}
        </div>
    );
}

// Compact Session Component
export function CompactSession({ baseNotchWidth, isHovered, contentOpacity }: CompactWidgetProps) {
    const { sessions, stopSession, getElapsedTime } = useSessions();
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
                        <span style={{ color: 'rgba(255,255,255,0.6)', fontSize: 14 }}>No active session</span>
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
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <div className="compact-session-indicator recording" />
                    <span style={{ color: 'white', fontSize: 14, fontVariantNumeric: 'tabular-nums' }}>
                        {formatElapsed(getElapsedTime(activeSession))}
                    </span>
                </div>
            }
            right={
                <div
                    style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }}
                    onClick={(e) => { e.stopPropagation(); stopSession(activeSession.id); }}
                >
                    <span style={{ color: 'rgba(255,255,255,0.6)', fontSize: 12, maxWidth: 80, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {activeSession.name}
                    </span>
                    <IconPlayerStop size={16} color="rgba(255,255,255,0.6)" />
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
