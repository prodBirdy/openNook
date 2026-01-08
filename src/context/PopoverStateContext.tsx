import { createContext, useContext, useState, useCallback, ReactNode } from 'react';

interface PopoverStateContextType {
    isPopoverOpen: boolean;
    setPopoverOpen: (open: boolean) => void;
}

const PopoverStateContext = createContext<PopoverStateContextType | undefined>(undefined);

export function PopoverStateProvider({ children }: { children: ReactNode }) {
    const [isPopoverOpen, setIsPopoverOpen] = useState(false);

    const setPopoverOpen = useCallback((open: boolean) => {
        setIsPopoverOpen(open);
    }, []);

    return (
        <PopoverStateContext.Provider value={{ isPopoverOpen, setPopoverOpen }}>
            {children}
        </PopoverStateContext.Provider>
    );
}

export function usePopoverState() {
    const context = useContext(PopoverStateContext);
    if (!context) {
        throw new Error('usePopoverState must be used within a PopoverStateProvider');
    }
    return context;
}

// A simpler hook that returns a fallback if the provider isn't present
export function usePopoverStateOptional() {
    const context = useContext(PopoverStateContext);
    return context ?? { isPopoverOpen: false, setPopoverOpen: () => { } };
}
