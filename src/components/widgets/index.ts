/**
 * Widget Index - Import all widgets to trigger their registration.
 * This file should be imported once at app startup.
 */

// Built-in widgets
import './CalendarWidget';
import './RemindersWidget';
import './TimerWidget';
import './SessionWidget';

// Re-export registry and types for convenience
export { WidgetRegistry, registerWidget, unregisterWidget } from './WidgetRegistry';
export type { WidgetManifest, CompactWidgetProps, WidgetEnabledState, WidgetInstanceState } from './WidgetTypes';
