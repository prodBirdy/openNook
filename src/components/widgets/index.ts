/**
 * Widget Index - Initialize widgets and export registry
 * This file should be imported once at app startup.
 */

import { loadBuiltinWidgets } from './widgetConfig';

// Load built-in widgets via dynamic imports
// This promise resolves when all widgets are registered
export const widgetsReady = loadBuiltinWidgets();

// Re-export registry and types for convenience
export { WidgetRegistry, registerWidget, unregisterWidget } from './WidgetRegistry';
export type { WidgetManifest, CompactWidgetProps, WidgetEnabledState, WidgetInstanceState } from './WidgetTypes';
export { WidgetWrapper } from './WidgetWrapper';
export { WidgetAddDialog } from './WidgetAddDialog';
