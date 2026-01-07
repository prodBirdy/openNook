import { motion, AnimatePresence } from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useState, useCallback, useRef, useMemo } from 'react';
import { useNotchInfo } from '../hooks/useNotchInfo';
import './DynamicIsland.css';
import { CompactMedia } from './island/CompactMedia';
import { CompactFiles } from './island/CompactFiles';
import { CompactIdle } from './island/CompactIdle';
import { CompactOnboard } from './island/CompactOnboard';
import { ModeIndicator } from './island/ModeIndicator';
import { ExpandedIsland } from './island/ExpandedIsland';
import { useWidgets } from '../context/WidgetContext';
import { useTimerContext } from '../context/TimerContext';
import { useSessionContext } from '../context/SessionContext';

import { useFileTray } from '../hooks/useFileTray';
import { useMediaPlayer } from '../hooks/useMediaPlayer';

// artworkColorCache moved to useMediaPlayer

export function DynamicIsland() {
    const { notchInfo } = useNotchInfo();
    const {
        enabledCompactWidgets,
        hasActiveInstance,
    } = useWidgets();

    const { timers } = useTimerContext();
    const { sessions } = useSessionContext();
    // Mode management
    const [preferredModeId, setPreferredModeId] = useState<string | null>(null);
    const [isInitialLaunch, setIsInitialLaunch] = useState(true);

    // State
    const [isHovered, setIsHovered] = useState(false);
    const [expanded, setExpanded] = useState(false);
    const [isAnimating, setIsAnimating] = useState(false);
    const [notes, setNotes] = useState('');
    const [activeTab, setActiveTab] = useState<'widgets' | 'files'>('widgets');
    const [windowSize, setWindowSize] = useState({ width: window.innerWidth, height: window.innerHeight });

    const islandRef = useRef<HTMLDivElement>(null);
    const notesTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const [settings, setSettings] = useState({
        showCalendar: false,
        showReminders: false,
        showMedia: true,
        baseWidth: 160,
        baseHeight: 38,
        liquidGlassMode: false,
    });
    const lastModeCycleRef = useRef<number>(0);
    const prevHasMediaRef = useRef<boolean>(false);
    const prevHasFilesRef = useRef<boolean>(false);

    // Extracted Hooks
    const {
        nowPlaying,
        visualizerColor,
        hasMedia,
        handlePlayPause,
        handleNextTrack,
        handlePreviousTrack,
        handleSeek
    } = useMediaPlayer(expanded, settings.showMedia);

    const { files, setFiles } = useFileTray(setExpanded, setActiveTab, setIsAnimating);

    // Determine mode - memoized
    const hasFiles = files.length > 0;

    // System modes that aren't strict widgets but behave like one
    const availableModes = useMemo(() => {
        const modes: { id: string, priority: number }[] = [];

        // 1. Add enabled compact widgets
        enabledCompactWidgets.forEach(widget => {
            // Check if widget needs to be "active" to show (like timer)
            // or if it should always show when enabled (like weather maybe?)

            // For now, check if legacy Timer/Session context has active items
            // This is a bridge until all state moves to WidgetContext completely
            let isActive = false;

            if (widget.id === 'timer') {
                isActive = hasActiveInstance('timer') || timers.some(t => t.isRunning);
            } else if (widget.id === 'session') {
                isActive = hasActiveInstance('session') || sessions.some(s => s.isActive);
            } else {
                // Default to showing if enabled
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

        modes.push({ id: 'onboard', priority: 200 }); // Always include onboard
        modes.push({ id: 'idle', priority: 999 });    // Always include idle

        // Sort by priority (lower is better)
        return modes.sort((a, b) => a.priority - b.priority).map(m => m.id);
    }, [enabledCompactWidgets, hasMedia, hasFiles, hasActiveInstance, timers, sessions]);

    const activeModeId = useMemo(() => {
        if (preferredModeId && availableModes.includes(preferredModeId)) return preferredModeId;

        // If preferred mode is not available, pick the highest priority available mode
        // (which is the first one in the sorted availableModes array)
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
    }, [availableModes, activeModeId]);

    // Initial launch: keep onboard mode for 10 seconds
    useEffect(() => {
        const timer = setTimeout(() => {
            setIsInitialLaunch(false);
        }, 10000); // 10 seconds

        return () => clearTimeout(timer);
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

    // Initialization and listeners
    useEffect(() => {
        // Load notes
        invoke<string>('load_notes')
            .then(setNotes)
            .catch(err => console.error('Failed to load notes:', err));

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
    }, []);

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
            setPreferredModeId('media');
        }
        // Files were added - switch to them (but only if not playing media)
        else if (!prevFiles && hasFiles && !hasMedia) {
            setPreferredModeId('files');
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
                // media, files, onboard and most widgets have similar hover dimensions
                // Ideally this should come from widget config
                width = baseNotchWidth + 125;
                height = notchHeight + 15;
            }
        } else if (mode === 'idle') {
            // idle is smaller
            width = baseNotchWidth;
            height = notchHeight;
        } else {
            // Standard expanded width for active widgets
            width = baseNotchWidth + 120;
            height = notchHeight;
        }

        return { targetWidth: width, targetHeight: height };
    }, [expanded, isHovered, mode, baseNotchWidth, notchHeight, windowSize.width, windowSize.height]);

    const contentOpacity = (mode === 'idle' || mode === 'onboard' || hasMedia || hasFiles || mode !== 'idle') ? 1 : 0;



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
                    />
                )}
            </motion.div >
        </div >
    );
}
