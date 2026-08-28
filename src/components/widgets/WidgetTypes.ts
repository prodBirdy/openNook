import { ComponentType } from 'react';

/**
 * Props passed to compact widget components
 */
export interface CompactWidgetProps {
    baseNotchWidth: number;
    isHovered: boolean;
    contentOpacity?: number;
}

/**
 * Widget manifest defines a widget's metadata and components
 */
export interface WidgetManifest {
    /** Unique identifier for the widget */
    id: string;
    /** Display name shown in UI */
    name: string;
    /** Short description for settings */
    description: string;
    /** Icon component (from @tabler/icons-react) */
    icon: ComponentType<{ size?: number; color?: string }>;
    /** Main widget component for expanded view */
    ExpandedComponent: ComponentType;
    /** Optional compact view component */
    CompactComponent?: ComponentType<CompactWidgetProps>;
    /** Whether widget is enabled by default */
    defaultEnabled: boolean;
    /** Widget category for grouping in settings */
    category: 'productivity' | 'media' | 'utility';
    /** Minimum width in pixels for expanded view */
    minWidth?: number;
    /** Whether this widget can show in compact mode (requires CompactComponent) */
    hasCompactMode: boolean;
    /** Priority for compact mode display (lower = higher priority) */
    compactPriority?: number;
}

/**
 * State for a widget instance (for widgets that support multiple instances like timers)
 */
export interface WidgetInstanceState {
    id: string;
    widgetId: string;
    isActive: boolean;
    data: Record<string, unknown>;
    createdAt: number;
}

/**
 * Widget enabled state stored in localStorage
 */
export interface WidgetEnabledState {
    [widgetId: string]: boolean;
}
