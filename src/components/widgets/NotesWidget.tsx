import { useState, useEffect } from 'react';
import { IconNotebook, IconEdit, IconEye, IconDeviceFloppy } from '@tabler/icons-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { registerWidget } from './WidgetRegistry';
import { WidgetWrapper } from './WidgetWrapper';
import { useDynamicIslandStore } from '../../stores/useDynamicIslandStore';
import { Textarea } from '@/components/ui/textarea';
import { Button } from '@/components/ui/button';

export function NotesWidget() {
    const { notes, saveNotes, loadNotes } = useDynamicIslandStore();
    const [isEditing, setIsEditing] = useState(false);
    const [localNotes, setLocalNotes] = useState(notes);

    useEffect(() => {
        loadNotes();
    }, [loadNotes]);

    useEffect(() => {
        setLocalNotes(notes);
    }, [notes]);

    const handleSave = async () => {
        await saveNotes(localNotes);
        setIsEditing(false);
    };

    return (
        <WidgetWrapper
            title="Notes"
            className="flex flex-col p-5 h-full box-border overflow-hidden"
            headerActions={[
                <Button
                    key="toggle-edit"
                    variant="ghost"
                    size="icon"
                    className="w-8 h-8 rounded-full text-white/40 hover:bg-white/10 hover:text-white"
                    onClick={() => {
                        if (isEditing) {
                            handleSave();
                        } else {
                            setIsEditing(true);
                        }
                    }}
                >
                    {isEditing ? <IconDeviceFloppy size={18} /> : <IconEdit size={18} />}
                </Button>,
                isEditing && (
                    <Button
                        key="view"
                        variant="ghost"
                        size="icon"
                        className="w-8 h-8 rounded-full text-white/40 hover:bg-white/10 hover:text-white"
                        onClick={() => setIsEditing(false)}
                    >
                        <IconEye size={18} />
                    </Button>
                )
            ].filter(Boolean)}
        >
            <div className="flex-1 flex flex-col min-h-0 mt-2">
                {isEditing ? (
                    <Textarea
                        value={localNotes}
                        onChange={(e) => setLocalNotes(e.target.value)}
                        onKeyDown={(e) => {
                            if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                                handleSave();
                            }
                        }}
                        placeholder="Type your notes here... (Markdown supported)"
                        className="flex-1 resize-none bg-white/5 border-white/10 focus:border-white/20 text-white font-mono text-sm p-4 rounded-xl h-full"
                        autoFocus
                    />
                ) : (
                    <div 
                        className="flex-1 overflow-y-auto pr-2 text-white/90 prose prose-invert prose-sm max-w-none scrollbar-hide cursor-text"
                        onClick={() => setIsEditing(true)}
                    >
                        {localNotes ? (
                            <ReactMarkdown remarkPlugins={[remarkGfm]}>{localNotes}</ReactMarkdown>
                        ) : (
                            <div className="h-full flex flex-col items-center justify-center text-white/20 italic">
                                <span>Click to add notes</span>
                            </div>
                        )}
                    </div>
                )}
            </div>
        </WidgetWrapper>
    );
}

// Register the notes widget
registerWidget({
    id: 'notes',
    name: 'Notes',
    description: 'Markdown notes editor',
    icon: IconNotebook,
    ExpandedComponent: NotesWidget,
    defaultEnabled: true,
    category: 'productivity',
    minWidth: 300,
    hasCompactMode: false,
});
