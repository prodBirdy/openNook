import { memo, useMemo, useState, useEffect } from 'react';
import { motion } from 'motion/react';
import { IconPlayerSkipBackFilled, IconPlayerPlayFilled, IconPlayerPauseFilled, IconPlayerSkipForwardFilled } from '@tabler/icons-react';
import { getDominantColor } from '../utils/imageUtils';
import './DynamicIsland.css';

interface NowPlayingData {
    title: string | null;
    artist: string | null;
    album: string | null;
    artwork_base64: string | null;
    duration: number | null;
    elapsed_time: number | null;
    is_playing: boolean;
    audio_levels: number[] | null;
    app_name: string | null;
}

interface ExpandedMediaProps {
    nowPlaying: NowPlayingData;
    onPlayPause: (e: React.MouseEvent) => void;
    onNext: (e: React.MouseEvent) => void;
    onPrevious: (e: React.MouseEvent) => void;
    onSeek: (position: number) => Promise<void>;
}

// Memoized ExpandedMedia component
export const ExpandedMedia = memo(function ExpandedMedia({
    nowPlaying,
    onPlayPause,
    onNext,
    onPrevious,
    onSeek
}: ExpandedMediaProps) {
    const formatTime = (seconds: number) => {
        const mins = Math.floor(seconds / 60);
        const secs = Math.floor(seconds % 60);
        return `${mins}:${secs.toString().padStart(2, '0')}`;
    };

    const [isSeizing, setIsSeizing] = useState(false);
    const [localProgress, setLocalProgress] = useState(0);
    const [glowColor, setGlowColor] = useState<string | null>(null);
    const [glowOpacity, setGlowOpacity] = useState(0);

    const progress = useMemo(() => {
        if (!nowPlaying.duration || !nowPlaying.elapsed_time) return 0;
        return Math.min(100, (nowPlaying.elapsed_time / nowPlaying.duration) * 100);
    }, [nowPlaying.duration, nowPlaying.elapsed_time]);

    // Update local progress when not dragging
    useEffect(() => {
        if (!isSeizing) {
            setLocalProgress(progress);
        }
    }, [progress, isSeizing]);

    // Extract dominant color for glow
    useEffect(() => {
        if (nowPlaying.artwork_base64) {
            const src = `data:image/png;base64,${nowPlaying.artwork_base64}`;
            getDominantColor(src).then(rgb => {
                if (rgb) {
                    setGlowColor(`rgb(${rgb[0]}, ${rgb[1]}, ${rgb[2]})`);
                    setGlowOpacity(1);
                } else {
                    setGlowOpacity(0);
                }
            });
        } else {
            setGlowOpacity(0);
            setTimeout(() => setGlowColor(null), 300); // Clear after fade out
        }
    }, [nowPlaying.artwork_base64]);

    const handleSeek = async (e: React.PointerEvent<HTMLDivElement>) => {
        if (!nowPlaying.duration) {
            setIsSeizing(false);
            return;
        }

        const rect = e.currentTarget.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const percentage = Math.min(100, Math.max(0, (x / rect.width) * 100));
        const newTime = (percentage / 100) * nowPlaying.duration;

        // Keep optimistic update visible while waiting
        setLocalProgress(percentage);

        // Wait for backend confirmation
        await onSeek(newTime);

        // Only release lock after backend is done
        setIsSeizing(false);
    };

    const handleMouseMove = (e: React.PointerEvent<HTMLDivElement>) => {
        if (isSeizing && nowPlaying.duration) {
            const rect = e.currentTarget.getBoundingClientRect();
            const x = e.clientX - rect.left;
            const percentage = Math.min(100, Math.max(0, (x / rect.width) * 100));
            setLocalProgress(percentage);
        }
    }

    return (
        <>
            <div className="media-top-row">
                <div
                    className="expanded-album-cover"
                    style={{
                        boxShadow: glowColor ? `0 8px 32px -4px ${glowColor}` : 'none',
                        transition: 'box-shadow 0.5s ease',
                        opacity: glowOpacity ? 1 : 1, // Keep element visible, just toggle shadow
                    }}
                >
                    {nowPlaying.artwork_base64 ? (
                        <img
                            src={`data:image/png;base64,${nowPlaying.artwork_base64}`}
                            alt={nowPlaying.title || 'Album cover'}
                            style={{ width: '100%', height: '100%', objectFit: 'cover', borderRadius: '12px' }}
                        />
                    ) : (
                        <div className="expanded-album-placeholder" />
                    )}
                </div>
                <div className="expanded-track-info">
                    <h3>{nowPlaying.title || 'Unknown Title'}</h3>
                    <p>{nowPlaying.artist || 'Unknown Artist'}</p>
                </div>
            </div>

            {(nowPlaying.duration && nowPlaying.duration > 0) && (
                <div
                    className="progress-container"
                    // Inline padding removed for CSS handling
                    onPointerDown={(e) => {
                        e.stopPropagation();
                        // Capture pointer to track drag even if mouse leaves element bounds
                        e.currentTarget.setPointerCapture(e.pointerId);
                        setIsSeizing(true);
                        handleMouseMove(e); // Snap immediately to click position
                    }}
                    onPointerUp={(e) => {
                        e.stopPropagation();
                        e.currentTarget.releasePointerCapture(e.pointerId);
                        // Do NOT setIsSeizing(false) here - handleSeek depends on it blocking updates
                        handleSeek(e); // Commit seek
                    }}
                    onPointerMove={(e) => {
                        if (isSeizing) handleMouseMove(e);
                    }}
                >
                    <div className="progress-bar-bg">
                        <motion.div
                            className="progress-bar-fill"
                            initial={{ width: 0 }}
                            animate={{ width: `${localProgress}%` }}
                            transition={{
                                type: 'tween',
                                duration: 0.3,
                            }}
                        />
                    </div>
                    <div className="progress-times" style={{ marginTop: '6px' }}>
                        <span className="time-current">
                            {formatTime(isSeizing && nowPlaying.duration
                                ? (localProgress / 100) * nowPlaying.duration
                                : nowPlaying.elapsed_time || 0)}
                        </span>
                        <span className="time-duration">{formatTime(nowPlaying.duration)}</span>
                    </div>
                </div>
            )}

            <div className="expanded-controls" onClick={(e) => e.stopPropagation()}>
                <div className="control-btn" onClick={(e) => { e.stopPropagation(); onPrevious(e); }}>
                    <IconPlayerSkipBackFilled size={28} />
                </div>
                <div className="control-btn play-pause" onClick={(e) => { e.stopPropagation(); onPlayPause(e); }}>
                    {nowPlaying.is_playing ? (
                        <IconPlayerPauseFilled size={32} color="black" />
                    ) : (
                        <IconPlayerPlayFilled size={32} color="black" />
                    )}
                </div>
                <div className="control-btn" onClick={(e) => { e.stopPropagation(); onNext(e); }}>
                    <IconPlayerSkipForwardFilled size={28} />
                </div>
            </div>
        </>
    );
});
