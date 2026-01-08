/**
 * External Counter Plugin - Example
 *
 * Demonstrates how to create an external plugin using TSX.
 * Uses WidgetWrapper and other openNook UI components.
 */

// Access the plugin API from the global window object
const api = (window as any).__openNookPluginAPI__;

// Destructure what we need
const {
    registerWidget,
    React,
    WidgetWrapper,
    IconBox
} = api;

const { useState, useEffect } = React;

// Storage key for persistence
const STORAGE_KEY = 'external-counter-value';

/**
 * Main widget component (expanded view)
 */
function ExternalCounterWidget() {
    const [count, setCount] = useState(() => {
        const saved = localStorage.getItem(STORAGE_KEY);
        return saved ? parseInt(saved, 10) : 0;
    });

    // Persist count
    useEffect(() => {
        localStorage.setItem(STORAGE_KEY, count.toString());
    }, [count]);

    return (
        <WidgetWrapper title="Counter" icon={IconBox}>
            <div className="flex flex-col items-center justify-center gap-4 py-4">
                {/* Counter display */}
                <div className="text-5xl font-bold text-white tabular-nums">
                    {count}
                </div>

                {/* Buttons */}
                <div className="flex gap-3">
                    {/* Decrement */}
                    <button
                        onClick={(e: React.MouseEvent) => { e.stopPropagation(); setCount((c: number) => c - 1); }}
                        className="flex h-10 w-10 items-center justify-center rounded-full bg-white/10 text-white text-xl hover:bg-white/20 transition-colors"
                    >
                        −
                    </button>

                    {/* Reset */}
                    <button
                        onClick={(e: React.MouseEvent) => { e.stopPropagation(); setCount(0); }}
                        className="flex h-10 w-10 items-center justify-center rounded-full bg-white/5 text-white/60 text-sm hover:bg-white/10 hover:text-white transition-colors"
                    >
                        ↺
                    </button>

                    {/* Increment */}
                    <button
                        onClick={(e: React.MouseEvent) => { e.stopPropagation(); setCount((c: number) => c + 1); }}
                        className="flex h-10 w-10 items-center justify-center rounded-full bg-blue-500 text-white text-xl hover:bg-blue-400 transition-colors"
                    >
                        +
                    </button>
                </div>
            </div>
        </WidgetWrapper>
    );
}

/**
 * Compact widget component (notification bar)
 */
function CompactExternalCounter({ contentOpacity }: { contentOpacity: number }) {
    const [count, setCount] = useState(() => {
        const saved = localStorage.getItem(STORAGE_KEY);
        return saved ? parseInt(saved, 10) : 0;
    });

    // Poll for changes (since we can't share state easily)
    useEffect(() => {
        const interval = setInterval(() => {
            const saved = localStorage.getItem(STORAGE_KEY);
            if (saved) setCount(parseInt(saved, 10));
        }, 500);
        return () => clearInterval(interval);
    }, []);

    return (
        <div
            style={{ opacity: contentOpacity }}
            className="flex items-center gap-1 text-xs text-white/80"
        >
            <IconBox size={14} />
            <span className="tabular-nums font-medium">{count}</span>
        </div>
    );
}

// Register the widget
registerWidget({
    id: 'external-counter',
    name: 'External Counter',
    description: 'Example external plugin - a simple counter',
    icon: IconBox,
    ExpandedComponent: ExternalCounterWidget,
    CompactComponent: CompactExternalCounter,
    defaultEnabled: false,
    category: 'utility',
    minWidth: 200,
    hasCompactMode: true,
    compactPriority: 100
});

console.log('✅ External Counter plugin registered');
