/**
 * Widget Configuration - Centralized widget declarations
 *
 * This file provides:
 * 1. A list of built-in widgets with dynamic imports
 * 2. A function to load all widgets (built-in + external plugins)
 */

import { loadExternalPlugins } from '../../services/pluginLoader';

export interface WidgetConfigEntry {
    id: string;
    module: () => Promise<unknown>;
}

/**
 * List of built-in widgets to load
 * Each entry's module() will trigger the widget's self-registration
 */
export const builtinWidgets: WidgetConfigEntry[] = [
    { id: 'calendar', module: () => import('./CalendarWidget') },
    { id: 'reminders', module: () => import('./RemindersWidget') },
    { id: 'timer', module: () => import('./TimerWidget') },
    { id: 'session', module: () => import('./SessionWidget') },
    { id: 'mirror', module: () => import('./MirrorWidget') },
    { id: 'speedtest', module: () => import('./SpeedTestWidget') },
];

/**
 * Load all built-in widgets
 * This triggers each widget's registerWidget() call
 */
export async function loadBuiltinWidgets(): Promise<void> {
    await Promise.all(builtinWidgets.map(w => w.module()));
}

/**
 * Load all widgets (built-in + external plugins)
 * This is the main entry point for widget initialization
 */
export async function loadAllWidgets(): Promise<void> {
    // Load built-in widgets first
    await loadBuiltinWidgets();

    // Then load external plugins from ~/.opennook/plugins/
    await loadExternalPlugins();
}
