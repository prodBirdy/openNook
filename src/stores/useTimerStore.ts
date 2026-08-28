import { create } from 'zustand';
import { emit, listen } from '@tauri-apps/api/event';
import { dbService } from '../services/DatabaseService';
import confetti from 'canvas-confetti';
import { playNotificationSound } from '../utils/soundUtils';

export interface TimerInstance {
    id: string;
    name: string;
    duration: number;
    remaining: number;
    isRunning: boolean;
    createdAt: number;
    lastStartTime?: number;
}

interface TimerState {
    timers: TimerInstance[];
    isLoaded: boolean;
}

interface TimerActions {
    loadTimers: () => Promise<void>;
    addTimer: (name: string, durationSeconds: number) => string;
    removeTimer: (id: string) => void;
    toggleTimer: (id: string) => void;
    resetTimer: (id: string) => void;
    setTimers: (timers: TimerInstance[]) => void;
    tick: () => void;
    setupListeners: () => () => void;
    getDerivedTimers: () => TimerInstance[];
}

type TimerStore = TimerState & TimerActions;

const TIMER_STORAGE_KEY = 'timer-instances';
const TIMER_STATE_CHANGED_EVENT = 'timer-state-changed';
const senderId = Math.random().toString(36).substring(7);

let tickInterval: ReturnType<typeof setInterval> | null = null;

