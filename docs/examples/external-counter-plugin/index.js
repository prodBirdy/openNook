/**
 * External Counter Plugin - Example
 *
 * Demonstrates how to create an external plugin using WidgetWrapper.
 * Uses React.createElement for browser compatibility.
 */

(function () {
  // Wait for the plugin API to be available
  function init() {
    if (!window.__openNookPluginAPI__) {
      console.warn('Plugin API not available yet, retrying...');
      setTimeout(init, 100);
      return;
    }

    const { registerWidget, React, WidgetWrapper, IconBox } = window.__openNookPluginAPI__;
    const { useState, useEffect, createElement: h } = React;

    const STORAGE_KEY = 'external-counter-value';

    /**
     * Main widget component (expanded view)
     */
    function ExternalCounterWidget() {
      const [count, setCount] = useState(() => {
        const saved = localStorage.getItem(STORAGE_KEY);
        return saved ? parseInt(saved, 10) : 0;
      });

      useEffect(() => {
        localStorage.setItem(STORAGE_KEY, count.toString());
      }, [count]);

      return h(WidgetWrapper, { title: 'Counter', icon: IconBox },
        h('div', { className: 'flex flex-col items-center justify-center gap-4 py-4' }, [
          // Counter display
          h('div', {
            key: 'count',
            className: 'text-5xl font-bold text-white tabular-nums'
          }, count),

          // Buttons
          h('div', { key: 'buttons', className: 'flex gap-3' }, [
            h('button', {
              key: 'dec',
              onClick: (e) => { e.stopPropagation(); setCount(c => c - 1); },
              className: 'flex h-10 w-10 items-center justify-center rounded-full bg-white/10 text-white text-xl hover:bg-white/20 transition-colors'
            }, '−'),
            h('button', {
              key: 'reset',
              onClick: (e) => { e.stopPropagation(); setCount(0); },
              className: 'flex h-10 w-10 items-center justify-center rounded-full bg-white/5 text-white/60 text-sm hover:bg-white/10 hover:text-white transition-colors'
            }, '↺'),
            h('button', {
              key: 'inc',
              onClick: (e) => { e.stopPropagation(); setCount(c => c + 1); },
              className: 'flex h-10 w-10 items-center justify-center rounded-full bg-blue-500 text-white text-xl hover:bg-blue-400 transition-colors'
            }, '+')
          ])
        ])
      );
    }

    /**
     * Compact widget component
     */
    function CompactExternalCounter(props) {
      const [count, setCount] = useState(() => {
        const saved = localStorage.getItem(STORAGE_KEY);
        return saved ? parseInt(saved, 10) : 0;
      });

      useEffect(() => {
        const interval = setInterval(() => {
          const saved = localStorage.getItem(STORAGE_KEY);
          if (saved) setCount(parseInt(saved, 10));
        }, 500);
        return () => clearInterval(interval);
      }, []);

      return h('div', {
        style: { opacity: props.contentOpacity },
        className: 'flex items-center gap-1 text-xs text-white/80'
      }, [
        h(IconBox, { key: 'icon', size: 14 }),
        h('span', { key: 'count', className: 'tabular-nums font-medium' }, count)
      ]);
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
  }

  init();
})();
