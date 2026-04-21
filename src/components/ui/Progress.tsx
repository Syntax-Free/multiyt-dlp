import React from 'react';
import { twMerge } from 'tailwind-merge';

interface ProgressProps extends React.HTMLAttributes<HTMLDivElement> {
    value?: number;
    variant?: 'default' | 'success' | 'error';
}

const Progress = React.forwardRef<HTMLDivElement, ProgressProps>(
  ({ className, value = 0, variant = 'default', ...props }, ref) => {
    
    // Default maps to our Cyan theme
    const colors = {
        default: 'bg-theme-cyan shadow-[0_0_10px_rgba(0,242,234,0.4)]',
        success: 'bg-emerald-500',
        error: 'bg-theme-red shadow-[0_0_10px_rgba(255,0,80,0.4)]',
    };

    return (
      <div 
        ref={ref} 
        className={twMerge("relative w-full h-1 bg-zinc-900 rounded-full overflow-hidden", className)}
        {...props}
      >
        <div 
            className={twMerge(
                "h-full transition-all duration-500 ease-linear rounded-full",
                colors[variant] || colors.default
            )}
            style={{ width: `${value}%` }}
        />
      </div>
    );
  }
);

Progress.displayName = 'Progress';

export { Progress };