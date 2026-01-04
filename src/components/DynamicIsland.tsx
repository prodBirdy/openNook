import { motion, AnimatePresence } from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useState, useCallback } from 'react';
import { useNotification } from '../context/NotificationContext';
import { useNotchInfo } from '../hooks/useNotchInfo';
import './DynamicIsland.css';

interface NowPlayingData {
    title: string | null;
    artist: string | null;
    album: string | null;
    artwork_base64: string | null;
    duration: number | null;
    elapsed_time: number | null;
    is_playing: boolean;
    audio_levels: number[] | null;
}

function AudioVisualizer({ isPlaying, audioLevels }: { isPlaying: boolean; audioLevels: number[] | null }) {
    // 6 bars for the visualizer
    const barCount = 6;

    // Use real audio levels if available, otherwise default to low values
    const levels = audioLevels && audioLevels.length >= barCount
        ? audioLevels.slice(0, barCount)
        : Array(barCount).fill(0.15);

    return (
        <div className="audio-visualizer">
            {levels.map((level, i) => (
                <motion.div
                    key={i}
                    className="visualizer-bar"
                    animate={{
                        scaleY: isPlaying ? Math.max(0.15, Math.min(1, level)) : 0.15,
                    }}
                    transition={{
                        type: 'tween',
                        ease: [0.25, 0.1, 0.25, 1], // Custom cubic bezier for smoother motion
                        duration: 0.2, // Longer interpolation (200ms) for smoother transitions
                    }}
                />
            ))}
        </div>
    );
}

function AlbumCover({ artwork, title, isPlaying }: {
    artwork: string | null;
    title: string | null;
    isPlaying: boolean;
}) {
    return (
        <motion.div
            className="album-cover"
            animate={{
                scale: isPlaying ? 1 : 0.9,
                opacity: isPlaying ? 1 : 0.7,
            }}
            transition={{
                type: 'spring',
                stiffness: 300,
                damping: 25,
            }}
        >
            {artwork ? (
                <img
                    src={`data:image/png;base64,${artwork}`}
                    alt={title || 'Album cover'}
                    className="album-cover__image"
                />
            ) : (
                <div className="album-cover__placeholder">
                    <span className="album-cover__icon"></span>
                </div>
            )}
        </motion.div>
    );
}

