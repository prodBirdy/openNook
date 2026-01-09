import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { FileItem } from '../components/FileTray';
import { useDynamicIslandStore } from './useDynamicIslandStore';

interface FileTrayState {
    files: FileItem[];
    droppedFiles: string[];
    isLoaded: boolean;
}

interface FileTrayActions {
    loadFiles: () => Promise<void>;
    setFiles: (files: FileItem[]) => void;
    addFiles: (newFiles: FileItem[]) => void;
    addDroppedFiles: (paths: string[]) => void;
    processDroppedFiles: () => Promise<void>;
    removeFile: (path: string) => void;
    setupListeners: () => () => void;
}

type FileTrayStore = FileTrayState & FileTrayActions;

export const useFileTrayStore = create<FileTrayStore>((set, get) => ({
    files: [],
    droppedFiles: [],
    isLoaded: false,

    loadFiles: async () => {
        try {
            const loadedFiles = await invoke<FileItem[]>('load_file_tray');

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

            set({ files: resolvedFiles, isLoaded: true });
        } catch (err) {
            console.error('Failed to load file tray:', err);
            set({ isLoaded: true });
        }
    },

    setFiles: (files: FileItem[]) => {
        set({ files });
        if (files.length > 0) {
            invoke('save_file_tray', { files }).catch(console.error);
        }
    },

    addFiles: (newFiles: FileItem[]) => {
        const { files } = get();
        const existingPaths = new Set(files.map(f => f.path));
        const uniqueNewFiles = newFiles.filter(f => !existingPaths.has(f.path));
        const updated = [...files, ...uniqueNewFiles];

        set({ files: updated });
        invoke('save_file_tray', { files: updated }).catch(console.error);
    },

    addDroppedFiles: (paths: string[]) => {
        set((state: FileTrayState) => ({ droppedFiles: [...state.droppedFiles, ...paths] }));
    },

    processDroppedFiles: async () => {
        const { droppedFiles } = get();
        if (droppedFiles.length === 0) return;

        const newFilesPromises = droppedFiles.map(async (path: string) => {
            const name = path.split(/[/\\]/).pop() || path;
            const ext = name.split('.').pop()?.toLowerCase();
            let type = 'unknown';
            if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg'].includes(ext || '')) {
                type = `image/${ext}`;
            }

            let resolvedPath = path;
            let size = 0;
            let lastModified = Date.now();

            try {
                resolvedPath = await invoke<string>('resolve_path', { path });
                const metadata = await invoke<{ size: number; lastModified: number }>('get_file_metadata', { path });
                size = metadata.size;
                lastModified = metadata.lastModified;
            } catch (e) {
                console.error('Failed to get file info', e);
            }

            return {
                name,
                size,
                path,
                resolvedPath,
                type,
                lastModified
            };
        });

        const newFiles = await Promise.all(newFilesPromises);
        get().addFiles(newFiles);
        set({ droppedFiles: [] });
    },

    removeFile: (path: string) => {
        const { files } = get();
        const updated = files.filter(f => f.path !== path);
        set({ files: updated });
        invoke('save_file_tray', { files: updated }).catch(console.error);
    },

    setupListeners: () => {
        const handleDragEnter = () => {
            console.log('Drag enter detected');
            const islandStore = useDynamicIslandStore.getState();
            islandStore.setExpanded(true);
            islandStore.setActiveTab('files');
            islandStore.setIsAnimating(true);
            invoke('trigger_haptics').catch(console.error);
        };

        const handleFileDrop = (event: { payload: string[] }) => {
            console.log('File drop detected (backend):', event);
            if (event.payload && Array.isArray(event.payload)) {
                get().addDroppedFiles(event.payload);
                get().processDroppedFiles();
            }
        };

        const unlistenDragEnter = listen('tauri://drag-enter', handleDragEnter);
        const unlistenBackendDragEnter = listen('drag-enter-event', handleDragEnter);
        const unlistenBackendFileDrop = listen('file-drop-event', handleFileDrop);
        const unlistenFileDropHover = listen('tauri://file-drop-hover', handleDragEnter);

        return () => {
            unlistenDragEnter.then(fn => fn());
            unlistenBackendDragEnter.then(fn => fn());
            unlistenBackendFileDrop.then(fn => fn());
            unlistenFileDropHover.then(fn => fn());
        };
    }
}));

// Selectors
export const selectFiles = (state: FileTrayStore) => state.files;
export const selectHasFiles = (state: FileTrayStore) => state.files.length > 0;
