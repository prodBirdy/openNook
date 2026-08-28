import { memo, useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { motion } from 'motion/react';

interface SmartAudioVisualizerProps {
    isPlaying: boolean;
    fallbackLevels: number[] | null;
    color: string | null;
}

export const SmartAudioVisualizer = memo(function SmartAudioVisualizer({
    isPlaying,
    fallbackLevels,
    color
}: SmartAudioVisualizerProps) {
    const barsRef = useRef<(HTMLDivElement | null)[]>([]);
    const targetLevelsRef = useRef<number[]>(new Array(6).fill(0.15));
    const currentLevelsRef = useRef<number[]>(new Array(6).fill(0.15));
    const animationFrameRef = useRef<number | null>(null);

    // Initial Refs setup
    useEffect(() => {
        barsRef.current = barsRef.current.slice(0, 6);
    }, []);

    // Set fallback levels when props change
    useEffect(() => {
        if (fallbackLevels) {
            targetLevelsRef.current = fallbackLevels;
            // If starting from scratch, snap to it, otherwise let it interpolate
            if (!isPlaying) {
                currentLevelsRef.current = [...fallbackLevels];
            }
        }
    }, [fallbackLevels, isPlaying]);

    // Data Listener
    useEffect(() => {
        const unlistenAudioLevels = listen<number[]>('audio-levels-update', (event) => {
            const levels = event.payload;
            if (levels && levels.length > 0) {
                targetLevelsRef.current = levels;
            }
        });

        return () => {
            unlistenAudioLevels.then(fn => fn());
        };
    }, []);

    // Animation Loop
    useEffect(() => {
        const animate = () => {
            const barColor = color || '#e1e1e1';

            // Interpolation factor (0.0 to 1.0)
            // Higher = snappier, Lower = smoother
            // 0.15 at 60fps is a good balance for "following" the 30fps data
            const lerpFactor = 0.15;

            for (let i = 0; i < 6; i++) {
                const bar = barsRef.current[i];
                if (!bar) continue;

                if (!isPlaying) {
                    // Decay to 0.15
                    targetLevelsRef.current[i] = 0.15;
                }

                const target = targetLevelsRef.current[i] || 0.15;
                const current = currentLevelsRef.current[i] || 0.15;

                // Lerp formula: current + (target - current) * factor
                const next = current + (target - current) * lerpFactor;
                currentLevelsRef.current[i] = next;

                const scale = Math.max(0.15, Math.min(1, next));

                bar.style.transform = `scaleY(${scale})`;
                bar.style.backgroundColor = barColor;
            }

            animationFrameRef.current = requestAnimationFrame(animate);
        };

        animationFrameRef.current = requestAnimationFrame(animate);

        return () => {
            if (animationFrameRef.current) cancelAnimationFrame(animationFrameRef.current);
        };
    }, [isPlaying, color]);

    return (
        <div className="flex items-center justify-center gap-[2.5px] h-5 pr-1">
            {[...Array(6)].map((_, i) => (
                <motion.div
                    key={i}
                    ref={el => { barsRef.current[i] = el as HTMLDivElement; }}
                    className="w-[3.5px] h-full bg-[#e1e1e1] rounded-full  will-change-transform"
                    style={{
                        transform: 'scaleY(0.15)',
                        backgroundColor: color || '#e1e1e1',
                        // CSS transition for background color, but transform is managed by loop
                        transition: 'background-color 0.5s ease',
                    }}
                />
            ))}
        </div>
    );
});
