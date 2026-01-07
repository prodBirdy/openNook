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
        <div className={`widget-wrapper widget-card apple-style ${className}`} onClick={(e) => e.stopPropagation()} >
            <div className="widget-header" >
                {title && <span className="widget-title">{title}</span>}
                {headerActions && headerActions.length > 0 && (
                    <div className="widget-header-actions">
                        {headerActions}
                    </div>
                )}
            </div>
            <div className="widget-body" >
                {children}
            </div>
        </div>
    );
}
