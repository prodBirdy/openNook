import { motion, AnimatePresence } from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useState, useCallback, useRef, useMemo } from 'react';
import { useNotchInfo } from '../hooks/useNotchInfo';
import { getDominantColor } from '../utils/imageUtils';
import './DynamicIsland.css';
import { FileItem } from './FileTray';
import { CompactMedia } from './island/CompactMedia';
import { CompactFiles } from './island/CompactFiles';
import { CompactIdle } from './island/CompactIdle';
import { CompactOnboard } from './island/CompactOnboard';
import { ModeIndicator } from './island/ModeIndicator';
import { ExpandedIsland } from './island/ExpandedIsland';
import { NowPlayingData } from './island/types';

const artworkColorCache = new Map<string, string | null>();

export function DynamicIsland() {
    const { notchInfo } = useNotchInfo();
    const [nowPlaying, setNowPlaying] = useState<NowPlayingData | null>(null);
    const [visualizerColor, setVisualizerColor] = useState<string | null>(null);
    const [isHovered, setIsHovered] = useState(false);
    // isCoverHovered moved to CompactMedia
    const [expanded, setExpanded] = useState(false);
    const [isAnimating, setIsAnimating] = useState(false);
    const [notes, setNotes] = useState('');
    const [activeTab, setActiveTab] = useState<'widgets' | 'files'>('widgets');
    const [windowSize, setWindowSize] = useState({ width: window.innerWidth, height: window.innerHeight });
    const [droppedFiles, setDroppedFiles] = useState<string[]>([]);
    const [files, setFiles] = useState<FileItem[]>([]);
    const [preferredMode, setPreferredMode] = useState<'media' | 'files' | 'onboard' | 'idle' | null>('onboard');
    const [isInitialLaunch, setIsInitialLaunch] = useState(true);

    const islandRef = useRef<HTMLDivElement>(null);
    const notesTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const fetchTimeoutRefs = useRef<ReturnType<typeof setTimeout>[]>([]);
    const [settings, setSettings] = useState({
        showCalendar: false,
        showReminders: false,
        showMedia: true,
        baseWidth: 160,
        baseHeight: 38,
        liquidGlassMode: false,
    });
    const lastArtworkRef = useRef<string | null | undefined>(null);
    const expandedRef = useRef(expanded);
    const lastModeCycleRef = useRef<number>(0);
    const prevHasMediaRef = useRef<boolean>(false);
    const prevHasFilesRef = useRef<boolean>(false);

    // Update expandedRef whenever expanded changes
    useEffect(() => {
        expandedRef.current = expanded;
    }, [expanded]);

    // Initial launch: keep onboard mode for 10 seconds
    useEffect(() => {
        const timer = setTimeout(() => {
            setIsInitialLaunch(false);
        }, 10000); // 10 seconds

        return () => clearTimeout(timer);
    }, []);

    // Load files on mount
    useEffect(() => {
        invoke<FileItem[]>('load_file_tray')
            .then(async (loadedFiles) => {
                // Resolve paths for loaded files
                const resolvedFiles = await Promise.all(loadedFiles.map(async (file) => {
                    if (file.path) {
                        try {
                            const resolvedPath = await invoke<string>('resolve_path', { path: file.path });
                            return { ...file, resolvedPath };
                        } catch (e) {
                            console.error(`Failed to resolve path for ${file.name}:`, e);
                            return file;
                        }
                    }
                    return file;
                }));
                setFiles(resolvedFiles);
            })
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
            const processFiles = async () => {
                const newFilesPromises = droppedFiles.map(async path => {
                    // Extract filename from path
                    const name = path.split(/[/\\]/).pop() || path;
                    // Simple extension check for type
                    const ext = name.split('.').pop()?.toLowerCase();
                    let type = 'unknown';
                    if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg'].includes(ext || '')) {
                        type = `image/${ext}`;
                    }

                    let resolvedPath = path;
                    try {
                        resolvedPath = await invoke<string>('resolve_path', { path });
                    } catch (e) {
                        console.error('Failed to resolve path', e);
                    }

                    return {
                        name,
                        size: 0, // We don't have size from backend event immediately, could fetch if needed
                        path,
                        resolvedPath,
                        type,
                        lastModified: Date.now()
                    };
                });

                const newFiles = await Promise.all(newFilesPromises);

                setFiles(prev => {
                    // Avoid duplicates
                    const existingPaths = new Set(prev.map(f => f.path));
                    const uniqueNewFiles = newFiles.filter(f => !existingPaths.has(f.path));
                    const updated = [...prev, ...uniqueNewFiles];
                    // Save immediately
                    invoke('save_file_tray', { files: updated }).catch(console.error);
                    return updated;
                });
            };

            processFiles();
            setDroppedFiles([]);
        }
    }, [droppedFiles]);

    // Load settings
    useEffect(() => {
        const loadSettings = () => {
            const saved = localStorage.getItem('app-settings');
            if (saved) {
                try {
                    const parsed = JSON.parse(saved);
                    setSettings(prev => ({ ...prev, ...parsed }));
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

    const availableModes = useMemo(() => {
        const modes: ('media' | 'files' | 'onboard' | 'idle')[] = [];
        if (hasMedia) modes.push('media');
        if (hasFiles) modes.push('files');
        modes.push('onboard'); // Always include onboard as an option
        modes.push('idle'); // Always include idle as an option
        return modes;
    }, [hasMedia, hasFiles]);

    const mode: 'media' | 'files' | 'onboard' | 'idle' = useMemo(() => {
        if (preferredMode && availableModes.includes(preferredMode)) return preferredMode;
        // Default priority: Media > Files > Onboard > Idle
        if (availableModes.includes('media')) return 'media';
        if (availableModes.includes('files')) return 'files';
        if (availableModes.includes('onboard')) return 'onboard';
        return 'idle';
    }, [availableModes, preferredMode]);

    const cycleMode = useCallback((direction: 'next' | 'prev') => {
        if (availableModes.length <= 1) return;

        const currentIndex = availableModes.indexOf(mode);
        let nextIndex;
        if (direction === 'next') {
            nextIndex = (currentIndex + 1) % availableModes.length;
        } else {
            nextIndex = (currentIndex - 1 + availableModes.length) % availableModes.length;
        }

        const newMode = availableModes[nextIndex];
        setPreferredMode(newMode);
        invoke('trigger_haptics').catch(console.error);
    }, [availableModes, mode]);

    // Auto-switch to new modes when they become available (but not during initial launch)
    useEffect(() => {
        const prevMedia = prevHasMediaRef.current;
        const prevFiles = prevHasFilesRef.current;

        // Don't auto-switch during initial 10-second onboarding period
        if (isInitialLaunch) {
            // Update refs but don't switch modes
            prevHasMediaRef.current = hasMedia;
            prevHasFilesRef.current = hasFiles;
            return;
        }

        // Media started playing - switch to it
        if (!prevMedia && hasMedia) {
            setPreferredMode('media');
        }
        // Files were added - switch to them (but only if not playing media)
        else if (!prevFiles && hasFiles && !hasMedia) {
            setPreferredMode('files');
        }

        // Update refs for next comparison
        prevHasMediaRef.current = hasMedia;
        prevHasFilesRef.current = hasFiles;
    }, [hasMedia, hasFiles, isInitialLaunch]);

    // Memoize notch dimensions
    const { notchHeight, baseNotchWidth } = useMemo(() => ({
        notchHeight: Math.max(settings.baseHeight, notchInfo?.notch_height ? notchInfo.notch_height - 20 : 38),
        baseNotchWidth: Math.max(notchInfo?.notch_width ? notchInfo.notch_width : 160),
    }), [notchInfo?.notch_height, notchInfo?.notch_width, settings.baseHeight, settings.baseWidth]);

    // Memoize target dimensions
    const { targetWidth, targetHeight } = useMemo(() => {
        let width = baseNotchWidth;
        let height = notchHeight;

        if (expanded) {
            width = windowSize.width - 40;
            height = windowSize.height;
        } else if (isHovered) {
            if (mode === 'idle') {
                width = baseNotchWidth + 30;
                height = notchHeight + 10;
            } else {
                // media, files, onboard all have the same dimensions
                width = baseNotchWidth + 125;
                height = notchHeight + 15;
            }
        } else if (mode === 'idle') {
            // idle is smaller
            width = baseNotchWidth;
            height = notchHeight;
        } else {
            // media, files, onboard all have the same dimensions
            width = baseNotchWidth + 120;
            height = notchHeight + 8;
        }

        return { targetWidth: width, targetHeight: height };
    }, [expanded, isHovered, mode, baseNotchWidth, notchHeight, windowSize.width, windowSize.height]);

    const contentOpacity = (mode === 'onboard' || mode === 'idle' || hasMedia || hasFiles) ? 1 : 0;

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
            if (!prev) {
                // Expanding
                setIsAnimating(true);
                // Set initial tab based on current mode
                if (mode === 'files') {
                    setActiveTab('files');
                } else {
                    setActiveTab('widgets');
                }
                invoke('trigger_haptics').catch(console.error);

            } else {
                invoke('trigger_haptics').catch(console.error);
            }
            return !prev;
        });
    }, [mode]);

    // Hover handlers for haptics
    const handleHoverStart = useCallback(() => {
        invoke('trigger_haptics').catch(console.error);
    }, []);

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
            invoke('trigger_haptics').catch(console.error);
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
                invoke('trigger_haptics').catch(console.error);
            } else if (expanded && e.deltaY > 20) {
                setExpanded(false);
                setIsAnimating(true);
                invoke('trigger_haptics').catch(console.error);
            } else if (expanded) {
                // Horizontal swipe for tabs
                if (e.deltaX > 20 && activeTab === 'widgets') {
                    setActiveTab('files');
                } else if (e.deltaX < -20 && activeTab === 'files') {
                    setActiveTab('widgets');
                }
            } else if (!expanded) {
                // Horizontal wheel for trackpad mode cycling
                // Add cooldown to prevent overshooting (500ms between cycles)
                const now = Date.now();
                const timeSinceLastCycle = now - lastModeCycleRef.current;

                if (Math.abs(e.deltaX) > 20 && timeSinceLastCycle > 500) {
                    lastModeCycleRef.current = now;
                    if (e.deltaX > 20) cycleMode('next');
                    else cycleMode('prev');
                }
            }
        }
    }, [expanded, isAnimating, activeTab, mode, cycleMode]);

    const handleChildWheel = useCallback((e: React.WheelEvent) => {
        // If vertical scroll dominates, stop propagation to prevent closing the island
        if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
            e.stopPropagation();
            return;
        }

        // Check if we can scroll horizontally in the direction of the gesture
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
                className={`dynamic-island ${mode} ${expanded ? 'expanded' : ''} ${settings.liquidGlassMode ? 'liquid-glass' : ''}`}
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
                    <motion.div
                        key={expanded ? 'island-expanded' : `island-${mode}-collapsed`}
                        style={{
                            width: '100%',
                            height: '100%',
                            display: 'flex',
                            alignItems: 'center',
                        }}
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        transition={fadeTransition}
                    >
                        {expanded ? (
                            <ExpandedIsland
                                activeTab={activeTab}
                                setActiveTab={setActiveTab}
                                notchHeight={notchHeight}
                                baseNotchWidth={baseNotchWidth}
                                settings={settings}
                                handleSettingsClick={handleSettingsClick}
                                nowPlaying={nowPlaying}
                                handlePlayPause={handlePlayPause}
                                handleNextTrack={handleNextTrack}
                                handlePreviousTrack={handlePreviousTrack}
                                handleSeek={handleSeek}
                                files={files}
                                setFiles={setFiles}
                                notes={notes}
                                handleNotesChange={handleNotesChange}
                                handleNotesClick={handleNotesClick}
                                handleChildWheel={handleChildWheel}
                            />
                        ) : mode === 'media' && nowPlaying ? (
                            <CompactMedia
                                nowPlaying={nowPlaying}
                                isHovered={isHovered}
                                baseNotchWidth={baseNotchWidth}
                                visualizerColor={visualizerColor}
                                contentOpacity={contentOpacity}
                            />
                        ) : mode === 'files' ? (
                            <CompactFiles
                                files={files}
                                isHovered={isHovered}
                                baseNotchWidth={baseNotchWidth}
                                contentOpacity={contentOpacity}
                            />
                        ) : mode === 'onboard' ? (
                            <CompactOnboard
                                baseNotchWidth={baseNotchWidth}
                                isHovered={isHovered}
                                contentOpacity={contentOpacity}
                            />
                        ) : mode === 'idle' ? (
                            <CompactIdle
                                baseNotchWidth={baseNotchWidth}
                                isHovered={isHovered}
                                contentOpacity={contentOpacity}
                            />
                        ) : null}
                    </motion.div>
                </AnimatePresence>

                {/* Mode indicator dots - only show when not expanded */}
                {!expanded && (
                    <ModeIndicator
                        availableModes={availableModes}
                        currentMode={mode}
                    />
                )}
            </motion.div >
        </div >
    );
}
