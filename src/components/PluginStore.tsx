import { useState, useEffect } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { emit } from '@tauri-apps/api/event';
import { IconFolder, IconBrandGit, IconTrash, IconRefresh, IconPlug, IconLoader2 } from '@tabler/icons-react';
import {
    getInstalledPlugins,
    installPluginFromFolder,
    installPluginFromGit,
    deletePlugin,
    getPluginsDirectoryPath,
    PluginInfo
} from '../services/pluginLoader';

const PLUGIN_CHANGED_EVENT = 'plugin-changed';

interface PluginStoreProps {
    onPluginChange?: () => void;
}

export function PluginStore({ onPluginChange }: PluginStoreProps) {
    const [plugins, setPlugins] = useState<PluginInfo[]>([]);
    const [loading, setLoading] = useState(true);
    const [installing, setInstalling] = useState(false);
    const [gitUrl, setGitUrl] = useState('');
    const [showGitInput, setShowGitInput] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [pluginsDir, setPluginsDir] = useState<string>('');

    // Load installed plugins
    const loadPlugins = async () => {
        try {
            setLoading(true);
            const installed = await getInstalledPlugins();
            setPlugins(installed);
            setError(null);
        } catch (e) {
            setError(`Failed to load plugins: ${e}`);
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        loadPlugins();
        getPluginsDirectoryPath().then(setPluginsDir);
    }, []);

    // Install from folder
    const handleInstallFromFolder = async () => {
        try {
            setInstalling(true);
            setError(null);

            const selected = await open({
                directory: true,
                multiple: false,
                title: 'Select Plugin Folder'
            });

            if (selected && typeof selected === 'string') {
                const pluginInfo = await installPluginFromFolder(selected);
                await loadPlugins();
                // Notify main window to reload plugins
                await emit(PLUGIN_CHANGED_EVENT, { action: 'install', pluginId: pluginInfo.manifest.id });
                onPluginChange?.();
            }
        } catch (e) {
            setError(`Failed to install: ${e}`);
        } finally {
            setInstalling(false);
        }
    };

    // Install from Git
    const handleInstallFromGit = async () => {
        if (!gitUrl.trim()) {
            setError('Please enter a Git repository URL');
            return;
        }

        try {
            setInstalling(true);
            setError(null);
            setShowGitInput(false);

            const pluginInfo = await installPluginFromGit(gitUrl.trim());
            setGitUrl('');
            await loadPlugins();
            // Notify main window to reload plugins
            await emit(PLUGIN_CHANGED_EVENT, { action: 'install', pluginId: pluginInfo.manifest.id });
            onPluginChange?.();
        } catch (e) {
            setError(`Failed to install: ${e}`);
        } finally {
            setInstalling(false);
        }
    };

    // Delete plugin
    const handleDelete = async (pluginId: string) => {
        try {
            setError(null);
            await deletePlugin(pluginId);
            await loadPlugins();
            // Notify main window to reload plugins
            await emit(PLUGIN_CHANGED_EVENT, { action: 'delete', pluginId });
            onPluginChange?.();
        } catch (e) {
            setError(`Failed to delete: ${e}`);
        }
    };

    return (
        <div className="plugin-store">
            {/* Installed Plugins */}
            <div className="settings-group">
                <div className="plugin-list">
                    {loading ? (
                        <div className="plugin-loading">
                            <IconLoader2 size={20} className="spinning" />
                            <span>Loading plugins...</span>
                        </div>
                    ) : plugins.length === 0 ? (
                        <div className="plugin-empty">
                            <IconPlug size={24} style={{ opacity: 0.5 }} />
                            <span>No external plugins installed</span>
                            <span className="plugin-hint">
                                Plugins directory: {pluginsDir}
                            </span>
                        </div>
                    ) : (
                        plugins.map(plugin => (
                            <div className="plugin-item" key={plugin.manifest.id}>
                                <div className="plugin-info">
                                    <span className="plugin-name">{plugin.manifest.name}</span>
                                    <span className="plugin-meta">
                                        v{plugin.manifest.version}
                                        {plugin.manifest.author && ` • ${plugin.manifest.author}`}
                                    </span>
                                    <span className="plugin-desc">{plugin.manifest.description}</span>
                                </div>
                                <div className="plugin-actions">
                                    <button
                                        className="plugin-action-btn danger"
                                        onClick={() => handleDelete(plugin.manifest.id)}
                                        title="Delete plugin"
                                    >
                                        <IconTrash size={16} />
                                    </button>
                                </div>
                            </div>
                        ))
                    )}
                </div>
            </div>

            {/* Error message */}
            {error && (
                <div className="plugin-error">
                    {error}
                </div>
            )}

            {/* Git URL Input */}
            {showGitInput && (
                <div className="git-input-container">
                    <input
                        type="text"
                        value={gitUrl}
                        onChange={(e) => setGitUrl(e.target.value)}
                        placeholder="https://github.com/user/plugin.git"
                        className="git-input"
                        autoFocus
                        onKeyDown={(e) => {
                            if (e.key === 'Enter') handleInstallFromGit();
                            if (e.key === 'Escape') setShowGitInput(false);
                        }}
                    />
                    <button
                        className="git-submit-btn"
                        onClick={handleInstallFromGit}
                        disabled={installing}
                    >
                        Install
                    </button>
                    <button
                        className="git-cancel-btn"
                        onClick={() => setShowGitInput(false)}
                    >
                        Cancel
                    </button>
                </div>
            )}

            {/* Install buttons */}
            <div className="plugin-install-buttons">
                <button
                    className="plugin-install-btn"
                    onClick={handleInstallFromFolder}
                    disabled={installing}
                >
                    {installing ? <IconLoader2 size={16} className="spinning" /> : <IconFolder size={16} />}
                    <span>From Folder</span>
                </button>
                <button
                    className="plugin-install-btn"
                    onClick={() => setShowGitInput(true)}
                    disabled={installing || showGitInput}
                >
                    <IconBrandGit size={16} />
                    <span>From Git URL</span>
                </button>
                <button
                    className="plugin-install-btn secondary"
                    onClick={loadPlugins}
                    disabled={loading}
                    title="Refresh plugin list"
                >
                    <IconRefresh size={16} className={loading ? 'spinning' : ''} />
                </button>
            </div>
        </div>
    );
}
