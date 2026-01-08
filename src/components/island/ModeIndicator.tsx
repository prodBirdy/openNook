import { motion } from 'motion/react';
import { cn } from '@/lib/utils';

interface ModeIndicatorProps {
    availableModes: string[];
    currentMode: string;
    onModeChange: (mode: string) => void;
}

export function ModeIndicator({ availableModes, currentMode, onModeChange }: ModeIndicatorProps) {
    if (availableModes.length <= 1) return null;

    return (
        <div className="absolute bottom-1 left-1/2 gap-1 -translate-x-1/2 flex items-center">
            {availableModes.map((mode) => {
                const isActive = mode === currentMode;
                return (
                    <button
                        key={mode}
                        onClick={(e) => {
                            e.stopPropagation();
                            onModeChange(mode);
                        }}
                        className=" cursor-pointer group outline-none"
                    >
                        <motion.div
                            className={cn(
                                "rounded-full duration-200 ease-[cubic-bezier(0.25,0.1,0.25,1)]",
                                isActive ? "w-[8px] h-[4px] bg-white/90" : "w-[4px] h-[4px] bg-white/30 group-hover:bg-white/50"
                            )}
                            animate={{
                                width: isActive ? 8 : 4,
                            }}
                            transition={{
                                type: 'spring',
                                stiffness: 300,
                                damping: 20,
                            }}
                        />
                    </button>
                );
            })}
        </div>
    );
}
