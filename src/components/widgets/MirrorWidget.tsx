import { useEffect, useRef, useState, useCallback } from 'react';
import { IconCamera, IconCameraOff } from '@tabler/icons-react';
import { registerWidget } from './WidgetRegistry';
import { WidgetWrapper } from './WidgetWrapper';
import { WidgetDialog } from './WidgetDialog';

export function MirrorWidget() {
    const videoRef = useRef<HTMLVideoElement>(null);
    const [error, setError] = useState<string | null>(null);
    const [isActive, setIsActive] = useState(false);
    const [isLoading, setIsLoading] = useState(false);

    const [stream, setStream] = useState<MediaStream | null>(null);

    const setVideoRef = useCallback((element: HTMLVideoElement | null) => {
        videoRef.current = element;
        if (element && stream) {
            element.srcObject = stream;
        }
    }, [stream]);

    useEffect(() => {
        let localStream: MediaStream | null = null;
        let isMounted = true;

        async function setupCamera() {
            if (!isActive) return;

            setIsLoading(true);
            try {
                localStream = await navigator.mediaDevices.getUserMedia({
                    video: {
                        facingMode: 'user',
                        aspectRatio: 1,
                        width: { ideal: 1080 },
                        height: { ideal: 1080 }
                    },
                    audio: false
                });

                if (isMounted) {
                    setStream(localStream);
                    setError(null);
                } else {
                    localStream.getTracks().forEach(track => track.stop());
                }
            } catch (err) {
                if (isMounted) {
                    console.error('Error accessing camera:', err);
                    setError('Access denied');
                }
            } finally {
                if (isMounted) {
                    setIsLoading(false);
                }
            }
        }

        setupCamera();

        return () => {
            isMounted = false;
            if (localStream) {
                localStream.getTracks().forEach(track => track.stop());
            }
            setStream(null);
        };
    }, [isActive]);

    return (
        <WidgetWrapper
            title="Mirror"
            className="flex flex-col p-0 h-full box-border overflow-hidden bg-black relative rounded-[22px] border border-white/10 aspect-square"
        >
            <div className="flex flex-col items-center justify-center h-full gap-4 p-4 text-center cursor-pointer group select-none relative"
                onClick={() => setIsActive(true)}
            >
                <div className="relative z-10 w-16 h-16 rounded-full bg-neutral-800 border border-white/10 flex items-center justify-center transition-all duration-300 group-hover:bg-neutral-700 group-hover:scale-105">
                    <IconCamera size={28} className="text-white/90" />
                </div>
            </div>

            <WidgetDialog
                open={isActive}
                onOpenChange={setIsActive}
                title="Mirror"
                variant="fullscreen"
            >
                {error ? (
                    <div className="flex flex-col items-center justify-center h-full text-white/50 gap-3 p-4 text-center z-10">
                        <div className="w-12 h-12 rounded-full bg-red-500/10 flex items-center justify-center">
                            <IconCameraOff size={24} className="text-red-400" />
                        </div>
                        <span className="text-sm font-medium text-red-200/80">{error}</span>
                    </div>
                ) : (
                    <div className="relative w-full h-full group bg-black">
                        <video
                            ref={setVideoRef}
                            autoPlay
                            playsInline
                            className="w-full h-full object-cover transform scale-x-[-1]"
                        />

                        {isLoading && (
                            <div className="absolute inset-0 flex items-center justify-center bg-black">
                                <div className="w-8 h-8 border-2 border-white/20 border-t-white rounded-full animate-spin" />
                            </div>
                        )}
                    </div>
                )}
            </WidgetDialog>
        </WidgetWrapper>
    );
}

// Register the mirror widget
registerWidget({
    id: 'mirror',
    name: 'Mirror',
    description: 'Use your camera as a mirror',
    icon: IconCamera,
    ExpandedComponent: MirrorWidget,
    defaultEnabled: false,
    category: 'utility',
    minWidth: 50,
    hasCompactMode: false
});
