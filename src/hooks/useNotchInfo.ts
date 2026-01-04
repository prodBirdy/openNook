import { invoke } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';

/**
 * Information about the macOS notch and screen dimensions
 */
export interface NotchInfo {
    /** Whether the screen has a notch (safeAreaInsets.top > 0) */
    has_notch: boolean;
    /** Height of the notch/safe area inset from the top (typically 30-40px on notched MacBooks) */
    notch_height: number;
    /** Width of the notch (the black area at the top center) */
    notch_width: number;
    /** Full screen width in points */
    screen_width: number;
    /** Full screen height in points */
    screen_height: number;
    /** The visible (usable) height below the notch */
    visible_height: number;
}

/**
 * Get notch information from the Tauri backend
 * Uses NSScreen.safeAreaInsets on macOS 12.0+
 */
export async function getNotchInfo(): Promise<NotchInfo> {
    return invoke<NotchInfo>('get_notch_info');
}

/**
 * Position the window at the notch (centered at top of screen)
 */
export async function positionAtNotch(): Promise<void> {
    return invoke('position_at_notch');

}

/**
 * Resize and position the window to fit at the notch area
 * @param width - Window width in logical pixels
 * @param height - Window height in logical pixels
 */
export async function fitToNotch(width: number, height: number): Promise<void> {
    return invoke('fit_to_notch', { width, height });
}

/**
 * Set whether the window should ignore mouse events (click-through)
 * When true, clicks pass through to the underlying application
 * @param ignore - Whether to ignore mouse events
 */
export async function setClickThrough(ignore: boolean): Promise<void> {
    return invoke('set_click_through', { ignore });
}

/**
 * Activate the window (focus it)
 */
export async function activateWindow(): Promise<void> {
    return invoke('activate_window');
}

/**
 * React hook to get notch information
 * Fetches notch info on mount and returns the current state
 */
export function useNotchInfo() {
    const [notchInfo, setNotchInfo] = useState<NotchInfo | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<Error | null>(null);

    useEffect(() => {
        let mounted = true;

        async function fetchNotchInfo() {
            try {
                const info = await getNotchInfo();
                if (mounted) {
                    setNotchInfo(info);
                    setLoading(false);
                }
            } catch (err) {
                if (mounted) {
                    setError(err instanceof Error ? err : new Error('Failed to get notch info'));
                    setLoading(false);
                }
            }
        }

        fetchNotchInfo();

        return () => {
            mounted = false;
        };
    }, []);

    return { notchInfo, loading, error, positionAtNotch, fitToNotch, setClickThrough, activateWindow };
}
