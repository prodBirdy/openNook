import { memo, useMemo, useCallback, useState } from 'react';
import { motion, AnimatePresence } from 'motion/react';

// The comment is line 3.
// The overlay is lines 75-90.
// I'll do two chunks if possible? No, replace_file_content is single contiguous.
// I'll do multi_replace_file_content.

import { convertFileSrc } from '@tauri-apps/api/core';
import { IconPlayerPlayFilled, IconPlayerPauseFilled } from '@tabler/icons-react';

interface AlbumCoverProps {
    artwork: string | null;
    title: string | null;
    isPlaying: boolean;
    onHoverChange?: (hovered: boolean) => void;
    onClick?: (e: React.MouseEvent) => void;
}

// Memoized AlbumCover to prevent unnecessary re-renders
export const AlbumCover = memo(function AlbumCover({
    artwork,
    title,
    isPlaying,
    onHoverChange,
    onClick
}: AlbumCoverProps) {
    // Memoize transition config
    const transition = useMemo(() => ({
        type: 'spring' as const,
        stiffness: 300,
        damping: 25,
    }), []);

    // Memoize animation values
    const animateValues = useMemo(() => ({
        scale: isPlaying ? 1 : 0.9,
        opacity: 1,
    }), [isPlaying]);

    // Local hover state for overlay
    const [isHovered, setIsHovered] = useState(false);

    // Stable callbacks
    const handleHoverStart = useCallback(() => {
        setIsHovered(true);
        onHoverChange?.(true);
    }, [onHoverChange]);

    const handleHoverEnd = useCallback(() => {
        setIsHovered(false);
        onHoverChange?.(false);
    }, [onHoverChange]);

    return (
        <motion.div
            className="w-full h-full flex items-center justify-center pl-[2px]"
            animate={animateValues}
            transition={transition}
            onHoverStart={handleHoverStart}
            onHoverEnd={handleHoverEnd}
            onClick={onClick}
            style={{ cursor: onClick ? 'pointer' : 'default' }}
        >
            {artwork ? (
                <img
                    src={(artwork.length < 500 && (artwork.startsWith('/') || artwork.match(/^[a-zA-Z]:/))) ? convertFileSrc(artwork) : `data:image/png;base64,${artwork}`}
                    alt={title || 'Album cover'}
                    className="w-[26px] h-[26px] rounded-[5px] object-cover shadow-[0_1px_4px_rgba(0,0,0,0.3)]"
                />
            ) : (
                <div className="w-[26px] h-[26px] rounded-[5px] bg-[linear-gradient(135deg,#333,#111)] flex items-center justify-center text-white/50 text-[14px]">
                    <span className="album-cover__icon"></span>
                </div>
            )}

            <AnimatePresence>
                {isHovered && (
                    <motion.div
                        className="absolute inset-0 bg-black/30 flex items-center justify-center rounded-[inherit] backdrop-blur-[1px]"
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        transition={{ duration: 0.2 }}
                    >
                        {isPlaying ? (
                            <IconPlayerPauseFilled size={16} color="white" />
                        ) : (
                            <IconPlayerPlayFilled size={16} color="white" />
                        )}
                    </motion.div>
                )}
            </AnimatePresence>
        </motion.div>
    );
});
