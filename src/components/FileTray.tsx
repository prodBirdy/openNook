import { useState, useCallback, useRef } from 'react';
import { IconX, IconUpload } from '@tabler/icons-react';
import { motion, AnimatePresence } from 'motion/react';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { Button } from './ui/button';
import { useFileTrayStore } from '../stores/useFileTrayStore';

export interface FileItem {
    name: string;
    size: number;
    path?: string; // Optional, might not be available depending on browser security context in Tauri
    resolvedPath?: string;
    type: string;
    lastModified: number;
}

export function FileTray() {
    const files = useFileTrayStore(state => state.files);
    const addFiles = useFileTrayStore(state => state.addFiles);
    const removeFileByPath = useFileTrayStore(state => state.removeFile);
    const [isDragging, setIsDragging] = useState(false);
    const dropZoneRef = useRef<HTMLDivElement>(null);

    const handleDragEnter = useCallback((e: React.DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        setIsDragging(true);
    }, []);

    const handleDragLeave = useCallback((e: React.DragEvent) => {
        e.preventDefault();
        e.stopPropagation();

        // Only set dragged to false if we're leaving the drop zone entirely
        if (dropZoneRef.current && !dropZoneRef.current.contains(e.relatedTarget as Node)) {
            setIsDragging(false);
        }
    }, []);

    const handleDragOver = useCallback((e: React.DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        setIsDragging(true);
    }, []);

    const handleDrop = useCallback(async (e: React.DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        setIsDragging(false);

        const droppedFilesPromises = Array.from(e.dataTransfer.files).map(async file => {
            const path = (file as any).path;
            let resolvedPath = path;
            if (path) {
                invoke('on_file_drop', { path }).catch(console.error);
                invoke('trigger_haptics').catch(console.error);
                try {
                    resolvedPath = await invoke('resolve_path', { path });
                } catch (e) {
                    console.error('Failed to resolve path', e);
                }
            }
            return {
                name: file.name,
                size: file.size,
                path: path, // Tauri/Electron often exposes path
                resolvedPath: resolvedPath,
                type: file.type,
                lastModified: file.lastModified
            };
        });

        const droppedFiles = await Promise.all(droppedFilesPromises);
        addFiles(droppedFiles);
    }, [addFiles]);

    const handleWheel = useCallback((e: React.WheelEvent) => {
        // Stop propagation of vertical scroll to prevent closing the island
        if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
            e.stopPropagation();
        }
    }, []);

    const handleClick = useCallback((e: React.MouseEvent) => {
        e.stopPropagation();
    }, []);

    const removeFile = useCallback((file: FileItem) => {
        if (file.path) {
            removeFileByPath(file.path);
        }
    }, [removeFileByPath]);


    const handleFileClick = useCallback(async (file: FileItem) => {
        if (file.path) {
            try {
                await invoke('open_file', { path: file.path });
            } catch (error) {
                console.error('Failed to open file:', error);
            }
        }
    }, []);

    const handleFileContextMenu = useCallback(async (e: React.MouseEvent, file: FileItem) => {
        e.preventDefault();
        if (file.path) {
            try {
                await invoke('reveal_file', { path: file.path });
            } catch (error) {
                console.error('Failed to reveal file:', error);
            }
        }
    }, []);

    const createResizedIcon = async (src: string): Promise<Uint8Array | null> => {
        return new Promise((resolve) => {
            const img = new Image();
            img.crossOrigin = "Anonymous";
            img.onload = () => {
                try {
                    const canvas = document.createElement('canvas');
                    canvas.width = 30;
                    canvas.height = 30;
                    const ctx = canvas.getContext('2d');
                    if (!ctx) { resolve(null); return; }

                    // Draw image scaled to 30x30
                    ctx.drawImage(img, 0, 0, 30, 30);

                    canvas.toBlob((blob) => {
                        if (!blob) { resolve(null); return; }
                        blob.arrayBuffer().then(buf => resolve(new Uint8Array(buf)));
                    }, 'image/png');
                } catch (e) {
                    console.error("Canvas error", e);
                    resolve(null);
                }
            };
            img.onerror = () => resolve(null);
            img.src = src;
        });
    };

    const handleNativeDrag = useCallback(async (_e: React.MouseEvent, file: FileItem) => {
        const path = file.resolvedPath || file.path;
        if (path) {
            invoke('trigger_haptics').catch(console.error);
            try {
                let dragIconPath = path;

                // Try to generate a small icon
                const src = convertFileSrc(path);
                const iconData = await createResizedIcon(src);

                if (iconData) {
                    try {
                        dragIconPath = await invoke('save_drag_icon', { iconData: Array.from(iconData) });
                    } catch (e) {
                        console.error('Failed to save drag icon', e);
                    }
                }

                // Use native drag plugin for proper file binary transfer
                // This initiates an OS-level drag session
                const { startDrag } = await import('@crabnebula/tauri-plugin-drag');
                await startDrag({
                    item: [path],
                    icon: dragIconPath // Reverted to use path as icon is required. Note: may cause large drag image for high-res files.
                });

                // If we get here, drag completed successfully - remove file from tray
                removeFileByPath(path);
            } catch (error) {
                // Drag was cancelled or failed - file stays in tray
                console.log('Drag cancelled or failed:', error);
            }
        }
    }, [removeFileByPath]);

    const formatSize = (bytes: number) => {
        if (bytes === 0) return '';
        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
    };

    return (
        <div
            className={`flex-1 bg-white/5 rounded-[16px] border-2 border-dashed border-white/10 relative overflow-hidden transition-all duration-200 `}
            ref={dropZoneRef}
            onDragEnter={handleDragEnter}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
            onWheel={handleWheel}
            onClick={handleClick}

        >
            {files.length === 0 ? (
                <div className="w-full h-full flex flex-col items-center justify-center text-white/40 gap-3">
                    <IconUpload size={48} stroke={1} color="rgba(255,255,255,0.3)" />
                    <p className="text-sm m-0">Drop files here to store them temporarily</p>
                </div>
            ) : (
                <div className="grid grid-cols-[repeat(auto-fill,minmax(100px,1fr))] gap-3 overflow-y-auto h-full box-border" style={{ padding: '12px' }}>
                    <AnimatePresence mode="popLayout">
                        {files.map((file, index) => (
                            <motion.div
                                key={`${file.name}-${file.lastModified}-${index}`}
                                className="group bg-white/8 max-h-full rounded-xl  flex flex-col items-center gap-2 relative cursor-grab transition-colors duration-200 hover:bg-white/12"
                                layout
                                initial={{ opacity: 0, scale: 0.8 }}
                                animate={{ opacity: 1, scale: 1 }}
                                exit={{ opacity: 0, scale: 0.8 }}
                                transition={{ type: "spring", stiffness: 500, damping: 30 }}
                                onMouseDown={(e) => handleNativeDrag(e, file)}
                                onClick={() => handleFileClick(file)}
                                onContextMenu={(e) => handleFileContextMenu(e, file)}
                                style={{ cursor: 'pointer', padding: '12px' }}
                            >
                                <div className="flex flex-col items-center w-full gap-[2px]">
                                    <span className="text-[12px] text-white/90 whitespace-nowrap overflow-hidden text-ellipsis w-full text-center" title={file.name}>{file.name}</span>
                                    {file.size > 0 && <span className="text-[10px] text-white/50">{formatSize(file.size)}</span>}
                                </div>
                                <div className="flex items-center justify-center bg-white/5 rounded-lg w-full aspect-square overflow-hidden" >
                                    {(file.resolvedPath || file.path) && (
                                        <img
                                            src={convertFileSrc(file.resolvedPath || file.path!)}
                                            alt={file.name}
                                            style={{
                                                width: '100%',
                                                height: '100%',
                                                objectFit: 'scale-down',
                                                borderRadius: 4
                                            }}
                                            onError={(e) => {
                                                e.currentTarget.style.display = 'none';
                                            }}
                                        />
                                    )}
                                </div>

                                <Button
                                    variant="ghost"
                                    size="icon"
                                    className="absolute top-1 right-1 w-5 h-5 rounded-full "
                                    onMouseDown={(e) => {
                                        e.stopPropagation();
                                    }}
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        removeFile(file);
                                    }}
                                >
                                    <IconX size={14} />
                                </Button>
                            </motion.div>
                        ))}
                    </AnimatePresence>
                </div>
            )}

            {isDragging && (
                <div className="absolute inset-0 bg-black/60 backdrop-blur-xs flex items-center justify-center z-10 pointer-events-none">
                    <motion.div
                        initial={{ scale: 0.9, opacity: 0 }}
                        animate={{ scale: 1, opacity: 1 }}
                    >
                        <IconUpload size={32} color="white" stroke={1.5} />
                    </motion.div>
                </div>
            )}
        </div>
    );
}
