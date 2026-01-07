import React, { ReactNode } from 'react';

interface WidgetAddDialogProps {
    title: string;
    onClose: () => void;
    onSubmit: (e: React.FormEvent) => void;
    children?: ReactNode;
    submitLabel?: string;
    submitIcon?: ReactNode;
    submitDisabled?: boolean;
    mainInput?: React.InputHTMLAttributes<HTMLInputElement> & {
        icon?: ReactNode;
    };
}

export function WidgetAddDialog({
    title,
    onClose,
    onSubmit,
    children,
    submitLabel = "Add",
    submitIcon,
    submitDisabled,
    mainInput
}: WidgetAddDialogProps) {
    return (
        <div className="widget-overlay">
            <form onSubmit={onSubmit} className="creation-form">
                <div className="form-header">
                    <span className="form-title">{title}</span>
                    <button type="button" className="close-button" onClick={onClose}>Cancel</button>
                </div>

                {mainInput && (
                    <div style={{ position: 'relative' }}>
                        <input
                            className="form-input"
                            style={{ paddingLeft: mainInput.icon ? 42 : 12 }}
                            autoFocus
                            {...mainInput}
                        />
                        {mainInput.icon && (
                            <div style={{
                                position: 'absolute',
                                left: 10,
                                top: '50%',
                                transform: 'translateY(-50%)',
                                pointerEvents: 'none',
                                opacity: 0.8,
                                display: 'flex',
                                alignItems: 'center',
                                justifyContent: 'center'
                            }}>
                                {mainInput.icon}
                            </div>
                        )}
                    </div>
                )}

                {children}

                <button
                    type="submit"
                    className="submit-button"
                    disabled={submitDisabled}
                >
                    {submitIcon && (
                        <span style={{ marginRight: 4, display: 'flex', alignItems: 'center' }}>
                            {submitIcon}
                        </span>
                    )}
                    {submitLabel}
                </button>
            </form>
        </div>
    );
}
