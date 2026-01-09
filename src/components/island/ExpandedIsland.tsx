import { motion, AnimatePresence } from 'motion/react';
import { IconSettings, IconLayoutGrid, IconFiles } from '@tabler/icons-react';
import { ExpandedMedia } from './ExpandedMedia';
import { FileTray } from '../FileTray';
import { useWidgetStore } from '../../stores/useWidgetStore';
import { WidgetWrapper } from '../widgets/WidgetWrapper';
import { useMemo } from 'react';
import { PopoverProvider } from '../../context/PopoverContext';

interface ExpandedIslandProps {
    activeTab: 'widgets' | 'files';
    setActiveTab: (tab: 'widgets' | 'files') => void;
    notchHeight: number;
    baseNotchWidth: number;
    settings: {
        showMedia: boolean;
        showCalendar: boolean;
        showReminders: boolean;
        baseWidth: number;
        baseHeight: number;
        liquidGlassMode: boolean;
    };
    handleSettingsClick: () => void;
    notes: string;
    handleNotesChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void;
    handleNotesClick: (e: React.MouseEvent) => void;
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
    notes,
    handleNotesChange,
    handleNotesClick,
    handleChildWheel,
    setIsPopoverOpen
}: ExpandedIslandProps) {
    const widgets = useWidgetStore(state => state.widgets);
    const widgetEnabledState = useWidgetStore(state => state.enabledState);

    // Compute enabled widgets with memoization to avoid infinite loops
    const enabledWidgets = useMemo(() =>
        widgets.filter(w => widgetEnabledState[w.id]),
        [widgets, widgetEnabledState]
    );

    return (
        <motion.div
            key="expanded-content"
            className="island-content expanded-content"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.3 }}
        >
            {/* Top Menu Bar */}
            <div className="expanded-menu-bar" style={{ height: notchHeight }}>
                <div className="tab-control-container">
                    <div className="tab-pill-background">
                        <div
                            className="tab-pill-active-bg"
                            style={{
                                transform: `translateX(${activeTab === 'widgets' ? '0%' : '100%'})`
                            }}
                        />
                        <div
                            className={`tab-pill-option ${activeTab === 'widgets' ? 'active' : ''}`}
                            onClick={(e) => { e.stopPropagation(); setActiveTab('widgets'); }}
                        >
                            <IconLayoutGrid size={16} />
                        </div>
                        <div
                            className={`tab-pill-option ${activeTab === 'files' ? 'active' : ''}`}
                            onClick={(e) => { e.stopPropagation(); setActiveTab('files'); }}
                        >
                            <IconFiles size={16} />
                        </div>
                    </div>
                </div>
                <div className="media-spacer" style={{ width: baseNotchWidth, height: '100%' }} />
                <div style={{ flex: 1, display: 'flex', justifyContent: 'flex-end', alignItems: 'center' }}>
                    <div
                        className="settings-button"
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
            <div className="main-content-area">
                <AnimatePresence mode="wait">
                    {activeTab === 'widgets' ? (
                        <motion.div
                            key="widgets"
                            className="widgets-container"
                            initial={{ opacity: 0, x: -20 }}
                            animate={{ opacity: 1, x: 0 }}
                            exit={{ opacity: 0, x: -20 }}
                            transition={{ duration: 0.3 }}
                            onWheel={handleChildWheel}
                        >
                            {/* Media player is a special case - uses Zustand store */}
                            {settings.showMedia && <ExpandedMedia />}

                            {/* Dynamically render enabled widgets from the registry */}
                            <PopoverProvider onOpenChange={setIsPopoverOpen}>
                                {enabledWidgets.map(widget => (
                                    <widget.ExpandedComponent key={widget.id} />
                                ))}
                            </PopoverProvider>
                            <WidgetWrapper title="Notes">

                                <textarea
                                    className="notes-field"
                                    placeholder="Type your notes here..."
                                    value={notes}
                                    onChange={handleNotesChange}
                                    onClick={handleNotesClick}
                                />

                            </WidgetWrapper>

                        </motion.div>
                    ) : (
                        <motion.div
                            key="files"
                            className="flex-1 flex flex-col  overflow-hidden"
                            style={{ padding: '20px' }}
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
