import { DynamicIsland } from './components/DynamicIsland';
import Settings from './windows/Settings/Settings';
import { NotificationProvider } from './context/NotificationContext';
import { WidgetProvider } from './context/WidgetContext';
import { TimerProvider } from './context/TimerContext';
import { SessionProvider } from './context/SessionContext';
import { useReminders } from './hooks/useReminders';
import './App.css';

// Import widget index to trigger all widget registrations
import './components/widgets';

import { invoke } from '@tauri-apps/api/core';
import { useEffect } from 'react';

function ReminderManager() {
  useReminders();
  return null;
}

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
            <DynamicIsland />
            <ReminderManager />
          </NotificationProvider>
        </SessionProvider>
      </TimerProvider>
    </WidgetProvider>
  );
}

export default App;
