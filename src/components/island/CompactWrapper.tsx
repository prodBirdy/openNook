import { motion } from 'motion/react';
import { ReactNode } from 'react';

export interface CompactWrapperProps {
    left?: ReactNode;
    right?: ReactNode;
    baseNotchWidth: number;
    isHovered: boolean;
    contentOpacity: number;
    className?: string;
    id?: string;
}

export function CompactWrapper({
    left,
    right,
    baseNotchWidth,
    isHovered,
    contentOpacity,
    className = "island-content",
    id
}: CompactWrapperProps) {
    return (
        <motion.div
            key={id}
            className={className}
            initial={{ opacity: 0 }}
            animate={{ opacity: contentOpacity }}
            transition={{ duration: 0.3 }}
            style={{
                pointerEvents: isHovered ? 'auto' : 'none',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                padding: '0 12px',
            }}
        >
            <div className="media-left" style={{ flex: 1, minWidth: 0, height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'flex-start' }}>
                {left}
            </div>

            <div className="media-spacer" style={{ width: baseNotchWidth }} />

            <div className="media-right" style={{ flex: 1, minWidth: 0, height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'flex-end' }}>
                {right}
            </div>
        </motion.div>
    );
}
