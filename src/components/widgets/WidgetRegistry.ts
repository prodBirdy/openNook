import { WidgetManifest } from './WidgetTypes';

/**
 * Central registry for all available widgets.
 * Widgets register themselves on import.
 */
class WidgetRegistryClass {
    private widgets: Map<string, WidgetManifest> = new Map();

    /**
     * Register a widget with the system
     */
    register(manifest: WidgetManifest): void {
        if (this.widgets.has(manifest.id)) {
            console.warn(`Widget "${manifest.id}" is already registered. Overwriting.`);
        }
        this.widgets.set(manifest.id, manifest);
    }

    /**
     * Unregister a widget from the system
     */
    unregister(id: string): boolean {
        return this.widgets.delete(id);
    }

    /**
     * Get a widget by its ID
     */
    get(id: string): WidgetManifest | undefined {
        return this.widgets.get(id);
    }

    /**
     * Get all registered widgets
     */
    getAll(): WidgetManifest[] {
        return Array.from(this.widgets.values());
    }

    /**
     * Get widgets by category
     */
    getByCategory(category: WidgetManifest['category']): WidgetManifest[] {
        return this.getAll().filter(w => w.category === category);
    }

    /**
     * Get widgets that have compact mode
     */
    getCompactWidgets(): WidgetManifest[] {
        return this.getAll().filter(w => w.hasCompactMode && w.CompactComponent);
    }

    /**
     * Check if a widget is registered
     */
    has(id: string): boolean {
        return this.widgets.has(id);
    }

    /**
     * Get count of registered widgets
     */
    get count(): number {
        return this.widgets.size;
    }
}

// Singleton instance
export const WidgetRegistry = new WidgetRegistryClass();

/**
 * Helper function to register a widget
 */
export function registerWidget(manifest: WidgetManifest): void {
    WidgetRegistry.register(manifest);
}

/**
 * Helper function to unregister a widget
 */
export function unregisterWidget(id: string): boolean {
    return WidgetRegistry.unregister(id);
}
