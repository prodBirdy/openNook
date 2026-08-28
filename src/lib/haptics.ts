import { invoke } from '@tauri-apps/api/core';

/**
 * Available haptic feedback patterns
 */
export type HapticPattern =
    | 'generic'      // General purpose haptic
    | 'alignment'    // Subtle haptic for alignment
    | 'levelChange'  // Strong haptic for level changes
    | 'light'        // Light tap
    | 'medium'       // Medium tap (default)
    | 'heavy'        // Heavy impact
    | 'selection'    // Quick selection feedback
    | 'success'      // Double tap for success
    | 'error';       // Triple tap for errors

/**
 * Haptic configuration options
 */
export interface HapticConfig {
    pattern: HapticPattern;
    intensity?: number; // 0.0 - 1.0
}

/**
 * Trigger haptic feedback on macOS
 *
 * @param config - Haptic configuration or pattern name
 * @returns Promise that resolves when haptic is triggered
 *
 * @example
 * ```ts
 * // Simple usage with default pattern
 * await triggerHaptic();
 *
 * // With specific pattern
 * await triggerHaptic('light');
 *
 * // With full configuration
 * await triggerHaptic({ pattern: 'generic', intensity: 0.8 });
 * ```
 */
export async function triggerHaptic(
    config?: HapticPattern | HapticConfig
): Promise<void> {
    try {
        if (!config) {
            // Default medium pattern
            await invoke('trigger_haptics');
        } else if (typeof config === 'string') {
            // Pattern name only
            await invoke('trigger_haptics', { config: { pattern: config } });
        } else {
            // Full configuration
            await invoke('trigger_haptics', { config });
        }
    } catch (error) {
        // Silently fail on platforms without haptic support
        console.debug('Haptic feedback not supported:', error);
    }
}

/**
 * React hook for haptic feedback
 *
 * @returns Object with trigger function
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { trigger } = useHaptics();
 *
 *   return (
 *     <button onClick={() => trigger('selection')}>
 *       Click Me
 *     </button>
 *   );
 * }
 * ```
 */
export function useHaptics() {
    const trigger = async (config?: HapticPattern | HapticConfig) => {
        await triggerHaptic(config);
    };

    return { trigger };
}

/**
 * Predefined haptic feedback helpers for common use cases
 */
export const Haptics = {
    /** Light tap - for hover states, minor actions */
    light: () => triggerHaptic('light'),

    /** Medium tap - for standard clicks, confirmations */
    medium: () => triggerHaptic('medium'),

    /** Heavy impact - for important actions */
    heavy: () => triggerHaptic('heavy'),

    /** Quick selection feedback */
    selection: () => triggerHaptic('selection'),

    /** Success pattern (double tap) */
    success: () => triggerHaptic('success'),

    /** Error pattern (triple tap) */
    error: () => triggerHaptic('error'),

    /** Alignment/snapping feedback */
    alignment: () => triggerHaptic('alignment'),

    /** Level change feedback */
    levelChange: () => triggerHaptic('levelChange'),
};
