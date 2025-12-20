import { parseError } from '@/utils/errorRegistry';
import { useAppContext } from '@/contexts/AppContext';
import { Settings, ExternalLink, RefreshCw } from 'lucide-react';
import { Button } from './Button';

interface SmartErrorProps {
    error?: string;
    stderr?: string;
}

export function SmartError({ error, stderr }: SmartErrorProps) {
    const { openSettings } = useAppContext();
    const { title, description, actionLabel, actionType, actionTarget, rawMatches } = parseError(stderr, error);

    const handleAction = () => {
        if (actionType === 'OPEN_SETTINGS' && actionTarget) {
            const [tab, section] = actionTarget.split(':');
            openSettings(tab, section);
        } else if (actionType === 'OPEN_URL' && actionTarget) {
            window.open(actionTarget, '_blank');
        }
    };

    return (
        <div className="text-xs space-y-2">
            <div className={`p-3 rounded border font-sans ${
                rawMatches 
                    ? "bg-theme-red/10 border-theme-red/30 text-zinc-300" 
                    : "bg-zinc-950 border-zinc-800 text-zinc-500"
            }`}>
                <div className="flex items-start gap-2">
                    <div className="flex-1">
                        <div className={`font-bold mb-1 ${rawMatches ? "text-theme-red" : "text-zinc-400"}`}>
                            {title}
                        </div>
                        <div className="leading-relaxed opacity-90">
                            {description}
                        </div>
                    </div>
                </div>

                {actionLabel && (
                    <div className="mt-3 pt-2 border-t border-white/5 flex">
                        <Button 
                            size="sm" 
                            variant="ghost" 
                            className="h-6 px-0 text-theme-cyan hover:bg-transparent hover:underline hover:text-theme-cyan/80 p-0"
                            onClick={handleAction}
                        >
                            {actionType === 'OPEN_SETTINGS' && <Settings className="h-3 w-3 mr-1.5" />}
                            {actionType === 'OPEN_URL' && <ExternalLink className="h-3 w-3 mr-1.5" />}
                            {actionType === 'RETRY_WITH_AUTH' && <RefreshCw className="h-3 w-3 mr-1.5" />}
                            {actionLabel}
                        </Button>
                    </div>
                )}
            </div>
        </div>
    );
}