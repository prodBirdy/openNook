import { createContext, useContext, ReactNode } from 'react';

interface PopoverContextType {
    setIsPopoverOpen: (open: boolean) => void;
}

const PopoverContext = createContext<PopoverContextType | null>(null);

export function PopoverProvider({ children, onOpenChange }: { children: ReactNode, onOpenChange: (open: boolean) => void }) {
    return (
        <PopoverContext.Provider value={{ setIsPopoverOpen: onOpenChange }}>
            {children}
        </PopoverContext.Provider>
    );
}

export function usePopoverContext() {
    const context = useContext(PopoverContext);
    // Return a no-op function if not within provider (for components that might be used outside)
    return context ?? { setIsPopoverOpen: () => { } };
}
