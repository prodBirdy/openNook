import { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { IconPlayerPlay, IconPlayerStop, IconPlus, IconTrash, IconActivity, IconBriefcase, IconCode, IconBook, IconMoon, IconCpu, IconBulb, IconHeadphones, IconCoffee, IconDeviceLaptop, IconWriting } from '@tabler/icons-react';
import { z } from 'zod';
import { registerWidget } from './WidgetRegistry';
import { CompactWidgetProps } from './WidgetTypes';
import { CompactWrapper } from '../island/CompactWrapper';
import { useSessionContext } from '../../context/SessionContext';
import { WidgetWrapper } from './WidgetWrapper';
import { WidgetAddDialog } from './WidgetAddDialog';
import { cn } from '@/lib/utils';
import { Separator } from '../ui/separator';
import { Button } from '../ui/button';

const AVAILABLE_ICONS = [
    { name: 'activity', Icon: IconActivity, label: 'Default' },
    { name: 'code', Icon: IconCode, label: 'Code' },
    { name: 'briefcase', Icon: IconBriefcase, label: 'Work' },
    { name: 'book', Icon: IconBook, label: 'Study' },
    { name: 'writing', Icon: IconWriting, label: 'Write' },
    { name: 'moon', Icon: IconMoon, label: 'Rest' },
    { name: 'bulb', Icon: IconBulb, label: 'Focus' },
    { name: 'cpu', Icon: IconCpu, label: 'Deep' },
    { name: 'headphones', Icon: IconHeadphones, label: 'Listen' },
    { name: 'coffee', Icon: IconCoffee, label: 'Break' },
    { name: 'laptop', Icon: IconDeviceLaptop, label: 'Meeting' },
];

// Zod schema for session form
const sessionFormSchema = z.object({
    name: z.string().min(1, "Session name is required"),
    icon: z.string().default('activity'),
});

type SessionFormValues = z.infer<typeof sessionFormSchema>;

function formatElapsed(seconds: number, compact = false): string {
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

function getSessionIcon(session: { name: string, icon?: string }) {
    if (session.icon) {
        const found = AVAILABLE_ICONS.find(i => i.name === session.icon);
        if (found) return found.Icon;
    }

    // Fallback to heuristic
    const n = session.name.toLowerCase();
    if (n.includes('code') || n.includes('dev') || n.includes('program')) return IconCode;
    if (n.includes('work') || n.includes('job') || n.includes('busines')) return IconBriefcase;
    if (n.includes('read') || n.includes('book') || n.includes('study')) return IconBook;
    if (n.includes('sleep') || n.includes('rest') || n.includes('nap')) return IconMoon;
    return IconActivity; // Default
}

export function SessionWidget() {
    const { sessions, startSession, stopSession, resumeSession, removeSession, getElapsedTime } = useSessionContext();
    const [showAddDialog, setShowAddDialog] = useState(false);
    const [selectedIcon, setSelectedIcon] = useState('activity');

    const activeSessions = sessions.filter(s => s.isActive);
    const completedSessions = sessions.filter(s => !s.isActive).slice(0, 5);

    const handleStartSession = (data: SessionFormValues) => {
        startSession(data.name, selectedIcon);
        setSelectedIcon('activity');
    };

    return (
        <WidgetWrapper
            title="Sessions"
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
                onOpenChange={(open) => {
                    setShowAddDialog(open);
                    if (!open) setSelectedIcon('activity');
                }}
                title="New Session"
                schema={sessionFormSchema}
                defaultValues={{ name: '', icon: 'activity' }}
                onSubmit={handleStartSession}
                fields={[
                    {
                        name: 'name',
                        label: 'Session Name',
                        placeholder: 'e.g., Deep Work',
                        icon: (() => {
                            const SelectedIcon = AVAILABLE_ICONS.find(i => i.name === selectedIcon)?.Icon || IconActivity;
                            return <SelectedIcon size={18} className="text-primary" />;
                        })(),
                        autoFocus: true,
                        required: true,
                    },
                ]}
                submitLabel="Start Session"
                submitIcon={<IconPlayerPlay size={16} />}
            >
                {() => (
                    <div className="space-y-2">
                        <label className="text-sm font-medium text-foreground">Icon</label>
                        <div className="flex gap-2 flex-wrap">
                            {AVAILABLE_ICONS.map(({ name, Icon }) => (
                                <div
                                    key={name}
                                    className={cn(
                                        "w-8 h-8 rounded-full flex items-center justify-center cursor-pointer transition-all",
                                        "border ",
                                        selectedIcon === name
                                            ? "bg-primary border-primary-foreground"
                                            : "bg-primary/20 border-transparent"
                                    )}
                                    onClick={() => setSelectedIcon(name)}
                                >
                                    <Icon size={16} className={selectedIcon === name ? "text-primary-foreground" : "text-primary"} />
                                </div>
                            ))}
                        </div>
                    </div>
                )}
            </WidgetAddDialog>

            {activeSessions.length === 0 && completedSessions.length === 0 && !showAddDialog ? (
                <div className="flex-1 flex flex-col items-center justify-center gap-3 text-white/30 text-sm">
                    <span>No sessions</span>
                    <button className="bg-white/10 border-none px-4 py-2 rounded-[20px] text-white text-[13px] font-medium cursor-pointer transition-colors duration-200 hover:bg-white/20" onClick={() => setShowAddDialog(true)}>Start Session</button>
                </div>
            ) : (
                <div className="flex flex-col gap-1 flex-1 overflow-y-auto min-h-0 pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                    <AnimatePresence mode="popLayout" initial={false}>
                        {/* Active Sessions */}
                        {activeSessions.map(session => (
                            <motion.div
                                layout
                                initial={{ opacity: 0, scale: 0.9, y: 20 }}
                                animate={{ opacity: 1, scale: 1, y: 0 }}
                                exit={{ opacity: 0, scale: 0.9, y: -20 }}
                                transition={{ type: "spring", bounce: 0.3, duration: 0.4 }}
                                className="group flex items-center gap-2 px-4 py-3 rounded-[20px] bg-[var(--accent-color,#0A84FF)]/5 border border-[var(--accent-color,#0A84FF)]/10 transition-colors duration-200 cursor-default hover:bg-[var(--accent-color,#0A84FF)]/10"
                                key={session.id}
                            >
                                <div className="w-11 h-11 flex items-center justify-center shrink-0 bg-[var(--accent-color,#0A84FF)]/20 rounded-full text-[var(--accent-color,#0A84FF)]">
                                    {(() => {
                                        const Icon = getSessionIcon(session);
                                        return <Icon size={24} />;
                                    })()}
                                </div>
                                <div className='flex-1 flex flex-col justify-center'>
                                    <div className='text-[26px] font-medium text-white/95 tabular-nums tracking-[-0.5px] leading-none'>{formatElapsed(getElapsedTime(session))}</div>
                                    <div className='text-[13px] text-white/40 mt-0.5 font-medium' >{session.name || 'Focus Session'}</div>
                                </div>

                                <div className="flex items-center gap-1 opacity-0 translate-x-[10px] transition-all duration-200 ease-[cubic-bezier(0.25,0.1,0.25,1)] group-hover:opacity-100 group-hover:translate-x-0">
                                    <Button
                                        variant="ghost"
                                        className="w-8 h-8 rounded-full border-none bg-white/10 text-white flex items-center justify-center cursor-pointer transition-all duration-200 hover:bg-red-500/20 hover:text-[#FF453A] hover:scale-110 p-0"
                                        onClick={(e) => { e.stopPropagation(); stopSession(session.id); }}
                                        title="Stop Session"
                                    >
                                        <IconPlayerStop size={18} />
                                    </Button>
                                </div>
                            </motion.div>
                        ))}

                        {activeSessions.length > 0 && completedSessions.length > 0 && (
                            <motion.div layout key="separator">
                                <Separator className="my-3 bg-white/10" />
                            </motion.div>
                        )}

                        {/* Completed Sessions */}
                        {completedSessions.map(session => (
                            <motion.div
                                layout
                                initial={{ opacity: 0, scale: 0.9, y: 20 }}
                                animate={{ opacity: 1, scale: 1, y: 0 }}
                                exit={{ opacity: 0, scale: 0.9, y: -20 }}
                                transition={{ type: "spring", bounce: 0.3, duration: 0.4 }}
                                className="group flex items-center gap-2 px-4 py-3 rounded-[20px] bg-transparent border border-transparent transition-colors duration-200 cursor-default hover:bg-white/5"
                                key={session.id}
                                onClick={(e) => { e.stopPropagation(); resumeSession(session.id); }}
                                style={{ cursor: 'pointer' }}
                            >
                                <div className="w-11 h-11 flex items-center justify-center shrink-0 bg-white/5 rounded-full text-white/60">
                                    {(() => {
                                        const Icon = getSessionIcon(session);
                                        return <Icon size={24} />;
                                    })()}
                                </div>
                                <div className="flex-1 flex flex-col justify-center">
                                    <div className="text-[26px] font-medium text-white/70 tabular-nums tracking-[-0.5px] leading-none">{formatElapsed(getElapsedTime(session))}</div>
                                    <div className="text-[13px] text-white/30 mt-0.5 font-medium">{session.name || 'Focus Session'}</div>
                                </div>

                                <div className="flex items-center gap-1 opacity-0 translate-x-[10px] transition-all duration-200 ease-[cubic-bezier(0.25,0.1,0.25,1)] group-hover:opacity-100 group-hover:translate-x-0">
                                    <Button
                                        variant="ghost"
                                        className="w-8 h-8 rounded-full border-none bg-white/10 text-white flex items-center justify-center cursor-pointer transition-all duration-200 hover:bg-white/20 hover:scale-110 p-0"
                                        onClick={(e) => { e.stopPropagation(); resumeSession(session.id); }}
                                        title="Resume Session"
                                    >
                                        <IconPlayerPlay size={18} />
                                    </Button>
                                    <Button
                                        variant="ghost"
                                        className="w-8 h-8 rounded-full border-none bg-white/10 text-white flex items-center justify-center cursor-pointer transition-all duration-200 hover:bg-red-500/20 hover:text-[#FF453A] hover:scale-110 p-0"
                                        onClick={(e) => { e.stopPropagation(); removeSession(session.id); }}
                                        title="Delete Entry"
                                    >
                                        <IconTrash size={18} />
                                    </Button>
                                </div>
                            </motion.div>
                        ))}
                    </AnimatePresence>
                </div>
            )}
        </WidgetWrapper>
    );
}

// Compact Session Component
export function CompactSession({ baseNotchWidth, isHovered, contentOpacity }: CompactWidgetProps) {
    const { sessions, stopSession, getElapsedTime } = useSessionContext();
    const [, forceUpdate] = useState(0);

    // Force re-render every second for elapsed time
    useEffect(() => {
        const hasActive = sessions.some(s => s.isActive);
        if (!hasActive) return;
        const interval = setInterval(() => forceUpdate(n => n + 1), 1000);
        return () => clearInterval(interval);
    }, [sessions]);

    const activeSession = sessions.find(s => s.isActive);

    if (!activeSession) {
        return (
            <CompactWrapper
                id="session-compact"
                baseNotchWidth={baseNotchWidth}
                isHovered={isHovered}
                contentOpacity={contentOpacity}
                left={
                    <div className="flex items-center gap-2">
                        <IconActivity size={20} color="white" />
                    </div>
                }
            />
        );
    }

    return (
        <CompactWrapper
            id="session-compact"
            baseNotchWidth={baseNotchWidth}
            isHovered={isHovered}
            contentOpacity={contentOpacity}
            left={
                <div
                    onClick={(e) => { e.stopPropagation(); stopSession(activeSession.id); }}
                    className="flex items-center justify-center w-[26px] h-[26px] rounded-full cursor-pointer duration-200 ease-[cubic-bezier(0.25,0.1,0.25,1)] "
                >
                    <div className="flex items-center justify-center text-white">
                        {(() => {
                            const Icon = getSessionIcon(activeSession);
                            return <Icon size={14} />;
                        })()}
                    </div>
                </div>
            }
            right={
                <div className="flex items-center gap-2">
                    <span className="text-white text-sm tabular-nums">
                        {formatElapsed(getElapsedTime(activeSession), true)}
                    </span>
                </div>
            }
        />
    );
}

// Register the session widget
registerWidget({
    id: 'session',
    name: 'Sessions',
    description: 'Track work sessions',
    icon: IconActivity,
    ExpandedComponent: SessionWidget,
    CompactComponent: CompactSession,
    defaultEnabled: false,
    category: 'productivity',
    minWidth: 260,
    hasCompactMode: true,
    compactPriority: 15
});
