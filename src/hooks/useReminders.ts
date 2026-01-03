import { useEffect, useRef } from 'react';
import { useNotification } from '../context/NotificationContext';

// Reminder intervals (commented out for dev purposes)
/*
const HYDRATION_INTERVAL_MS = 2000;
const SCREEN_BREAK_INTERVAL_MS = 20 * 60 * 1000; // 20 minutes
*/

export function useReminders() {
    const { showNotification } = useNotification();
    const hydrationTimer = useRef<number | null>(null);
    const screenBreakTimer = useRef<number | null>(null);

    useEffect(() => {
        // Hydration reminder (disabled for dev)
        /*
        hydrationTimer.current = window.setInterval(() => {
            showNotification('hydration', 'Time to drink some water!');
        }, HYDRATION_INTERVAL_MS);

        // Screen break reminder
        screenBreakTimer.current = window.setInterval(() => {
            showNotification('screenBreak', 'Look away from the screen');
        }, SCREEN_BREAK_INTERVAL_MS);
        */

        return () => {
            if (hydrationTimer.current) clearInterval(hydrationTimer.current);
            if (screenBreakTimer.current) clearInterval(screenBreakTimer.current);
        };
    }, [showNotification]);
}
