import { useAppContext } from '@/contexts/AppContext';
import { AlertCircle, Trash2, FileText, Check, Save, X, Loader2, Database, AlertTriangle } from 'lucide-react';
import { Button } from '../ui/Button';
import { clearDownloadHistory, getDownloadHistory, saveDownloadHistory } from '@/api/invoke';
import { useState, useRef, useEffect } from 'react';
import { twMerge } from 'tailwind-merge';

export function GeneralSettings() {
    const { 
        maxConcurrentDownloads, 
        maxTotalInstances, 
        setConcurrency,
        logLevel,
        setLogLevel
    } = useAppContext();

    // --- History Management State ---
    const [clearStatus, setClearStatus] = useState<'idle' | 'confirming' | 'cleared'>('idle');
    const [clearTimer, setClearTimer] = useState(100);
    const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
    
    // Editor State
    const [isEditingHistory, setIsEditingHistory] = useState(false);
    const [historyContent, setHistoryContent] = useState('');
    const originalHistoryRef = useRef(''); 
    const [isLoadingHistory, setIsLoadingHistory] = useState(false);
    const [isSavingHistory, setIsSavingHistory] = useState(false);

    // Helper to handle line ending differences between OS files and Textarea
    const normalize = (str: string) => str.replace(/\r\n/g, '\n');

    // --- Confirmation Timer Logic ---
    useEffect(() => {
        if (clearStatus === 'confirming') {
            setClearTimer(100);
            const startTime = Date.now();
            const duration = 5000; // 5 seconds

            timerRef.current = setInterval(() => {
                const elapsed = Date.now() - startTime;
                const remaining = Math.max(0, 100 - (elapsed / duration) * 100);
                
                setClearTimer(remaining);

                if (remaining <= 0) {
                    setClearStatus('idle');
                    if (timerRef.current) clearInterval(timerRef.current);
                }
            }, 16);
        } else {
            if (timerRef.current) clearInterval(timerRef.current);
        }

        return () => {
            if (timerRef.current) clearInterval(timerRef.current);
        };
    }, [clearStatus]);

    const handleChange = (key: 'max_concurrent_downloads' | 'max_total_instances', value: number) => {
        let concurrent = maxConcurrentDownloads;
        let total = maxTotalInstances;

        if (key === 'max_concurrent_downloads') {
            concurrent = value;
            if (value > total) {
                total = value;
            }
        } else {
            total = value;
            if (value < concurrent) {
                concurrent = value;
            }
        }
        setConcurrency(concurrent, total);
    };

    const handleClearHistory = async () => {
        if (clearStatus === 'idle') {
            setClearStatus('confirming');
            return;
        }

        if (clearStatus === 'confirming') {
            try {
                await clearDownloadHistory();
                setClearStatus('cleared');
                setTimeout(() => setClearStatus('idle'), 2000);
            } catch (error) {
                console.error("Failed to clear history", error);
                setClearStatus('idle');
            }
        }
    };

    const handleOpenEditor = async () => {
        setIsLoadingHistory(true);
        try {
            const content = await getDownloadHistory();
            setHistoryContent(content);
            originalHistoryRef.current = content;
            setIsEditingHistory(true);
        } catch (error) {
            console.error("Failed to load history", error);
            alert("Failed to read history file: " + error);
        } finally {
            setIsLoadingHistory(false);
        }
    };

    const handleCloseEditor = (e?: React.MouseEvent) => {
        if (e) {
            e.preventDefault();
            e.stopPropagation();
        }

        const isDirty = normalize(historyContent) !== normalize(originalHistoryRef.current);
        
        // Syn: even gemini 3 couldn't fix this confirmation dialogue.
        if (isDirty) {
            const confirmed = window.confirm("You have unsaved changes in your archive file. Are you sure you want to discard them?");
            if (!confirmed) return;
        }

        // Reset to original to avoid state retention issues
        setHistoryContent(originalHistoryRef.current);
        setIsEditingHistory(false);
    };

    const handleSaveEditor = async () => {
        setIsSavingHistory(true);
        try {
            await saveDownloadHistory(historyContent);
            originalHistoryRef.current = historyContent; // Update reference so it's no longer dirty
            setIsEditingHistory(false);
        } catch (error) {
            console.error("Failed to save history", error);
            alert("Failed to save history file: " + error);
        } finally {
            setIsSavingHistory(false);
        }
    };

    // --- FULL SCREEN EDITOR MODE ---
    if (isEditingHistory) {
        const isDirty = normalize(historyContent) !== normalize(originalHistoryRef.current);

        return (
            <div className="absolute inset-0 bg-zinc-950 z-50 flex flex-col animate-fade-in">
                {/* Editor Header */}
                <div className="flex items-center justify-between px-6 py-4 border-b border-zinc-800 bg-zinc-900">
                    <div className="flex items-center gap-3">
                         <div className="p-2 rounded bg-zinc-800 text-zinc-400">
                            <FileText className="h-5 w-5" />
                         </div>
                         <div>
                             <div className="flex items-center gap-2">
                                <h3 className="font-bold text-zinc-100">Archive Editor</h3>
                                {isDirty && (
                                    <span className="text-[10px] bg-theme-cyan/20 text-theme-cyan px-1.5 py-0.5 rounded font-bold uppercase tracking-widest border border-theme-cyan/30">
                                        Modified
                                    </span>
                                )}
                             </div>
                             <p className="text-xs text-zinc-500 font-mono">downloads.txt</p>
                         </div>
                    </div>
                    <div className="flex items-center gap-2">
                        <Button 
                            variant="secondary"
                            onClick={handleCloseEditor}
                            className="h-8 gap-2"
                            disabled={isSavingHistory}
                        >
                            <X className="h-4 w-4" />
                            Close
                        </Button>
                        <Button 
                            variant="default"
                            onClick={handleSaveEditor}
                            className="h-8 gap-2 min-w-[100px]"
                            disabled={isSavingHistory}
                        >
                            {isSavingHistory ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
                            Save
                        </Button>
                    </div>
                </div>

                {/* Editor Content */}
                <div className="flex-1 relative">
                    <textarea 
                        value={historyContent}
                        onChange={(e) => setHistoryContent(e.target.value)}
                        className="absolute inset-0 w-full h-full bg-zinc-950 text-zinc-300 font-mono text-xs p-6 resize-none focus:outline-none focus:ring-1 focus:ring-theme-cyan/10 leading-relaxed"
                        spellCheck={false}
                        placeholder="No history entries found."
                    />
                </div>
            </div>
        );
    }

    // --- STANDARD SETTINGS MODE ---
    return (
        <div className="space-y-10 animate-fade-in pb-12 relative">
            {/* Queue Management Section */}
            <div id="section-queue" className="space-y-4 scroll-mt-6">
                <div>
                    <h3 className="text-base font-medium text-zinc-100 flex items-center gap-2">
                        Queue Management
                    </h3>
                    <p className="text-sm text-zinc-500">
                        Control how many downloads happen at once to manage bandwidth and CPU usage.
                    </p>
                </div>
                <hr className="border-zinc-800" />

                <div className="space-y-8 bg-zinc-900/30 p-5 rounded-lg border border-zinc-800/50">
                    <div className="space-y-4">
                        <div className="flex justify-between items-center">
                            <label className="text-sm font-medium text-zinc-300">Active Downloads</label>
                            <span className="text-theme-cyan font-mono font-bold bg-theme-cyan/10 px-2 py-1 rounded border border-theme-cyan/20">
                                {maxConcurrentDownloads}
                            </span>
                        </div>
                        <input
                            type="range"
                            min="1"
                            max="15"
                            value={maxConcurrentDownloads}
                            onChange={(e) => handleChange('max_concurrent_downloads', parseInt(e.target.value))}
                            className="w-full h-2 bg-zinc-800 rounded-lg appearance-none cursor-pointer accent-theme-cyan hover:accent-theme-cyan/80 transition-all"
                        />
                        <p className="text-xs text-zinc-500">
                            Maximum number of files downloading from the internet simultaneously.
                        </p>
                    </div>

                    <div className="space-y-4">
                        <div className="flex justify-between items-center">
                            <label className="text-sm font-medium text-zinc-300">Total Concurrent Instances</label>
                            <span className={`font-mono font-bold px-2 py-1 rounded border ${maxTotalInstances > 10 ? 'text-theme-red bg-theme-red/10 border-theme-red/20' : 'text-theme-cyan bg-theme-cyan/10 border-theme-cyan/20'}`}>
                                {maxTotalInstances}
                            </span>
                        </div>
                        <input
                            type="range"
                            min="1"
                            max="20"
                            value={maxTotalInstances}
                            onChange={(e) => handleChange('max_total_instances', parseInt(e.target.value))}
                            className={`w-full h-2 bg-zinc-800 rounded-lg appearance-none cursor-pointer transition-all ${maxTotalInstances > 10 ? 'accent-theme-red hover:accent-theme-red/80' : 'accent-theme-cyan hover:accent-theme-cyan/80'}`}
                        />
                        <p className="text-xs text-zinc-500">
                            Includes active downloads AND videos that are currently merging/processing.
                        </p>

                        {/* High Usage Warning */}
                        {maxTotalInstances > 10 && (
                            <div className="flex items-start gap-3 p-3 bg-red-950/20 border border-red-500/20 rounded-md animate-fade-in">
                                <AlertTriangle className="h-5 w-5 text-red-500 flex-shrink-0" />
                                <div className="space-y-1">
                                    <p className="text-xs font-bold text-red-400">High CPU Usage Warning</p>
                                    <p className="text-[10px] text-red-300/70 leading-relaxed">
                                        Running more than 10 simultaneous ffmpeg instances may significantly degrade system performance on mid-range hardware or lower. Proceed with caution.
                                    </p>
                                </div>
                            </div>
                        )}
                    </div>
                </div>
            </div>

            {/* History Management Section */}
            <div id="section-history" className="space-y-4 scroll-mt-6">
                <div>
                    <h3 className="text-base font-medium text-zinc-100">Download History</h3>
                    <p className="text-sm text-zinc-500">
                        Manage the database of previously downloaded URLs to prevent duplicates.
                    </p>
                </div>
                <hr className="border-zinc-800" />

                <div className="bg-zinc-900/30 px-5 py-4 rounded-lg border border-zinc-800/50 flex items-center justify-between gap-4">
                    <div className="flex items-center gap-3">
                        <div className="p-2.5 bg-zinc-800 rounded-md text-zinc-400">
                            <Database className="h-5 w-5" />
                        </div>
                        <div>
                            <div className="text-sm font-medium text-zinc-200">Archive File</div>
                            <div className="text-xs text-zinc-500">Tracks downloaded URLs</div>
                        </div>
                    </div>

                    <div className="flex flex-col gap-2 w-[220px]">
                        <Button 
                            variant="secondary" 
                            size="sm" 
                            className="h-9 border-zinc-700 hover:border-zinc-500 hover:text-white"
                            onClick={handleOpenEditor}
                            disabled={isLoadingHistory || clearStatus === 'confirming'}
                        >
                            {isLoadingHistory ? <Loader2 className="h-3.5 w-3.5 animate-spin mr-2" /> : <FileText className="h-3.5 w-3.5 mr-2" />}
                            Edit Archive Manually
                        </Button>

                        <button 
                            disabled={clearStatus === 'cleared'}
                            onClick={handleClearHistory}
                            className={twMerge(
                                "relative h-9 rounded-md transition-all duration-300 flex items-center justify-center overflow-hidden border",
                                clearStatus === 'idle' && "bg-zinc-900 border-zinc-800 text-zinc-400 hover:border-red-500/50 hover:text-red-400",
                                clearStatus === 'confirming' && "bg-zinc-900 border-red-500 text-red-500",
                                clearStatus === 'cleared' && "bg-emerald-500/10 border-emerald-500/50 text-emerald-500"
                            )}
                        >
                            {/* Visual Wipe Indicator */}
                            {clearStatus === 'confirming' && (
                                <div 
                                    className="absolute inset-0 bg-red-600/20"
                                    style={{ 
                                        width: `${clearTimer}%`, 
                                        transition: 'width 16ms linear',
                                        left: 0
                                    }}
                                />
                            )}

                            <span className="relative z-10 flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest">
                                {clearStatus === 'idle' && (
                                    <>
                                        <Trash2 className="h-3.5 w-3.5" />
                                        Clear History
                                    </>
                                )}
                                {clearStatus === 'confirming' && (
                                    <>
                                        <AlertTriangle className="h-3.5 w-3.5 animate-pulse" />
                                        Click Again to Erase
                                    </>
                                )}
                                {clearStatus === 'cleared' && (
                                    <>
                                        <Check className="h-3.5 w-3.5" />
                                        Database Cleared
                                    </>
                                )}
                            </span>
                        </button>
                    </div>
                </div>
            </div>

            {/* Debugging Section */}
            <div id="section-logging" className="space-y-4 scroll-mt-6">
                <div>
                    <h3 className="text-base font-medium text-zinc-100">Application Logging</h3>
                    <p className="text-sm text-zinc-500">
                        Configure system verbosity for troubleshooting. Logs are saved to <code>.multiyt-dlp/logs</code>.
                    </p>
                </div>
                <hr className="border-zinc-800" />

                <div className="bg-zinc-900/30 p-5 rounded-lg border border-zinc-800/50 space-y-4">
                    <div className="flex items-start justify-between gap-4">
                        <div className="space-y-1">
                            <label className="text-sm font-medium text-zinc-300">Log Verbosity</label>
                            <div className="text-xs text-zinc-500 max-w-xs">
                                Controls how much detail is written to the log files.
                            </div>
                        </div>
                        
                        <select 
                            value={logLevel}
                            onChange={(e) => setLogLevel(e.target.value)}
                            className="bg-zinc-900 border border-zinc-800 rounded-md px-3 py-1.5 text-sm text-zinc-200 focus:outline-none focus:border-theme-cyan/50 focus:ring-1 focus:ring-theme-cyan/50 transition-all"
                        >
                            <option value="off">Off</option>
                            <option value="error">Error</option>
                            <option value="warn">Warn</option>
                            <option value="info">Info</option>
                            <option value="debug">Debug</option>
                            <option value="trace">Trace</option>
                        </select>
                    </div>

                    {logLevel === 'debug' || logLevel === 'trace' ? (
                        <div className="flex items-center gap-3 text-xs text-amber-500 bg-amber-950/20 border border-amber-900/50 p-3 rounded-md animate-fade-in">
                            <AlertCircle className="h-4 w-4 flex-shrink-0" />
                            <span>High verbosity enabled. This will generate large log files and may impact performance.</span>
                        </div>
                    ) : null}
                </div>
            </div>
        </div>
    );
}