/**
 * Widget Configuration - Centralized widget declarations
 *
 * This file provides:
 * 1. A list of built-in widgets with dynamic imports
 * 2. A function to load all widgets and register them
 */

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
];

/**
 * Load all built-in widgets
 * This triggers each widget's registerWidget() call
 */
export async function loadBuiltinWidgets(): Promise<void> {
    await Promise.all(builtinWidgets.map(w => w.module()));
}
