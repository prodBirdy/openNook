import { DynamicIsland } from './components/DynamicIsland';
import Settings from './windows/Settings/Settings';
import { NotificationProvider } from './context/NotificationContext';
import { useReminders } from './hooks/useReminders';
import './App.css';

function ReminderManager() {
  useReminders();
  return null;
}

function App() {
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
