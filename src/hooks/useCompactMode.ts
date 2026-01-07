import { useState, useCallback, useMemo, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';

export type CompactMode = 'media' | 'files' | 'onboard' | 'idle';

interface CompactModeState {
    hasMedia: boolean;
    hasFiles: boolean;
    isInitialLaunch: boolean;
}

interface UseCompactModeOptions {
    /** Initial launch duration in ms (default: 10000) */
    initialLaunchDuration?: number;
}

interface UseCompactModeResult {
    /** Current active mode */
    mode: CompactMode;
    /** Available modes based on current context */
    availableModes: CompactMode[];
    /** User's preferred mode (overrides automatic selection until context changes) */
    preferredMode: CompactMode | null;
    /** Cycle to next/prev available mode */
    cycleMode: (direction: 'next' | 'prev') => void;
    /** Whether we're in the initial launch period */
    isInitialLaunch: boolean;
    /** Update context state */
    updateState: (state: Partial<CompactModeState>) => void;
}

/**
 * Centralized hook for compact mode selection logic.
 *
 * Priority order: Media (10) > Files (30) > Onboard (50) > Idle (100)
 * User switching overrides automatic selection until a new context becomes available.
 */
export function useCompactMode(options: UseCompactModeOptions = {}): UseCompactModeResult {
    const { initialLaunchDuration = 10000 } = options;

    const [state, setState] = useState<CompactModeState>({
        hasMedia: false,
        hasFiles: false,
        isInitialLaunch: true,
    });

    const [preferredMode, setPreferredMode] = useState<CompactMode | null>('onboard');
    const prevStateRef = useRef<CompactModeState>(state);

    // Initial launch timer
    useEffect(() => {
        const timer = setTimeout(() => {
            setState(prev => ({ ...prev, isInitialLaunch: false }));
        }, initialLaunchDuration);

        return () => clearTimeout(timer);
    }, [initialLaunchDuration]);

    // Compute available modes based on context
    const availableModes = useMemo<CompactMode[]>(() => {
        const modes: CompactMode[] = [];
        if (state.hasMedia) modes.push('media');
        if (state.hasFiles) modes.push('files');
        modes.push('onboard'); // Always available
        modes.push('idle');    // Always available as fallback
        return modes;
    }, [state.hasMedia, state.hasFiles]);

    // Determine current mode based on priority and user preference
    const mode = useMemo<CompactMode>(() => {
        // If user has selected a mode and it's still available, use it
        if (preferredMode && availableModes.includes(preferredMode)) {
            return preferredMode;
        }
        // Default priority: Media > Files > Onboard > Idle
        if (availableModes.includes('media')) return 'media';
        if (availableModes.includes('files')) return 'files';
        if (availableModes.includes('onboard')) return 'onboard';
        return 'idle';
    }, [availableModes, preferredMode]);

    // Auto-switch when new context becomes available (but not during initial launch)
    useEffect(() => {
        const prevState = prevStateRef.current;

        // Don't auto-switch during initial launch
        if (state.isInitialLaunch) {
            prevStateRef.current = state;
            return;
        }

        // Media started playing - switch to it (reset user preference)
        if (!prevState.hasMedia && state.hasMedia) {
            setPreferredMode('media');
        }
        // Files were added - switch to them (but only if not playing media)
        else if (!prevState.hasFiles && state.hasFiles && !state.hasMedia) {
            setPreferredMode('files');
        }

        prevStateRef.current = state;
    }, [state]);

    // Cycle through available modes (user override)
    const cycleMode = useCallback((direction: 'next' | 'prev') => {
        if (availableModes.length <= 1) return;

        const currentIndex = availableModes.indexOf(mode);
        let nextIndex: number;

        if (direction === 'next') {
            nextIndex = (currentIndex + 1) % availableModes.length;
        } else {
            nextIndex = (currentIndex - 1 + availableModes.length) % availableModes.length;
        }

        const newMode = availableModes[nextIndex];
        setPreferredMode(newMode);
        invoke('trigger_haptics').catch(console.error);
    }, [availableModes, mode]);

    // Update state helper
    const updateState = useCallback((partial: Partial<CompactModeState>) => {
        setState(prev => ({ ...prev, ...partial }));
    }, []);

    return {
        mode,
        availableModes,
        preferredMode,
        cycleMode,
        isInitialLaunch: state.isInitialLaunch,
        updateState,
    };
}
