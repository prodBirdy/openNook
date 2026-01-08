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
    contentOpacity: number;
}

export function CompactMedia({
    nowPlaying,
    isHovered,
    baseNotchWidth,
    visualizerColor,
    contentOpacity
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
            className="island-content files-content"
            baseNotchWidth={baseNotchWidth}
            isHovered={isHovered}
            contentOpacity={contentOpacity}
            left={
                <div className="album-cover-wrapper" style={{ width: 'auto' }}>
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
                                className="album-info-reveal"
                                initial={{ opacity: 0, y: -8, x: '-50%' }}
                                animate={{ opacity: 1, y: 4, x: '-50%' }}
                                exit={{ opacity: 0, y: -8, x: '-50%' }}
                                transition={{ type: 'spring', stiffness: 500, damping: 30 }}
                            >
                                <div className="album-info-title">{nowPlaying.title || 'Unknown Title'}</div>
                                <div className="album-info-artist">{nowPlaying.artist || 'Unknown Artist'}</div>
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
