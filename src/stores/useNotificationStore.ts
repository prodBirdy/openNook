import { create } from 'zustand';

export type NotificationType = 'hydration' | 'screenBreak';

export interface Notification {
    id: string;
    type: NotificationType;
    message: string;
    icon: string;
}

const ICONS: Record<NotificationType, string> = {
    hydration: '💧',
    screenBreak: '👁️',
};

interface NotificationState {
    notification: Notification | null;
}

interface NotificationActions {
    showNotification: (type: NotificationType, message: string) => void;
    dismissNotification: () => void;
}

type NotificationStore = NotificationState & NotificationActions;

export const useNotificationStore = create<NotificationStore>((set) => ({
    notification: {
        id: 'dev-hydration',
        type: 'hydration',
        message: 'OpenNook is running!',
        icon: ICONS['hydration'],
    },

    showNotification: (type, message) => {
        set({
            notification: {
                id: crypto.randomUUID(),
                type,
                message,
                icon: ICONS[type],
            }
        });
    },

    dismissNotification: () => {
        set({ notification: null });
    }
}));

// Selectors
export const selectNotification = (state: NotificationStore) => state.notification;
