import { useState, useEffect } from 'react';
import { useWidgetStore } from '../../stores/useWidgetStore';
import { clampIslandPosition } from '../../stores/useDynamicIslandStore';
import { PluginStore } from '../../components/PluginStore';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { Slider } from '@/components/ui/slider';
import {
    Settings as SettingsIcon,
    Palette,
    Music,
    Grid,
    Puzzle
} from 'lucide-react';
import { cn } from '@/lib/utils';

interface SettingsState {
    showMedia: boolean;
    liquidGlassMode: boolean;
    nonNotchMode: boolean;
    islandColor: string;
    islandPositionX: number;
    islandPositionY: number;
}

type Tab = 'general' | 'appearance' | 'media' | 'widgets' | 'plugins';

export default function Settings() {
    const widgets = useWidgetStore(state => state.widgets);
    const enabledState = useWidgetStore(state => state.enabledState);
    const toggleWidget = useWidgetStore(state => state.toggleWidget);
    const [settings, setSettings] = useState<SettingsState>({
        showMedia: true,
        liquidGlassMode: false,
        nonNotchMode: false,
        islandColor: '#000000',
        islandPositionX: 50,
        islandPositionY: 0,
    });
    const [activeTab, setActiveTab] = useState<Tab>('general');

    // Initialize widget store
    useEffect(() => {
        // Load widgets on mount
        useWidgetStore.getState().loadWidgets();

        // Setup listeners for cross-window sync and plugin hot-reload
        const cleanupWidget = useWidgetStore.getState().setupListeners();

        return () => {
            import('@tauri-apps/api/core').then(({ invoke }) => {
                invoke('deactivate_window');
            });
            cleanupWidget();
        };
    }, []);

    useEffect(() => {
        const saved = localStorage.getItem('app-settings');
        if (saved) {
            try {
                const parsed = JSON.parse(saved);
                setSettings({
                    showMedia: true,
                    liquidGlassMode: false,
                    nonNotchMode: false,
                    islandColor: '#000000',
                    ...parsed,
                    islandPositionX: clampIslandPosition(parsed.islandPositionX, 50),
                    islandPositionY: clampIslandPosition(parsed.islandPositionY, 0),
                });
            } catch (e) {
                console.error("Failed to parse settings", e);
            }
        }
    }, []);

    const persistSettings = (next: SettingsState) => {
        localStorage.setItem('app-settings', JSON.stringify(next));
        window.dispatchEvent(new StorageEvent('storage', {
            key: 'app-settings',
            newValue: JSON.stringify(next)
        }));

        import('@tauri-apps/api/core').then(({ invoke }) => {
            invoke('update_window_settings', {
                extraWidth: 400.0,
                extraHeight: 800.0,
                non_notch_mode: next.nonNotchMode,
                positionX: next.islandPositionX,
                positionY: next.islandPositionY,
            }).catch(console.error);
        });
    };

    const updateSetting = <K extends keyof SettingsState>(key: K, value: SettingsState[K]) => {
        setSettings(prev => {
            const next = { ...prev, [key]: value };
            persistSettings(next);
            return next;
        });
    };

    return (
        <div className="flex w-full h-screen bg-background text-foreground overflow-hidden font-sans selection:bg-white/20">
            {/* Sidebar with top padding for traffic lights */}
            <aside
                data-tauri-drag-region
                className="w-48 bg-sidebar border-r border-sidebar-border flex flex-col pt-10 pb-4 px-2 select-none text-sidebar-foreground relative"
            >
                {/* Traffic lights drag area overlay */}
                <div data-tauri-drag-region className="absolute top-0 left-0 w-full h-10 z-50" />

                {/* Traffic lights area is essentially empty space in the sidebar now */}
                <div data-tauri-drag-region className="px-3 mb-6 mt-4 flex items-center gap-2 opacity-50">
                    <SettingsIcon className="w-4 h-4" />
                    <span className="text-xs font-medium tracking-wide uppercase">Settings</span>
                </div>

                <nav className="flex-1 space-y-1">
                    <SidebarItem
                        active={activeTab === 'general'}
                        onClick={() => setActiveTab('general')}
                        icon={<SettingsIcon className="w-4 h-4" />}
                        label="General"
                    />
                    <SidebarItem
                        active={activeTab === 'appearance'}
                        onClick={() => setActiveTab('appearance')}
                        icon={<Palette className="w-4 h-4" />}
                        label="Appearance"
                    />
                    <SidebarItem
                        active={activeTab === 'media'}
                        onClick={() => setActiveTab('media')}
                        icon={<Music className="w-4 h-4" />}
                        label="Media"
                    />
                    <SidebarItem
                        active={activeTab === 'widgets'}
                        onClick={() => setActiveTab('widgets')}
                        icon={<Grid className="w-4 h-4" />}
                        label="Widgets"
                    />
                    <SidebarItem
                        active={activeTab === 'plugins'}
                        onClick={() => setActiveTab('plugins')}
                        icon={<Puzzle className="w-4 h-4" />}
                        label="Plugins"
                    />
                </nav>
            </aside>

            {/* Content Area */}
            <main className="flex-1 overflow-y-auto bg-background">
                {/* Drag region header for the content area too */}
                <div data-tauri-drag-region className="h-10 w-full shrink-0 sticky top-0 z-10" />

                <div className="px-8 pb-10 max-w-2xl">
                    <h1 className="text-2xl font-semibold mb-6 capitalize">{activeTab}</h1>

                    {activeTab === 'general' && (
                        <div className="space-y-6">
                            <div className="bg-card rounded-2xl mb-6 overflow-hidden border border-border">
                                <div className="flex items-center justify-between p-3 px-4 border-b border-border last:border-b-0">
                                    <div className="flex flex-col">
                                        <Label className="text-[15px] font-normal">Launch at Login</Label>
                                    </div>
                                    <Switch disabled />
                                </div>
                            </div>
                        </div>
                    )}

                    {activeTab === 'appearance' && (
                        <div className="space-y-6">
                            <div className="bg-card rounded-2xl mb-6 overflow-hidden border border-border">
                                <div className="flex items-center justify-between p-3 px-4 border-b border-border last:border-b-0">
                                    <div className="flex flex-col">
                                        <Label className="text-[15px] font-normal">Liquid Glass Mode</Label>
                                        <span className="text-xs text-muted-foreground mt-0.5">Experimental translucency effect</span>
                                    </div>
                                    <Switch
                                        checked={settings.liquidGlassMode}
                                        onCheckedChange={(c) => updateSetting('liquidGlassMode', c)}
                                    />
                                </div>
                            </div>
                            <div className="bg-card rounded-2xl mb-6 overflow-hidden border border-border">
                                <div className="flex items-center justify-between p-3 px-4 border-b border-border last:border-b-0">
                                    <div className="flex flex-col">
                                        <Label className="text-[15px] font-normal">Non-Notch Mode</Label>
                                        <span className="text-xs text-muted-foreground mt-0.5">Hide island when idle (hover top to show)</span>
                                    </div>
                                    <Switch
                                        checked={settings.nonNotchMode}
                                        onCheckedChange={(c) => updateSetting('nonNotchMode', c)}
                                    />
                                </div>
                            </div>
                            <div className="bg-card rounded-2xl mb-6 overflow-hidden border border-border">
                                <div className="flex items-center justify-between p-3 px-4 border-b border-border last:border-b-0">
                                    <div className="flex flex-col">
                                        <Label className="text-[15px] font-normal">Island Color</Label>
                                        <span className="text-xs text-muted-foreground mt-0.5">Background color of the dynamic island</span>
                                    </div>
                                    <input
                                        type="color"
                                        aria-label="Island color"
                                        value={/^#[0-9a-fA-F]{6}$/.test(settings.islandColor) ? settings.islandColor : '#000000'}
                                        onChange={(e) => updateSetting('islandColor', e.target.value)}
                                        className="h-8 w-8 cursor-pointer rounded-md border border-border bg-transparent p-0.5"
                                    />
                                </div>
                            </div>
                            <div className="bg-card rounded-2xl mb-6 overflow-hidden border border-border">
                                <div className="flex flex-col gap-4 p-3 px-4">
                                    <div className="flex items-start justify-between gap-3">
                                        <div className="flex flex-col">
                                            <Label className="text-[15px] font-normal">Island Position</Label>
                                            <span className="text-xs text-muted-foreground mt-0.5">Move the island anywhere on the screen</span>
                                        </div>
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="sm"
                                            className="h-7 px-2 text-xs text-muted-foreground"
                                            onClick={() => {
                                                setSettings(prev => {
                                                    const next = { ...prev, islandPositionX: 50, islandPositionY: 0 };
                                                    persistSettings(next);
                                                    return next;
                                                });
                                            }}
                                        >
                                            Reset
                                        </Button>
                                    </div>
                                    <div className="flex flex-col gap-2">
                                        <div className="flex items-center justify-between">
                                            <span className="text-xs text-muted-foreground">Horizontal</span>
                                            <span className="text-xs tabular-nums text-muted-foreground">{Math.round(settings.islandPositionX)}</span>
                                        </div>
                                        <Slider
                                            min={0}
                                            max={100}
                                            step={1}
                                            value={[settings.islandPositionX]}
                                            onValueChange={([value]) => updateSetting('islandPositionX', value)}
                                            aria-label="Island horizontal position"
                                        />
                                    </div>
                                    <div className="flex flex-col gap-2">
                                        <div className="flex items-center justify-between">
                                            <span className="text-xs text-muted-foreground">Vertical</span>
                                            <span className="text-xs tabular-nums text-muted-foreground">{Math.round(settings.islandPositionY)}</span>
                                        </div>
                                        <Slider
                                            min={0}
                                            max={100}
                                            step={1}
                                            value={[settings.islandPositionY]}
                                            onValueChange={([value]) => updateSetting('islandPositionY', value)}
                                            aria-label="Island vertical position"
                                        />
                                    </div>
                                </div>
                            </div>
                        </div>
                    )}

                    {activeTab === 'media' && (
                        <div className="space-y-6">
                            <div className="bg-card rounded-2xl mb-6 overflow-hidden border border-border">
                                <div className="flex items-center justify-between p-3 px-4 border-b border-border last:border-b-0">
                                    <div className="flex flex-col">
                                        <Label className="text-[15px] font-normal">Media Controls</Label>
                                        <span className="text-xs text-muted-foreground mt-0.5">Show now playing info</span>
                                    </div>
                                    <Switch
                                        checked={settings.showMedia}
                                        onCheckedChange={(c) => updateSetting('showMedia', c)}
                                    />
                                </div>
                            </div>
                        </div>
                    )}

                    {activeTab === 'widgets' && (
                        <div className="space-y-8">
                            {Object.entries(
                                widgets.reduce((acc, widget) => {
                                    const category = widget.category || 'other';
                                    if (!acc[category]) acc[category] = [];
                                    acc[category].push(widget);
                                    return acc;
                                }, {} as Record<string, typeof widgets>)
                            ).map(([category, categoryWidgets]) => (
                                <div key={category}>
                                    <h3 className="text-sm font-medium text-white/40 uppercase tracking-wider mb-3 px-1">
                                        {category}
                                    </h3>
                                    <div className="bg-card rounded-2xl mb-6 overflow-hidden border border-border">
                                        {categoryWidgets.map(widget => (
                                            <WidgetToggle
                                                key={widget.id}
                                                widget={widget}
                                                active={enabledState[widget.id] ?? widget.defaultEnabled}
                                                onToggle={() => toggleWidget(widget.id)}
                                            />
                                        ))}
                                    </div>
                                </div>
                            ))}
                        </div>
                    )}

                    {activeTab === 'plugins' && (
                        <div className="space-y-6">
                            <PluginStore />
                        </div>
                    )}
                </div>
            </main>
        </div>
    );
}

function SidebarItem({ active, onClick, icon, label }: { active: boolean, onClick: () => void, icon: React.ReactNode, label: string }) {
    return (
        <Button
            variant="ghost"
            onClick={onClick}
            className={cn(
                "w-full justify-start gap-3 px-3 py-2 h-auto font-normal",
                active
                    ? 'bg-primary/90 text-primary-foreground shadow-sm hover:bg-primary/90 hover:text-primary-foreground'
                    : 'text-muted-foreground hover:text-foreground hover:bg-white/5'
            )}
        >
            {icon}
            <span>{label}</span>
        </Button>
    );
}

function WidgetToggle({ widget, active, onToggle }: { widget: any, active: boolean, onToggle: () => void }) {
    return (
        <div className="flex items-center justify-between p-3 px-4 border-b border-border last:border-b-0">
            <div className="flex flex-col">
                <Label className="text-[15px] font-normal">{widget.name}</Label>
                <span className="text-xs text-muted-foreground mt-0.5">{widget.description}</span>
            </div>
            <Switch
                checked={active}
                onCheckedChange={() => onToggle()}
            />
        </div>
    );
}
