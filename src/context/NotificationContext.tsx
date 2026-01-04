import { createContext, useContext, useState, useCallback, ReactNode } from 'react';

export type NotificationType = 'hydration' | 'screenBreak';

export interface Notification {
    id: string;
    type: NotificationType;
    message: string;
    icon: string;
}

interface NotificationContextType {
    notification: Notification | null;
    showNotification: (type: NotificationType, message: string) => void;
    dismissNotification: () => void;
}

const NotificationContext = createContext<NotificationContextType | null>(null);

const ICONS: Record<NotificationType, string> = {
    hydration: '💧',
    screenBreak: '👁️',
};

export function NotificationProvider({ children }: { children: ReactNode }) {
    const [notification, setNotification] = useState<Notification | null>({
        id: 'dev-hydration',
        type: 'hydration',
        message: 'OpenNook is running!',
        icon: ICONS['hydration'],
    });

    const showNotification = useCallback((type: NotificationType, message: string) => {
        setNotification({
            id: crypto.randomUUID(),
            type,
            message,
            icon: ICONS[type],
        });
    }, []);

    const dismissNotification = useCallback(() => {
        setNotification(null);
    }, []);

    return (
        <NotificationContext.Provider value={{ notification, showNotification, dismissNotification }}>
            {children}
        </NotificationContext.Provider>
    );
}

export function useNotification() {
    const context = useContext(NotificationContext);
    if (!context) {
        throw new Error('useNotification must be used within NotificationProvider');
    }
    return context;
}
