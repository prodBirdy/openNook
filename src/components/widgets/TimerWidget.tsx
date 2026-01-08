import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { IconPlayerPlay, IconPlayerPause, IconRefresh, IconPlus, IconTrash, IconClock } from '@tabler/icons-react';
import { z } from 'zod';
import { registerWidget } from './WidgetRegistry';
import { CompactWidgetProps } from './WidgetTypes';
import { CompactWrapper } from '../island/CompactWrapper';
import { useTimerContext } from '../../context/TimerContext';
import { WidgetWrapper } from './WidgetWrapper';
import { WidgetAddDialog } from './WidgetAddDialog';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

const PRESETS = [
    { label: '5m', seconds: 5 * 60 },
    { label: '15m', seconds: 15 * 60 },
    { label: '25m', seconds: 25 * 60 },
    { label: '1h', seconds: 60 * 60 },
];

// Zod schema for timer form
const timerFormSchema = z.object({
    name: z.string().optional(),
    minutes: z.coerce.number().min(1, "Duration must be at least 1 minute"),
});

type TimerFormValues = z.infer<typeof timerFormSchema>;

function formatTime(seconds: number, compact = false): string {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;

    if (compact) {
        if (h > 0) {
            return m > 0 ? `${h}h${m.toString().padStart(2, '0')}` : `${h}h`;
        }
        if (m > 0) {
            return `${m}m${s.toString().padStart(2, '0')}`;
        }
        return `${s}s`;
    }

    if (h > 0) {
        return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
    }
    return `${m}:${s.toString().padStart(2, '0')}`;
}

export function TimerWidget() {
    const { timers, addTimer, removeTimer, toggleTimer, resetTimer } = useTimerContext();
    const [showAddDialog, setShowAddDialog] = useState(false);

    const handleAddTimer = (data: TimerFormValues) => {
        addTimer(data.name || '', data.minutes * 60);
    };

    const handlePresetClick = (seconds: number) => {
        addTimer('', seconds);
        setShowAddDialog(false);
    };

    return (
        <WidgetWrapper
            title="Timers"
            className="flex flex-col p-5 h-full box-border overflow-hidden"
            headerActions={[
                !showAddDialog && (
                    <button
                        key="add"
                        className="bg-transparent border-none text-white/40 cursor-pointer p-1.5 rounded-full flex items-center justify-center transition-all duration-200 hover:bg-white/10 hover:text-white"
                        onClick={(e) => { e.stopPropagation(); setShowAddDialog(true); }}
                    >
                        <IconPlus size={18} />
                    </button>
                )
            ].filter(Boolean)}
        >
            <WidgetAddDialog
                open={showAddDialog}
                onOpenChange={setShowAddDialog}
                title="New Timer"
                schema={timerFormSchema}
                defaultValues={{ name: '', minutes: 25 }}
                onSubmit={handleAddTimer}
                fields={[
                    {
                        name: 'name',
                        label: 'Label',
                        placeholder: 'Timer Label (optional)',
                        icon: <IconClock size={18} className="text-primary" />,
                        autoFocus: true,
                    },
                    {
                        name: 'minutes',
                        label: 'Duration (minutes)',
                        placeholder: 'Minutes',
                        type: 'number',
                        required: true,
                    },
                ]}
                submitLabel="Start Timer"
                submitIcon={<IconPlayerPlay size={16} />}
            >
                {() => (
                    <div className="flex gap-2 flex-wrap">
                        {PRESETS.map(preset => (
                            <Button
                                key={preset.label}
                                type="button"
                                variant="secondary"
                                size="sm"
                                onClick={() => handlePresetClick(preset.seconds)}
                            >
                                {preset.label}
                            </Button>
                        ))}
                    </div>
                )}
            </WidgetAddDialog>

            {timers.length === 0 ? (
                <div className="flex-1 flex flex-col items-center justify-center gap-3 text-white/30 text-sm">
                    <span>No active timers</span>
                    <button className="bg-white/10 border-none px-4 py-2 rounded-[20px] text-white text-[13px] font-medium cursor-pointer transition-colors duration-200 hover:bg-white/20" onClick={() => setShowAddDialog(true)}>Create Timer</button>
                </div>
            ) : (
                <div className="flex flex-col gap-1 flex-1 overflow-y-auto min-h-0 pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                    <AnimatePresence mode="popLayout" initial={false}>
                        {timers.map(timer => {
                            const progress = timer.duration > 0
                                ? ((timer.duration - timer.remaining) / timer.duration) * 100
                                : 0;

                            return (
                                <motion.div
                                    layout
                                    initial={{ opacity: 0, scale: 0.9, y: 20 }}
                                    animate={{ opacity: 1, scale: 1, y: 0 }}
                                    exit={{ opacity: 0, scale: 0.9, y: -20 }}
                                    transition={{ type: "spring", bounce: 0.3, duration: 0.4 }}
                                    className={cn(
                                        "group flex items-center gap-2 px-4 py-3 rounded-[20px] transition-colors duration-200 cursor-default border border-transparent",
                                        timer.remaining === 0 ? 'bg-red-500/10 border-red-500/20 hover:bg-red-500/20' : 'bg-transparent hover:bg-white/5'
                                    )}
                                    key={timer.id}
                                >
                                    <div
                                        className="relative w-11 h-11 shrink-0 cursor-pointer rounded-full transition-transform active:scale-95"
                                        onClick={(e) => { e.stopPropagation(); toggleTimer(timer.id); }}
                                    >
                                        <svg viewBox="0 0 44 44" className="w-full h-full -rotate-90">
                                            <circle
                                                className="stroke-white/10"
                                                cx="22" cy="22" r="20"
                                                fill="none"
                                                strokeWidth="3"
                                            />
                                            <motion.circle
                                                className={cn("transition-[stroke] duration-300", timer.remaining === 0 ? "stroke-[#FF453A]" : "stroke-[var(--accent-color,#0A84FF)]")}
                                                cx="22" cy="22" r="20"
                                                fill="none"
                                                strokeWidth="3"
                                                strokeDasharray={2 * Math.PI * 20}
                                                animate={{ strokeDashoffset: (2 * Math.PI * 20) * (1 - progress / 100) }}
                                                transition={{ duration: 1, ease: "linear" }}
                                                strokeLinecap="round"
                                            />
                                        </svg>
                                        <div className="absolute inset-0 flex items-center justify-center text-white">
                                            {timer.isRunning ? (
                                                <IconPlayerPause size={16} fill="currentColor" className="opacity-0 group-hover:opacity-100 transition-opacity" />
                                            ) : (
                                                <IconPlayerPlay size={16} fill="currentColor" className={cn("ml-0.5 transition-opacity", timer.remaining === 0 ? "opacity-0" : "opacity-50 group-hover:opacity-100")} />
                                            )}
                                        </div>
                                    </div>

                                    <div className="flex-1 flex flex-col justify-center">
                                        <div className="text-[26px] font-medium text-white/95 tabular-nums tracking-[-0.5px] leading-none">{formatTime(timer.remaining)}</div>
                                        {timer.name && <div className="text-[13px] text-white/40 mt-0.5 font-medium">{timer.name}</div>}
                                    </div>

                                    <div className="flex items-center gap-1 opacity-0 translate-x-[10px] transition-all duration-200 ease-[cubic-bezier(0.25,0.1,0.25,1)] group-hover:opacity-100 group-hover:translate-x-0">
                                        <Button
                                            variant="ghost"
                                            className="w-8 h-8 rounded-full border-none bg-white/10 text-white flex items-center justify-center cursor-pointer transition-all duration-200 hover:bg-white/20 hover:scale-110 p-0"
                                            onClick={(e) => { e.stopPropagation(); resetTimer(timer.id); }}
                                            title="Reset"
                                        >
                                            <IconRefresh size={18} />
                                        </Button>
                                        <Button
                                            variant="ghost"
                                            className="w-8 h-8 rounded-full border-none bg-white/10 text-white flex items-center justify-center cursor-pointer transition-all duration-200 hover:bg-red-500/20 hover:text-[#FF453A] hover:scale-110 p-0"
                                            onClick={(e) => { e.stopPropagation(); removeTimer(timer.id); }}
                                            title="Delete"
                                        >
                                            <IconTrash size={18} />
                                        </Button>
                                    </div>
                                </motion.div>
                            );
                        })}
                    </AnimatePresence>
                </div>
            )}
        </WidgetWrapper>
    );
}

