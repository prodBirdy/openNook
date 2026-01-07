import { memo, useMemo, useCallback, useState } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import { IconPlayerPlayFilled, IconPlayerPauseFilled } from '@tabler/icons-react';
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

            <AnimatePresence>
                {isHovered && (
                    <motion.div
                        className="album-cover-overlay"
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        transition={{ duration: 0.2 }}
                        style={{
                            position: 'absolute',
                            inset: 0,
                            backgroundColor: 'rgba(0,0,0,0.3)',
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'center',
                            borderRadius: 'inherit',
                            backdropFilter: 'blur(1px)'
                        }}
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
