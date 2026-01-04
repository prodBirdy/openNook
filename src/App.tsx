import { DynamicIsland } from './components/DynamicIsland';
import { NotificationProvider } from './context/NotificationContext';
import { useReminders } from './hooks/useReminders';
import './App.css';

function ReminderManager() {
  useReminders();
  return null;
}

function App() {
  return (
    <NotificationProvider>
      <DynamicIsland />
      <ReminderManager />
    </NotificationProvider>
  );
}

export default App;
