import { useEffect } from 'react';
import { IconFolder, IconBrandGit, IconTrash, IconRefresh, IconPlug, IconLoader2 } from '@tabler/icons-react';
import { usePluginStore } from '../stores/usePluginStore';

interface PluginStoreProps {
    onPluginChange?: () => void;
}

export function PluginStore({ onPluginChange }: PluginStoreProps) {
    const {
        plugins,
        loading,
        installing,
        gitUrl,
        showGitInput,
        error,
        pluginsDir,
        setGitUrl,
        setShowGitInput,
        loadPlugins,
        installFromFolder,
        installFromGit,
        deletePlugin: deletePluginAction,
        initialize
    } = usePluginStore();

    useEffect(() => {
        initialize();
    }, [initialize]);

    const handleInstallFromFolder = () => installFromFolder(onPluginChange);
    const handleInstallFromGit = () => installFromGit(onPluginChange);
    const handleDelete = (pluginId: string) => deletePluginAction(pluginId, onPluginChange);

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