export function DynamicIsland() {
    const { notification, dismissNotification } = useNotification();
    const { notchInfo } = useNotchInfo();
    const [nowPlaying, setNowPlaying] = useState<NowPlayingData | null>(null);
    const [audioLevels, setAudioLevels] = useState<number[] | null>(null);
    const [isHovered, setIsHovered] = useState(false);

    // Determine the current state logic
    // Priority: Notification > Media > Idle
    // User request: "only be there when there is something playing"
    const hasMedia = !!(nowPlaying && nowPlaying.is_playing);

    const mode: 'notification' | 'media' | 'idle' = notification
        ? 'notification'
        : hasMedia
            ? 'media'
            : 'idle';

    // Fetch media info - only update when track changes
    const fetchNowPlaying = useCallback(async () => {
        try {
            const data = await invoke<NowPlayingData>('get_now_playing');

            // Update state, preserving artwork if not provided (track hasn't changed)
            setNowPlaying(prev => {
                const trackChanged = !prev ||
                    prev.title !== data.title ||
                    prev.artist !== data.artist ||
                    prev.is_playing !== data.is_playing;

                if (trackChanged && data.is_playing) {
                    console.log('🎵 Now Playing:', {
                        title: data.title,
                        artist: data.artist,
                        album: data.album,
                        hasArtwork: !!data.artwork_base64,
                    });
                }

                // If artwork is not provided but we have previous artwork and track hasn't changed,
                // preserve the previous artwork
                let artwork = data.artwork_base64;
                if (!artwork && prev && !trackChanged && prev.artwork_base64) {
                    artwork = prev.artwork_base64;
                }

                // Always update to get latest audio levels and elapsed time
                return {
                    ...data,
                    artwork_base64: artwork,
                };
            });
        } catch (error) {
            console.error('Failed to fetch now playing:', error);
        }
    }, []);

    useEffect(() => {
        // Initial fetch
        fetchNowPlaying();

        // Poll every 3 seconds for track info (less frequent)
        const trackInterval = setInterval(fetchNowPlaying, 3000);

        // Listen to audio levels stream from Rust (emitted at ~60fps)
        const unlistenAudioLevels = listen<number[]>('audio-levels-update', (event) => {
            const levels = event.payload;
            // Always update levels, even if nowPlaying isn't populated yet
            setAudioLevels(levels);
            setNowPlaying(prev => {
                if (!prev) return prev;
                return {
                    ...prev,
                    audio_levels: levels,
                };
            });
        });

        return () => {
            clearInterval(trackInterval);
            unlistenAudioLevels.then(fn => fn());
        };
    }, [fetchNowPlaying]);

    // Handle mouse events for notch detection - Rust controls window activation/click-through
    useEffect(() => {
        console.log('Setting up mouse event listeners...');

        const unlistenEnter = listen('mouse-entered-notch', () => {
            setIsHovered(true);
        });

        const unlistenExit = listen('mouse-exited-notch', () => {
            setIsHovered(false);
        });

        return () => {
            unlistenEnter.then(fn => fn());
            unlistenExit.then(fn => fn());
        };
    }, []);

    // Click-through is managed by mouse enter/exit events above
    // When not hovered, clicks pass through to underlying windows

    const notchHeight = notchInfo?.notch_height ? notchInfo.notch_height - 40 : 30; // base notch height
    // We adjust the notch width for sizing calculations
    const baseNotchWidth = notchInfo?.notch_width ? notchInfo.notch_width - 40 : 160;

    // Calculate dimensions based on mode and hover state
    let targetWidth = baseNotchWidth;
    let targetHeight = notchHeight;

    // Content Opacity Logic:
    // Show content when music is playing, fade slightly when not hovered
    const contentOpacity = hasMedia ? (isHovered ? 1 : 0.8) : 0;

    // If not hovered, stays at baseNotchWidth/notchHeight
    if (mode === 'media') {
        // Media Mode - Rust controls window size, but we need to match it for content layout
        // baseNotchWidth = notch_width - 40
        // Not hovered: notch_width + 120 = baseNotchWidth + 160
        // Hovered: notch_width + 400 = baseNotchWidth + 440
        if (isHovered) {
            targetWidth = baseNotchWidth + 440; // Match Rust hovered width (notch_width + 400)
            targetHeight = notchHeight + 80; // Match Rust hovered height
        } else {
            // When not hovered, minimum width to show album cover and visualizer
            targetWidth = baseNotchWidth + 160; // Match Rust not-hovered width (notch_width + 120)
            targetHeight = notchHeight;
        }
    }

    // Debug: log hover and mode state changes
    useEffect(() => {
        console.log('📊 State:', { isHovered, mode, hasMedia, targetWidth, targetHeight, baseNotchWidth });
    }, [isHovered, mode, hasMedia, targetWidth, targetHeight, baseNotchWidth]);

    // For media, we might want to ensure height accommodates the content when expanded
    // But user wants "init state where it is smaller".
    // Let's ensure the CSS handles the border-radius as requested (0 0 20px 20px).

    return (
        <div className="dynamic-island-container">
            <motion.div
                className={`dynamic-island ${mode}`}
                initial={false}
                animate={{
                    width: targetWidth,
                    height: targetHeight,
                }}
                transition={{
                    type: 'spring',
                    stiffness: 400,
                    damping: 30,
                    mass: 0.8
                }}
                onHoverStart={() => {
                    // Window activation is handled by Rust via mouse monitoring
                    if (mode === 'notification') {
                        invoke('trigger_haptics').catch(console.error);
                    }
                }}
                onHoverEnd={() => {
                    // Window deactivation is handled by Rust via mouse monitoring
                }}
                onClick={() => {
                    // Allow clicking to dismiss notification only if accessible (hovered)
                    if (mode === 'notification' && isHovered) dismissNotification();
                }}
                style={{
                    // Enforce the border radius user wanted via inline style
                    borderRadius: '0px 0px 20px 20px'
                }}
            >
                {/* Content Rendering - Fades in on hover */}
                <AnimatePresence mode="wait">
                    {mode === 'media' && nowPlaying && (
                        <motion.div
                            key="media-content"
                            className="island-content media-content"
                            initial={{ opacity: 0 }}
                            animate={{ opacity: contentOpacity }}
                            transition={{ duration: 0.3 }}
                            style={{ pointerEvents: isHovered ? 'auto' : 'none' }}
                        >
                            <div className="media-left" style={{ width: 60, height: '100%' }}>
                                <AlbumCover
                                    artwork={nowPlaying.artwork_base64}
                                    title={nowPlaying.title}
                                    isPlaying={nowPlaying.is_playing}
                                />
                            </div>

                            {/* Spacer for the physical notch */}
                            <div className="media-spacer" style={{ width: baseNotchWidth }} />

                            <div className="media-right" style={{ width: 60, height: '100%' }}>
                                <AudioVisualizer
                                    isPlaying={nowPlaying.is_playing}
                                    audioLevels={audioLevels ?? nowPlaying.audio_levels}
                                />
                            </div>
                        </motion.div>
                    )}
                </AnimatePresence>
            </motion.div>
        </div>
    );
}
