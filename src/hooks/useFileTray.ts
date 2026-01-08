import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { FileItem } from '../components/FileTray';

export function useFileTray(setExpanded: (expanded: boolean) => void, setActiveTab: (tab: 'widgets' | 'files') => void, setIsAnimating: (animating: boolean) => void) {
    const [files, setFiles] = useState<FileItem[]>([]);
    const [droppedFiles, setDroppedFiles] = useState<string[]>([]);

    // Load files on mount
    useEffect(() => {
        invoke<FileItem[]>('load_file_tray')
            .then(async (loadedFiles) => {
                // Resolve paths for loaded files
                const resolvedFiles = await Promise.all(loadedFiles.map(async (file) => {
                    if (file.path) {
                        try {
                            const resolvedPath = await invoke<string>('resolve_path', { path: file.path });
                            return { ...file, resolvedPath };
                        } catch (e) {
                            console.error(`Failed to resolve path for ${file.name}:`, e);
                            return file;
                        }
                    }
                    return file;
                }));
                setFiles(resolvedFiles);
            })
            .catch(err => console.error('Failed to load file tray:', err));
    }, []);

    // Save files whenever they change
    useEffect(() => {
        if (files.length > 0) {
            invoke('save_file_tray', { files }).catch(err => console.error('Failed to save file tray:', err));
        }
    }, [files]);

    // Process external files (from backend drop event)
    useEffect(() => {
        if (droppedFiles && droppedFiles.length > 0) {
            const processFiles = async () => {
                const newFilesPromises = droppedFiles.map(async path => {
                    // Extract filename from path
                    const name = path.split(/[/\\]/).pop() || path;
                    // Simple extension check for type
                    const ext = name.split('.').pop()?.toLowerCase();
                    let type = 'unknown';
                    if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg'].includes(ext || '')) {
                        type = `image/${ext}`;
                    }

                    let resolvedPath = path;
                    try {
                        resolvedPath = await invoke<string>('resolve_path', { path });
                    } catch (e) {
                        console.error('Failed to resolve path', e);
                    }

                    return {
                        name,
                        size: 0, // We don't have size from backend event immediately, could fetch if needed
                        path,
                        resolvedPath,
                        type,
                        lastModified: Date.now()
                    };
                });

                const newFiles = await Promise.all(newFilesPromises);

                setFiles(prev => {
                    // Avoid duplicates
                    const existingPaths = new Set(prev.map(f => f.path));
                    const uniqueNewFiles = newFiles.filter(f => !existingPaths.has(f.path));
                    const updated = [...prev, ...uniqueNewFiles];
                    // Save immediately
                    invoke('save_file_tray', { files: updated }).catch(console.error);
                    return updated;
                });
            };

            processFiles();
            setDroppedFiles([]);
        }
    }, [droppedFiles]);

    // Drag and drop listener
    useEffect(() => {
        const handleDragEnter = (event: unknown) => {
            console.log('Drag enter detected:', event);
            setExpanded(true);
            setActiveTab('files');
            setIsAnimating(true);
            invoke('trigger_haptics').catch(console.error);
        };

        const handleFileDrop = (event: { payload: string[] }) => {
            console.log('File drop detected (backend):', event);
            if (event.payload && Array.isArray(event.payload)) {
                setDroppedFiles(prev => [...prev, ...event.payload]);
            }
        };

        const unlistenDragEnter = listen('tauri://drag-enter', handleDragEnter);
        const unlistenBackendDragEnter = listen('drag-enter-event', handleDragEnter);
        const unlistenBackendFileDrop = listen('file-drop-event', handleFileDrop);
        // Fallback for different Tauri versions/configs
        const unlistenFileDropHover = listen('tauri://file-drop-hover', handleDragEnter);

        return () => {
            unlistenDragEnter.then(fn => fn());
            unlistenBackendDragEnter.then(fn => fn());
            unlistenBackendFileDrop.then(fn => fn());
            unlistenFileDropHover.then(fn => fn());
        };
    }, [setExpanded, setActiveTab, setIsAnimating]);

    return {
        files,
        setFiles
    };
}
