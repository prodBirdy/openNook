import { ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import { XIcon } from 'lucide-react';

interface WidgetDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    title: string;
    children: ReactNode;
    className?: string;
    headerActions?: ReactNode;
    variant?: 'default' | 'fullscreen';
}

export function WidgetDialog({
    open,
    onOpenChange,
    title,
    children,
    className = "",
    headerActions,
    variant = 'default',
}: WidgetDialogProps) {

    if (!open) return null;

    return (
        <div
            className={`absolute inset-0 z-50 flex flex-col bg-card/95 backdrop-blur-xl animate-in fade-in zoom-in-95 duration-200 ${className}`}
            onClick={(e) => e.stopPropagation()}
            onPointerDown={(e) => e.stopPropagation()}
        >
            {variant === 'default' ? (
                <div className="flex items-center justify-between w-full p-4 pb-2 shrink-0">
                    <div className="flex items-center gap-2">
                        <h3 className="text-lg font-semibold leading-none tracking-tight">{title}</h3>
                    </div>
                    <div className="flex items-center gap-1">
                        {headerActions}
                        <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 hover:bg-accent rounded-full"
                            onClick={() => onOpenChange(false)}
                        >
                            <span className="sr-only">Close</span>
                            <XIcon className="h-4 w-4" />
                        </Button>
                    </div>
                </div>
            ) : (
                <Button
                    variant="ghost"
                    size="icon"
                    className="absolute top-2 left-2 z-50 h-8 w-8 hover:bg-black/20 text-white/70 hover:text-white rounded-full bg-black/10 backdrop-blur-md"
                    onClick={() => onOpenChange(false)}
                >
                    <span className="sr-only">Close</span>
                    <XIcon className="h-4 w-4" />
                </Button>
            )}

            <div className={`flex-1 overflow-hidden relative w-full ${variant === 'fullscreen' ? 'h-full' : ''}`}>
                {children}
            </div>
        </div>
    );
}
