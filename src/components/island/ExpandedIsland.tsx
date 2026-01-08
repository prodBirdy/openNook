import { motion, AnimatePresence } from 'motion/react';
import { IconSettings, IconLayoutGrid, IconFiles } from '@tabler/icons-react';
import { ExpandedMedia } from '../ExpandedMedia';
import { FileTray, FileItem } from '../FileTray';
import { NowPlayingData } from './types';
import { useWidgets } from '../../context/WidgetContext';
import { WidgetWrapper } from '../widgets/WidgetWrapper';

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
    nowPlaying: NowPlayingData | null;
    handlePlayPause: (e: React.MouseEvent) => void;
    handleNextTrack: (e: React.MouseEvent) => void;
    handlePreviousTrack: (e: React.MouseEvent) => void;
    handleSeek: (position: number) => Promise<void>;
    files: FileItem[];
    setFiles: (files: FileItem[]) => void;
    notes: string;
    handleNotesChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void;
    handleNotesClick: (e: React.MouseEvent) => void;
    handleChildWheel: (e: React.WheelEvent) => void;
}

export function ExpandedIsland({
    activeTab,
    setActiveTab,
    notchHeight,
    baseNotchWidth,
    settings,
    handleSettingsClick,
    nowPlaying,
    handlePlayPause,
    handleNextTrack,
    handlePreviousTrack,
    handleSeek,
    files,
    setFiles,
    notes,
    handleNotesChange,
    handleNotesClick,
    handleChildWheel
}: ExpandedIslandProps) {
    const { enabledWidgets } = useWidgets();

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
                            {/* Media player is a special case - needs props */}
                            {settings.showMedia && (
                                <>
                                    {nowPlaying ? (
                                        <ExpandedMedia
                                            nowPlaying={nowPlaying}
                                            onPlayPause={handlePlayPause}
                                            onNext={handleNextTrack}
                                            onPrevious={handlePreviousTrack}
                                            onSeek={handleSeek}
                                        />
                                    ) : (
                                        <div className="no-media-message">No media playing</div>
                                    )}
                                </>
                            )}

                            {/* Dynamically render enabled widgets from the registry */}
                            {enabledWidgets.map(widget => (

                                <widget.ExpandedComponent />

                            ))}
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
                            <FileTray files={files} onUpdateFiles={setFiles} />
                        </motion.div>
                    )}
                </AnimatePresence>
            </div>
        </motion.div>
    );
}
