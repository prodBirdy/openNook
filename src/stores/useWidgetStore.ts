import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { WidgetManifest, WidgetEnabledState, WidgetInstanceState } from '../components/widgets/WidgetTypes';
import { WidgetRegistry, widgetsReady } from '../components/widgets';

const INSTANCES_STORAGE_KEY = 'widget-instances';
const WIDGET_STATE_CHANGED_EVENT = 'widget-state-changed';
const senderId = Math.random().toString(36).substring(7);

let _isExternalUpdate = false;

interface WidgetState {
    widgets: WidgetManifest[];
    enabledState: WidgetEnabledState;
    instances: WidgetInstanceState[];
    isLoaded: boolean;
}

interface WidgetActions {
    loadWidgets: () => Promise<void>;
    toggleWidget: (widgetId: string) => void;
    setWidgetEnabled: (widgetId: string, enabled: boolean) => void;
    addInstance: (widgetId: string, data?: Record<string, unknown>) => string;
    updateInstance: (instanceId: string, data: Partial<WidgetInstanceState['data']>) => void;
    removeInstance: (instanceId: string) => void;
    getInstancesForWidget: (widgetId: string) => WidgetInstanceState[];
    hasActiveInstance: (widgetId: string) => boolean;
    setWidgets: (widgets: WidgetManifest[]) => void;
    setEnabledState: (state: WidgetEnabledState) => void;
    setupListeners: () => () => void;
    // Computed getters as functions
    getEnabledWidgets: () => WidgetManifest[];
    getEnabledCompactWidgets: () => WidgetManifest[];
}

type WidgetStore = WidgetState & WidgetActions;

export const useWidgetStore = create<WidgetStore>((set, get) => ({
    widgets: [],
    enabledState: {},
    instances: [],
    isLoaded: false,

    // Computed getters as functions
    getEnabledWidgets: () => {
        const state = get();
        return state.widgets.filter(w => state.enabledState[w.id]);
    },

    getEnabledCompactWidgets: () => {
        const state = get();
        return state.widgets.filter(w => state.enabledState[w.id] && w.hasCompactMode && w.CompactComponent);
    },

    loadWidgets: async () => {
        // Wait for built-in widgets to be registered
        await widgetsReady;

        const allWidgets = WidgetRegistry.getAll();

        // Load saved enabled state from Rust backend
        let enabledState: WidgetEnabledState = {};
        try {
            const saved = await invoke<{ enabled: WidgetEnabledState }>('load_widget_state');
            // Merge with defaults for any new widgets
            allWidgets.forEach(w => {
                enabledState[w.id] = saved.enabled[w.id] ?? w.defaultEnabled;
            });
        } catch (e) {
            console.error('Failed to load widget state from backend:', e);
            // Fall back to defaults
            allWidgets.forEach(w => {
                enabledState[w.id] = w.defaultEnabled;
            });
        }

        // Load saved instances from localStorage
        let instances: WidgetInstanceState[] = [];
        const savedInstances = localStorage.getItem(INSTANCES_STORAGE_KEY);
        if (savedInstances) {
            try {
                instances = JSON.parse(savedInstances);
            } catch (e) {
                console.error('Failed to parse widget instances:', e);
            }
        }

        set({ widgets: allWidgets, enabledState, instances, isLoaded: true });
    },

    toggleWidget: (widgetId) => {
        const { enabledState } = get();
        const updated = {
            ...enabledState,
            [widgetId]: !enabledState[widgetId]
        };

        set({ enabledState: updated });

        // Persist and sync
        invoke('save_widget_state', { state: { enabled: updated } }).catch(console.error);
        emit(WIDGET_STATE_CHANGED_EVENT, { enabled: updated, senderId }).catch(console.error);
    },

    setWidgetEnabled: (widgetId, enabled) => {
        const { enabledState } = get();
        const updated = {
            ...enabledState,
            [widgetId]: enabled
        };

        set({ enabledState: updated });
        invoke('save_widget_state', { state: { enabled: updated } }).catch(console.error);
        emit(WIDGET_STATE_CHANGED_EVENT, { enabled: updated, senderId }).catch(console.error);
    },

    addInstance: (widgetId, data = {}) => {
        const id = `${widgetId}-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
        const instance: WidgetInstanceState = {
            id,
            widgetId,
            isActive: false,
            data,
            createdAt: Date.now()
        };

        const { instances } = get();
        const updated = [...instances, instance];
        set({ instances: updated });
        localStorage.setItem(INSTANCES_STORAGE_KEY, JSON.stringify(updated));

        return id;
    },

    updateInstance: (instanceId, data) => {
        const { instances } = get();
        const updated = instances.map(inst =>
            inst.id === instanceId
                ? { ...inst, data: { ...inst.data, ...data } }
                : inst
        );

        set({ instances: updated });
        localStorage.setItem(INSTANCES_STORAGE_KEY, JSON.stringify(updated));
    },

    removeInstance: (instanceId) => {
        const { instances } = get();
        const updated = instances.filter(inst => inst.id !== instanceId);

        set({ instances: updated });
        localStorage.setItem(INSTANCES_STORAGE_KEY, JSON.stringify(updated));
    },

    getInstancesForWidget: (widgetId) => {
        const { instances } = get();
        return instances.filter(inst => inst.widgetId === widgetId);
    },

    hasActiveInstance: (widgetId) => {
        const { instances } = get();
        return instances.some(inst => inst.widgetId === widgetId && inst.isActive);
    },

    setWidgets: (widgets) => set({ widgets }),

    setEnabledState: (enabledState) => set({ enabledState }),

    setupListeners: () => {
        // Listen for cross-window sync
        const unlistenPromise = listen<{ enabled: WidgetEnabledState, senderId: string }>(
            WIDGET_STATE_CHANGED_EVENT,
            (event) => {
                if (event.payload.senderId === senderId) return;

                console.log('Received widget state update from other window');
                _isExternalUpdate = true;

                const { widgets } = get();
                const merged: WidgetEnabledState = {};
                widgets.forEach(w => {
                    merged[w.id] = event.payload.enabled[w.id] ?? get().enabledState[w.id];
                });
                get().setEnabledState(merged);

                setTimeout(() => { _isExternalUpdate = false; }, 100);
            }
        );

        // Subscribe to registry changes for hot-loaded plugins
        const unsubscribeRegistry = WidgetRegistry.subscribe(() => {
            const allWidgets = WidgetRegistry.getAll();
            set({ widgets: allWidgets });

            // Add default enabled state for new widgets
            const { enabledState } = get();
            const updated = { ...enabledState };
            allWidgets.forEach(w => {
                if (!(w.id in updated)) {
                    updated[w.id] = w.defaultEnabled;
                }
            });
            set({ enabledState: updated });
        });

        return () => {
            unlistenPromise.then(fn => fn());
            unsubscribeRegistry();
        };
    }
}));

// Selectors for use in components
export const selectWidgets = (state: WidgetStore) => state.widgets;
export const selectEnabledState = (state: WidgetStore) => state.enabledState;
export const selectInstances = (state: WidgetStore) => state.instances;

// Computed selectors - use these in components
export const selectEnabledWidgets = (state: WidgetStore) =>
    state.widgets.filter(w => state.enabledState[w.id]);

export const selectEnabledCompactWidgets = (state: WidgetStore) =>
    state.widgets.filter(w => state.enabledState[w.id] && w.hasCompactMode && w.CompactComponent);
