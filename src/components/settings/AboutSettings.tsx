import { useEffect, useState } from 'react';
import { getName } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';
import { checkDependencies, installDependency, openExternalLink } from '@/api/invoke';
import { DependencyInfo } from '@/types';
import { Copy, Check, Terminal, AlertCircle, Cpu, Download, Loader2, ArrowUpCircle, RefreshCw, Zap, Box, Lock } from 'lucide-react';
import icon from '@/assets/icon.webp';
import { Button } from '../ui/Button';
import { Progress } from '../ui/Progress';
import { useAppContext } from '@/contexts/AppContext';
import { useDownloadManager } from '@/hooks/useDownloadManager';

interface InstallProgress {
    name: string;
    percentage: number;
    status: string;
}

interface DependencyRowProps {
    info: DependencyInfo;
    onInstall?: () => void;
    installingState?: InstallProgress | null;
    label?: string;
    description?: string;
    isQueueBusy: boolean;
}

const DependencyRow = ({ info, onInstall, installingState, label, description, isQueueBusy }: DependencyRowProps) => {
    const [copied, setCopied] = useState(false);
    
    // Detection logic for isolated (managed) vs system-wide binaries
    const isManaged = info.path && (
        info.path.includes('.multiyt-dlp') || 
        info.path.includes('AppData') || 
        info.path.includes('Library/Application Support') ||
        info.path.includes('.local/share')
    );
    const isAvailable = info.available;
    const isSystemOnly = isAvailable && !isManaged;
    const isUpdatingThis = installingState && (
        installingState.name.toLowerCase().includes(info.name.toLowerCase()) || 
        (label && installingState.name.toLowerCase().includes(label.toLowerCase()))
    );

    const handleCopy = () => {
        if (info.path) {
            navigator.clipboard.writeText(info.path);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        }
    };

    return (
        <div className="bg-zinc-900/50 border border-zinc-800 rounded-lg p-4 flex flex-col gap-3 transition-all">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                    <div className="p-2 rounded-md bg-zinc-800 text-zinc-400">
                        {label?.includes('Aria2') ? <Zap className="h-4 w-4 text-theme-cyan" /> : <Terminal className="h-4 w-4" />}
                    </div>
                    <div>
                        <div className="font-semibold text-zinc-200 text-sm">
                            {label || info.name}
                            {isSystemOnly && (
                                <span className="ml-2 text-[9px] bg-zinc-800 text-zinc-500 px-1.5 py-0.5 rounded border border-zinc-700 uppercase">System Path</span>
                            )}
                        </div>
                        {isAvailable ? (
                             <div className="text-[10px] text-emerald-500 font-mono flex items-center gap-1">
                                <Check className="h-3 w-3" /> {info.version || 'Detected'}
                                {!info.is_supported && <span className="text-theme-red ml-2 font-bold uppercase tracking-tighter">Incompatible</span>}
                                {info.is_supported && !info.is_recommended && <span className="text-amber-500 ml-2">Legacy Build</span>}
                             </div>
                        ) : (
                             <div className="text-[10px] text-theme-red font-mono flex items-center gap-1">
                                <AlertCircle className="h-3 w-3" /> Not Installed
                             </div>
                        )}
                    </div>
                </div>
                
                {onInstall && (
                    <Button 
                        size="sm" 
                        variant={isSystemOnly ? "neon" : "outline"}
                        onClick={onInstall} 
                        className="h-7 text-[10px] uppercase font-bold tracking-wider"
                        disabled={!!installingState || isQueueBusy}
                        title={isQueueBusy ? "Modifications locked during active downloads" : ""}
                    >
                        {isUpdatingThis ? (
                            <Loader2 className="h-3 w-3 animate-spin" />
                        ) : isSystemOnly ? (
                            <>
                                <Box className="h-3 w-3 mr-1" /> 
                                Localize
                            </>
                        ) : isAvailable ? (
                            <>
                                <RefreshCw className="h-3 w-3 mr-1" /> 
                                Update
                            </>
                        ) : (
                            <>
                                <Download className="h-3 w-3 mr-1" /> 
                                Install
                            </>
                        )}
                    </Button>
                )}
            </div>

            {/* Help text for Localization */}
            {isSystemOnly && !isUpdatingThis && !isQueueBusy && (
                <div className="text-[10px] text-zinc-500 leading-relaxed bg-theme-cyan/5 p-2 rounded border border-theme-cyan/20">
                    <span className="text-theme-cyan font-bold">Note:</span> This binary is currently provided by your operating system. Click <span className="text-zinc-300">"Localize"</span> to download an app-managed version for better stability and automatic updates.
                </div>
            )}

            {/* Description for Optional/Indeterminate Deps */}
            {description && !isAvailable && !isUpdatingThis && (
                <div className="text-[10px] text-zinc-500 leading-relaxed bg-zinc-950/50 p-2 rounded border border-zinc-800/50">
                    {description}
                </div>
            )}

            {/* Installation Progress Bar */}
            {isUpdatingThis && (
                <div className="space-y-1.5 animate-fade-in">
                    <div className="flex justify-between text-[10px] font-mono">
                        <span className="text-theme-cyan uppercase">{installingState.status}</span>
                        <span className="text-zinc-400">{installingState.percentage}%</span>
                    </div>
                    <Progress value={installingState.percentage} className="h-1" />
                </div>
            )}

            {info.path && !isUpdatingThis && (
                <div className="relative group">
                    <input 
                        readOnly
                        value={info.path} 
                        className="w-full bg-zinc-950 text-zinc-500 text-xs font-mono py-2 px-3 rounded border border-zinc-800 focus:outline-none"
                    />
                    <button 
                        onClick={handleCopy}
                        className="absolute right-1 top-1 bottom-1 px-3 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded text-xs flex items-center gap-2 transition-colors"
                        title="Copy Path"
                    >
                        {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
                    </button>
                </div>
            )}
        </div>
    );
};

export function AboutSettings() {
    const [appName, setAppName] = useState("Loading...");
    const [deps, setDeps] = useState<{ yt_dlp?: DependencyInfo, ffmpeg?: DependencyInfo, js_runtime?: DependencyInfo, aria2?: DependencyInfo }>({});
    const [loading, setLoading] = useState(true);
    const [activeInstall, setActiveInstall] = useState<InstallProgress | null>(null);
    const [checkingUpdate, setCheckingUpdate] = useState(false);

    const { currentVersion, latestVersion, isUpdateAvailable, checkAppUpdate } = useAppContext();
    const { downloads } = useDownloadManager();

    // Check if any job is currently active or queued
    const isQueueBusy = Array.from(downloads.values()).some(
        d => d.status === 'downloading' || d.status === 'pending'
    );

    const fetchData = async () => {
        try {
            const name = await getName();
            const dependencies = await checkDependencies();
            setAppName(name);
            setDeps(dependencies);
        } catch (e) {
            console.error("Failed to fetch system info", e);
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        fetchData();

        const unlisten = listen<InstallProgress>('install-progress', (event) => {
            setActiveInstall(event.payload);
        });

        return () => {
            unlisten.then(f => f());
        };
    }, []);

    const handleInstall = async (name: string) => {
        if (activeInstall || isQueueBusy) return;

        try {
            await installDependency(name);
            await fetchData();
        } catch (e) {
            console.error(`Installation failed: ${e}`);
        } finally {
            setActiveInstall(null);
        }
    };

    const handleUpdateCheck = async () => {
        setCheckingUpdate(true);
        await checkAppUpdate();
        setTimeout(() => setCheckingUpdate(false), 500);
    };

    if (loading) {
        return <div className="p-10 text-center text-zinc-500 text-sm animate-pulse">Scanning System...</div>;
    }

    return (
        <div className="space-y-6 animate-fade-in pb-10">
            <div className="flex items-center gap-5 pb-4 border-b border-zinc-800">
                <img src={icon} className="w-16 h-16 rounded-xl shadow-glow-cyan" alt="App Icon" />
                <div className="flex-1">
                    <h2 className="text-xl font-bold text-zinc-100 tracking-tight">{appName}</h2>
                    <div className="flex items-center gap-2 mt-1">
                        <span className="text-xs font-mono text-zinc-500">v{currentVersion}</span>
                        <span className="px-1.5 py-0.5 text-[9px] bg-theme-cyan/10 text-theme-cyan border border-theme-cyan/20 rounded uppercase font-bold tracking-wider">
                            Stable
                        </span>
                    </div>
                </div>
            </div>

            <div className="space-y-4">
                 <div className="bg-zinc-900/30 border border-zinc-800 p-4 rounded-lg flex items-center justify-between">
                    <div>
                        <div className="text-sm font-medium text-zinc-200">Application Version</div>
                        {isUpdateAvailable ? (
                            <div className="text-xs text-theme-cyan mt-1 flex items-center gap-2">
                                <ArrowUpCircle className="h-3 w-3" />
                                <span>Update Available: v{latestVersion}</span>
                            </div>
                        ) : (
                            <div className="text-xs text-zinc-500 mt-1">You are on the latest version.</div>
                        )}
                    </div>
                    <div className="flex items-center gap-2">
                        {isUpdateAvailable && (
                            <Button 
                                size="sm" 
                                variant="neon" 
                                className="h-8 text-xs"
                                disabled={isQueueBusy}
                                onClick={() => openExternalLink("https://github.com/zqily/multiyt-dlp/releases/latest")}
                            >
                                <Download className="h-3 w-3 mr-1" /> Update
                            </Button>
                        )}
                        <Button size="sm" variant="secondary" className="h-8 w-8 p-0" onClick={handleUpdateCheck}>
                            <RefreshCw className={`h-3 w-3 ${checkingUpdate ? 'animate-spin' : ''}`} />
                        </Button>
                    </div>
                </div>
            </div>

            <div id="section-deps" className="space-y-3 pt-4 border-t border-zinc-800 scroll-mt-6">
                <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2 text-sm text-zinc-400 font-medium">
                        <Cpu className="h-4 w-4" />
                        <span>System Dependencies</span>
                    </div>
                    {isQueueBusy && (
                        <div className="flex items-center gap-2 text-[10px] font-bold text-amber-500 bg-amber-500/10 px-2 py-1 rounded border border-amber-500/20 animate-pulse">
                            <Lock className="h-3 w-3" />
                            <span>MODIFICATIONS LOCKED</span>
                        </div>
                    )}
                </div>

                {isQueueBusy && (
                    <div className="p-3 bg-zinc-900/80 border border-zinc-800 rounded-lg text-xs text-zinc-400 leading-relaxed mb-2">
                        Dependencies cannot be installed or updated while downloads are in progress. Please wait for the queue to finish or cancel active tasks to unlock these settings.
                    </div>
                )}
                
                <div className="grid grid-cols-1 gap-3">
                    {deps.yt_dlp && (
                        <DependencyRow 
                            info={deps.yt_dlp} 
                            onInstall={() => handleInstall('yt-dlp')}
                            installingState={activeInstall}
                            label="yt-dlp"
                            isQueueBusy={isQueueBusy}
                        />
                    )}
                    {deps.ffmpeg && (
                        <DependencyRow 
                            info={deps.ffmpeg} 
                            onInstall={() => handleInstall('ffmpeg')}
                            installingState={activeInstall}
                            label="FFmpeg"
                            isQueueBusy={isQueueBusy}
                        />
                    )}
                    {deps.aria2 && (
                        <DependencyRow 
                            info={deps.aria2} 
                            onInstall={() => handleInstall('aria2')}
                            installingState={activeInstall}
                            label="Aria2c (Accelerator)"
                            description="Optional high-speed downloader. Improves update speed for dependencies by using multiple concurrent connections."
                            isQueueBusy={isQueueBusy}
                        />
                    )}
                    {deps.js_runtime && (
                        <DependencyRow 
                            info={deps.js_runtime} 
                            onInstall={() => {
                                 const name = deps.js_runtime?.name?.toLowerCase() || '';
                                 // Force Deno if Node is detected for localization, as we don't provider Node installers
                                 if (name.includes('node') || !name) handleInstall('deno');
                                 else handleInstall(name);
                            }}
                            installingState={activeInstall}
                            label={`JS Runtime (${deps.js_runtime.name})`}
                            isQueueBusy={isQueueBusy}
                        />
                    )}
                </div>
            </div>
        </div>
    );
}