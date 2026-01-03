import { NotificationProvider } from './context/NotificationContext';
import { DynamicIslandAlert } from './components/DynamicIslandAlert';
import { useReminders } from './hooks/useReminders';
import './App.css';

function ReminderManager() {
  useReminders();
  return null;
}

function App() {
  return (
    <NotificationProvider>
      <DynamicIslandAlert />
      <ReminderManager />
    </NotificationProvider>
  );
}

export default App;
