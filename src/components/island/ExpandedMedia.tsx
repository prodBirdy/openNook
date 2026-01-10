import { memo, useMemo, useState, useEffect } from 'react';
import { motion } from 'motion/react';
import { IconPlayerSkipBackFilled, IconPlayerPlayFilled, IconPlayerPauseFilled, IconPlayerSkipForwardFilled } from '@tabler/icons-react';
import { getDominantColor } from '../../utils/imageUtils';
import { WidgetWrapper } from '../widgets/WidgetWrapper';
import { useMediaPlayerStore } from '../../stores/useMediaPlayerStore';
import { ScrollingText } from '../ui/scrolling-text';

// Memoized ExpandedMedia component
export const ExpandedMedia = memo(function ExpandedMedia() {
    const nowPlaying = useMediaPlayerStore(state => state.nowPlaying);
    const handlePlayPause = useMediaPlayerStore(state => state.handlePlayPause);
    const handleNextTrack = useMediaPlayerStore(state => state.handleNextTrack);
    const handlePreviousTrack = useMediaPlayerStore(state => state.handlePreviousTrack);
    const handleSeek = useMediaPlayerStore(state => state.handleSeek);

    // Early return if no media is playing
    if (!nowPlaying) {
        return null;
    }
    const formatTime = (seconds: number) => {
        const mins = Math.floor(seconds / 60);
        const secs = Math.floor(seconds % 60);
        return `${mins}:${secs.toString().padStart(2, '0')}`;
    };

    const [isSeizing, setIsSeizing] = useState(false);
    const [localProgress, setLocalProgress] = useState(0);
    const [glowColor, setGlowColor] = useState<string | null>(null);

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
                } else {
                    setGlowColor(null);
                }
            });
        } else {
            setTimeout(() => setGlowColor(null), 300); // Clear after fade out
        }
    }, [nowPlaying.artwork_base64]);

    const handleSeekInternal = async (e: React.PointerEvent<HTMLDivElement>) => {
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
        await handleSeek(newTime);

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

    const headerActions = [
        <>
            <div
                className="w-[52px] h-[52px] rounded-[10px] overflow-visible shrink-0 "
            >
                <div
                    className="absolute w-[52px] h-[52px] rounded-[10px] overflow-visible shrink-0 shadow-[0_4px_12px_rgba(0,0,0,0.4),0_8px_32px_-4px_var(--glow-color,transparent)] transition-shadow duration-500 ease-in-out"
                    style={{
                        '--glow-color': glowColor || 'transparent',
                    } as React.CSSProperties}
                ></div>
                {nowPlaying.artwork_base64 ? (
                    <img
                        src={`data:image/png;base64,${nowPlaying.artwork_base64}`}
                        alt={nowPlaying.title || 'Album cover'}
                        className="w-full h-full object-cover rounded-[12px]"
                    />
                ) : (
                    <div className="w-full h-full bg-linear-to-br from-[#2a2a2a] to-[#1a1a1a] flex items-center justify-center border border-white/10" />
                )}
            </div>
            <div className="w-full flex-1 flex flex-col justify-center text-left overflow-hidden min-w-0 ">
                <ScrollingText className="text-[17px] font-semibold  text-white tracking-[-0.01em]">
                    {nowPlaying.title || 'Unknown Title'}
                </ScrollingText>
                <ScrollingText className="text-[13px] text-white/60 m-0">
                    {nowPlaying.artist || 'Unknown Artist'}
                </ScrollingText>
            </div>
        </>
    ]

    return (

        <WidgetWrapper headerActions={headerActions} className="flex flex-col gap-3 h-full overflow-hidden " >
            <div
                className={` ${nowPlaying.duration && nowPlaying.duration > 0 ? 'cursor-pointer opacity-100' : 'cursor-default opacity-50'} `}
                onPointerDown={(e) => {
                    if (!nowPlaying.duration || nowPlaying.duration <= 0) return;
                    e.stopPropagation();
                    e.currentTarget.setPointerCapture(e.pointerId);
                    setIsSeizing(true);
                    handleMouseMove(e);
                }}
                onPointerUp={(e) => {
                    if (!nowPlaying.duration || nowPlaying.duration <= 0) return;
                    e.stopPropagation();
                    e.currentTarget.releasePointerCapture(e.pointerId);
                    // Do NOT setIsSeizing(false) here - handleSeekInternal depends on it blocking updates
                    handleSeekInternal(e); // Commit seek
                }}
                onPointerMove={(e) => {
                    if (isSeizing) handleMouseMove(e);
                }}
            >
                <div className="w-full h-1 bg-white/15 rounded-[2px] cursor-pointer ">
                    <motion.div
                        className="h-full bg-white rounded-[2px]"
                        initial={{ width: 0 }}
                        animate={{ width: `${localProgress}%` }}
                        transition={{
                            type: 'tween',
                            duration: 0.1,
                        }}
                    />
                </div>
                <div className="flex justify-between text-[11px] font-mono font-medium text-white/40 tracking-[-0.02em] pt-[2px] px-[1px] mt-[6px]">
                    <span>
                        {formatTime(isSeizing && nowPlaying.duration
                            ? (localProgress / 100) * nowPlaying.duration
                            : nowPlaying.elapsed_time || 0)}
                    </span>
                    <span>{formatTime(nowPlaying.duration || 0)}</span>
                </div>
            </div>

            <div className="flex items-center justify-center gap-[36px] p-0 mt-0" onClick={(e) => e.stopPropagation()}>
                <div className="flex items-center justify-center cursor-pointer opacity-90 transition-all duration-200 ease-[cubic-bezier(0.25,0.1,0.25,1)] text-white text-[20px] hover:opacity-100 hover:scale-110 active:scale-[0.92]" onClick={(e) => { e.stopPropagation(); handlePreviousTrack(e); }}>
                    <IconPlayerSkipBackFilled size={24} />
                </div>
                <div className="flex items-center justify-center cursor-pointer opacity-90 transition-all duration-200 ease-[cubic-bezier(0.25,0.1,0.25,1)] w-10 h-10 text-[24px] bg-white rounded-full text-black  hover:opacity-100 hover:scale-[1.08] hover:bg-[#f5f5f5] active:scale-[0.95]" onClick={(e) => { e.stopPropagation(); handlePlayPause(e); }}>
                    {nowPlaying.is_playing ? (
                        <IconPlayerPauseFilled size={24} className="text-black" />
                    ) : (
                        <IconPlayerPlayFilled size={24} className="text-black" />
                    )}
                </div>
                <div className="flex items-center justify-center cursor-pointer opacity-90 transition-all duration-200 ease-[cubic-bezier(0.25,0.1,0.25,1)] text-white text-[20px] hover:opacity-100 hover:scale-110 active:scale-[0.92]" onClick={(e) => { e.stopPropagation(); handleNextTrack(e); }}>
                    <IconPlayerSkipForwardFilled size={24} />
                </div>
            </div>
        </WidgetWrapper>

    );
});
