import { useRef, useState, useLayoutEffect } from 'react';
import { motion } from 'motion/react';

interface ScrollingTextProps {
    children: string;
    className?: string; // Expect this to contain font styles, margins, padding, etc.
}

export function ScrollingText({ children, className = "" }: ScrollingTextProps) {
    const [isOverflowing, setIsOverflowing] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const measureRef = useRef<HTMLSpanElement>(null);

    useLayoutEffect(() => {
        const check = () => {
            if (containerRef.current && measureRef.current) {
                // measureRef is 'max-content', so offsetWidth is the full required width including padding from className
                const contentWidth = measureRef.current.offsetWidth;
                // containerRef is constrained by parent. clientWidth is available inner width (including padding)
                const containerWidth = containerRef.current.clientWidth;

                // Check for overflow.
                // We consider it overflowing if the content (plus its inherent padding)
                // is wider than the container's standard width.
                // Added a small buffer (0.5) for floating point rendering differences.
                setIsOverflowing(contentWidth > containerWidth + 0.5);
            }
        };

        check();

        // Observer for container resize (e.g. window resize or panel expansion)
        const observer = new ResizeObserver(check);
        if (containerRef.current) observer.observe(containerRef.current);

        return () => {
            observer.disconnect();
        }
    }, [children, className]);

    const duration = Math.max((children.length * 8) / 30, 20); // Slightly slower read speed

    return (
        <div
            ref={containerRef}
            className={`w-full overflow-hidden whitespace-nowrap relative ${className}`}
            title={children}
        >
            {/* Ghost element for measurement.
                 It needs to exactly mimic the spacing/size of the content
                 if it were rendered normally in this container.
             */}
            <span
                ref={measureRef}
                className={className}
                style={{
                    position: 'absolute',
                    visibility: 'hidden',
                    width: 'max-content',
                    height: 0,
                    margin: 0,
                    display: 'inline-block',
                    left: 0,
                    top: 0,
                    // Reset border/outline to not affect measurement if they exist in className
                    border: 'none',
                    outline: 'none'
                }}
                aria-hidden="true"
            >
                {children}
            </span>

            {!isOverflowing ? (
                <span className="truncate block">
                    {children}
                </span>
            ) : (
                <div className="flex">
                    <motion.div
                        className="flex will-change-transform"

                        animate={{ x: "-50%" }}
                        initial={{ x: 0 }}
                        transition={{
                            duration: duration,
                            ease: "linear",
                            repeat: Infinity,
                            repeatType: "loop"
                        }}
                    >
                        {/* We rely on inheritance for text styles from the parent 'className'
                             We add explicit padding/gap locally for the marquee loop */}
                        <div className="flex items-center">
                            <span className="whitespace-nowrap">{children}</span>
                            <span className="w-2 block shrink-0" /> {/* Spacer */}
                        </div>
                        <div className="flex items-center">
                            <span className="whitespace-nowrap">{children}</span>
                            <span className="w-2 block shrink-0" /> {/* Spacer */}
                        </div>
                    </motion.div>
                </div>
            )}
        </div>
    );
}
