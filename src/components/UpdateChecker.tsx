import { useEffect, useState } from 'react';
import { useAppContext } from '@/contexts/AppContext';
import { SFIcon } from './icons/SFIcon';
import { twMerge } from 'tailwind-merge';

export function UpdateChecker() {
    const { updateCheckStatus } = useAppContext();
    const [visible, setVisible] = useState(false);
    
    // Auto-hide logic for success state
    useEffect(() => {
        if (updateCheckStatus === 'checking' || updateCheckStatus === 'error') {
            setVisible(true);
        } else if (updateCheckStatus === 'success') {
            const timer = setTimeout(() => setVisible(false), 3000);
            return () => clearTimeout(timer);
        } else {
            setVisible(false);
        }
    }, [updateCheckStatus]);

    if (!visible) return null;

    const isError = updateCheckStatus === 'error';

    return (
        <div className="fixed bottom-6 right-6 z-40 flex items-center gap-3 px-4 py-3 bg-zinc-950/90 backdrop-blur-md border border-zinc-800 rounded-lg shadow-2xl animate-fade-in pointer-events-none select-none">
            <style>{`
                @keyframes shake {
                    0%, 100% { transform: translateX(0); }
                    20% { transform: translateX(-5px); }
                    40% { transform: translateX(5px); }
                    60% { transform: translateX(-5px); }
                    80% { transform: translateX(5px); }
                }
                .animate-shake {
                    animation: shake 0.5s cubic-bezier(.36,.07,.19,.97) both;
                }
            `}</style>
            
            <div className={twMerge(
                "w-7 h-7 transition-colors duration-500 ease-in-out",
                isError ? "text-theme-red animate-shake" : "text-zinc-500 animate-spin"
            )}>
                <SFIcon className={twMerge(
                    "w-full h-full",
                    isError ? "drop-shadow-[0_0_10px_rgba(255,0,80,0.6)]" : ""
                )} />
            </div>
            
            <span className={twMerge(
                "text-xs font-black tracking-widest uppercase transition-colors duration-500",
                isError ? "text-theme-red animate-shake" : "text-zinc-500"
            )}>
                {isError ? "Update Check Failed" : "Checking for updates..."}
            </span>
        </div>
    );
}