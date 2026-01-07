import { createContext, useContext, useState, useEffect, useCallback, useRef, ReactNode } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { dbService } from '../services/DatabaseService';

export interface Session {
    id: string;
    name: string;
    icon?: string; // Icon name/identifier
    startTime: number; // Unix timestamp in ms
    endTime: number | null; // null if still running
    isActive: boolean;
    isCompleted: boolean;
}

interface SessionContextValue {
    sessions: Session[];
    startSession: (name: string, icon?: string) => string;
    stopSession: (id: string) => void;
    resumeSession: (id: string) => void;
    removeSession: (id: string) => void;
    getElapsedTime: (session: Session) => number;
}

const SessionContext = createContext<SessionContextValue | null>(null);

const SESSION_STORAGE_KEY = 'session-instances';
const SESSION_STATE_CHANGED_EVENT = 'session-state-changed';

export function SessionProvider({ children }: { children: ReactNode }) {
    const [sessions, setSessions] = useState<Session[]>([]);
    const [, forceUpdate] = useState(0);
    const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
    const isExternalUpdate = useRef(false);
    const senderId = useRef(Math.random().toString(36).substring(7));
    const [isLoaded, setIsLoaded] = useState(false);

    // Load sessions from storage
    useEffect(() => {
        const loadSessions = async () => {
            try {
                const saved = await dbService.getSetting<Session[]>(SESSION_STORAGE_KEY);
                if (saved) {
                    setSessions(saved);
                }
            } catch (error) {
                console.error('Failed to load sessions:', error);
            } finally {
                setIsLoaded(true);
            }
        };
        loadSessions();
    }, []);

    // Listen for session state changes from other windows
    useEffect(() => {
        const unlisten = listen<{ sessions: Session[], senderId: string }>(SESSION_STATE_CHANGED_EVENT, (event) => {
            // Ignore updates from ourselves
            if (event.payload.senderId === senderId.current) {
                return;
            }

            console.log('Received session state update from other window');
            isExternalUpdate.current = true;
            setSessions(event.payload.sessions);
            setTimeout(() => { isExternalUpdate.current = false; }, 100);
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, []);

    // Save sessions to storage and emit event
    useEffect(() => {
        if (!isLoaded) return;

        // Save to DB (async but we don't await in useEffect)
        dbService.saveSetting(SESSION_STORAGE_KEY, sessions).catch(console.error);

        // Emit event to sync with other windows
        if (!isExternalUpdate.current) {
            emit(SESSION_STATE_CHANGED_EVENT, {
                sessions,
                senderId: senderId.current
            }).catch(e => console.error('Failed to emit session state event:', e));
        }
    }, [sessions, isLoaded]);

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

    const resumeSession = useCallback((id: string) => {
        setSessions(prev => prev.map(session => {
            if (session.id === id) {
                const now = Date.now();
                // If it was stopped, endTime is set. If running, endTime is null (use now).
                const currentEndTime = session.endTime || now;
                // Calculate previously elapsed time
                const duration = currentEndTime - session.startTime;

                // Adjust start time so that (now - newStartTime) equals the previous duration
                // This effectively "pauses" the time that passed while it was inactive
                return {
                    ...session,
                    isActive: true,
                    isCompleted: false,
                    endTime: null,
                    startTime: now - duration
                };
            }
            return session;
        }));
    }, []);

    const startSession = useCallback((name: string, icon?: string) => {
        const id = `session-${Date.now()}`;
        const newSession: Session = {
            id,
            name: name || 'Work Session',
            icon,
            startTime: Date.now(),
            endTime: null,
            isActive: true,
            isCompleted: false
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
    }, []); // Removed dependency as Date.now() changes, but this is a pure helper essentially.
    // In a hook this should be stable. The re-renders are driven by the interval + forceUpdate.

    return (
        <SessionContext.Provider value={{ sessions, startSession, stopSession, resumeSession, removeSession, getElapsedTime }}>
            {children}
        </SessionContext.Provider>
    );
}

export function useSessionContext() {
    const context = useContext(SessionContext);
    if (!context) {
        throw new Error('useSessionContext must be used within a SessionProvider');
    }
    return context;
}
