import { useEffect } from 'react';
import { IconFolder, IconBrandGit, IconTrash, IconRefresh, IconPlug, IconLoader2 } from '@tabler/icons-react';
import { usePluginStore } from '../stores/usePluginStore';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

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
        <div className="flex flex-col gap-3">
            {/* Installed Plugins */}
            <div className="bg-card rounded-2xl overflow-hidden border border-border">
                <div className="flex flex-col">
                    {loading ? (
                        <div className="flex flex-col items-center justify-center gap-2 p-6 text-white/50 text-[13px]">
                            <IconLoader2 size={20} className="animate-spin" />
                            <span>Loading plugins...</span>
                        </div>
                    ) : plugins.length === 0 ? (
                        <div className="flex flex-col items-center justify-center gap-2 p-6 text-white/50 text-[13px]">
                            <IconPlug size={24} style={{ opacity: 0.5 }} />
                            <span>No external plugins installed</span>
                            <span className="text-[11px] opacity-60 break-all text-center">
                                Plugins directory: {pluginsDir}
                            </span>
                        </div>
                    ) : (
                        plugins.map(plugin => (
                            <div className="flex items-center justify-between p-3 px-4 border-b border-border last:border-b-0" key={plugin.manifest.id}>
                                <div className="flex flex-col gap-0.5 flex-1 min-w-0">
                                    <span className="text-sm font-medium text-foreground">{plugin.manifest.name}</span>
                                    <span className="text-[11px] text-muted-foreground">
                                        v{plugin.manifest.version}
                                        {plugin.manifest.author && ` • ${plugin.manifest.author}`}
                                    </span>
                                    <span className="text-xs text-muted-foreground truncate">{plugin.manifest.description}</span>
                                </div>
                                <div className="flex gap-1 ml-3">
                                    <Button
                                        variant="ghost"
                                        size="icon"
                                        className="h-8 w-8 text-muted-foreground hover:text-red-500 hover:bg-red-500/10"
                                        onClick={() => handleDelete(plugin.manifest.id)}
                                        title="Delete plugin"
                                    >
                                        <IconTrash size={16} />
                                    </Button>
                                </div>
                            </div>
                        ))
                    )}
                </div>
            </div>

            {/* Error message */}
            {error && (
                <div className="bg-red-500/15 border border-red-500/30 rounded-lg p-2.5 px-3 text-red-400 text-xs">
                    {error}
                </div>
            )}

            {/* Git URL Input */}
            {showGitInput && (
                <div className="flex gap-2 p-3 bg-white/5 rounded-lg items-center">
                    <Input
                        type="text"
                        value={gitUrl}
                        onChange={(e) => setGitUrl(e.target.value)}
                        placeholder="https://github.com/user/plugin.git"
                        className="flex-1 bg-black/30 border-white/10 h-9 text-[13px] focus-visible:ring-primary"
                        autoFocus
                        onKeyDown={(e) => {
                            if (e.key === 'Enter') handleInstallFromGit();
                            if (e.key === 'Escape') setShowGitInput(false);
                        }}
                    />
                    <Button
                        variant="secondary"
                        size="sm"
                        onClick={handleInstallFromGit}
                        disabled={installing}
                        className="hover:bg-green-500 hover:text-white transition-colors"
                    >
                        Install
                    </Button>
                    <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => setShowGitInput(false)}
                    >
                        Cancel
                    </Button>
                </div>
            )}

            {/* Install buttons */}
            <div className="flex gap-2">
                <Button
                    variant="secondary"
                    className="flex-1 gap-2"
                    onClick={handleInstallFromFolder}
                    disabled={installing}
                >
                    {installing ? <IconLoader2 size={16} className="animate-spin" /> : <IconFolder size={16} />}
                    <span>From Folder</span>
                </Button>
                <Button
                    variant="secondary"
                    className="flex-1 gap-2"
                    onClick={() => setShowGitInput(true)}
                    disabled={installing || showGitInput}
                >
                    <IconBrandGit size={16} />
                    <span>From Git URL</span>
                </Button>
                <Button
                    variant="secondary"
                    size="icon"
                    className="shrink-0"
                    onClick={loadPlugins}
                    disabled={loading}
                    title="Refresh plugin list"
                >
                    <IconRefresh size={16} className={loading ? 'animate-spin' : ''} />
                </Button>
            </div>
        </div>
    );
}
