import { DynamicIsland } from './components/DynamicIsland';
import Settings from './windows/Settings/Settings';
import { NotificationProvider } from './context/NotificationContext';
import { useReminders } from './hooks/useReminders';
import './App.css';

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
    return <Settings />;
  }

  return (
    <NotificationProvider>
      <DynamicIsland />
      <ReminderManager />
    </NotificationProvider>
  );
}

export default App;
