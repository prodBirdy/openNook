import { useEffect, useState } from 'react';
import { useTimerStore } from '../stores/useTimerStore';

// Hook for derived timers with auto-refresh
export function useDerivedTimers() {
    const timers = useTimerStore(state => state.timers);
    const getDerivedTimers = useTimerStore(state => state.getDerivedTimers);

    // Force re-render when there are running timers
    const [, forceUpdate] = useState(0);

    useEffect(() => {
        const hasRunning = timers.some(t => t.isRunning);
        if (!hasRunning) return;

        const interval = setInterval(() => forceUpdate(n => n + 1), 1000);
        return () => clearInterval(interval);
    }, [timers]);

    return getDerivedTimers();
}
