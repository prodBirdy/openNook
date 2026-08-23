import { motion } from 'motion/react';
import { ReactNode } from 'react';
import { cn } from '../../lib/utils';

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
    className = "w-full h-full flex items-center overflow-visible",
    id
}: CompactWrapperProps) {
    return (
        <motion.div
            key={id}
            className={cn(
                className,
                "flex items-center justify-between px-3",
                isHovered ? "pointer-events-auto" : "pointer-events-none"
            )}
            initial={{ opacity: 0 }}
            animate={{ opacity: contentOpacity }}
            transition={{ duration: 0.3 }}
        >
            <div className="flex-1 min-w-0 h-full flex items-center justify-start">
                {left}
            </div>

            <div className="shrink-0 text-[var(--accent-color)] text-sm" style={{ width: baseNotchWidth }} />

            <div className="flex-1 min-w-0 h-full flex items-center justify-end">
                {right}
            </div>
        </motion.div>
    );
}
