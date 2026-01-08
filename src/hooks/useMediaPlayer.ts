import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { NowPlayingData } from '../components/island/types';
import { getDominantColor } from '../utils/imageUtils';

// Cache for artwork colors to avoid re-processing
const artworkColorCache = new Map<string, string | null>();

export function useMediaPlayer(expanded: boolean, showMediaSetting: boolean = true) {
    const [nowPlaying, setNowPlaying] = useState<NowPlayingData | null>(null);
    const [visualizerColor, setVisualizerColor] = useState<string | null>(null);
    const [mediaPersist, setMediaPersist] = useState(false);

    // Refs for stable callbacks and state tracking
    const mediaPersistTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const prevIsPlayingRef = useRef<boolean>(false);
    const lastArtworkRef = useRef<string | null | undefined>(null);
    const fetchTimeoutRefs = useRef<ReturnType<typeof setTimeout>[]>([]);
    const expandedRef = useRef(expanded);

    // Keep ref in sync
    useEffect(() => {
        expandedRef.current = expanded;
    }, [expanded]);

    // Clear timeouts helper
    const clearFetchTimeouts = useCallback(() => {
        fetchTimeoutRefs.current.forEach(clearTimeout);
        fetchTimeoutRefs.current = [];
    }, []);

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
                if (prev && !trackChanged && !expandedRef.current) {
                    if (prev.duration === data.duration && prev.album === data.album) {
                        return prev;
                    }
                }

                // Optimization: Reuse artwork string reference
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

    // Schedule fetch helper
    const scheduleFetch = useCallback((delays: number[]) => {
        clearFetchTimeouts();
        delays.forEach(delay => {
            const timeout = setTimeout(fetchNowPlaying, delay);
            fetchTimeoutRefs.current.push(timeout);
        });
    }, [fetchNowPlaying, clearFetchTimeouts]);

    // Media persistence logic
    useEffect(() => {
        const isPlaying = nowPlaying?.is_playing ?? false;
        const wasPlaying = prevIsPlayingRef.current;
        prevIsPlayingRef.current = isPlaying;

        if (wasPlaying && !isPlaying) {
            // Just paused - enable persistence
            setMediaPersist(true);
            if (mediaPersistTimeoutRef.current) clearTimeout(mediaPersistTimeoutRef.current);
            mediaPersistTimeoutRef.current = setTimeout(() => {
                setMediaPersist(false);
            }, 10000); // 10 seconds
        } else if (isPlaying) {
            // Started playing - clear persistence
            setMediaPersist(false);
            if (mediaPersistTimeoutRef.current) {
                clearTimeout(mediaPersistTimeoutRef.current);
                mediaPersistTimeoutRef.current = null;
            }
        }
    }, [nowPlaying?.is_playing]);

    // Media controls
    const handlePlayPause = useCallback(async (e?: React.MouseEvent) => {
        if (e) e.stopPropagation();
        setNowPlaying((prev) => (prev ? { ...prev, is_playing: !prev.is_playing } : null));
        try {
            await invoke('media_play_pause');
            scheduleFetch([100, 300]);
        } catch (err) {
            console.error('Failed to toggle play/pause:', err);
        }
    }, [scheduleFetch]);

    const handleNextTrack = useCallback(async (e?: React.MouseEvent) => {
        if (e) e.stopPropagation();
        try {
            await invoke('media_next_track');
            scheduleFetch([100, 400, 800]);
        } catch (err) {
            console.error('Failed to skip to next track:', err);
        }
    }, [scheduleFetch]);

    const handlePreviousTrack = useCallback(async (e?: React.MouseEvent) => {
        if (e) e.stopPropagation();
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
            setNowPlaying((prev) => (prev ? { ...prev, elapsed_time: position } : null));
            scheduleFetch([100, 400, 800]);
        } catch (err) {
            console.error('Failed to seek:', err);
        }
    }, [scheduleFetch]);

    // Update visualizer color
    useEffect(() => {
        const artwork = nowPlaying?.artwork_base64;
        if (artwork === lastArtworkRef.current) return;
        lastArtworkRef.current = artwork;

        if (!artwork) {
            setVisualizerColor(null);
            return;
        }

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

    // Initialization and key listeners
    useEffect(() => {
        fetchNowPlaying();
        const trackInterval = setInterval(fetchNowPlaying, 1000);

        const handleKeyDown = (e: KeyboardEvent) => {
            const mediaKeys = ['MediaPlayPause', 'MediaTrackNext', 'MediaTrackPrevious'];
            const fnKeys = ['F7', 'F8', 'F9'];

            if (mediaKeys.includes(e.key) || fnKeys.includes(e.key)) {
                setTimeout(fetchNowPlaying, 150);
                setTimeout(fetchNowPlaying, 500);
            }
        };
        window.addEventListener('keydown', handleKeyDown);

        return () => {
            clearInterval(trackInterval);
            window.removeEventListener('keydown', handleKeyDown);
        };
    }, [fetchNowPlaying]);

    const hasMedia = !!(nowPlaying && (nowPlaying.is_playing || mediaPersist) && showMediaSetting);

    return {
        nowPlaying,
        setNowPlaying, // Exposed for optimistic updates if needed elsewhere
        visualizerColor,
        hasMedia,
        handlePlayPause,
        handleNextTrack,
        handlePreviousTrack,
        handleSeek,
        fetchNowPlaying
    };
}
