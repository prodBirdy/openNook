import { ReactNode } from 'react';

interface WidgetWrapperProps {
    title?: string;
    headerActions?: ReactNode[];
    children: ReactNode;
    className?: string;
}

/**
 * A common wrapper for extended view widgets.
 * Provides consistent styling for the header and body across all widgets.
 */
export function WidgetWrapper({ title, headerActions, children, className = '' }: WidgetWrapperProps) {
    return (
        <div
            className={`relative flex h-full min-w-[300px] flex-1 flex-col overflow-hidden rounded-[28px] border border-white/10 bg-linear-to-br from-[rgba(30,30,32,0.7)] to-[rgba(20,20,22,0.8)] p-3 font-sans shadow-[0_8px_32px_rgba(0,0,0,0.4),inset_0_0_0_1px_rgba(255,255,255,0.05)] backdrop-blur-[30px] backdrop-saturate-200 ${className} p-4`}
            onClick={(e) => e.stopPropagation()}
            style={{
                padding: '1rem',
            }}
        >
            <div className=" flex items-center justify-between text-[17px] font-semibold tracking-[-0.01em] text-white/95" >
                {title && <span className="">{title}</span>}
                {headerActions && headerActions.length > 0 && (
                    <div className="flex items-center gap-[5px] overflow-hidden">
                        {headerActions}
                    </div>
                )}
            </div>
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden" >
                {children}
            </div>
        </div>
    );
}
