import { motion, AnimatePresence } from 'motion/react';
import { useEffect } from 'react';
import { useNotification } from '../context/NotificationContext';
import { setClickThrough } from '../hooks/useNotchInfo';
import './DynamicIslandAlert.css';

export function DynamicIslandAlert() {
    const { notification, dismissNotification } = useNotification();

    // Toggle click-through based on notification state
    useEffect(() => {
        // When notification appears, disable click-through so user can interact
        // When notification disappears, enable click-through so clicks pass through
        setClickThrough(!notification).catch(console.error);
    }, [notification]);

    // Auto-dismiss after timeout (disabled for dev purposes)
    /*
    useEffect(() => {
        if (!notification) return;

        const timer = setTimeout(dismissNotification, AUTO_DISMISS_MS);
        return () => clearTimeout(timer);
    }, [notification, dismissNotification]);
    */

    return (
        <AnimatePresence mode="wait">
            {notification && (
                <motion.div
                    key={notification.id}
                    className="dynamic-island-alert"
                    onClick={dismissNotification}
                    // Slide down from behind the notch - grow out effect
                    initial={{
                        opacity: 0,
                        scale: 0.8,
                        y: -30,
                    }}
                    animate={{
                        opacity: 1,
                        scale: 1,
                        y: 30, // Move down to be visible below the notch
                    }}
                    exit={{
                        opacity: 0,
                        scale: 0.85,
                        y: -20,
                    }}
                    transition={{
                        type: 'spring',
                        stiffness: 400,
                        damping: 30,
                        mass: 0.8,
                    }}
                    // Subtle hover interaction
                    whileHover={{ scale: 1.02 }}
                    whileTap={{ scale: 0.98 }}
                >
                    <span className="dynamic-island-alert__icon">
                        {notification.icon}
                    </span>
                    <span className="dynamic-island-alert__message">
                        {notification.message}
                    </span>
                </motion.div>
            )}
        </AnimatePresence>
    );
}
