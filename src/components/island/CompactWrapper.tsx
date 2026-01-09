import { motion } from 'motion/react';
import { ReactNode } from 'react';

export interface CompactWrapperProps {
    left?: ReactNode;
    right?: ReactNode;
    baseNotchWidth: number;
    isHovered: boolean;
    contentOpacity?: number;
    className?: string;
    id?: string;
}

export function CompactWrapper({
    left,
    right,
    baseNotchWidth,
    isHovered,
    contentOpacity = 1,
    className = "island-content",
    id
}: CompactWrapperProps) {
    return (
        <motion.div
            key={id}
            className={`${className} flex items-center justify-between`}
            initial={{ opacity: 0 }}
            animate={{ opacity: contentOpacity }}
            transition={{ duration: 0.3 }}
            style={{
                pointerEvents: isHovered ? 'auto' : 'none',
                padding: '0 12px',
            }}
        >
            <div className=" flex-1 min-w-0 h-full flex items-center justify-start">
                {left}
            </div>

            <div className="media-spacer text-accent text-sm" style={{ width: baseNotchWidth }} >

            </div>

            <div className=" flex-1 min-w-0 h-full flex items-center justify-end">
                {right}
            </div>
        </motion.div>
    );
}
