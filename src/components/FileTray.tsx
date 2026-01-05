import { useState, useCallback, useRef, useEffect } from 'react';
import { IconFile, IconX, IconUpload } from '@tabler/icons-react';
import { motion, AnimatePresence } from 'motion/react';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import './DynamicIsland.css'; // Reusing island CSS for now, will add specific styles later if needed

export interface FileItem {
    name: string;
    size: number;
    path?: string; // Optional, might not be available depending on browser security context in Tauri
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

    const handleDrop = useCallback((e: React.DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        setIsDragging(false);

        const droppedFiles = Array.from(e.dataTransfer.files).map(file => {
            const path = (file as any).path;
            if (path) {
                invoke('on_file_drop', { path }).catch(console.error);
            }
            return {
                name: file.name,
                size: file.size,
                path: path, // Tauri/Electron often exposes path
                type: file.type,
                lastModified: file.lastModified
            };
        });

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

    const handleDragStart = useCallback((e: React.DragEvent, file: FileItem) => {
        if (file.path) {
            e.dataTransfer.effectAllowed = 'copyMove';
            // Try standard URI list
            e.dataTransfer.setData('text/uri-list', `file://${file.path}`);
            e.dataTransfer.setData('text/plain', file.path);
            // Try DownloadURL (Chrome specific, might not work in WKWebView but worth a shot)
            e.dataTransfer.setData('DownloadURL', `${file.type}:${file.name}:file://${file.path}`);

            // Also invoke backend to see if we can trigger native drag
            invoke('start_drag', { path: file.path }).catch(console.error);
        }
    }, []);

    const formatSize = (bytes: number) => {
        if (bytes === 0) return '';
        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
    };

    const isImage = (file: FileItem) => {
        return file.type.startsWith('image/') ||
            ['.png', '.jpg', '.jpeg', '.gif', '.webp', '.svg'].some(ext => file.name.toLowerCase().endsWith(ext));
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
                    <AnimatePresence>
                        {files.map((file, index) => (
                            <motion.div
                                key={`${file.name}-${file.lastModified}-${index}`}
                                className="file-item"
                                layout
                                initial={{ opacity: 0, scale: 0.8 }}
                                animate={{ opacity: 1, scale: 1 }}
                                exit={{ opacity: 0, scale: 0.8 }}
                                transition={{ type: "spring", stiffness: 500, damping: 30 }}
                                draggable
                                onDragStart={(e) => handleDragStart(e, file)}
                                onClick={() => handleFileClick(file)}
                                onContextMenu={(e) => handleFileContextMenu(e, file)}
                                style={{ cursor: 'pointer' }}
                            >
                                <div className="file-icon-wrapper">
                                    {isImage(file) && file.path ? (
                                        <img
                                            src={convertFileSrc(file.path)}
                                            alt={file.name}
                                            style={{ width: '100%', height: '100%', objectFit: 'cover', borderRadius: 6 }}
                                            onError={(e) => {
                                                console.error(`Failed to load image: ${file.path}`, e);
                                                // Fallback to icon
                                                e.currentTarget.style.display = 'none';
                                                e.currentTarget.parentElement?.classList.add('image-load-error');
                                            }}
                                        />
                                    ) : (
                                        <IconFile size={32} stroke={1.5} color="white" />
                                    )}
                                    {/* Show icon if image failed to load (handled via CSS/JS logic above, but simpler to just show icon if hidden) */}
                                </div>
                                <div className="file-info">
                                    <span className="file-name" title={file.name}>{file.name}</span>
                                    {file.size > 0 && <span className="file-size">{formatSize(file.size)}</span>}
                                </div>
                                <button
                                    className="remove-file-btn"
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
                        <IconUpload size={64} color="white" stroke={1.5} />
                    </motion.div>
                </div>
            )}
        </div>
    );
}
