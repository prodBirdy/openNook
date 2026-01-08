import { create } from 'zustand';
import { emit } from '@tauri-apps/api/event';
import {
    getInstalledPlugins,
    installPluginFromFolder,
    installPluginFromGit,
    deletePlugin,
    getPluginsDirectoryPath,
    PluginInfo
} from '../services/pluginLoader';

const PLUGIN_CHANGED_EVENT = 'plugin-changed';

interface PluginState {
    plugins: PluginInfo[];
    loading: boolean;
    installing: boolean;
    gitUrl: string;
    showGitInput: boolean;
    error: string | null;
    pluginsDir: string;
}

interface PluginActions {
    setGitUrl: (url: string) => void;
    setShowGitInput: (show: boolean) => void;
    setError: (error: string | null) => void;
    loadPlugins: () => Promise<void>;
    installFromFolder: (onPluginChange?: () => void) => Promise<void>;
    installFromGit: (onPluginChange?: () => void) => Promise<void>;
    deletePlugin: (pluginId: string, onPluginChange?: () => void) => Promise<void>;
    initialize: () => Promise<void>;
}

type PluginStore = PluginState & PluginActions;

export const usePluginStore = create<PluginStore>((set, get) => ({
    // State
    plugins: [],
    loading: true,
    installing: false,
    gitUrl: '',
    showGitInput: false,
    error: null,
    pluginsDir: '',

    // Actions
    setGitUrl: (url) => set({ gitUrl: url }),
    setShowGitInput: (show) => set({ showGitInput: show }),
    setError: (error) => set({ error }),

    loadPlugins: async () => {
        try {
            set({ loading: true });
            const installed = await getInstalledPlugins();
            set({ plugins: installed, error: null });
        } catch (e) {
            set({ error: `Failed to load plugins: ${e}` });
        } finally {
            set({ loading: false });
        }
    },

    installFromFolder: async (onPluginChange) => {
        const { open } = await import('@tauri-apps/plugin-dialog');

        try {
            set({ installing: true, error: null });

            const selected = await open({
                directory: true,
                multiple: false,
                title: 'Select Plugin Folder'
            });

            if (selected && typeof selected === 'string') {
                const pluginInfo = await installPluginFromFolder(selected);
                await get().loadPlugins();
                // Notify main window to reload plugins
                await emit(PLUGIN_CHANGED_EVENT, { action: 'install', pluginId: pluginInfo.manifest.id });
                onPluginChange?.();
            }
        } catch (e) {
            set({ error: `Failed to install: ${e}` });
        } finally {
            set({ installing: false });
        }
    },

    installFromGit: async (onPluginChange) => {
        const { gitUrl } = get();

        if (!gitUrl.trim()) {
            set({ error: 'Please enter a Git repository URL' });
            return;
        }

        try {
            set({ installing: true, error: null, showGitInput: false });

            const pluginInfo = await installPluginFromGit(gitUrl.trim());
            set({ gitUrl: '' });
            await get().loadPlugins();
            // Notify main window to reload plugins
            await emit(PLUGIN_CHANGED_EVENT, { action: 'install', pluginId: pluginInfo.manifest.id });
            onPluginChange?.();
        } catch (e) {
            set({ error: `Failed to install: ${e}` });
        } finally {
            set({ installing: false });
        }
    },

    deletePlugin: async (pluginId, onPluginChange) => {
        try {
            set({ error: null });
            await deletePlugin(pluginId);
            await get().loadPlugins();
            // Notify main window to reload plugins
            await emit(PLUGIN_CHANGED_EVENT, { action: 'delete', pluginId });
            onPluginChange?.();
        } catch (e) {
            set({ error: `Failed to delete: ${e}` });
        }
    },

    initialize: async () => {
        const pluginsDir = await getPluginsDirectoryPath();
        set({ pluginsDir });
        await get().loadPlugins();
    }
}));
