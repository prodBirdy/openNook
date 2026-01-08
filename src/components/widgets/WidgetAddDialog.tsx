import React, { ReactNode } from 'react';
import { useForm, SubmitHandler, Path, DefaultValues } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import {
    Form,
    FormControl,
    FormField,
    FormItem,
    FormLabel,
    FormMessage,
} from '@/components/ui/form';
import {
    InputGroup,
    InputGroupAddon,
    InputGroupInput,
} from '@/components/ui/input-group';
import { Button } from '@/components/ui/button';
import { XIcon } from 'lucide-react';
import { DatePicker } from '@/components/ui/date-picker';
import { usePopoverContext } from '@/context/PopoverContext';

// Generic form field configuration
export interface FormFieldConfig {
    name: string;
    label?: string;
    placeholder?: string;
    type?: React.HTMLInputTypeAttribute;
    icon?: ReactNode;
    required?: boolean;
    autoFocus?: boolean;
}

// Props for WidgetAddDialog with inferred schema type
interface WidgetAddDialogProps<TSchema extends z.ZodSchema> {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    title: string;
    schema: TSchema;
    defaultValues?: DefaultValues<z.infer<TSchema>>;
    onSubmit: SubmitHandler<z.infer<TSchema>>;
    fields: FormFieldConfig[];
    submitLabel?: string;
    submitIcon?: ReactNode;
    children?: ReactNode | ((form: ReturnType<typeof useForm<z.infer<TSchema>>>) => ReactNode);
}

export function WidgetAddDialog<TSchema extends z.ZodSchema>({
    open,
    onOpenChange,
    title,
    schema,
    defaultValues,
    onSubmit,
    fields,
    submitLabel = "Add",
    submitIcon,
    children,
}: WidgetAddDialogProps<TSchema>) {
    type FormData = z.infer<TSchema>;
    const { setIsPopoverOpen } = usePopoverContext();

    const form = useForm<FormData>({
        resolver: zodResolver(schema),
        defaultValues: defaultValues,
    });

    // Reset form when opening (only on transition from closed to open)
    const wasOpen = React.useRef(open);
    React.useEffect(() => {
        if (open && !wasOpen.current) {
            form.reset(defaultValues);
        }
        wasOpen.current = open;
    }, [open, defaultValues, form]);

    const handleSubmit = async (data: FormData) => {
        try {
            await onSubmit(data);
            form.reset();
            onOpenChange(false);
        } catch (error) {
            console.error("Form submission error:", error);
        }
    };

    if (!open) return null;

    return (
        <div
            className="absolute p-4 inset-0 z-100 flex flex-col bg-card backdrop-blur-[25px]  animate-in fade-in zoom-in-95 duration-200  "
            onClick={(e) => e.stopPropagation()}
            onPointerDown={(e) => e.stopPropagation()}
        >
            <div className="flex items-center justify-between w-full">
                <div>
                    <h3 className="text-lg font-semibold leading-none tracking-tight">{title}</h3>
                </div>
                <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 hover:bg-accent rounded-full -mr-2 -mt-2"
                    onClick={() => onOpenChange(false)}
                >
                    <span className="sr-only">Close</span>
                    <XIcon className="h-4 w-4" />
                </Button>
            </div>

            <div className="flex-1 overflow-y-auto overflow-x-visible relative w-full">
                <Form {...form}>
                    <form onSubmit={form.handleSubmit(handleSubmit)} className="space-y-4 pb-4">
                        {fields.map((fieldConfig) => (
                            <FormField
                                key={fieldConfig.name}
                                control={form.control}
                                name={fieldConfig.name as Path<FormData>}
                                render={({ field }) => (
                                    <FormItem className="space-y-1">
                                        {fieldConfig.label && (
                                            <FormLabel className="text-xs font-medium">{fieldConfig.label}</FormLabel>
                                        )}
                                        <FormControl>
                                            {fieldConfig.type === 'date' || fieldConfig.type === 'datetime-local' ? (
                                                <DatePicker
                                                    date={field.value ? new Date(field.value) : undefined}
                                                    setDate={(date: Date | undefined) => {
                                                        if (date) {
                                                            field.onChange(date.toISOString());
                                                        } else {
                                                            field.onChange(undefined);
                                                        }
                                                    }}
                                                    placeholder={fieldConfig.placeholder}
                                                    showTime={fieldConfig.type === 'datetime-local'}
                                                    onOpenChange={setIsPopoverOpen}
                                                />
                                            ) : (
                                                <InputGroup >
                                                    {fieldConfig.icon && (
                                                        <InputGroupAddon>
                                                            {fieldConfig.icon}
                                                        </InputGroupAddon>
                                                    )}
                                                    <InputGroupInput
                                                        {...field}
                                                        value={field.value ?? ''}
                                                        type={fieldConfig.type || 'text'}
                                                        placeholder={fieldConfig.placeholder}
                                                        autoFocus={fieldConfig.autoFocus}
                                                    />
                                                </InputGroup>
                                            )}
                                        </FormControl>
                                        <FormMessage className="text-xs" />
                                    </FormItem>
                                )}
                            />
                        ))}

                        {typeof children === 'function' ? (children as Function)(form) : children}

                        <div className="pt-4">
                            <Button variant="default" type="submit" className="w-full" disabled={form.formState.isSubmitting}>
                                {submitIcon && (
                                    <span className="mr-2 flex items-center justify-center">
                                        {submitIcon}
                                    </span>
                                )}
                                {submitLabel}
                            </Button>
                        </div>
                    </form>
                </Form>
            </div>
        </div>
    );
}
