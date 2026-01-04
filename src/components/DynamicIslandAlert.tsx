import { motion, AnimatePresence } from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useState } from 'react';
import { useNotification } from '../context/NotificationContext';
import { activateWindow, setClickThrough, useNotchInfo } from '../hooks/useNotchInfo';
import './DynamicIslandAlert.css';

export function DynamicIslandAlert() {
    const { notification, dismissNotification } = useNotification();
    const { notchInfo } = useNotchInfo();
    const [isHovered, setIsHovered] = useState(false);

    // Listen for mouse enter/exit events from Rust backend
    useEffect(() => {
        const unlistenEnter = listen('mouse-entered-notch', () => {
            // Disable click-through so we can receive hover events
            setClickThrough(false).catch(console.error);
            activateWindow().catch(console.error);
        });

        const unlistenExit = listen('mouse-exited-notch', () => {
            // Re-enable click-through when mouse leaves
            setClickThrough(true).catch(console.error);
            setIsHovered(false);
        });

        return () => {
            unlistenEnter.then(fn => fn());
            unlistenExit.then(fn => fn());
        };
    }, []);

    // Toggle click-through based on notification state (still useful for when notification appears/disappears)
    useEffect(() => {
        if (!notification) {
            setClickThrough(true).catch(console.error);
        }
    }, [notification]);

    const notchHeight = notchInfo?.notch_height ? notchInfo.notch_height - 20 : 38;
    const notchWidth = notchInfo?.notch_width ? notchInfo.notch_width - 40 : 200;

    return (
        <AnimatePresence mode="wait">
            {notification && (
                <motion.div
                    key={notification.id}
                    className="dynamic-island-alert"
                    onClick={dismissNotification}
                    onHoverStart={() => {
                        setIsHovered(true);
                        activateWindow();
                        invoke('trigger_haptics').catch(console.error);
                    }}
                    onHoverEnd={() => setIsHovered(false)}
                    initial={{
                        height: notchHeight,
                        width: notchWidth,
                        opacity: 0,
                    }}
                    animate={{
                        height: isHovered ? notchHeight + 40 : notchHeight,
                        width: isHovered ? notchWidth + 40 : notchWidth,
                        opacity: 1,
                    }}
                    exit={{
                        height: notchHeight,
                        width: notchWidth,
                        opacity: 1,
                        transition: { duration: 0.2 }
                    }}
                    transition={{
                        type: 'spring',
                        stiffness: 400,
                        damping: 30,
                        mass: 0.8,
                    }}
                >
                    <div
                        className="dynamic-island-alert__content"
                        style={{
                            paddingTop: notchHeight + 5,
                            opacity: isHovered ? 1 : 0,
                            transform: `translateY(${isHovered ? 0 : -10}px)`,
                            transition: 'all 0.3s ease',
                        }}
                    >
                        <span className="dynamic-island-alert__icon">
                            {notification.icon}
                        </span>
                        <span className="dynamic-island-alert__message">
                            {notification.message}
                        </span>
                    </div>
                </motion.div>
            )}
        </AnimatePresence>
    );
}
