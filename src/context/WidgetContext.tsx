import { createContext, useContext, useState, useEffect, ReactNode, useCallback } from 'react';
import { WidgetManifest, WidgetEnabledState, WidgetInstanceState } from '../components/widgets/WidgetTypes';
import { WidgetRegistry } from '../components/widgets/WidgetRegistry';

const STORAGE_KEY = 'widget-enabled-state';
const INSTANCES_STORAGE_KEY = 'widget-instances';

interface WidgetContextValue {
    /** All registered widgets */
    widgets: WidgetManifest[];
    /** Enabled state for each widget */
    enabledState: WidgetEnabledState;
    /** Toggle a widget on/off */
    toggleWidget: (widgetId: string) => void;
    /** Set widget enabled state directly */
    setWidgetEnabled: (widgetId: string, enabled: boolean) => void;
    /** Get enabled widgets only */
    enabledWidgets: WidgetManifest[];
    /** Get enabled widgets with compact mode */
    enabledCompactWidgets: WidgetManifest[];
    /** Widget instances (for multi-instance widgets like timers) */
    instances: WidgetInstanceState[];
    /** Add a new widget instance */
    addInstance: (widgetId: string, data?: Record<string, unknown>) => string;
    /** Update an instance */
    updateInstance: (instanceId: string, data: Partial<WidgetInstanceState['data']>) => void;
    /** Remove an instance */
    removeInstance: (instanceId: string) => void;
    /** Get instances for a specific widget */
    getInstancesForWidget: (widgetId: string) => WidgetInstanceState[];
    /** Check if any instance of a widget is active */
    hasActiveInstance: (widgetId: string) => boolean;
}

const WidgetContext = createContext<WidgetContextValue | null>(null);

interface WidgetProviderProps {
    children: ReactNode;
}

export function WidgetProvider({ children }: WidgetProviderProps) {
    const [widgets, setWidgets] = useState<WidgetManifest[]>([]);
    const [enabledState, setEnabledState] = useState<WidgetEnabledState>({});
    const [instances, setInstances] = useState<WidgetInstanceState[]>([]);

    // Load widgets from registry and enabled state from storage
    useEffect(() => {
        const allWidgets = WidgetRegistry.getAll();
        setWidgets(allWidgets);

        // Load saved enabled state
        const saved = localStorage.getItem(STORAGE_KEY);
        if (saved) {
            try {
                const parsed = JSON.parse(saved) as WidgetEnabledState;
                // Merge with defaults for any new widgets
                const merged: WidgetEnabledState = {};
                allWidgets.forEach(w => {
                    merged[w.id] = parsed[w.id] ?? w.defaultEnabled;
                });
                setEnabledState(merged);
            } catch (e) {
                console.error('Failed to parse widget enabled state:', e);
                // Fall back to defaults
                const defaults: WidgetEnabledState = {};
                allWidgets.forEach(w => {
                    defaults[w.id] = w.defaultEnabled;
                });
                setEnabledState(defaults);
            }
        } else {
            // Use defaults
            const defaults: WidgetEnabledState = {};
            allWidgets.forEach(w => {
                defaults[w.id] = w.defaultEnabled;
            });
            setEnabledState(defaults);
        }

        // Load saved instances
        const savedInstances = localStorage.getItem(INSTANCES_STORAGE_KEY);
        if (savedInstances) {
            try {
                setInstances(JSON.parse(savedInstances));
            } catch (e) {
                console.error('Failed to parse widget instances:', e);
            }
        }
    }, []);

    // Save enabled state when it changes
    useEffect(() => {
        if (Object.keys(enabledState).length > 0) {
            localStorage.setItem(STORAGE_KEY, JSON.stringify(enabledState));
        }
    }, [enabledState]);

    // Save instances when they change
    useEffect(() => {
        localStorage.setItem(INSTANCES_STORAGE_KEY, JSON.stringify(instances));
    }, [instances]);

    const toggleWidget = useCallback((widgetId: string) => {
        setEnabledState(prev => ({
            ...prev,
            [widgetId]: !prev[widgetId]
        }));
    }, []);

    const setWidgetEnabled = useCallback((widgetId: string, enabled: boolean) => {
        setEnabledState(prev => ({
            ...prev,
            [widgetId]: enabled
        }));
    }, []);

    const addInstance = useCallback((widgetId: string, data: Record<string, unknown> = {}): string => {
        const id = `${widgetId}-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
        const instance: WidgetInstanceState = {
            id,
            widgetId,
            isActive: false,
            data,
            createdAt: Date.now()
        };
        setInstances(prev => [...prev, instance]);
        return id;
    }, []);

    const updateInstance = useCallback((instanceId: string, data: Partial<WidgetInstanceState['data']>) => {
        setInstances(prev => prev.map(inst =>
            inst.id === instanceId
                ? { ...inst, data: { ...inst.data, ...data } }
                : inst
        ));
    }, []);

    const removeInstance = useCallback((instanceId: string) => {
        setInstances(prev => prev.filter(inst => inst.id !== instanceId));
    }, []);

    const getInstancesForWidget = useCallback((widgetId: string) => {
        return instances.filter(inst => inst.widgetId === widgetId);
    }, [instances]);

    const hasActiveInstance = useCallback((widgetId: string) => {
        return instances.some(inst => inst.widgetId === widgetId && inst.isActive);
    }, [instances]);

    const enabledWidgets = widgets.filter(w => enabledState[w.id]);
    const enabledCompactWidgets = enabledWidgets.filter(w => w.hasCompactMode && w.CompactComponent);

    return (
        <WidgetContext.Provider value={{
            widgets,
            enabledState,
            toggleWidget,
            setWidgetEnabled,
            enabledWidgets,
            enabledCompactWidgets,
            instances,
            addInstance,
            updateInstance,
            removeInstance,
            getInstancesForWidget,
            hasActiveInstance
        }}>
            {children}
        </WidgetContext.Provider>
    );
}

/**
 * Hook to access widget context
 */
export function useWidgets(): WidgetContextValue {
    const context = useContext(WidgetContext);
    if (!context) {
        throw new Error('useWidgets must be used within a WidgetProvider');
    }
    return context;
}
