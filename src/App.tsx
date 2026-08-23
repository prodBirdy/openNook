import { DynamicIsland } from './components/island/DynamicIsland';
import Settings from './windows/Settings/Settings';
import './App.css';

// Import widget index to trigger all widget registrations
import './components/widgets';

import { invoke } from '@tauri-apps/api/core';
import { useEffect } from 'react';
import { listenForPluginChanges } from './services/pluginLoader';
import { useTimerStore } from './stores/useTimerStore';
import { useSessionStore } from './stores/useSessionStore';
import { useWidgetStore } from './stores/useWidgetStore';
import { useFileTrayStore } from './stores/useFileTrayStore';

function App() {
  // Initialize stores
  useEffect(() => {
    // Load all store data on mount
    useTimerStore.getState().loadTimers();
    useSessionStore.getState().loadSessions();
    useWidgetStore.getState().loadWidgets();
    useFileTrayStore.getState().loadFiles();

    // Setup listeners for cross-window sync and intervals
    const cleanupTimer = useTimerStore.getState().setupListeners();
    const cleanupSession = useSessionStore.getState().setupListeners();
    const cleanupWidget = useWidgetStore.getState().setupListeners();
    const cleanupFileTray = useFileTrayStore.getState().setupListeners();

    return () => {
      cleanupTimer();
      cleanupSession();
      cleanupWidget();
      cleanupFileTray();
    };
  }, []);

  useEffect(() => {
    // Fetch system accent color
    invoke<string>('get_system_accent_color')
      .then(color => {
        if (color) {
          document.documentElement.style.setProperty('--accent-color', color);
        }
      })
      .catch(err => console.error('Failed to get accent color:', err));

    // Listen for plugin changes from other windows (Settings)
    // This allows hot-reload when plugins are installed/deleted
    let unlistenPlugins: (() => void) | undefined;
    listenForPluginChanges().then(unlisten => {
      unlistenPlugins = unlisten;
    });

    return () => {
      if (unlistenPlugins) unlistenPlugins();
    };
  }, []);

  const isSettings = window.location.pathname === '/settings';

  if (isSettings) {
    return <Settings />;
  }

  return (
    <>
      <DynamicIsland />
      <div id="popover-mount" style={{ position: 'fixed', inset: 0, zIndex: 99999, pointerEvents: 'none' }} />
    </>
  );
}

export default App;
