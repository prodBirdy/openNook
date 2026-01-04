import { useState, useCallback, useRef } from 'react';
import { IconFile, IconX, IconUpload } from '@tabler/icons-react';
import { motion, AnimatePresence } from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import './DynamicIsland.css'; // Reusing island CSS for now, will add specific styles later if needed

interface FileItem {
    name: string;
    size: number;
    path?: string; // Optional, might not be available depending on browser security context in Tauri
    type: string;
    lastModified: number;
}

export function FileTray() {
    const [files, setFiles] = useState<FileItem[]>([]);
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

        setFiles(prev => [...prev, ...droppedFiles]);
    }, []);

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
        setFiles(prev => prev.filter((_, index) => index !== indexToRemove));
    }, []);

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

    const formatSize = (bytes: number) => {
        if (bytes === 0) return '0 B';
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
                                draggable // Allow dragging out? (Requires backend implementation for full drag-out support usually)
                                onClick={() => handleFileClick(file)}
                                onContextMenu={(e) => handleFileContextMenu(e, file)}
                                style={{ cursor: 'pointer' }}
                            >
                                <div className="file-icon-wrapper">
                                    <IconFile size={32} stroke={1.5} color="white" />
                                </div>
                                <div className="file-info">
                                    <span className="file-name" title={file.name}>{file.name}</span>
                                    <span className="file-size">{formatSize(file.size)}</span>
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
