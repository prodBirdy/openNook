import { createContext, useContext, useState, useEffect, ReactNode, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, emit } from '@tauri-apps/api/event';
import { WidgetManifest, WidgetEnabledState, WidgetInstanceState } from '../components/widgets/WidgetTypes';
import { WidgetRegistry, widgetsReady } from '../components/widgets';

const INSTANCES_STORAGE_KEY = 'widget-instances';
const WIDGET_STATE_CHANGED_EVENT = 'widget-state-changed';

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
    const [isLoaded, setIsLoaded] = useState(false);
    const isExternalUpdate = useRef(false);
    const senderId = useRef(Math.random().toString(36).substring(7));

    // Load widgets from registry and enabled state from Rust backend
    useEffect(() => {
        const loadWidgets = async () => {
            // Wait for built-in widgets to be registered
            await widgetsReady;

            const allWidgets = WidgetRegistry.getAll();
            setWidgets(allWidgets);

            // Load saved enabled state from Rust backend
            try {
                const saved = await invoke<{ enabled: WidgetEnabledState }>('load_widget_state');
                // Merge with defaults for any new widgets
                const merged: WidgetEnabledState = {};
                allWidgets.forEach(w => {
                    merged[w.id] = saved.enabled[w.id] ?? w.defaultEnabled;
                });
                setEnabledState(merged);
            } catch (e) {
                console.error('Failed to load widget state from backend:', e);
                // Fall back to defaults
                const defaults: WidgetEnabledState = {};
                allWidgets.forEach(w => {
                    defaults[w.id] = w.defaultEnabled;
                });
                setEnabledState(defaults);
            } finally {
                setIsLoaded(true);
            }

            // Load saved instances from localStorage (keeping instances local for now)
            const savedInstances = localStorage.getItem(INSTANCES_STORAGE_KEY);
            if (savedInstances) {
                try {
                    setInstances(JSON.parse(savedInstances));
                } catch (e) {
                    console.error('Failed to parse widget instances:', e);
                }
            }
        };

        loadWidgets();

        // Subscribe to registry changes for hot-loaded plugins
        const unsubscribe = WidgetRegistry.subscribe(() => {
            const allWidgets = WidgetRegistry.getAll();
            setWidgets(allWidgets);
            // Add default enabled state for new widgets
            setEnabledState(prev => {
                const updated = { ...prev };
                allWidgets.forEach(w => {
                    if (!(w.id in updated)) {
                        updated[w.id] = w.defaultEnabled;
                    }
                });
                return updated;
            });
        });

        return () => {
            unsubscribe();
        };
    }, []);

    // Listen for widget state changes from other windows
    useEffect(() => {
        const unlisten = listen<{ enabled: WidgetEnabledState, senderId: string }>(WIDGET_STATE_CHANGED_EVENT, async (event) => {
            // Ignore updates from ourselves
            if (event.payload.senderId === senderId.current) {
                return;
            }

            console.log('Received widget state update from other window');
            isExternalUpdate.current = true;
            setEnabledState(prev => {
                // Merge with current widgets
                const merged: WidgetEnabledState = {};
                widgets.forEach(w => {
                    merged[w.id] = event.payload.enabled[w.id] ?? prev[w.id];
                });
                return merged;
            });
            // Reset flag after state update
            setTimeout(() => { isExternalUpdate.current = false; }, 100);
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, [widgets]);

    // Save enabled state to Rust backend when it changes and emit event
    useEffect(() => {
        if (!isLoaded) return;

        if (Object.keys(enabledState).length > 0) {
            invoke('save_widget_state', { state: { enabled: enabledState } })
                .catch(e => console.error('Failed to save widget state:', e));

            // Emit event to other windows (skip if this was triggered by external update)
            if (!isExternalUpdate.current) {
                emit(WIDGET_STATE_CHANGED_EVENT, {
                    enabled: enabledState,
                    senderId: senderId.current
                }).catch(e => console.error('Failed to emit widget state event:', e));
            }
        }
    }, [enabledState, isLoaded]);

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
