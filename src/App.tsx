import { DynamicIsland } from './components/DynamicIsland';
import Settings from './windows/Settings/Settings';
import { NotificationProvider } from './context/NotificationContext';
import { WidgetProvider } from './context/WidgetContext';
import { TimerProvider } from './context/TimerContext';
import { SessionProvider } from './context/SessionContext';
import { PopoverStateProvider } from './context/PopoverStateContext';
import './App.css';

// Import widget index to trigger all widget registrations
import './components/widgets';

import { invoke } from '@tauri-apps/api/core';
import { useEffect } from 'react';
import { listenForPluginChanges } from './services/pluginLoader';

function App() {
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
    return (
      <WidgetProvider>
        <TimerProvider>
          <SessionProvider>
            <Settings />
          </SessionProvider>
        </TimerProvider>
      </WidgetProvider>
    );
  }

  return (
    <WidgetProvider>
      <TimerProvider>
        <SessionProvider>
          <NotificationProvider>
            <PopoverStateProvider>
              <DynamicIsland />
              <div id="popover-mount" style={{ position: 'fixed', inset: 0, zIndex: 99999, pointerEvents: 'none' }} />
            </PopoverStateProvider>
          </NotificationProvider>
        </SessionProvider>
      </TimerProvider>
    </WidgetProvider>
  );
}

export default App;
