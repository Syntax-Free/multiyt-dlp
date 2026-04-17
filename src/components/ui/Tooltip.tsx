import React, { useState, useRef, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { HelpCircle } from 'lucide-react';
import { twMerge } from 'tailwind-merge';

export function Tooltip({ content, children, className }: { content: React.ReactNode, children?: React.ReactNode, className?: string }) {
    const[isVisible, setIsVisible] = useState(false);
    const [coords, setCoords] = useState({ x: 0, y: 0 });
    const [shift, setShift] = useState(0);
    const triggerRef = useRef<HTMLDivElement>(null);

    const updateCoords = () => {
        if (triggerRef.current) {
            const rect = triggerRef.current.getBoundingClientRect();
            const x = rect.left + rect.width / 2;
            const y = rect.top;
            
            // Boundary detection to prevent screen-edge clipping
            let currentShift = 0;
            const tooltipHalfWidth = 130; // Approx half of max-w-[260px]
            const padding = 12;
            
            if (x - tooltipHalfWidth < padding) {
                currentShift = padding - (x - tooltipHalfWidth);
            } else if (x + tooltipHalfWidth > window.innerWidth - padding) {
                currentShift = (window.innerWidth - padding) - (x + tooltipHalfWidth);
            }

            setCoords({ x, y });
            setShift(currentShift);
        }
    };

    useEffect(() => {
        if (isVisible) {
            // Dismiss tooltips on scroll or resize to prevent visual detaching
            const handleScroll = () => setIsVisible(false);
            window.addEventListener('scroll', handleScroll, true);
            window.addEventListener('resize', handleScroll);
            return () => {
                window.removeEventListener('scroll', handleScroll, true);
                window.removeEventListener('resize', handleScroll);
            };
        }
    }, [isVisible]);

    return (
        <div 
            className="relative inline-flex items-center"
            onMouseEnter={() => { updateCoords(); setIsVisible(true); }}
            onMouseLeave={() => setIsVisible(false)}
            ref={triggerRef}
        >
            {children || <HelpCircle className={twMerge("h-3.5 w-3.5 text-zinc-500 hover:text-theme-cyan cursor-help transition-colors", className)} />}
            
            {isVisible && createPortal(
                <div 
                    className="fixed z-[100] pointer-events-none animate-fade-in"
                    style={{ left: coords.x, top: coords.y }}
                >
                    <div className="absolute bottom-full mb-1.5 -translate-x-1/2 drop-shadow-xl flex flex-col items-center">
                        <div 
                            className="w-max max-w-[260px] px-3 py-2 text-[11px] font-medium text-zinc-200 bg-zinc-900 border border-zinc-700 rounded-lg shadow-black/50 text-center leading-relaxed"
                            style={{ transform: `translateX(${shift}px)` }}
                        >
                            {content}
                        </div>
                        <div className="w-2 h-2 bg-zinc-900 border-b border-r border-zinc-700 rotate-45 -mt-1.5" />
                    </div>
                </div>,
                document.body
            )}
        </div>
    );
}