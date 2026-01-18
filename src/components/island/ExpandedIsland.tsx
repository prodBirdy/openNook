import { motion, AnimatePresence } from 'motion/react';
import { IconSettings, IconLayoutGrid, IconFiles } from '@tabler/icons-react';
import { ExpandedMedia } from './ExpandedMedia';
import { FileTray } from '../FileTray';
import { useWidgetStore } from '../../stores/useWidgetStore';
import { PluginErrorBoundary } from '../widgets/PluginErrorBoundary';
import { useMemo, useRef, useState } from 'react';
import { PopoverProvider } from '../../context/PopoverContext';
import { cn } from '../../lib/utils';

interface ExpandedIslandProps {
    activeTab: 'widgets' | 'files';
    setActiveTab: (tab: 'widgets' | 'files') => void;
    notchHeight: number;
    baseNotchWidth: number;
    settings: {
        showMedia: boolean;
        showCalendar: boolean;
        showReminders: boolean;
        liquidGlassMode: boolean;
    };
    handleSettingsClick: () => void;
    handleChildWheel: (e: React.WheelEvent) => void;
    setIsPopoverOpen: (open: boolean) => void;
}

export function ExpandedIsland({
    activeTab,
    setActiveTab,
    notchHeight,
    baseNotchWidth,
    settings,
    handleSettingsClick,
    handleChildWheel,
    setIsPopoverOpen
}: ExpandedIslandProps) {
    const widgets = useWidgetStore(state => state.widgets);
    const widgetEnabledState = useWidgetStore(state => state.enabledState);

    // Drag to scroll logic
    const widgetsContainerRef = useRef<HTMLDivElement>(null);
    const [isDragging, setIsDragging] = useState(false);
    const [startX, setStartX] = useState(0);
    const [scrollLeft, setScrollLeft] = useState(0);

    const handleMouseDown = (e: React.MouseEvent) => {
        if (!widgetsContainerRef.current) return;
        setIsDragging(true);
        setStartX(e.pageX - widgetsContainerRef.current.offsetLeft);
        setScrollLeft(widgetsContainerRef.current.scrollLeft);
    };

    const handleMouseLeave = () => {
        setIsDragging(false);
    };

    const handleMouseUp = () => {
        setIsDragging(false);
    };

    const handleMouseMove = (e: React.MouseEvent) => {
        if (!isDragging || !widgetsContainerRef.current) return;
        e.preventDefault();
        const x = e.pageX - widgetsContainerRef.current.offsetLeft;
        const walk = (x - startX) * 1.5; // Scroll-fast
        widgetsContainerRef.current.scrollLeft = scrollLeft - walk;
    };

    // Compute enabled widgets with memoization to avoid infinite loops
    const enabledWidgets = useMemo(() =>
        widgets.filter(w => widgetEnabledState[w.id]),
        [widgets, widgetEnabledState]
    );

    return (
        <motion.div
            key="expanded-content"
            className="w-full h-full max-h-[250px] flex flex-col items-center overflow-visible text-white box-border"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.3 }}
        >
            {/* Top Menu Bar */}
            <div className="w-full flex items-center justify-between px-5 box-border z-20" style={{ height: notchHeight }}>
                <div className="flex items-center h-full">
                    <div className="relative flex bg-white/10 rounded-[20px] p-[2px] h-8 w-20">
                        <div
                            className="absolute top-[2px] left-[2px] w-[calc(50%-2px)] h-[calc(100%-4px)] bg-white/20 rounded-[18px] transition-transform duration-300 ease-[cubic-bezier(0.4,0.0,0.2,1)]"
                            style={{
                                transform: `translateX(${activeTab === 'widgets' ? '0%' : '100%'})`
                            }}
                        />
                        <div
                            className={cn(
                                "flex-1 flex items-center justify-center z-10 cursor-pointer text-white/50 transition-colors duration-200",
                                activeTab === 'widgets' && "text-white"
                            )}
                            onClick={(e) => { e.stopPropagation(); setActiveTab('widgets'); }}
                        >
                            <IconLayoutGrid size={16} />
                        </div>
                        <div
                            className={cn(
                                "flex-1 flex items-center justify-center z-10 cursor-pointer text-white/50 transition-colors duration-200",
                                activeTab === 'files' && "text-white"
                            )}
                            onClick={(e) => { e.stopPropagation(); setActiveTab('files'); }}
                        >
                            <IconFiles size={16} />
                        </div>
                    </div>
                </div>
                <div className="shrink-0" style={{ width: baseNotchWidth, height: '100%' }} />
                <div style={{ flex: 1, display: 'flex', justifyContent: 'flex-end', alignItems: 'center' }}>
                    <div
                        className="flex items-center justify-center rounded-full cursor-pointer bg-white/10 transition-all duration-200 text-white/80 backdrop-blur-[10px] hover:bg-white/20 hover:text-white hover:scale-105 active:scale-95"
                        style={{ height: notchHeight - 4, width: notchHeight - 4 }}
                        onClick={(e) => {
                            e.stopPropagation();
                            handleSettingsClick();
                        }}
                    >
                        <IconSettings size={20} color="white" stroke={1.5} />
                    </div>
                </div>
            </div>

            {/* Main Content Area */}
            <div className="flex-1 w-full overflow-hidden relative flex flex-col">
                <AnimatePresence mode="wait">
                    {activeTab === 'widgets' ? (
                        <motion.div
                            key="widgets"
                            className="flex-1 flex flex-row gap-4 w-full p-5 box-border overflow-x-auto items-stretch [scrollbar-width:none] [-ms-overflow-style:none] [&::-webkit-scrollbar]:hidden"
                            ref={widgetsContainerRef}
                            initial={{ opacity: 0, x: -20 }}
                            animate={{ opacity: 1, x: 0 }}
                            exit={{ opacity: 0, x: -20 }}
                            transition={{ duration: 0.3 }}
                            onWheel={handleChildWheel}
                            onMouseDown={handleMouseDown}
                            onMouseLeave={handleMouseLeave}
                            onMouseUp={handleMouseUp}
                            onMouseMove={handleMouseMove}
                            style={{
                                cursor: isDragging ? 'grabbing' : 'grab',
                                userSelect: 'none'
                            }}
                        >
                            {/* Media player is a special case - uses Zustand store */}
                            {settings.showMedia && <ExpandedMedia />}

                            {/* Dynamically render enabled widgets from the registry */}
                            <PopoverProvider onOpenChange={setIsPopoverOpen}>
                                {enabledWidgets.map(widget => (
                                    <PluginErrorBoundary
                                        key={widget.id}
                                        pluginId={widget.id}
                                        pluginName={widget.name}
                                    >
                                        <widget.ExpandedComponent />
                                    </PluginErrorBoundary>
                                ))}
                            </PopoverProvider>
                        </motion.div>
                    ) : (
                        <motion.div
                            key="files"
                            className="flex-1 flex flex-col overflow-hidden p-5"
                            initial={{ opacity: 0, x: 20 }}
                            animate={{ opacity: 1, x: 0 }}
                            exit={{ opacity: 0, x: 20 }}
                            transition={{ duration: 0.3 }}
                        >
                            <FileTray />
                        </motion.div>
                    )}
                </AnimatePresence>
            </div>
        </motion.div>
    );
}
