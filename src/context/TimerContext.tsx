import { createContext, useContext, useState, useEffect, useCallback, useRef, ReactNode } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { dbService } from '../services/DatabaseService';
import confetti from 'canvas-confetti';
import { playNotificationSound } from '../utils/soundUtils';

export interface TimerInstance {
    id: string;
    name: string;
    duration: number; // Total duration in seconds
    remaining: number; // Remaining time in seconds (snapshot at last pause)
    isRunning: boolean;
    createdAt: number;
    lastStartTime?: number; // Timestamp when the timer was last started
}

interface TimerContextValue {
    timers: TimerInstance[];
    addTimer: (name: string, durationSeconds: number) => string;
    removeTimer: (id: string) => void;
    toggleTimer: (id: string) => void;
    resetTimer: (id: string) => void;
}

const TimerContext = createContext<TimerContextValue | null>(null);

const TIMER_STORAGE_KEY = 'timer-instances';
const TIMER_STATE_CHANGED_EVENT = 'timer-state-changed';

export function TimerProvider({ children }: { children: ReactNode }) {
    const [timers, setTimers] = useState<TimerInstance[]>([]);
    const [isLoaded, setIsLoaded] = useState(false);
    const [, forceUpdate] = useState(0); // Used to trigger re-renders for UI updates
    const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
    const isExternalUpdate = useRef(false);
    const senderId = useRef(Math.random().toString(36).substring(7));

    // Load timers from storage
    useEffect(() => {
        const loadTimers = async () => {
            try {
                const saved = await dbService.getSetting<TimerInstance[]>(TIMER_STORAGE_KEY);
                if (saved) {
                    // Don't auto-resume running timers on reload for simplicity,
                    // or we could calculate elapsed time if we wanted persistence across reloads.
                    // For now, we pause them to match previous behavior, but we could easily
                    // make them resume by keeping lastStartTime if we wanted.
                    // Let's stick to pausing on load to avoid confusion.
                    setTimers(saved.map(t => ({ ...t, isRunning: false, lastStartTime: undefined })));
                }
            } catch (error) {
                console.error('Failed to load timers:', error);
            } finally {
                setIsLoaded(true);
            }
        };
        loadTimers();
    }, []);

    // Listen for timer state changes from other windows
    useEffect(() => {
        const unlisten = listen<{ timers: TimerInstance[], senderId: string }>(TIMER_STATE_CHANGED_EVENT, (event) => {
            // Ignore updates from ourselves
            if (event.payload.senderId === senderId.current) {
                return;
            }

            console.log('Received timer state update from other window');
            isExternalUpdate.current = true;

            // We just update the state. The sender has already handled the logic.
            setTimers(event.payload.timers);
            setTimeout(() => { isExternalUpdate.current = false; }, 100);
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, []);

    // Save timers to storage and emit event
    // This now ONLY triggers when 'timers' changes, which happens on user interaction (start/stop/add/remove),
    // NOT on every tick.
    useEffect(() => {
        if (!isLoaded) return;

        // Save to DB
        dbService.saveSetting(TIMER_STORAGE_KEY, timers).catch(console.error);

        // Emit event to sync with other windows
        if (!isExternalUpdate.current && timers.length >= 0) {
            emit(TIMER_STATE_CHANGED_EVENT, {
                timers,
                senderId: senderId.current
            }).catch(e => console.error('Failed to emit timer state event:', e));
        }
    }, [timers, isLoaded]);

    // Tick mechanism for UI updates only
    useEffect(() => {
        const hasRunningTimers = timers.some(t => t.isRunning);

        if (hasRunningTimers && !intervalRef.current) {
            intervalRef.current = setInterval(() => {
                forceUpdate(n => n + 1);

                // Check for completion
                // We need to check if any timer has reached 0
                // If so, we need to update the state to stop it
                let completedAny = false;
                setTimers(prev => {
                    const next = prev.map(t => {
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
                        // Play notification sound and show confetti (only once even if multiple timers finished)
                        playNotificationSound();
                        confetti({
                            particleCount: 50,
                            spread: 50,
                            origin: { y: 0.1 },
                            zIndex: 99999, // Above everything including the island
                            shapes: ['circle'],
                            colors: ['#FFFF00']
                        });
                        return next;
                    }
                    return prev;
                });

            }, 1000);
        } else if (!hasRunningTimers && intervalRef.current) {
            clearInterval(intervalRef.current);
            intervalRef.current = null;
        }

        return () => {
            if (intervalRef.current) {
                clearInterval(intervalRef.current);
                intervalRef.current = null;
            }
        };
    }, [timers]); // Re-evaluate when timers state changes (e.g. started/stopped)

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
        setTimers(prev => prev.filter(t => t.id !== id));
    }, []);

    const toggleTimer = useCallback((id: string) => {
        setTimers(prev => prev.map(t => {
            if (t.id !== id) return t;

            if (t.isRunning) {
                // STOPPING
                // Calculate actual remaining time and store it
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
                if (t.remaining <= 0) return t; // Cannot start finished timer

                return {
                    ...t,
                    isRunning: true,
                    lastStartTime: Date.now()
                };
            }
        }));
    }, []);

    const resetTimer = useCallback((id: string) => {
        setTimers(prev => prev.map(t =>
            t.id === id
                ? { ...t, remaining: t.duration, isRunning: false, lastStartTime: undefined }
                : t
        ));
    }, []);

    // Calculate derived state for UI consumption
    const getDerivedTimers = () => {
        return timers.map(t => {
            let currentRemaining = t.remaining;
            if (t.isRunning && t.lastStartTime) {
                const elapsedSinceStart = (Date.now() - t.lastStartTime) / 1000;
                currentRemaining = Math.max(0, t.remaining - elapsedSinceStart);
            }
            return { ...t, remaining: Math.ceil(currentRemaining) };
        });
    };

    return (
        <TimerContext.Provider value={{ timers: getDerivedTimers(), addTimer, removeTimer, toggleTimer, resetTimer }}>
            {children}
        </TimerContext.Provider>
    );
}

export function useTimerContext() {
    const context = useContext(TimerContext);
    if (!context) {
        throw new Error('useTimerContext must be used within a TimerProvider');
    }
    return context;
}
