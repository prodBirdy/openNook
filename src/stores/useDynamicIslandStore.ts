import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

interface Settings {
    showCalendar: boolean;
    showReminders: boolean;
    showMedia: boolean;
    baseWidth: number;
    baseHeight: number;
    liquidGlassMode: boolean;
}

interface DynamicIslandState {
    // Mode management
    preferredModeId: string | null;
    isInitialLaunch: boolean;

    // UI state
    isHovered: boolean;
    expanded: boolean;
    isAnimating: boolean;
    activeTab: 'widgets' | 'files';

    // Content state
    notes: string;

    // Window dimensions
    windowSize: { width: number; height: number };

    // Settings
    settings: Settings;

    // Refs state (tracked in store for cross-component access)
    lastModeCycleTime: number;
}

interface DynamicIslandActions {
    // Mode actions
    setPreferredModeId: (modeId: string | null) => void;
    setIsInitialLaunch: (value: boolean) => void;

    // UI actions
    setIsHovered: (value: boolean) => void;
    setExpanded: (value: boolean | ((prev: boolean) => boolean)) => void;
    setIsAnimating: (value: boolean) => void;
    setActiveTab: (tab: 'widgets' | 'files') => void;

    // Content actions
    setNotes: (notes: string) => void;
    loadNotes: () => Promise<void>;
    saveNotes: (notes: string) => Promise<void>;

    // Window actions
    setWindowSize: (size: { width: number; height: number }) => void;

    // Settings actions
    setSettings: (settings: Partial<Settings>) => void;
    loadSettings: () => void;

    // Mode cycling
    setLastModeCycleTime: (time: number) => void;

    // Compound actions
    handleIslandClick: (currentMode: string) => void;
    handleExpandCollapse: (expand: boolean) => void;
}

type DynamicIslandStore = DynamicIslandState & DynamicIslandActions;

const DEFAULT_SETTINGS: Settings = {
    showCalendar: false,
    showReminders: false,
    showMedia: true,
    baseWidth: 160,
    baseHeight: 38,
    liquidGlassMode: false,
};

export const useDynamicIslandStore = create<DynamicIslandStore>((set, get) => ({
    // Initial state
    preferredModeId: null,
    isInitialLaunch: true,
    isHovered: false,
    expanded: false,
    isAnimating: false,
    activeTab: 'widgets',
    notes: '',
    windowSize: { width: window.innerWidth, height: window.innerHeight },
    settings: DEFAULT_SETTINGS,
    lastModeCycleTime: 0,

    // Mode actions
    setPreferredModeId: (modeId) => set({ preferredModeId: modeId }),
    setIsInitialLaunch: (value) => set({ isInitialLaunch: value }),

    // UI actions
    setIsHovered: (value) => set({ isHovered: value }),
    setExpanded: (value) => {
        if (typeof value === 'function') {
            set((state) => ({ expanded: value(state.expanded) }));
        } else {
            set({ expanded: value });
        }
    },
    setIsAnimating: (value) => set({ isAnimating: value }),
    setActiveTab: (tab) => set({ activeTab: tab }),

    // Content actions
    setNotes: (notes) => set({ notes }),
    loadNotes: async () => {
        try {
            const notes = await invoke<string>('load_notes');
            set({ notes });
        } catch (err) {
            console.error('Failed to load notes:', err);
        }
    },
    saveNotes: async (notes) => {
        try {
            await invoke('save_notes', { notes });
        } catch (err) {
            console.error('Failed to save notes:', err);
        }
    },

    // Window actions
    setWindowSize: (size) => set({ windowSize: size }),

    // Settings actions
    setSettings: (newSettings) => set((state) => ({
        settings: { ...state.settings, ...newSettings }
    })),
    loadSettings: () => {
        const saved = localStorage.getItem('app-settings');
        if (saved) {
            try {
                const parsed = JSON.parse(saved);
                set((state) => ({
                    settings: { ...state.settings, ...parsed }
                }));
            } catch (e) {
                console.error('Failed to parse settings:', e);
            }
        }
    },

    // Mode cycling
    setLastModeCycleTime: (time) => set({ lastModeCycleTime: time }),

    // Compound actions
    handleIslandClick: (currentMode) => {
        const { expanded, setExpanded, setIsAnimating, setActiveTab } = get();

        if (!expanded) {
            // Expanding
            setIsAnimating(true);
            // Set initial tab based on current mode
            if (currentMode === 'files') {
                setActiveTab('files');
            } else {
                setActiveTab('widgets');
            }
            invoke('trigger_haptics').catch(console.error);
        } else {
            invoke('trigger_haptics').catch(console.error);
        }

        setExpanded(!expanded);
    },

    handleExpandCollapse: (expand) => {
        const { setExpanded, setIsAnimating } = get();
        setExpanded(expand);
        setIsAnimating(true);
        invoke('trigger_haptics').catch(console.error);
    },
}));

// Selectors for optimized re-renders
export const selectExpanded = (state: DynamicIslandStore) => state.expanded;
export const selectIsHovered = (state: DynamicIslandStore) => state.isHovered;
export const selectIsAnimating = (state: DynamicIslandStore) => state.isAnimating;
export const selectActiveTab = (state: DynamicIslandStore) => state.activeTab;
export const selectSettings = (state: DynamicIslandStore) => state.settings;
export const selectNotes = (state: DynamicIslandStore) => state.notes;
export const selectPreferredModeId = (state: DynamicIslandStore) => state.preferredModeId;
export const selectIsInitialLaunch = (state: DynamicIslandStore) => state.isInitialLaunch;
export const selectWindowSize = (state: DynamicIslandStore) => state.windowSize;
