import { useRef, useEffect } from 'react';
import { CompactWrapper } from './CompactWrapper';
import logo from '../../assets/logo.png';
import animatedLogo from '../../assets/animated.mp4';
import { IconBrandGithub } from '@tabler/icons-react';

interface CompactOnboardProps {
    baseNotchWidth: number;
    isHovered: boolean;
    contentOpacity?: number;
}

export function CompactOnboard({ baseNotchWidth, isHovered }: CompactOnboardProps) {
    const videoRef = useRef<HTMLVideoElement>(null);

    useEffect(() => {
        const video = videoRef.current;
        if (!video) return;

        // Force muted state to allow autoplay
        video.muted = true;

        const playTrack = () => {
            if (video.paused) {
                video.play().catch(() => {
                    // If it still fails, the browser is strictly waiting for a gesture.
                    // We stop trying to save resources until that happens naturally via user action elsewhere.
                });
            }
        };

        // Try playing immediately
        playTrack();

        // Fallback: One single retry after 1s in case the engine was busy during launch
        const fallback = setTimeout(playTrack, 1000);

        return () => clearTimeout(fallback);
    }, []);

    return (
        <CompactWrapper
            id="onboard-content"
            baseNotchWidth={baseNotchWidth}
            isHovered={isHovered}
            left={
                <div className="flex items-center gap-2">
                    <video
                        ref={videoRef}
                        src={animatedLogo}
                        poster={logo}
                        className="h-7 w-auto opacity-100 pointer-events-none"
                        autoPlay
                        loop
                        muted
                        playsInline
                        preload="auto"
                        // Trigger play as soon as enough data is buffered¥
                        onCanPlay={() => videoRef.current?.play().catch(() => { })}
                    />
                </div>
            }
            right={
                <div className="flex items-center gap-2">
                    <a href="https://github.com/prodBirdy/openNook" target="_blank" rel="noopener noreferrer" onClick={(e) => e.stopPropagation()}>
                        <IconBrandGithub size={28} color="white" />
                    </a>
                </div>
            }
        />
    );
}