export const useTimerStore = create<TimerStore>((set, get) => ({
    timers: [],
    isLoaded: false,

    loadTimers: async () => {
        try {
            const saved = await dbService.getSetting<TimerInstance[]>(TIMER_STORAGE_KEY);
            if (saved) {
                // Pause running timers on load
                set({
                    timers: saved.map(t => ({ ...t, isRunning: false, lastStartTime: undefined })),
                    isLoaded: true
                });
            } else {
                set({ isLoaded: true });
            }
        } catch (error) {
            console.error('Failed to load timers:', error);
            set({ isLoaded: true });
        }
    },

    addTimer: (name, durationSeconds) => {
        const { timers } = get();
        const id = `timer-${Date.now()}`;
        const newTimer: TimerInstance = {
            id,
            name: name || `Timer ${timers.length + 1}`,
            duration: durationSeconds,
            remaining: durationSeconds,
            isRunning: false,
            createdAt: Date.now()
        };

        const updated = [...timers, newTimer];
        set({ timers: updated });

        // Persist and sync
        dbService.saveSetting(TIMER_STORAGE_KEY, updated).catch(console.error);
        emit(TIMER_STATE_CHANGED_EVENT, { timers: updated, senderId }).catch(console.error);

        return id;
    },

    removeTimer: (id) => {
        const { timers } = get();
        const updated = timers.filter(t => t.id !== id);
        set({ timers: updated });

        dbService.saveSetting(TIMER_STORAGE_KEY, updated).catch(console.error);
        emit(TIMER_STATE_CHANGED_EVENT, { timers: updated, senderId }).catch(console.error);
    },

    toggleTimer: (id) => {
        const { timers } = get();
        const updated = timers.map(t => {
            if (t.id !== id) return t;

            if (t.isRunning) {
                // STOPPING
                const elapsedSinceStart = t.lastStartTime ? (Date.now() - t.lastStartTime) / 1000 : 0;
                const newRemaining = Math.max(0, t.remaining - elapsedSinceStart);
                return {
                    ...t,
                    isRunning: false,
                    remaining: newRemaining,
                    lastStartTime: undefined
                };
            } else {
                // STARTING
                if (t.remaining <= 0) return t;
                return {
                    ...t,
                    isRunning: true,
                    lastStartTime: Date.now()
                };
            }
        });

        set({ timers: updated });
        dbService.saveSetting(TIMER_STORAGE_KEY, updated).catch(console.error);
        emit(TIMER_STATE_CHANGED_EVENT, { timers: updated, senderId }).catch(console.error);
    },

    resetTimer: (id) => {
        const { timers } = get();
        const updated = timers.map(t =>
            t.id === id
                ? { ...t, remaining: t.duration, isRunning: false, lastStartTime: undefined }
                : t
        );

        set({ timers: updated });
        dbService.saveSetting(TIMER_STORAGE_KEY, updated).catch(console.error);
        emit(TIMER_STATE_CHANGED_EVENT, { timers: updated, senderId }).catch(console.error);
    },

    setTimers: (timers) => {
        set({ timers });
    },

    tick: () => {
        const { timers } = get();
        let completedAny = false;

        const updated = timers.map(t => {
            if (!t.isRunning || !t.lastStartTime) return t;

            const elapsedSinceStart = (Date.now() - t.lastStartTime) / 1000;
            const currentRemaining = t.remaining - elapsedSinceStart;

            if (currentRemaining <= 0) {
                completedAny = true;
                return {
                    ...t,
                    remaining: 0,
                    isRunning: false,
                    lastStartTime: undefined
                };
            }
            return t;
        });

        if (completedAny) {
            set({ timers: updated });
            dbService.saveSetting(TIMER_STORAGE_KEY, updated).catch(console.error);
            emit(TIMER_STATE_CHANGED_EVENT, { timers: updated, senderId }).catch(console.error);

            // Play notification and confetti
            playNotificationSound();
            confetti({
                particleCount: 50,
                spread: 50,
                origin: { y: 0.1 },
                zIndex: 99999,
                shapes: ['circle'],
                colors: ['#FFFF00']
            });
        }
    },

    setupListeners: () => {
        // Listen for cross-window sync
        const unlistenPromise = listen<{ timers: TimerInstance[], senderId: string }>(
            TIMER_STATE_CHANGED_EVENT,
            (event) => {
                if (event.payload.senderId === senderId) return;

                console.log('Received timer state update from other window');
                get().setTimers(event.payload.timers);
            }
        );

        // Setup tick interval
        const checkAndSetupInterval = () => {
            const { timers } = get();
            const hasRunning = timers.some(t => t.isRunning);

            if (hasRunning && !tickInterval) {
                tickInterval = setInterval(() => get().tick(), 1000);
            } else if (!hasRunning && tickInterval) {
                clearInterval(tickInterval);
                tickInterval = null;
            }
        };

        // Subscribe to state changes
        const unsubscribe = useTimerStore.subscribe(checkAndSetupInterval);
        checkAndSetupInterval();

        return () => {
            unlistenPromise.then(fn => fn());
            unsubscribe();
            if (tickInterval) {
                clearInterval(tickInterval);
                tickInterval = null;
            }
        };
    },

    getDerivedTimers: () => {
        const { timers } = get();
        return timers.map(t => {
            let currentRemaining = t.remaining;
            if (t.isRunning && t.lastStartTime) {
                const elapsedSinceStart = (Date.now() - t.lastStartTime) / 1000;
                currentRemaining = Math.max(0, t.remaining - elapsedSinceStart);
            }
            return { ...t, remaining: Math.ceil(currentRemaining) };
        });
    }
}));

// Selectors
export const selectTimers = (state: TimerStore) => state.getDerivedTimers();
export const selectHasRunningTimer = (state: TimerStore) => state.timers.some(t => t.isRunning);

// Hook for derived timers with auto-refresh
export function useDerivedTimers() {
    const timers = useTimerStore(state => state.timers);
    const getDerivedTimers = useTimerStore(state => state.getDerivedTimers);

    // Force re-render when there are running timers
    const [, forceUpdate] = React.useState(0);

    React.useEffect(() => {
        const hasRunning = timers.some(t => t.isRunning);
        if (!hasRunning) return;

        const interval = setInterval(() => forceUpdate(n => n + 1), 1000);
        return () => clearInterval(interval);
    }, [timers]);

    return getDerivedTimers();
}

import React from 'react';
