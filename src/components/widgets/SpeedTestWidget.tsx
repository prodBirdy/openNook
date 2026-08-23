import { useState, useEffect, useRef } from 'react';
import { IconGauge, IconPlayerStop, IconPlayerPlay } from '@tabler/icons-react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { registerWidget } from './WidgetRegistry';
import { WidgetWrapper } from './WidgetWrapper';

interface SpeedTestResult {
    speed: number;
    unit: 'Mbps';
    timestamp: number;
}

export function SpeedTestWidget() {
    const [isRunning, setIsRunning] = useState(false);
    const [result, setResult] = useState<SpeedTestResult | null>(null);
    const [currentSpeed, setCurrentSpeed] = useState<number>(0);
    const [progress, setProgress] = useState(0);
    const abortRef = useRef(false);

    useEffect(() => {
        const unlisten = listen<{ speed: number, progress: number }>('speed_test_progress', (event) => {
            if (!abortRef.current) {
                // Auto-detect if test is running and sync UI state
                setIsRunning(true);
                setCurrentSpeed(event.payload.speed);
                setProgress(event.payload.progress);
            }
        });

        return () => {
            unlisten.then(fn => fn());
        };
    }, []);

    const runSpeedTest = async () => {
        abortRef.current = false;
        setIsRunning(true);
        setProgress(0);

        try {
            const speedMbps = await invoke<number>('run_speed_test');

            if (!abortRef.current) {
                setResult({
                    speed: speedMbps,
                    unit: 'Mbps',
                    timestamp: Date.now()
                });
                // Don't set progress or clear currentSpeed - let backend data persist
            }
        } catch (err) {
            console.error('Speed test error:', err);
        } finally {
            if (!abortRef.current) {
                setIsRunning(false);
            } else {
                setProgress(0);
                setCurrentSpeed(0);
            }
        }
    };

    const stopSpeedTest = () => {
        abortRef.current = true;
        setIsRunning(false);
        setCurrentSpeed(0);
        setProgress(0);
    };

    const getDisplaySpeed = () => {
        // Show live speed if we have it
        if (currentSpeed > 0) return currentSpeed;

        // Otherwise show last result
        if (result) return result.speed;

        return 0;
    };

    const speed = getDisplaySpeed();

    const formatSpeed = (val: number) => {
        if (val >= 1000) return { value: (val / 1000).toFixed(2), unit: 'Gbps' };
        if (val < 1 && val > 0) return { value: (val * 1000).toFixed(0), unit: 'Kbps' };
        return { value: val.toFixed(1), unit: 'Mbps' };
    };

    const { value, unit } = formatSpeed(speed);

    return (
        <WidgetWrapper
            title="Speed Test"
            className="h-full relative overflow-hidden"
        >
            <div className="flex items-center justify-between h-full px-4 w-full relative z-10">
                <div className="flex items-baseline gap-2">
                    <div className="text-4xl font-bold tabular-nums tracking-tighter leading-none text-white">
                        {value}
                    </div>
                    <div className="text-xs font-semibold text-white/40 uppercase tracking-widest translate-y-[-2px]">
                        {unit}
                    </div>
                </div>

                <button
                    onClick={isRunning ? stopSpeedTest : runSpeedTest}
                    className={`
                        flex items-center gap-2 px-5 py-2 rounded-full font-medium text-sm transition-all active:scale-95
                        ${isRunning
                            ? 'bg-red-500/15 text-red-500 hover:bg-red-500/25'
                            : 'bg-white/10 text-white hover:bg-white/20'
                        }
                    `}
                >
                    {isRunning ? (
                        <>
                            <IconPlayerStop size={16} className="opacity-90" />
                            <span>Stop</span>
                        </>
                    ) : (
                        <>
                            <IconPlayerPlay size={16} className="opacity-90" />
                            <span>Run</span>
                        </>
                    )}
                </button>
            </div>

            {/* Progress Bar */}
            {progress > 0 && (
                <div className="absolute bottom-0 left-0 h-[3px] bg-blue-500 transition-all duration-300 ease-out"
                    style={{ width: `${progress}%` }}
                />
            )}
        </WidgetWrapper>
    );
}

// Register the speed test widget
registerWidget({
    id: 'speedtest',
    name: 'Speed Test',
    description: 'Test your internet download speed',
    icon: IconGauge,
    ExpandedComponent: SpeedTestWidget,
    defaultEnabled: false,
    category: 'utility',
    minWidth: 280,
    hasCompactMode: false
});
