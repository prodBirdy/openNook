import { motion, AnimatePresence } from 'motion/react';
import { useEffect, useState } from 'react';
import { useNotification } from '../context/NotificationContext';
import { setClickThrough, useNotchInfo } from '../hooks/useNotchInfo';
import './DynamicIslandAlert.css';

export function DynamicIslandAlert() {
    const { notification, dismissNotification } = useNotification();
    const { notchInfo, activateWindow } = useNotchInfo();
    const [isHovered, setIsHovered] = useState(false);

    // Toggle click-through based on notification state
    useEffect(() => {
        setClickThrough(!notification).catch(console.error);
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
                            paddingTop: notchHeight,
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
