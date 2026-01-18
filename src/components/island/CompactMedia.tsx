import { motion, AnimatePresence } from 'motion/react';
import { CompactWrapper } from './CompactWrapper';
import { useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AlbumCover } from '../AlbumCover';
import { SmartAudioVisualizer } from '../AudioVisualizer';
import { NowPlayingData } from './types';

interface CompactMediaProps {
    nowPlaying: NowPlayingData;
    isHovered: boolean;
    baseNotchWidth: number;
    visualizerColor: string | null;
    contentOpacity?: number;
}

export function CompactMedia({
    nowPlaying,
    isHovered,
    baseNotchWidth,
    visualizerColor
}: CompactMediaProps) {
    const [isCoverHovered, setIsCoverHovered] = useState(false);

    const handleAlbumClick = useCallback((e: React.MouseEvent) => {
        e.stopPropagation();
        invoke('media_play_pause')
            .catch(err => console.error('Failed to toggle play/pause:', err));
    }, []);

    return (
        <CompactWrapper
            id="media-content"
            baseNotchWidth={baseNotchWidth}
            isHovered={isHovered}
            left={
                <div className="relative flex flex-col items-center h-full w-auto">
                    <AlbumCover
                        artwork={nowPlaying.artwork_base64}
                        title={nowPlaying.title}
                        isPlaying={nowPlaying.is_playing}
                        onHoverChange={setIsCoverHovered}
                        onClick={handleAlbumClick}
                    />
                    <AnimatePresence>
                        {isCoverHovered && isHovered && (nowPlaying.title || nowPlaying.artist) && (
                            <motion.div
                                className="absolute top-full left-1/2 mt-1 px-3 py-2 bg-black/90 backdrop-blur-xl border border-white/10 rounded-xl flex flex-col items-center text-center min-w-[100px] w-max max-w-[320px] z-[100] shadow-lg pointer-events-none"
                                initial={{ opacity: 0, y: -8, x: '-50%' }}
                                animate={{ opacity: 1, y: 4, x: '-50%' }}
                                exit={{ opacity: 0, y: -8, x: '-50%' }}
                                transition={{ type: 'spring', stiffness: 500, damping: 30 }}
                            >
                                <div className="text-white/95 text-[13px] font-semibold mb-0.5 whitespace-nowrap overflow-hidden text-ellipsis w-full">{nowPlaying.title || 'Unknown Title'}</div>
                                <div className="text-white/60 text-[11px] font-medium whitespace-nowrap overflow-hidden text-ellipsis w-full">{nowPlaying.artist || 'Unknown Artist'}</div>
                            </motion.div>
                        )}
                    </AnimatePresence>
                </div>
            }
            right={
                <SmartAudioVisualizer
                    isPlaying={nowPlaying.is_playing}
                    fallbackLevels={nowPlaying.audio_levels}
                    color={visualizerColor}
                />
            }
        />
    );

}
