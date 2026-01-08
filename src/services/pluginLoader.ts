/**
 * External Plugin Loader
 *
 * Scans ~/.opennook/plugins/ for user-installed widgets and loads them at runtime.
 * Plugins are frontend-only React components that self-register via registerWidget().
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { registerWidget, unregisterWidget, WidgetRegistry } from '../components/widgets/WidgetRegistry';
import { IconBox } from '@tabler/icons-react';

const PLUGIN_CHANGED_EVENT = 'plugin-changed';

/**
 * Plugin manifest from plugin.json
 */
export interface PluginManifest {
    id: string;
    name: string;
    version: string;
    description: string;
    author?: string;
    main: string;
    category: 'productivity' | 'media' | 'utility';
    minWidth?: number;
    hasCompactMode: boolean;
    compactPriority?: number;
    permissions: string[];
}

/**
 * Plugin info returned from backend
 */
interface PluginInfo {
    manifest: PluginManifest;
    bundle_path: string;
    plugin_dir: string;
}

/**
 * Load a single external plugin
 * The plugin bundle should call registerWidget() using the global API
 */
async function loadExternalPlugin(pluginInfo: PluginInfo): Promise<boolean> {
    try {
        console.log(`Loading external plugin: ${pluginInfo.manifest.name}`);

        // Read the plugin bundle content
        const bundleContent = await invoke<string>('read_plugin_bundle', {
            bundlePath: pluginInfo.bundle_path
        });

        // Create a script element and execute
        // This allows plugins to access globals we've set up
        const script = document.createElement('script');
        script.textContent = bundleContent;

        try {
            // Execute the plugin code
            document.head.appendChild(script);
            console.log(`Successfully loaded plugin: ${pluginInfo.manifest.name}`);
            return true;
        } finally {
            // Clean up script element
            document.head.removeChild(script);
        }
    } catch (error) {
        console.error(`Failed to load plugin ${pluginInfo.manifest.id}:`, error);
        return false;
    }
}

/**
 * Set up global API for external plugins
 * Plugins access this via window.__openNookPluginAPI__
 */
async function setupPluginAPI(): Promise<void> {
    // Only set up once
    if ((window as any).__openNookPluginAPI__) {
        return;
    }

    // Import React and components
    const [React, WidgetWrapperModule, WidgetAddDialogModule] = await Promise.all([
        import('react'),
        import('../components/widgets/WidgetWrapper'),
        import('../components/widgets/WidgetAddDialog'),
    ]);

    (window as any).__openNookPluginAPI__ = {
        // Core
        registerWidget,
        React,

        // UI Components
        WidgetWrapper: WidgetWrapperModule.WidgetWrapper,
        WidgetAddDialog: WidgetAddDialogModule.WidgetAddDialog,

        // Icons
        IconBox,
    };
    console.log('Plugin API initialized');
}

/**
 * Scan and load all external plugins from ~/.opennook/plugins/
 */
export async function loadExternalPlugins(): Promise<void> {
    // Set up global API first - must await!
    await setupPluginAPI();

    try {
        // Get list of plugins from backend
        const plugins = await invoke<PluginInfo[]>('scan_plugins_directory');

        if (plugins.length === 0) {
            console.log('No external plugins found');
            return;
        }

        console.log(`Found ${plugins.length} external plugin(s)`);

        // Load each plugin
        const results = await Promise.allSettled(
            plugins.map(plugin => loadExternalPlugin(plugin))
        );

        const loaded = results.filter(r => r.status === 'fulfilled' && r.value).length;
        console.log(`Loaded ${loaded}/${plugins.length} external plugins`);
    } catch (error) {
        console.error('Failed to scan plugins directory:', error);
    }
}

/**
 * Get the path to the plugins directory (for user guidance)
 */
export async function getPluginsDirectoryPath(): Promise<string> {
    return invoke<string>('get_plugins_directory_path');
}

/**
 * Get list of installed external plugins
 */
export async function getInstalledPlugins(): Promise<PluginInfo[]> {
    return invoke<PluginInfo[]>('scan_plugins_directory');
}

/**
 * Hot-load a single plugin (for installing new plugins without restart)
 */
export async function hotLoadPlugin(pluginInfo: PluginInfo): Promise<boolean> {
    // Ensure API is set up
    await setupPluginAPI();

    return loadExternalPlugin(pluginInfo);
}

/**
 * Install a plugin from a local folder
 */
export async function installPluginFromFolder(sourcePath: string): Promise<PluginInfo> {
    const pluginInfo = await invoke<PluginInfo>('install_plugin_from_folder', { sourcePath });

    // Hot-load the newly installed plugin
    await hotLoadPlugin(pluginInfo);

    return pluginInfo;
}

/**
 * Install a plugin from a Git repository URL
 */
export async function installPluginFromGit(repoUrl: string): Promise<PluginInfo> {
    const pluginInfo = await invoke<PluginInfo>('install_plugin_from_git', { repoUrl });

    // Hot-load the newly installed plugin
    await hotLoadPlugin(pluginInfo);

    return pluginInfo;
}

/**
 * Delete an installed plugin and unregister its widget
 */
export async function deletePlugin(pluginId: string): Promise<void> {
    // Unregister the widget first (this triggers UI update)
    unregisterWidget(pluginId);

    // Then delete the files
    await invoke('delete_plugin', { pluginId });

    console.log(`Plugin ${pluginId} deleted and unregistered.`);
}

/**
 * Listen for plugin changes from other windows (e.g., Settings)
 * Call this from the main window to stay in sync
 */
export async function listenForPluginChanges(): Promise<() => void> {
    const unlisten = await listen<{ action: string; pluginId: string }>(PLUGIN_CHANGED_EVENT, async (event) => {
        console.log('Plugin change event received:', event.payload);

        if (event.payload.action === 'install') {
            // A new plugin was installed in another window
            // Check if we already have it registered
            if (!WidgetRegistry.has(event.payload.pluginId)) {
                // Load the newly installed plugin
                const plugins = await invoke<PluginInfo[]>('scan_plugins_directory');
                const newPlugin = plugins.find(p => p.manifest.id === event.payload.pluginId);
                if (newPlugin) {
                    await hotLoadPlugin(newPlugin);
                    console.log(`Hot-loaded plugin from other window: ${event.payload.pluginId}`);
                }
            }
        } else if (event.payload.action === 'delete') {
            // A plugin was deleted in another window
            unregisterWidget(event.payload.pluginId);
            console.log(`Unloaded plugin from other window: ${event.payload.pluginId}`);
        }
    });

    return unlisten;
}

// Re-export PluginInfo for use in other modules
export type { PluginInfo };
