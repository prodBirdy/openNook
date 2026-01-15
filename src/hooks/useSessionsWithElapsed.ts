import { useEffect, useState } from 'react';
import { useSessionStore } from '../stores/useSessionStore';

export function useSessionsWithElapsed() {
    const sessions = useSessionStore(state => state.sessions);
    const getElapsedTime = useSessionStore(state => state.getElapsedTime);
    const [, forceUpdate] = useState(0);

    useEffect(() => {
        const hasActive = sessions.some(s => s.isActive);
        if (!hasActive) return;

        const interval = setInterval(() => forceUpdate(n => n + 1), 1000);
        return () => clearInterval(interval);
    }, [sessions]);

    return { sessions, getElapsedTime };
}
