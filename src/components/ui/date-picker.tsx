"use strict";
import * as React from "react";
import { format, isValid } from "date-fns";
import { Calendar as CalendarIcon } from "lucide-react";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Calendar } from "@/components/ui/calendar";
import {
    Popover,
    PopoverContent,
    PopoverTrigger,
} from "@/components/ui/popover";

export interface DatePickerProps {
    date?: Date;
    setDate: (date?: Date) => void;
    placeholder?: string;
    className?: string;
    showTime?: boolean;
    onOpenChange?: (open: boolean) => void;
}

export const DatePicker = React.forwardRef<HTMLButtonElement, DatePickerProps>(
    ({ date, setDate, placeholder = "Pick a date", className, showTime = false, onOpenChange, ...props }, ref) => {
        const [isOpen, setIsOpen] = React.useState(false);
        // Ensure we have a valid date object for display and calendar
        const validDate = date && isValid(date) ? date : undefined;

        const handleOpenChange = (open: boolean) => {
            setIsOpen(open);
            onOpenChange?.(open);
        };

        const handleTimeChange = (type: 'hour' | 'minute', value: string) => {
            if (!validDate) return;
            const newDate = new Date(validDate);
            if (type === 'hour') {
                const h = parseInt(value);
                if (!isNaN(h)) newDate.setHours(h);
            } else {
                const m = parseInt(value);
                if (!isNaN(m)) newDate.setMinutes(m);
            }
            setDate(newDate);
        };

        const containerElement = typeof document !== 'undefined' ? document.getElementById('popover-mount') : null;

        return (
            <Popover open={isOpen} onOpenChange={handleOpenChange} modal={false}>
                <PopoverTrigger asChild>
                    <Button
                        ref={ref}
                        variant={"outline"}
                        className={cn(
                            "w-full justify-start text-left font-normal h-10 px-3 bg-[#1e1e1e] border-white/10 hover:bg-white/5 text-white/90 hover:text-white transition-colors",
                            !validDate && "text-white/40",
                            className
                        )}
                        type="button"
                        onClick={(e) => e.stopPropagation()}
                        {...props}
                    >
                        <CalendarIcon className="mr-2 h-4 w-4" />
                        {validDate ? format(validDate, showTime ? "PPP HH:mm" : "PPP") : <span>{placeholder}</span>}
                    </Button>
                </PopoverTrigger>
                <PopoverContent
                    className="w-auto p-0 z-[20000]"
                    align="start"
                    side="bottom"
                    sideOffset={10}
                    collisionPadding={10}
                    container={containerElement}
                    onPointerDownOutside={(e) => e.preventDefault()}
                    onInteractOutside={(e) => e.preventDefault()}
                >
                    <div className="flex flex-col">
                        <Calendar
                            mode="single"
                            selected={validDate}
                            onSelect={(d) => {
                                if (d && validDate && showTime) {
                                    // Keep time if we already have a date
                                    d.setHours(validDate.getHours());
                                    d.setMinutes(validDate.getMinutes());
                                }
                                setDate(d);
                                if (!showTime) {
                                    // Close after selecting if not showing time
                                    handleOpenChange(false);
                                }
                            }}
                            initialFocus
                        />
                        {showTime && validDate && (
                            <div className="flex items-center justify-center p-3 border-t border-white/10 gap-2">
                                <input
                                    type="number"
                                    min="0"
                                    max="23"
                                    value={validDate.getHours().toString().padStart(2, '0')}
                                    onChange={(e) => handleTimeChange('hour', e.target.value)}
                                    className="w-12 bg-white/5 border border-white/10 rounded px-1 text-center text-sm text-white"
                                />
                                <span className="text-white/40">:</span>
                                <input
                                    type="number"
                                    min="0"
                                    max="59"
                                    value={validDate.getMinutes().toString().padStart(2, '0')}
                                    onChange={(e) => handleTimeChange('minute', e.target.value)}
                                    className="w-12 bg-white/5 border border-white/10 rounded px-1 text-center text-sm text-white"
                                />
                                <Button
                                    type="button"
                                    size="sm"
                                    variant="ghost"
                                    className="ml-2 text-xs"
                                    onClick={() => handleOpenChange(false)}
                                >
                                    Done
                                </Button>
                            </div>
                        )}
                    </div>
                </PopoverContent>
            </Popover>
        );
    }
);

DatePicker.displayName = "DatePicker";
