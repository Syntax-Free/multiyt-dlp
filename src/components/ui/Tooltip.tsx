import React, { useState, useRef, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { HelpCircle } from 'lucide-react';
import { twMerge } from 'tailwind-merge';

export function Tooltip({ content, children, className }: { content: React.ReactNode, children?: React.ReactNode, className?: string }) {
    const [isVisible, setIsVisible] = useState(false);
    const [coords, setCoords] = useState({ x: 0, y: 0 });
    const [shift, setShift] = useState(0);
    const triggerRef = useRef<HTMLDivElement>(null);
    const tooltipRef = useRef<HTMLDivElement>(null);

    const updateCoords = () => {
        if (triggerRef.current) {
            const rect = triggerRef.current.getBoundingClientRect();
            setCoords({ 
                x: rect.left + rect.width / 2, 
                y: rect.top 
            });
            setShift(0); // Reset shift to ensure natural boundary detection
        }
    };

    useEffect(() => {
        if (isVisible && tooltipRef.current) {
            const rect = tooltipRef.current.getBoundingClientRect();
            const tooltipHalfWidth = rect.width / 2;
            const padding = 12;
            const x = coords.x;
            
            let currentShift = 0;
            
            // Screen boundary detection using calculated accurate widths
            if (x - tooltipHalfWidth < padding) {
                currentShift = padding - (x - tooltipHalfWidth);
            } else if (x + tooltipHalfWidth > window.innerWidth - padding) {
                currentShift = (window.innerWidth - padding) - (x + tooltipHalfWidth);
            }

            // Cap the shift mathematically to prevent the arrow from detaching entirely
            const maxShift = Math.max(0, tooltipHalfWidth - 10); 
            if (currentShift > maxShift) currentShift = maxShift;
            if (currentShift < -maxShift) currentShift = -maxShift;

            setShift(currentShift);
        }
    }, [isVisible, coords.x]);

    useEffect(() => {
        if (isVisible) {
            // Dismiss tooltips dynamically on scroll/resize to prevent visual decoupling
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
                    <div 
                        className="absolute bottom-full mb-1.5 drop-shadow-xl"
                        style={{ transform: `translateX(calc(-50% + ${shift}px))` }}
                    >
                        <div 
                            ref={tooltipRef}
                            className="w-max max-w-[260px] px-3 py-2 text-[11px] font-medium text-zinc-200 bg-zinc-900 border border-zinc-700 rounded-lg shadow-black/50 text-center leading-relaxed"
                        >
                            {content}
                        </div>
                        {/* 
                            Arrow is counter-shifted inversely to ensure it always 
                            resolves its rotation origin accurately relative to the trigger.
                            It sits in a negative z-index stacking context underneath the opaque wrapper.
                        */}
                        <div 
                            className="absolute w-2 h-2 bg-zinc-900 border-b border-r border-zinc-700 -z-10"
                            style={{ 
                                bottom: '-4px',
                                left: `calc(50% - ${shift}px)`,
                                transform: 'translateX(-50%) rotate(45deg)'
                            }} 
                        />
                    </div>
                </div>,
                document.body
            )}
        </div>
    );
}