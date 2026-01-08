import { motion, AnimatePresence } from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useCallback, useRef, useMemo, useState } from 'react';
import { useNotchInfo } from '../../hooks/useNotchInfo';
import { CompactMedia } from './CompactMedia';
import { CompactFiles } from './CompactFiles';
import { CompactIdle } from './CompactIdle';
import { CompactOnboard } from './CompactOnboard';
import { ModeIndicator } from './ModeIndicator';
import { ExpandedIsland } from './ExpandedIsland';

import { useMediaPlayerStore } from '../../stores/useMediaPlayerStore';
import { useDynamicIslandStore } from '../../stores/useDynamicIslandStore';
import { useWidgetStore } from '../../stores/useWidgetStore';
import { useDerivedTimers } from '../../stores/useTimerStore';
import { useSessionsWithElapsed } from '../../stores/useSessionStore';
import { useFileTrayStore } from '../../stores/useFileTrayStore';

export function DynamicIsland() {
    const { notchInfo } = useNotchInfo();
    const widgets = useWidgetStore(state => state.widgets);
    const enabledState = useWidgetStore(state => state.enabledState);
    const hasActiveInstance = useWidgetStore(state => state.hasActiveInstance);

    // Compute enabled compact widgets outside the selector to avoid infinite loops
    const enabledCompactWidgets = useMemo(() =>
        widgets.filter(w => enabledState[w.id] && w.hasCompactMode && w.CompactComponent),
        [widgets, enabledState]
    );

    const timers = useDerivedTimers();
    const { sessions } = useSessionsWithElapsed();

    // Track if a popover is open to pause auto-collapse
    const [isPopoverOpen, setIsPopoverOpen] = useState(false);

    // Zustand store
    const {
        preferredModeId,
        setPreferredModeId,
        isInitialLaunch,
        setIsInitialLaunch,
        isHovered,
        setIsHovered,
        expanded,
        setExpanded,
        isAnimating,
        setIsAnimating,
        activeTab,
        setActiveTab,
        notes,
        setNotes,
        loadNotes,
        saveNotes,
        windowSize,
        setWindowSize,
        settings,
        loadSettings,
        lastModeCycleTime,
        setLastModeCycleTime,
    } = useDynamicIslandStore();

    const islandRef = useRef<HTMLDivElement>(null);
    const notesTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const prevHasMediaRef = useRef<boolean>(false);
    const prevHasFilesRef = useRef<boolean>(false);

    // Media player state from Zustand
    const nowPlaying = useMediaPlayerStore(state => state.nowPlaying);
    const visualizerColor = useMediaPlayerStore(state => state.visualizerColor);
    const fetchNowPlaying = useMediaPlayerStore(state => state.fetchNowPlaying);
    const hasMedia = useMediaPlayerStore(state => state.hasMedia(settings.showMedia));


    // Determine mode - memoized
    const hasFiles = useFileTrayStore(state => state.files.length > 0);

    // System modes that aren't strict widgets but behave like one
    const availableModes = useMemo(() => {
        const modes: { id: string, priority: number }[] = [];

        // 1. Add enabled compact widgets
        enabledCompactWidgets.forEach(widget => {
            let isActive = false;

            if (widget.id === 'timer') {
                isActive = hasActiveInstance('timer') || timers.some(t => t.isRunning);
            } else if (widget.id === 'session') {
                isActive = hasActiveInstance('session') || sessions.some(s => s.isActive);
            } else {
                isActive = true;
            }

            if (isActive) {
                modes.push({
                    id: widget.id,
                    priority: widget.compactPriority ?? 100
                });
            }
        });

        // 2. Add system modes
        if (hasMedia) modes.push({ id: 'media', priority: 50 });
        if (hasFiles) modes.push({ id: 'files', priority: 80 });

        modes.push({ id: 'onboard', priority: 999 });
        modes.push({ id: 'idle', priority: 200 });

        // Sort by priority (lower is better)
        return modes.sort((a, b) => a.priority - b.priority).map(m => m.id);
    }, [enabledCompactWidgets, hasMedia, hasFiles, hasActiveInstance, timers, sessions]);

    const activeModeId = useMemo(() => {
        if (preferredModeId && availableModes.includes(preferredModeId)) return preferredModeId;

        if (availableModes.length > 0) return availableModes[0];

        return 'idle';
    }, [availableModes, preferredModeId]);

    const mode = activeModeId;

    const cycleMode = useCallback((direction: 'next' | 'prev') => {
        if (availableModes.length <= 1) return;

        const currentIndex = availableModes.indexOf(activeModeId);
        let nextIndex;
        if (direction === 'next') {
            nextIndex = (currentIndex + 1) % availableModes.length;
        } else {
            nextIndex = (currentIndex - 1 + availableModes.length) % availableModes.length;
        }

        const newMode = availableModes[nextIndex];
        setPreferredModeId(newMode);
        invoke('trigger_haptics').catch(console.error);
    }, [availableModes, activeModeId, setPreferredModeId]);

    // Initial launch: keep onboard mode for 10 seconds
    useEffect(() => {
        const timer = setTimeout(() => {
            setIsInitialLaunch(false);
        }, 10000);

        return () => clearTimeout(timer);
    }, [setIsInitialLaunch]);

    // Toggle expanded mode
    const handleIslandClick = useCallback(() => {
        setExpanded(prev => {
            if (!prev) {
                setIsAnimating(true);
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
    }, [mode, setExpanded, setIsAnimating, setActiveTab]);

    // Hover handlers for haptics
    const handleHoverStart = useCallback(() => {
        invoke('trigger_haptics').catch(console.error);
    }, []);

    // Notes handlers
    const handleNotesChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const value = e.target.value;
        setNotes(value);

        if (notesTimeoutRef.current) {
            clearTimeout(notesTimeoutRef.current);
        }
        notesTimeoutRef.current = setTimeout(() => {
            saveNotes(value);
        }, 500);
    }, [setNotes, saveNotes]);

    const handleNotesClick = useCallback((e: React.MouseEvent) => {
        e.stopPropagation();
    }, []);

    // Initialization and listeners
    useEffect(() => {
        // Load notes
        loadNotes();

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

        // Mouse hover listeners
        const unlistenEnter = listen('mouse-entered-notch', () => setIsHovered(true));
        const unlistenExit = listen('mouse-exited-notch', () => setIsHovered(false));

        return () => {
            if (resizeTimeout) clearTimeout(resizeTimeout);
            window.removeEventListener('resize', handleResize);
            unlistenEnter.then(fn => fn());
            unlistenExit.then(fn => fn());
        };
    }, [loadNotes, setWindowSize, setIsHovered]);

    // Load settings
    useEffect(() => {
        loadSettings();

        const handleStorage = (e: StorageEvent) => {
            if (e.key === 'app-settings') loadSettings();
        };
        window.addEventListener('storage', handleStorage);
        return () => window.removeEventListener('storage', handleStorage);
    }, [loadSettings]);

    // Media player polling
    useEffect(() => {
        fetchNowPlaying(expanded);
        const trackInterval = setInterval(() => fetchNowPlaying(expanded), 1000);

        const handleKeyDown = (e: KeyboardEvent) => {
            const mediaKeys = ['MediaPlayPause', 'MediaTrackNext', 'MediaTrackPrevious'];
            const fnKeys = ['F7', 'F8', 'F9'];

            if (mediaKeys.includes(e.key) || fnKeys.includes(e.key)) {
                setTimeout(() => fetchNowPlaying(expanded), 150);
                setTimeout(() => fetchNowPlaying(expanded), 500);
            }
        };
        window.addEventListener('keydown', handleKeyDown);

        return () => {
            clearInterval(trackInterval);
            window.removeEventListener('keydown', handleKeyDown);
        };
    }, [fetchNowPlaying, expanded]);

    const handleSettingsClick = useCallback(async () => {
        try {
            await invoke('open_settings');
        } catch (e) {
            console.error('Failed to open settings:', e);
        }
    }, []);

    // Auto-switch to new modes when they become available (but not during initial launch)
    useEffect(() => {
        const prevMedia = prevHasMediaRef.current;
        const prevFiles = prevHasFilesRef.current;

        if (isInitialLaunch) {
            prevHasMediaRef.current = hasMedia;
            prevHasFilesRef.current = hasFiles;
            return;
        }

        // Media started playing - switch to it
        if (!prevMedia && hasMedia) {
            setPreferredModeId('media');
        }
        // Files were added - switch to them (but only if not playing media)
        else if (!prevFiles && hasFiles && !hasMedia) {
            setPreferredModeId('files');
        }

        prevHasMediaRef.current = hasMedia;
        prevHasFilesRef.current = hasFiles;
    }, [hasMedia, hasFiles, isInitialLaunch, setPreferredModeId]);

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
            height = Math.min(windowSize.height, 250);
        } else if (isHovered) {
            if (mode === 'idle') {
                width = baseNotchWidth + 30;
                height = notchHeight + 10;
            } else {
                width = baseNotchWidth + 125;
                height = notchHeight + 15;
            }
        } else if (mode === 'idle') {
            width = baseNotchWidth;
            // If in non-notch mode, collapse to 1px
            if (settings.nonNotchMode) {
                height = 1;
            } else {
                height = notchHeight;
            }
        } else {
            width = baseNotchWidth + 120;
            height = notchHeight;
        }

        return { targetWidth: width, targetHeight: height };
    }, [expanded, isHovered, mode, baseNotchWidth, notchHeight, windowSize.width, windowSize.height, settings.nonNotchMode]);

    const contentOpacity = (mode === 'idle' || mode === 'onboard' || hasMedia || hasFiles || mode !== 'idle') ? 1 : 0;

    useEffect(() => {
        // Auto-collapse when not hovered, but pause if a popover is open
        if (!isHovered && expanded && !isAnimating && !isPopoverOpen) {
            setExpanded(false);
        }
    }, [isHovered, expanded, isAnimating, isPopoverOpen, setExpanded]);

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
                if (e.deltaX > 20 && activeTab === 'widgets') {
                    setActiveTab('files');
                } else if (e.deltaX < -20 && activeTab === 'files') {
                    setActiveTab('widgets');
                }
            } else if (!expanded) {
                const now = Date.now();
                const timeSinceLastCycle = now - lastModeCycleTime;

                if (Math.abs(e.deltaX) > 20 && timeSinceLastCycle > 500) {
                    setLastModeCycleTime(now);
                    if (e.deltaX > 20) cycleMode('next');
                    else cycleMode('prev');
                }
            }
        }
    }, [expanded, isAnimating, activeTab, cycleMode, setExpanded, setIsAnimating, setActiveTab, lastModeCycleTime, setLastModeCycleTime]);

    const handleChildWheel = useCallback((e: React.WheelEvent) => {
        if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
            e.stopPropagation();
            return;
        }

        const container = e.currentTarget;
        const isScrollable = container.scrollWidth > container.clientWidth;

        if (isScrollable) {
            const canScrollRight = container.scrollLeft < (container.scrollWidth - container.clientWidth - 1);
            const canScrollLeft = container.scrollLeft > 1;

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

            const extraHeight = 0; // Previously was: isPopoverOpen ? 420 : 0

            invoke('update_ui_bounds', {
                x,
                y: rect.top,
                width: totalWidth,
                height: rect.height + extraHeight
            }).catch(console.error);
        };

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
                className={`dynamic-island ${mode} ${expanded ? 'expanded' : ''} ${settings.liquidGlassMode ? 'liquid-glass' : ''} ${settings.nonNotchMode ? 'no-wings' : ''} ${isHovered ? 'hovered' : ''}`}
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
                            maxHeight: '250px',
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
                                notes={notes}
                                handleNotesChange={handleNotesChange}
                                handleNotesClick={handleNotesClick}
                                handleChildWheel={handleChildWheel}
                                setIsPopoverOpen={setIsPopoverOpen}
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
                            <CompactIdle />
                        ) : (
                            // Dynamic widget rendering
                            (() => {
                                const widget = enabledCompactWidgets.find(w => w.id === mode);
                                if (widget && widget.CompactComponent) {
                                    return (
                                        <widget.CompactComponent
                                            baseNotchWidth={baseNotchWidth}
                                            isHovered={isHovered}
                                            contentOpacity={contentOpacity}
                                        />
                                    );
                                }
                                return null;
                            })()
                        )}
                    </motion.div>
                </AnimatePresence>

                {/* Mode indicator dots - only show when not expanded */}
                {!expanded && (
                    <ModeIndicator
                        availableModes={availableModes}
                        currentMode={mode}
                        onModeChange={setPreferredModeId}
                    />
                )}
            </motion.div >
        </div >
    );
}