// Compact Timer Component
export function CompactTimer({ baseNotchWidth, isHovered, contentOpacity }: CompactWidgetProps) {
    const { timers, toggleTimer } = useTimerContext();

    // Prioritize showing a completed timer so the user sees it immediately
    const completedTimer = timers.find(t => t.remaining === 0 && t.duration > 0);
    const activeTimer = completedTimer || timers.find(t => t.isRunning) || timers[0];

    if (!activeTimer) {
        return (
            <CompactWrapper
                id="timer-compact"
                baseNotchWidth={baseNotchWidth}
                isHovered={isHovered}
                contentOpacity={contentOpacity}
                left={
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <IconClock size={20} color="white" />
                        <span style={{ color: 'white', fontSize: 14 }}>No timers</span>
                    </div>
                }
            />
        );
    }

    const calcProgress = activeTimer.duration > 0
        ? ((activeTimer.duration - activeTimer.remaining) / activeTimer.duration) * 100
        : 0;
    const progress = Math.min(100, Math.max(0, Number.isFinite(calcProgress) ? calcProgress : 0));

    const radius = 5;
    const circumference = 2 * Math.PI * radius;
    const strokeLength = (progress / 100) * circumference;

    return (
        <CompactWrapper
            id="timer-compact"
            baseNotchWidth={baseNotchWidth}
            isHovered={isHovered}
            contentOpacity={contentOpacity}
            left={
                <div
                    style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }}
                    onClick={(e) => { e.stopPropagation(); toggleTimer(activeTimer.id); }}
                >

                    <div className="timer-ring-container" style={{ width: 24, height: 24 }}>
                        <svg viewBox="0 0 12 12" className="ring-svg">
                            <circle
                                className="ring-bg"
                                cx="6" cy="6" r="5"
                                fill="none"
                                strokeWidth="2"
                            />
                            <circle
                                className="ring-progress"
                                cx="6" cy="6" r="5"
                                fill="none"
                                strokeWidth="2"
                                strokeDasharray={`${strokeLength} ${circumference}`}
                                strokeLinecap="round"
                            />
                        </svg>
                    </div>
                </div>
            }
            right={
                <span style={{ color: 'white', fontSize: 14, fontVariantNumeric: 'tabular-nums' }}>
                    {formatTime(activeTimer.remaining, true)}
                </span>
            }
        />
    );
}

// Register the timer widget
registerWidget({
    id: 'timer',
    name: 'Timer',
    description: 'Countdown timer with presets',
    icon: IconClock,
    ExpandedComponent: TimerWidget,
    CompactComponent: CompactTimer,
    defaultEnabled: false,
    category: 'productivity',
    minWidth: 260,
    hasCompactMode: true,
    compactPriority: 10
});
