import { motion } from 'motion/react';

interface ModeIndicatorProps {
    availableModes: ('media' | 'files' | 'onboard' | 'idle')[];
    currentMode: 'media' | 'files' | 'onboard' | 'idle';
}

export function ModeIndicator({ availableModes, currentMode }: ModeIndicatorProps) {
    if (availableModes.length <= 1) return null;

    return (
        <div
            style={{
                position: 'absolute',
                bottom: '4px',
                left: '50%',
                transform: 'translateX(-50%)',
                display: 'flex',
                gap: '4px',
                alignItems: 'center',
                pointerEvents: 'none',
            }}
        >
            {availableModes.map((mode) => {
                const isActive = mode === currentMode;
                return (
                    <motion.div
                        key={mode}
                        style={{
                            width: isActive ? '5px' : '4px',
                            height: isActive ? '5px' : '4px',
                            borderRadius: '50%',
                            backgroundColor: isActive
                                ? 'rgba(255, 255, 255, 0.9)'
                                : 'rgba(255, 255, 255, 0.3)',
                        }}
                        animate={{
                            scale: isActive ? 1 : 0.8,
                        }}
                        transition={{
                            type: 'spring',
                            stiffness: 300,
                            damping: 20,
                        }}
                    />
                );
            })}
        </div>
    );
}
