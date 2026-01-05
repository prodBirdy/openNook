import { motion, AnimatePresence } from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useState, useCallback, useRef, useMemo } from 'react';
import { useNotchInfo } from '../hooks/useNotchInfo';
import { getDominantColor } from '../utils/imageUtils';
import { IconSettings, IconLayoutGrid, IconFiles } from '@tabler/icons-react';
import './DynamicIsland.css';
import { CalendarWidget } from './widgets/CalendarWidget';
import { RemindersWidget } from './widgets/RemindersWidget';
import { SmartAudioVisualizer } from './SmartAudioVisualizer';
import { AlbumCover } from './AlbumCover';
import { ExpandedMedia } from './ExpandedMedia';
import { FileTray, FileItem } from './FileTray';



interface NowPlayingData {
    title: string | null;
    artist: string | null;
    album: string | null;
    artwork_base64: string | null;
    duration: number | null;
    elapsed_time: number | null;
    is_playing: boolean;
    audio_levels: number[] | null;
    app_name: string | null;
}

const artworkColorCache = new Map<string, string | null>();

export function DynamicIsland() {
    const { notchInfo } = useNotchInfo();
    const [nowPlaying, setNowPlaying] = useState<NowPlayingData | null>(null);
    const [visualizerColor, setVisualizerColor] = useState<string | null>(null);
    const [isHovered, setIsHovered] = useState(false);
    const [isCoverHovered, setIsCoverHovered] = useState(false);
    const [expanded, setExpanded] = useState(false);
    const [isAnimating, setIsAnimating] = useState(false);
    const [notes, setNotes] = useState('');
    const [activeTab, setActiveTab] = useState<'widgets' | 'files'>('widgets');
    const [windowSize, setWindowSize] = useState({ width: window.innerWidth, height: window.innerHeight });
    const [droppedFiles, setDroppedFiles] = useState<string[]>([]);
    const [files, setFiles] = useState<FileItem[]>([]);

    const islandRef = useRef<HTMLDivElement>(null);
    const notesTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const fetchTimeoutRefs = useRef<ReturnType<typeof setTimeout>[]>([]);
    const [settings, setSettings] = useState({
        showCalendar: false,
        showReminders: false,
        showMedia: true,
    });
    const lastArtworkRef = useRef<string | null | undefined>(null);
    const expandedRef = useRef(expanded);

    // Update expandedRef whenever expanded changes
    useEffect(() => {
        expandedRef.current = expanded;
    }, [expanded]);

    // Load files on mount
    useEffect(() => {
        invoke<FileItem[]>('load_file_tray')
            .then(setFiles)
            .catch(err => console.error('Failed to load file tray:', err));
    }, []);

    // Save files whenever they change
    useEffect(() => {
        if (files.length > 0) {
            invoke('save_file_tray', { files }).catch(err => console.error('Failed to save file tray:', err));
        }
    }, [files]);

    // Process external files (from backend drop event)
    useEffect(() => {
        if (droppedFiles && droppedFiles.length > 0) {
            const newFiles = droppedFiles.map(path => {
                // Extract filename from path
                const name = path.split(/[/\\]/).pop() || path;
                // Simple extension check for type
                const ext = name.split('.').pop()?.toLowerCase();
                let type = 'unknown';
                if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg'].includes(ext || '')) {
                    type = `image/${ext}`;
                }

                return {
                    name,
                    size: 0, // We don't have size from backend event immediately, could fetch if needed
                    path,
                    type,
                    lastModified: Date.now()
                };
            });

            setFiles(prev => {
                // Avoid duplicates
                const existingPaths = new Set(prev.map(f => f.path));
                const uniqueNewFiles = newFiles.filter(f => !existingPaths.has(f.path));
                const updated = [...prev, ...uniqueNewFiles];
                // Save immediately
                invoke('save_file_tray', { files: updated }).catch(console.error);
                return updated;
            });

            setDroppedFiles([]);
        }
    }, [droppedFiles]);

    // Load settings
    useEffect(() => {
        const loadSettings = () => {
            const saved = localStorage.getItem('app-settings');
            if (saved) {
                try {
                    setSettings(JSON.parse(saved));
                } catch (e) { console.error(e); }
            }
        };
        loadSettings();

        const handleStorage = (e: StorageEvent) => {
            if (e.key === 'app-settings') loadSettings();
        };
        window.addEventListener('storage', handleStorage);
        return () => window.removeEventListener('storage', handleStorage);
    }, []);

    const handleSettingsClick = useCallback(async () => {
        try {
            await invoke('open_settings');
        } catch (e) {
            console.error('Failed to open settings:', e);
        }
    }, []);


    // Determine mode - memoized
    const hasMedia = !!(nowPlaying && nowPlaying.is_playing);
    const hasFiles = files.length > 0;
    const mode: 'media' | 'files' | 'idle' = hasMedia ? 'media' : (hasFiles ? 'files' : 'idle');

    // Memoize notch dimensions
    const { notchHeight, baseNotchWidth } = useMemo(() => ({
        notchHeight: notchInfo?.notch_height ? notchInfo.notch_height - 20 : 38,
        baseNotchWidth: notchInfo?.notch_width ? notchInfo.notch_width : 160,
    }), [notchInfo?.notch_height, notchInfo?.notch_width]);

    // Memoize target dimensions
    const { targetWidth, targetHeight } = useMemo(() => {
        let width = baseNotchWidth;
        let height = notchHeight;

        if (expanded) {
            width = windowSize.width - 40;
            height = windowSize.height;
        } else if (isHovered) {
            if (mode === 'media') {
                width = baseNotchWidth + 125;
                height = notchHeight + 15;
            } else if (mode === 'files') {
                width = baseNotchWidth + 125;
                height = notchHeight + 15;
            } else {
                width = baseNotchWidth + 30;
                height = notchHeight + 10;
            }
        } else if (mode === 'media') {
            width = baseNotchWidth + 120;
            height = notchHeight + 8;
        } else if (mode === 'files') {
            width = baseNotchWidth + 120;
            height = notchHeight + 8;
        }

        return { targetWidth: width, targetHeight: height };
    }, [expanded, isHovered, mode, baseNotchWidth, notchHeight, windowSize.width, windowSize.height]);

    const contentOpacity = (hasMedia || hasFiles) ? 1 : 0;

    // Stable fetch function
    const fetchNowPlaying = useCallback(async () => {
        try {
            const data = await invoke<NowPlayingData>('get_now_playing');

            setNowPlaying(prev => {
                const trackChanged = !prev ||
                    prev.title !== data.title ||
                    prev.artist !== data.artist ||
                    prev.is_playing !== data.is_playing;

                // Optimization: If only elapsed time changed and we are NOT expanded,
                // do NOT update state to prevent re-renders.
                // We always update if track changed or playing status changed.
                if (prev && !trackChanged && !expandedRef.current) {
                    // Check if other fields are same
                    if (prev.duration === data.duration && prev.album === data.album) {
                        return prev; // Return SAME object reference -> No re-render
                    }
                }

                if (trackChanged && data.is_playing) {
                    console.log('🎵 Now Playing:', {
                        title: data.title,
                        artist: data.artist,
                        album: data.album,
                        duration: data.duration,
                        elapsed: data.elapsed_time,
                        hasArtwork: !!data.artwork_base64,
                    });
                }

                // Optimization: Reuse artwork string reference if track hasn't changed.
                // This prevents passing a new large string to components every second,
                // allowing React.memo to work effectively on image components.
                let artwork = data.artwork_base64;
                if (prev && data.title === prev.title && data.artist === prev.artist && prev.artwork_base64) {
                    artwork = prev.artwork_base64;
                }

                return {
                    ...data,
                    artwork_base64: artwork,
                };
            });
        } catch (error) {
            console.error('Failed to fetch now playing:', error);
        }
    }, []);

    // Clear all pending fetch timeouts - helper function
    const clearFetchTimeouts = useCallback(() => {
        fetchTimeoutRefs.current.forEach(clearTimeout);
        fetchTimeoutRefs.current = [];
    }, []);

    // Schedule fetch with cleanup
    const scheduleFetch = useCallback((delays: number[]) => {
        clearFetchTimeouts();
        delays.forEach(delay => {
            const timeout = setTimeout(fetchNowPlaying, delay);
            fetchTimeoutRefs.current.push(timeout);
        });
    }, [fetchNowPlaying, clearFetchTimeouts]);

    // Media control handlers - consolidated timeout logic
    const handlePlayPause = useCallback(async (e: React.MouseEvent) => {
        e.stopPropagation();

        // Optimistic update
        setNowPlaying((prev) => (prev ? { ...prev, is_playing: !prev.is_playing } : null));

        try {
            await invoke('media_play_pause');
            scheduleFetch([100, 300]);
        } catch (err) {
            console.error('Failed to toggle play/pause:', err);
        }
    }, [scheduleFetch]);

    const handleNextTrack = useCallback(async (e: React.MouseEvent) => {
        e.stopPropagation();
        try {
            await invoke('media_next_track');
            scheduleFetch([100, 400, 800]);
        } catch (err) {
            console.error('Failed to skip to next track:', err);
        }
    }, [scheduleFetch]);

    const handlePreviousTrack = useCallback(async (e: React.MouseEvent) => {
        e.stopPropagation();
        try {
            await invoke('media_previous_track');
            scheduleFetch([100, 400, 800]);
        } catch (err) {
            console.error('Failed to go to previous track:', err);
        }
    }, [scheduleFetch]);

    const handleSeek = useCallback(async (position: number) => {
        try {
            await invoke('media_seek', { position });

            // Optimistic update (post-command) to prevent snap-back
            setNowPlaying((prev) => (prev ? { ...prev, elapsed_time: position } : null));

            scheduleFetch([100, 400, 800]);
        } catch (err) {
            console.error('Failed to seek:', err);
        }
    }, [scheduleFetch]);

    // Notes handlers
    const handleNotesChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const value = e.target.value;
        setNotes(value);

        if (notesTimeoutRef.current) {
            clearTimeout(notesTimeoutRef.current);
        }
        notesTimeoutRef.current = setTimeout(() => {
            invoke('save_notes', { notes: value })
                .catch(err => console.error('Failed to save notes:', err));
        }, 500);
    }, []);

    const handleNotesClick = useCallback((e: React.MouseEvent) => {
        e.stopPropagation();
    }, []);

    // Toggle expanded mode
    const handleIslandClick = useCallback(() => {
        setExpanded(prev => {
            if (!prev) setIsAnimating(true);
            return !prev;
        });
    }, []);

    // Hover handlers for haptics
    const handleHoverStart = useCallback(() => {
        invoke('trigger_haptics').catch(console.error);
    }, []);

    // Album click handler
    const handleAlbumClick = useCallback((e: React.MouseEvent) => {
        e.stopPropagation();
        if (nowPlaying?.app_name) {
            invoke('activate_media_app', { appName: nowPlaying.app_name })
                .catch(err => console.error('Failed to open media app:', err));
        }
    }, [nowPlaying?.app_name]);

    // Combined initialization and cleanup effect
    useEffect(() => {
        // Load notes
        invoke<string>('load_notes')
            .then(setNotes)
            .catch(err => console.error('Failed to load notes:', err));

        // Initial fetch
        fetchNowPlaying();

        // Poll at 1 second interval
        const trackInterval = setInterval(fetchNowPlaying, 1000);

        // Window resize handler with throttling
        let resizeTimeout: ReturnType<typeof setTimeout> | null = null;
        const handleResize = () => {
            if (resizeTimeout) return;
            resizeTimeout = setTimeout(() => {
                setWindowSize({ width: window.innerWidth, height: window.innerHeight });
                resizeTimeout = null;
            }, 100);
        };
        window.addEventListener('resize', handleResize);

        // Media key handler
        const handleKeyDown = (e: KeyboardEvent) => {
            const mediaKeys = ['MediaPlayPause', 'MediaTrackNext', 'MediaTrackPrevious'];
            const fnKeys = ['F7', 'F8', 'F9'];

            if (mediaKeys.includes(e.key) || fnKeys.includes(e.key)) {
                setTimeout(fetchNowPlaying, 150);
                setTimeout(fetchNowPlaying, 500);
            }
        };
        window.addEventListener('keydown', handleKeyDown);
        // Mouse hover listeners
        const unlistenEnter = listen('mouse-entered-notch', () => setIsHovered(true));
        const unlistenExit = listen('mouse-exited-notch', () => setIsHovered(false));

        return () => {
            clearInterval(trackInterval);
            if (resizeTimeout) clearTimeout(resizeTimeout);
            window.removeEventListener('resize', handleResize);
            window.removeEventListener('keydown', handleKeyDown);
            unlistenEnter.then(fn => fn());
            unlistenExit.then(fn => fn());
        };
    }, [fetchNowPlaying]);

    // Drag and drop listener
    useEffect(() => {
        const handleDragEnter = (event: any) => {
            console.log('Drag enter detected:', event);
            setExpanded(true);
            setActiveTab('files');
            setIsAnimating(true);
        };

        const handleFileDrop = (event: any) => {
            console.log('File drop detected (backend):', event);
            if (event.payload && Array.isArray(event.payload)) {
                setDroppedFiles(prev => [...prev, ...event.payload]);
            }
        };

        const unlistenDragEnter = listen('tauri://drag-enter', handleDragEnter);
        const unlistenBackendDragEnter = listen('drag-enter-event', handleDragEnter);
        const unlistenBackendFileDrop = listen('file-drop-event', handleFileDrop);
        // Fallback for different Tauri versions/configs
        const unlistenFileDropHover = listen('tauri://file-drop-hover', handleDragEnter);

        return () => {
            unlistenDragEnter.then(fn => fn());
            unlistenBackendDragEnter.then(fn => fn());
            unlistenBackendFileDrop.then(fn => fn());
            unlistenFileDropHover.then(fn => fn());
        };
    }, []);

    // Update visualizer color when artwork changes (with caching)
    useEffect(() => {
        const artwork = nowPlaying?.artwork_base64;

        // Skip if artwork hasn't changed
        if (artwork === lastArtworkRef.current) return;
        lastArtworkRef.current = artwork;

        if (!artwork) {
            setVisualizerColor(null);
            return;
        }

        // Check cache first
        if (artworkColorCache.has(artwork)) {
            setVisualizerColor(artworkColorCache.get(artwork) ?? null);
            return;
        }

        const src = `data:image/png;base64,${artwork}`;
        getDominantColor(src).then(rgb => {
            const color = rgb ? `rgb(${rgb[0]}, ${rgb[1]}, ${rgb[2]})` : null;
            artworkColorCache.set(artwork, color);

            // Limit cache size
            if (artworkColorCache.size > 50) {
                const firstKey = artworkColorCache.keys().next().value;
                if (firstKey) artworkColorCache.delete(firstKey);
            }

            setVisualizerColor(color);
        });
    }, [nowPlaying?.artwork_base64]);

    useEffect(() => {
        if (!isHovered && expanded && !isAnimating) {
            setExpanded(false);
        }
    }, [isHovered, expanded, isAnimating]);

    const handleWheel = useCallback((e: React.WheelEvent) => {
        if (!isAnimating) {
            if (!expanded && e.deltaY < -20) {
                setExpanded(true);
                setIsAnimating(true);
            } else if (expanded && e.deltaY > 20) {
                setExpanded(false);
                // We don't necessarily need to set isAnimating to true here if we want immediate response,
                // but for consistency with opening protection:
                // However, closer logic might benefit from being immediate.
                // Let's stick to the pattern:
                setIsAnimating(true);
            } else if (expanded) {
                // Horizontal swipe for tabs
                if (e.deltaX > 20 && activeTab === 'widgets') {
                    setActiveTab('files');
                } else if (e.deltaX < -20 && activeTab === 'files') {
                    setActiveTab('widgets');
                }
            }
        }
    }, [expanded, isAnimating, activeTab]);

    const handleChildWheel = useCallback((e: React.WheelEvent) => {
        // If vertical scroll dominates, stop propagation to prevent closing the island
        if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
            e.stopPropagation();
            return;
        }

        // Check if we can scroll horizontally in the direction of the delta
        const container = e.currentTarget;
        const isScrollable = container.scrollWidth > container.clientWidth;

        if (isScrollable) {
            const canScrollRight = container.scrollLeft < (container.scrollWidth - container.clientWidth - 1);
            const canScrollLeft = container.scrollLeft > 1;

            // If we can scroll in the direction of the gesture, stop propagation
            // so the parent doesn't switch tabs.
            if ((e.deltaX > 0 && canScrollRight) || (e.deltaX < 0 && canScrollLeft)) {
                e.stopPropagation();
            }
        }
    }, []);

    // Update bounds - debounced
    useEffect(() => {
        const updateBounds = () => {
            if (!islandRef.current) return;

            const rect = islandRef.current.getBoundingClientRect();
            const totalWidth = rect.width + 40;
            const x = rect.left - 20;

            invoke('update_ui_bounds', {
                x,
                y: rect.top,
                width: totalWidth,
                height: rect.height
            }).catch(console.error);
        };

        // Single delayed update to catch animation completion
        const timeoutId = setTimeout(updateBounds, 350);
        return () => clearTimeout(timeoutId);
    }, [targetWidth, targetHeight]);

    // Memoize spring transition
    const springTransition = useMemo(() => ({
        type: 'spring' as const,
        stiffness: 400,
        damping: 30,
        mass: 0.8
    }), []);

    // Memoize fade transition
    const fadeTransition = useMemo(() => ({ duration: 0.3 }), []);

    return (
        <div className={`dynamic-island-container ${expanded ? 'expanded' : ''}`}>
            <motion.div
                ref={islandRef}
                className={`dynamic-island ${mode} ${expanded ? 'expanded' : ''}`}
                initial={false}
                onAnimationComplete={() => setIsAnimating(false)}
                animate={{
                    width: targetWidth,
                    height: targetHeight,
                    borderRadius: '0px 0px 18px 18px',
                }}
                transition={springTransition}
                onHoverStart={handleHoverStart}
                onClick={handleIslandClick}
                onWheel={handleWheel}
                style={{ cursor: 'pointer' }}
            >
                <AnimatePresence mode="wait">
                    {expanded ? (
                        <motion.div
                            key="expanded-content"
                            className="island-content expanded-content"
                            initial={{ opacity: 0 }}
                            animate={{ opacity: 1 }}
                            exit={{ opacity: 0 }}
                            transition={fadeTransition}
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
                                            transition={fadeTransition}
                                            onWheel={handleChildWheel}
                                        >
                                            {settings.showMedia && (
                                                <div
                                                    className={`expanded-media-player widget-card ${(!nowPlaying?.duration || nowPlaying.duration <= 0) ? 'no-progress' : ''}`}
                                                    onClick={(e) => e.stopPropagation()}
                                                >
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
                                                </div>
                                            )}

                                            {settings.showCalendar && (
                                                <div className="widget-card" onClick={(e) => e.stopPropagation()} style={{ minWidth: 280 }}>
                                                    <CalendarWidget />
                                                </div>
                                            )}

                                            {settings.showReminders && (
                                                <div className="widget-card" onClick={(e) => e.stopPropagation()} style={{ minWidth: 260 }}>
                                                    <RemindersWidget />
                                                </div>
                                            )}

                                            <div className="expanded-notes-section widget-card" onClick={(e) => e.stopPropagation()}>
                                                <textarea
                                                    className="notes-field"
                                                    placeholder="Type your notes here..."
                                                    value={notes}
                                                    onChange={handleNotesChange}
                                                    onClick={handleNotesClick}
                                                />
                                            </div>
                                        </motion.div>
                                    ) : (
                                        <motion.div
                                            key="files"
                                            className="file-tray-wrapper"
                                            initial={{ opacity: 0, x: 20 }}
                                            animate={{ opacity: 1, x: 0 }}
                                            exit={{ opacity: 0, x: 20 }}
                                            transition={fadeTransition}
                                        >
                                            <FileTray files={files} onUpdateFiles={setFiles} />
                                        </motion.div>
                                    )}
                                </AnimatePresence>
                            </div>
                        </motion.div>
                    ) : mode === 'media' && nowPlaying ? (
                        <motion.div
                            key="media-content"
                            className="island-content media-content"
                            initial={{ opacity: 0 }}
                            animate={{ opacity: contentOpacity }}
                            transition={fadeTransition}
                            style={{ pointerEvents: isHovered ? 'auto' : 'none' }}
                        >
                            <div className="media-left" style={{ width: 60, height: '100%' }}>
                                <div className="album-cover-wrapper">
                                    <AlbumCover
                                        artwork={nowPlaying.artwork_base64}
                                        title={nowPlaying.title}
                                        isPlaying={nowPlaying.is_playing}
                                        onHoverChange={setIsCoverHovered}
                                        onClick={handleAlbumClick}
                                    />
                                    <AnimatePresence>
                                        {isCoverHovered && isHovered && (nowPlaying.title || nowPlaying.artist) && (
                                            <motion.div
                                                className="album-info-reveal"
                                                initial={{ opacity: 0, y: -8, x: '-50%' }}
                                                animate={{ opacity: 1, y: 4, x: '-50%' }}
                                                exit={{ opacity: 0, y: -8, x: '-50%' }}
                                                transition={{ type: 'spring', stiffness: 500, damping: 30 }}
                                            >
                                                <div className="album-info-title">{nowPlaying.title || 'Unknown Title'}</div>
                                                <div className="album-info-artist">{nowPlaying.artist || 'Unknown Artist'}</div>
                                            </motion.div>
                                        )}
                                    </AnimatePresence>
                                </div>
                            </div>

                            <div className="media-spacer" style={{ width: baseNotchWidth }} />

                            <div className="media-right" style={{ width: 60, height: '100%' }}>
                                <SmartAudioVisualizer
                                    isPlaying={nowPlaying.is_playing}
                                    fallbackLevels={nowPlaying.audio_levels}
                                    color={visualizerColor}
                                />
                            </div>
                        </motion.div>
                    ) : mode === 'files' ? (
                        <motion.div
                            key="files-content"
                            className="island-content files-content"
                            initial={{ opacity: 0 }}
                            animate={{ opacity: contentOpacity }}
                            transition={fadeTransition}
                            style={{
                                pointerEvents: isHovered ? 'auto' : 'none',
                                display: 'flex',
                                alignItems: 'center',
                                justifyContent: 'space-between',
                                width: '100%',
                                padding: '0 12px'
                            }}
                        >
                            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                                <IconFiles size={20} color="white" stroke={1.5} />
                            </div>

                            <div className="media-spacer" style={{ width: baseNotchWidth }} />

                            <div style={{ display: 'flex', alignItems: 'center' }}>
                                {files.length}
                            </div>
                        </motion.div>
                    ) : null}
                </AnimatePresence>
            </motion.div >
        </div >
    );
}
