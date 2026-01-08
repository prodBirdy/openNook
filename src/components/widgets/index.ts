/**
 * Widget Index - Initialize widgets and export registry
 * This file should be imported once at app startup.
 */

import { loadAllWidgets } from './widgetConfig';

// Load all widgets (built-in + external plugins)
// This promise resolves when all widgets are registered
export const widgetsReady = loadAllWidgets();

// Re-export registry and types for convenience
export { WidgetRegistry, registerWidget, unregisterWidget } from './WidgetRegistry';
export type { WidgetManifest, CompactWidgetProps, WidgetEnabledState, WidgetInstanceState } from './WidgetTypes';
export { WidgetWrapper } from './WidgetWrapper';
export { WidgetAddDialog } from './WidgetAddDialog';
