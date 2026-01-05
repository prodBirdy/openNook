import { useState, useCallback, useRef } from 'react';
import { IconX, IconUpload } from '@tabler/icons-react';
import { motion, AnimatePresence } from 'motion/react';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import './DynamicIsland.css'; // Reusing island CSS for now, will add specific styles later if needed

export interface FileItem {
    name: string;
    size: number;
    path?: string; // Optional, might not be available depending on browser security context in Tauri
    resolvedPath?: string;
    type: string;
    lastModified: number;
}

interface FileTrayProps {
    files: FileItem[];
    onUpdateFiles: (files: FileItem[]) => void;
}

export function FileTray({ files, onUpdateFiles }: FileTrayProps) {
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

        const updated = [...files, ...droppedFiles];
        onUpdateFiles(updated);
    }, [files, onUpdateFiles]);

    const handleWheel = useCallback((e: React.WheelEvent) => {
        // Stop propagation of vertical scroll to prevent closing the island
        if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
            e.stopPropagation();
        }
    }, []);

    const handleClick = useCallback((e: React.MouseEvent) => {
        e.stopPropagation();
    }, []);

    const removeFile = useCallback((indexToRemove: number) => {
        const updated = files.filter((_, index) => index !== indexToRemove);
        onUpdateFiles(updated);
    }, [files, onUpdateFiles]);


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

    const handleNativeDrag = useCallback(async (_e: React.MouseEvent, file: FileItem, index: number) => {
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
                const updated = files.filter((_, i) => i !== index);
                onUpdateFiles(updated);
            } catch (error) {
                // Drag was cancelled or failed - file stays in tray
                console.log('Drag cancelled or failed:', error);
            }
        }
    }, [files, onUpdateFiles]);

    const formatSize = (bytes: number) => {
        if (bytes === 0) return '';
        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
    };



    return (
        <div
            className={`file-tray-container ${isDragging ? 'dragging' : ''}`}
            ref={dropZoneRef}
            onDragEnter={handleDragEnter}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
            onWheel={handleWheel}
            onClick={handleClick}
        >
            {files.length === 0 ? (
                <div className="empty-tray-state">
                    <IconUpload size={48} stroke={1} color="rgba(255,255,255,0.3)" />
                    <p>Drop files here to store them temporarily</p>
                </div>
            ) : (
                <div className="file-grid">
                    <AnimatePresence mode="popLayout">
                        {files.map((file, index) => (
                            <motion.div
                                key={`${file.name}-${file.lastModified}-${index}`}
                                className="file-item"
                                layout
                                initial={{ opacity: 0, scale: 0.8 }}
                                animate={{ opacity: 1, scale: 1 }}
                                exit={{ opacity: 0, scale: 0.8 }}
                                transition={{ type: "spring", stiffness: 500, damping: 30 }}
                                onMouseDown={(e) => handleNativeDrag(e, file, index)}
                                onClick={() => handleFileClick(file)}
                                onContextMenu={(e) => handleFileContextMenu(e, file)}
                                style={{ cursor: 'pointer' }}
                            >
                                <div className="file-info">
                                    <span className="file-name" title={file.name}>{file.name}</span>
                                    {file.size > 0 && <span className="file-size">{formatSize(file.size)}</span>}
                                </div>
                                <div className="file-icon-wrapper" >
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

                                <button
                                    className="remove-file-btn"
                                    onMouseDown={(e) => {
                                        e.stopPropagation();
                                    }}
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        removeFile(index);
                                    }}
                                >
                                    <IconX size={14} />
                                </button>
                            </motion.div>
                        ))}
                    </AnimatePresence>
                </div>
            )}

            {isDragging && (
                <div className="drag-overlay">
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
