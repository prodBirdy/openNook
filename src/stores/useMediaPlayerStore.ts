import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { NowPlayingData } from '../components/island/types';
import { getDominantColor } from '../utils/imageUtils';

// Cache for artwork colors to avoid re-processing
const artworkColorCache = new Map<string, string | null>();

interface MediaPlayerState {
    nowPlaying: NowPlayingData | null;
    visualizerColor: string | null;
    mediaPersist: boolean;
}

interface MediaPlayerActions {
    setNowPlaying: (data: NowPlayingData | null) => void;
    setVisualizerColor: (color: string | null) => void;
    setMediaPersist: (persist: boolean) => void;
    fetchNowPlaying: (expanded?: boolean) => Promise<void>;
    handlePlayPause: (e?: React.MouseEvent) => Promise<void>;
    handleNextTrack: (e?: React.MouseEvent) => Promise<void>;
    handlePreviousTrack: (e?: React.MouseEvent) => Promise<void>;
    handleSeek: (position: number) => Promise<void>;
    updateVisualizerColor: (artwork: string | null | undefined) => Promise<void>;
}

interface MediaPlayerDerived {
    hasMedia: (showMediaSetting: boolean) => boolean;
}

type MediaPlayerStore = MediaPlayerState & MediaPlayerActions & MediaPlayerDerived;

let mediaPersistTimeout: ReturnType<typeof setTimeout> | null = null;
let prevIsPlaying = false;
let lastArtwork: string | null | undefined = null;
let expandedRef = false;

export const useMediaPlayerStore = create<MediaPlayerStore>((set, get) => ({
    nowPlaying: null,
    visualizerColor: null,
    mediaPersist: false,

    hasMedia: (showMediaSetting) => {
        const { nowPlaying, mediaPersist } = get();
        return !!(nowPlaying && (nowPlaying.is_playing || mediaPersist) && showMediaSetting);
    },

    setNowPlaying: (data) => set({ nowPlaying: data }),
    setVisualizerColor: (color) => set({ visualizerColor: color }),
    setMediaPersist: (persist) => set({ mediaPersist: persist }),

    fetchNowPlaying: async (expanded = false) => {
        expandedRef = expanded;
        try {
            const data = await invoke<NowPlayingData>('get_now_playing');
            const { nowPlaying: prev } = get();

            const trackChanged = !prev ||
                prev.title !== data.title ||
                prev.artist !== data.artist ||
                prev.is_playing !== data.is_playing;

            // Optimization: If only elapsed time changed and we are NOT expanded,
            // do NOT update state to prevent re-renders.
            if (prev && !trackChanged && !expandedRef) {
                if (prev.duration === data.duration && prev.album === data.album) {
                    return;
                }
            }

            // Optimization: Reuse artwork string reference
            let artwork = data.artwork_base64;
            if (prev && data.title === prev.title && data.artist === prev.artist && prev.artwork_base64) {
                artwork = prev.artwork_base64;
            }

            const newData = {
                ...data,
                artwork_base64: artwork,
            };

            set({ nowPlaying: newData });

            // Handle play state changes for persistence
            const isPlaying = newData.is_playing;
            const wasPlaying = prevIsPlaying;
            prevIsPlaying = isPlaying;

            if (wasPlaying && !isPlaying) {
                // Just paused - enable persistence
                set({ mediaPersist: true });
                if (mediaPersistTimeout) clearTimeout(mediaPersistTimeout);
                mediaPersistTimeout = setTimeout(() => {
                    set({ mediaPersist: false });
                }, 10000);
            } else if (isPlaying) {
                // Started playing - clear persistence
                set({ mediaPersist: false });
                if (mediaPersistTimeout) {
                    clearTimeout(mediaPersistTimeout);
                    mediaPersistTimeout = null;
                }
            }

            // Update visualizer color if artwork changed
            if (artwork !== lastArtwork) {
                lastArtwork = artwork;
                get().updateVisualizerColor(artwork);
            }
        } catch (error) {
            console.error('Failed to fetch now playing:', error);
        }
    },

    handlePlayPause: async (e) => {
        if (e) e.stopPropagation();
        const { nowPlaying } = get();
        set({ nowPlaying: nowPlaying ? { ...nowPlaying, is_playing: !nowPlaying.is_playing } : null });
        try {
            await invoke('media_play_pause');
            const { fetchNowPlaying } = get();
            setTimeout(() => fetchNowPlaying(), 100);
            setTimeout(() => fetchNowPlaying(), 300);
        } catch (err) {
            console.error('Failed to toggle play/pause:', err);
        }
    },

    handleNextTrack: async (e) => {
        if (e) e.stopPropagation();
        try {
            await invoke('media_next_track');
            const { fetchNowPlaying } = get();
            setTimeout(() => fetchNowPlaying(), 100);
            setTimeout(() => fetchNowPlaying(), 400);
            setTimeout(() => fetchNowPlaying(), 800);
        } catch (err) {
            console.error('Failed to skip to next track:', err);
        }
    },

    handlePreviousTrack: async (e) => {
        if (e) e.stopPropagation();
        try {
            await invoke('media_previous_track');
            const { fetchNowPlaying } = get();
            setTimeout(() => fetchNowPlaying(), 100);
            setTimeout(() => fetchNowPlaying(), 400);
            setTimeout(() => fetchNowPlaying(), 800);
        } catch (err) {
            console.error('Failed to go to previous track:', err);
        }
    },

    handleSeek: async (position) => {
        try {
            await invoke('media_seek', { position });
            const { nowPlaying } = get();
            set({ nowPlaying: nowPlaying ? { ...nowPlaying, elapsed_time: position } : null });
            const { fetchNowPlaying } = get();
            setTimeout(() => fetchNowPlaying(), 100);
            setTimeout(() => fetchNowPlaying(), 400);
            setTimeout(() => fetchNowPlaying(), 800);
        } catch (err) {
            console.error('Failed to seek:', err);
        }
    },

    updateVisualizerColor: async (artwork) => {
        if (!artwork) {
            set({ visualizerColor: null });
            return;
        }

        if (artworkColorCache.has(artwork)) {
            set({ visualizerColor: artworkColorCache.get(artwork) ?? null });
            return;
        }

        const src = `data:image/png;base64,${artwork}`;
        const rgb = await getDominantColor(src);
        const color = rgb ? `rgb(${rgb[0]}, ${rgb[1]}, ${rgb[2]})` : null;
        artworkColorCache.set(artwork, color);

        // Limit cache size
        if (artworkColorCache.size > 50) {
            const firstKey = artworkColorCache.keys().next().value;
            if (firstKey) artworkColorCache.delete(firstKey);
        }

        set({ visualizerColor: color });
    }
}));

// Selectors
export const selectNowPlaying = (state: MediaPlayerStore) => state.nowPlaying;
export const selectVisualizerColor = (state: MediaPlayerStore) => state.visualizerColor;
