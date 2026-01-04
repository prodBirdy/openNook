import { memo, useMemo, useCallback } from 'react';
import { motion } from 'motion/react';
import './DynamicIsland.css'; // Assuming CSS is shared or moved. Ideally specific CSS should be here.

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

    // Stable callbacks
    const handleHoverStart = useCallback(() => onHoverChange?.(true), [onHoverChange]);
    const handleHoverEnd = useCallback(() => onHoverChange?.(false), [onHoverChange]);

    return (
        <motion.div
            className="album-cover"
            animate={animateValues}
            transition={transition}
            onHoverStart={handleHoverStart}
            onHoverEnd={handleHoverEnd}
            onClick={onClick}
            style={{ cursor: onClick ? 'pointer' : 'default' }}
        >
            {artwork ? (
                <img
                    src={`data:image/png;base64,${artwork}`}
                    alt={title || 'Album cover'}
                    className="album-cover__image"
                />
            ) : (
                <div className="album-cover__placeholder">
                    <span className="album-cover__icon"></span>
                </div>
            )}
        </motion.div>
    );
});
