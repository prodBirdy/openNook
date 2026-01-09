import { create } from 'zustand';
import { emit, listen } from '@tauri-apps/api/event';
import { dbService } from '../services/DatabaseService';
import React from 'react';

export interface Session {
    id: string;
    name: string;
    icon?: string;
    startTime: number;
    endTime: number | null;
    isActive: boolean;
    isCompleted: boolean;
}

interface SessionState {
    sessions: Session[];
    isLoaded: boolean;
}

interface SessionActions {
    loadSessions: () => Promise<void>;
    startSession: (name: string, icon?: string) => string;
    stopSession: (id: string) => void;
    resumeSession: (id: string) => void;
    removeSession: (id: string) => void;
    getElapsedTime: (session: Session) => number;
    setSessions: (sessions: Session[]) => void;
    setupListeners: () => () => void;
}

type SessionStore = SessionState & SessionActions;

const SESSION_STORAGE_KEY = 'session-instances';
const SESSION_STATE_CHANGED_EVENT = 'session-state-changed';
const senderId = Math.random().toString(36).substring(7);


let tickInterval: ReturnType<typeof setInterval> | null = null;
let forceUpdateCallback: (() => void) | null = null;

export const useSessionStore = create<SessionStore>((set, get) => ({
    sessions: [],
    isLoaded: false,

    loadSessions: async () => {
        try {
            const saved = await dbService.getSetting<Session[]>(SESSION_STORAGE_KEY);
            if (saved) {
                set({ sessions: saved, isLoaded: true });
            } else {
                set({ isLoaded: true });
            }
        } catch (error) {
            console.error('Failed to load sessions:', error);
            set({ isLoaded: true });
        }
    },

    startSession: (name, icon) => {
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

        const { sessions } = get();
        const updated = [newSession, ...sessions];
        set({ sessions: updated });

        dbService.saveSetting(SESSION_STORAGE_KEY, updated).catch(console.error);
        emit(SESSION_STATE_CHANGED_EVENT, { sessions: updated, senderId }).catch(console.error);

        return id;
    },

    stopSession: (id) => {
        const { sessions } = get();
        const updated = sessions.map(session =>
            session.id === id
                ? { ...session, endTime: Date.now(), isActive: false }
                : session
        );

        set({ sessions: updated });
        dbService.saveSetting(SESSION_STORAGE_KEY, updated).catch(console.error);
        emit(SESSION_STATE_CHANGED_EVENT, { sessions: updated, senderId }).catch(console.error);
    },

    resumeSession: (id) => {
        const { sessions } = get();
        const updated = sessions.map(session => {
            if (session.id === id) {
                const now = Date.now();
                const currentEndTime = session.endTime || now;
                const duration = currentEndTime - session.startTime;

                return {
                    ...session,
                    isActive: true,
                    isCompleted: false,
                    endTime: null,
                    startTime: now - duration
                };
            }
            return session;
        });

        set({ sessions: updated });
        dbService.saveSetting(SESSION_STORAGE_KEY, updated).catch(console.error);
        emit(SESSION_STATE_CHANGED_EVENT, { sessions: updated, senderId }).catch(console.error);
    },

    removeSession: (id) => {
        const { sessions } = get();
        const updated = sessions.filter(s => s.id !== id);

        set({ sessions: updated });
        dbService.saveSetting(SESSION_STORAGE_KEY, updated).catch(console.error);
        emit(SESSION_STATE_CHANGED_EVENT, { sessions: updated, senderId }).catch(console.error);
    },

    getElapsedTime: (session) => {
        const end = session.endTime || Date.now();
        return Math.floor((end - session.startTime) / 1000);
    },

    setSessions: (sessions) => {
        set({ sessions });
    },

    setupListeners: () => {
        // Listen for cross-window sync
        const unlistenPromise = listen<{ sessions: Session[], senderId: string }>(
            SESSION_STATE_CHANGED_EVENT,
            (event) => {
                if (event.payload.senderId === senderId) return;

                console.log('Received session state update from other window');
                get().setSessions(event.payload.sessions);
            }
        );

        // Setup tick interval for active sessions
        const checkAndSetupInterval = () => {
            const { sessions } = get();
            const hasActive = sessions.some(s => s.isActive);

            if (hasActive && !tickInterval) {
                tickInterval = setInterval(() => {
                    forceUpdateCallback?.();
                }, 1000);
            } else if (!hasActive && tickInterval) {
                clearInterval(tickInterval);
                tickInterval = null;
            }
        };

        const unsubscribe = useSessionStore.subscribe(checkAndSetupInterval);
        checkAndSetupInterval();

        return () => {
            unlistenPromise.then(fn => fn());
            unsubscribe();
            if (tickInterval) {
                clearInterval(tickInterval);
                tickInterval = null;
            }
        };
    }
}));

// Selectors
export const selectSessions = (state: SessionStore) => state.sessions;
export const selectHasActiveSession = (state: SessionStore) => state.sessions.some(s => s.isActive);

// Hook with auto-refresh for elapsed time
export function useSessionsWithElapsed() {
    const sessions = useSessionStore(state => state.sessions);
    const getElapsedTime = useSessionStore(state => state.getElapsedTime);
    const [, forceUpdate] = React.useState(0);

    React.useEffect(() => {
        forceUpdateCallback = () => forceUpdate(n => n + 1);
        return () => { forceUpdateCallback = null; };
    }, []);

    React.useEffect(() => {
        const hasActive = sessions.some(s => s.isActive);
        if (!hasActive) return;

        const interval = setInterval(() => forceUpdate(n => n + 1), 1000);
        return () => clearInterval(interval);
    }, [sessions]);

    return { sessions, getElapsedTime };
}
