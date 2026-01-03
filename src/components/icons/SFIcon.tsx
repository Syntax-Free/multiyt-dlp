import { twMerge } from "tailwind-merge";

interface SynIconProps {
    className?: string;
}

export function SFIcon({ className }: SynIconProps) {
    return (
        <svg 
            xmlns="http://www.w3.org/2000/svg" 
            viewBox="0 0 1080 1080" 
            className={twMerge("overflow-visible", className)}
        >
            <g>
                {/* Top/Left Shape - Cyan with strong Cyan Glow */}
                <polygon 
                    points="113.23 570.22 378.87 701.43 378.87 624.82 191.68 531.68 378.87 438.55 378.87 361.93 113.23 494.98"
                    className="fill-theme-cyan drop-shadow-[0_0_12px_rgba(0,242,234,0.9)]"
                />
                {/* Bottom/Right Shape - Red with strong Red Glow */}
                <polygon 
                    points="966.12 494.89 700.48 363.68 700.48 440.3 887.67 533.43 700.48 626.57 700.48 703.18 966.12 570.14"
                    className="fill-theme-red drop-shadow-[0_0_12px_rgba(255,0,80,0.9)]"
                />
            </g>
        </svg>
    );
}