import { useAppContext } from '@/contexts/AppContext';
import { AlertCircle, Trash2, FileText, Check, Save, X, Loader2, Database, AlertTriangle, Search, ChevronDown, Rocket, Layers } from 'lucide-react';
import { Button } from '../ui/Button';
import { clearDownloadHistory, getDownloadHistory, saveDownloadHistory } from '@/api/invoke';
import { useState, useRef, useEffect } from 'react';
import { twMerge } from 'tailwind-merge';

export function GeneralSettings() {
    const { 
        maxConcurrentDownloads, 
        maxTotalInstances, 
        setConcurrency,
        useConcurrentFragments,
        concurrentFragments,
        setFragmentSettings,
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

    // Search State
    const [isSearchOpen, setIsSearchOpen] = useState(false);
    const [searchTerm, setSearchTerm] = useState('');
    const [lastMatchIndex, setLastMatchIndex] = useState(-1);
    const searchInputRef = useRef<HTMLInputElement>(null);
    const textareaRef = useRef<HTMLTextAreaElement>(null);

    // Helper to handle line ending differences
    const normalize = (str: string) => str.replace(/\r\n/g, '\n');

    // --- Confirmation Timer Logic ---
    useEffect(() => {
        if (clearStatus === 'confirming') {
            setClearTimer(100);
            const startTime = Date.now();
            const duration = 5000;

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
        return () => { if (timerRef.current) clearInterval(timerRef.current); };
    }, [clearStatus]);

    // Handle Editor Shortcuts
    useEffect(() => {
        if (isEditingHistory) {
            const handleKeyDown = (e: KeyboardEvent) => {
                if ((e.ctrlKey || e.metaKey) && (e.key === 'f' || e.key === 'k')) {
                    e.preventDefault();
                    setIsSearchOpen(true);
                    setTimeout(() => {
                        searchInputRef.current?.focus();
                        searchInputRef.current?.select();
                    }, 50);
                }
                if (e.key === 'Escape' && isSearchOpen) {
                    setIsSearchOpen(false);
                    textareaRef.current?.focus();
                }
            };
            window.addEventListener('keydown', handleKeyDown);
            return () => window.removeEventListener('keydown', handleKeyDown);
        }
    }, [isEditingHistory, isSearchOpen]);

    const handleChangeConcurrency = (key: 'max_concurrent_downloads' | 'max_total_instances', value: number) => {
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
        if (e) { e.preventDefault(); e.stopPropagation(); }
        const isDirty = normalize(historyContent) !== normalize(originalHistoryRef.current);
        if (isDirty) {
            const confirmed = window.confirm("You have unsaved changes in your archive file. Are you sure you want to discard them?");
            if (!confirmed) return;
        }
        setHistoryContent(originalHistoryRef.current);
        setIsEditingHistory(false);
        setIsSearchOpen(false);
        setSearchTerm('');
    };

    const handleSaveEditor = async () => {
        setIsSavingHistory(true);
        try {
            await saveDownloadHistory(historyContent);
            originalHistoryRef.current = historyContent;
            setIsEditingHistory(false);
        } catch (error) {
            console.error("Failed to save history", error);
            alert("Failed to save history file: " + error);
        } finally {
            setIsSavingHistory(false);
        }
    };

    const handleFindNext = (e?: React.FormEvent) => {
        if (e) e.preventDefault();
        if (!searchTerm || !textareaRef.current) return;
        const content = textareaRef.current.value;
        const searchLower = searchTerm.toLowerCase();
        const startPos = lastMatchIndex + 1;
        let nextIndex = content.toLowerCase().indexOf(searchLower, startPos);
        if (nextIndex === -1) {
            nextIndex = content.toLowerCase().indexOf(searchLower, 0);
        }
        if (nextIndex !== -1) {
            textareaRef.current.focus();
            textareaRef.current.setSelectionRange(nextIndex, nextIndex + searchTerm.length);
            const lineHeight = 16; 
            const line = content.substring(0, nextIndex).split('\n').length;
            textareaRef.current.scrollTop = (line - 5) * lineHeight;
            setLastMatchIndex(nextIndex);
        }
    };

    // --- FULL SCREEN EDITOR MODE ---
    if (isEditingHistory) {
        const isDirty = normalize(historyContent) !== normalize(originalHistoryRef.current);
        return (
            <div className="absolute inset-0 bg-zinc-950 z-50 flex flex-col animate-fade-in">
                {/* Editor Header */}
                <div className="flex flex-col border-b border-zinc-800 bg-zinc-900 shadow-xl z-10">
                    <div className="flex items-center justify-between px-6 py-4">
                        <div className="flex items-center gap-3">
                            <div className="p-2 rounded bg-zinc-800 text-zinc-400">
                                <FileText className="h-5 w-5" />
                            </div>
                            <div>
                                <div className="flex items-center gap-2">
                                    <h3 className="font-bold text-zinc-100">Archive Editor</h3>
                                    {isDirty && (
                                        <span className="text-[10px] bg-theme-cyan/20 text-theme-cyan px-1.5 py-0.5 rounded font-bold uppercase tracking-widest border border-theme-cyan/30">Modified</span>
                                    )}
                                </div>
                                <p className="text-xs text-zinc-500 font-mono">downloads.txt</p>
                            </div>
                        </div>
                        <div className="flex items-center gap-2">
                            <Button variant="secondary" onClick={() => setIsSearchOpen(!isSearchOpen)} className={twMerge("h-8 gap-2", isSearchOpen && "bg-zinc-800 text-theme-cyan")} title="Find (Ctrl+F)">
                                <Search className="h-4 w-4" /> Find
                            </Button>
                            <div className="w-px h-6 bg-zinc-800 mx-1" />
                            <Button variant="secondary" onClick={handleCloseEditor} className="h-8 gap-2" disabled={isSavingHistory}>
                                <X className="h-4 w-4" /> Close
                            </Button>
                            <Button variant="default" onClick={handleSaveEditor} className="h-8 gap-2 min-w-[100px]" disabled={isSavingHistory}>
                                {isSavingHistory ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />} Save
                            </Button>
                        </div>
                    </div>
                    {/* Search Sub-header */}
                    {isSearchOpen && (
                        <div className="px-6 py-3 bg-zinc-950 border-t border-zinc-800 flex items-center gap-3 animate-fade-in">
                            <form onSubmit={handleFindNext} className="flex-1 max-w-md relative">
                                <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-zinc-500" />
                                <input ref={searchInputRef} type="text" placeholder="Find in file..." value={searchTerm} onChange={(e) => { setSearchTerm(e.target.value); setLastMatchIndex(-1); }} className="w-full bg-zinc-900 border border-zinc-800 rounded px-9 py-1.5 text-xs text-zinc-200 focus:outline-none focus:border-theme-cyan/50 focus:ring-1 focus:ring-theme-cyan/50" />
                                {searchTerm && <button type="button" onClick={() => setSearchTerm('')} className="absolute right-3 top-1/2 -translate-y-1/2 text-zinc-500 hover:text-zinc-300"><X className="h-3 w-3" /></button>}
                            </form>
                            <Button size="sm" variant="secondary" className="h-7 text-[10px] gap-1" onClick={() => handleFindNext()}><ChevronDown className="h-3 w-3" /> Next</Button>
                            <button onClick={() => setIsSearchOpen(false)} className="text-xs text-zinc-500 hover:text-zinc-300 ml-auto">ESC to close</button>
                        </div>
                    )}
                </div>
                <div className="flex-1 relative">
                    <textarea ref={textareaRef} value={historyContent} onChange={(e) => setHistoryContent(e.target.value)} className="absolute inset-0 w-full h-full bg-zinc-950 text-zinc-300 font-mono text-xs p-6 resize-none focus:outline-none focus:ring-1 focus:ring-theme-cyan/10 leading-relaxed" spellCheck={false} placeholder="No history entries found." />
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
                        Execution Strategy
                    </h3>
                    <p className="text-sm text-zinc-500">
                        Select how the engine processes your download queue.
                    </p>
                </div>
                <hr className="border-zinc-800" />

                <div className="bg-zinc-900/30 p-5 rounded-lg border border-zinc-800/50 space-y-6">
                    
                    {/* Mode Toggle */}
                    <div className="grid grid-cols-2 gap-3 p-1 bg-zinc-950 rounded-lg border border-zinc-800">
                        <button 
                            onClick={() => setFragmentSettings(false, concurrentFragments)}
                            className={twMerge(
                                "flex items-center justify-center gap-3 py-3 rounded-md transition-all duration-300 border",
                                !useConcurrentFragments 
                                    ? "bg-zinc-800 border-zinc-600 text-white shadow-lg" 
                                    : "bg-transparent border-transparent text-zinc-500 hover:text-zinc-300 hover:bg-zinc-900"
                            )}
                        >
                            <Layers className={twMerge("h-5 w-5", !useConcurrentFragments && "text-theme-cyan")} />
                            <div className="text-left">
                                <div className="text-sm font-bold">Fleet Mode</div>
                                <div className="text-[10px] opacity-70">Parallel Files</div>
                            </div>
                        </button>
                        
                        <button 
                             onClick={() => setFragmentSettings(true, concurrentFragments)}
                             className={twMerge(
                                "flex items-center justify-center gap-3 py-3 rounded-md transition-all duration-300 border",
                                useConcurrentFragments 
                                    ? "bg-zinc-800 border-zinc-600 text-white shadow-lg" 
                                    : "bg-transparent border-transparent text-zinc-500 hover:text-zinc-300 hover:bg-zinc-900"
                            )}
                        >
                            <Rocket className={twMerge("h-5 w-5", useConcurrentFragments && "text-amber-500")} />
                            <div className="text-left">
                                <div className="text-sm font-bold">Blitz Mode</div>
                                <div className="text-[10px] opacity-70">Single File Turbo</div>
                            </div>
                        </button>
                    </div>

                    {/* Mode Description Area */}
                    <div className="animate-fade-in min-h-[160px]">
                        {!useConcurrentFragments ? (
                            // FLEET MODE SETTINGS
                            <div className="space-y-6">
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
                                        onChange={(e) => handleChangeConcurrency('max_concurrent_downloads', parseInt(e.target.value))}
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
                                        onChange={(e) => handleChangeConcurrency('max_total_instances', parseInt(e.target.value))}
                                        className={`w-full h-2 bg-zinc-800 rounded-lg appearance-none cursor-pointer transition-all ${maxTotalInstances > 10 ? 'accent-theme-red hover:accent-theme-red/80' : 'accent-theme-cyan hover:accent-theme-cyan/80'}`}
                                    />
                                    <p className="text-xs text-zinc-500">
                                        Includes active downloads AND videos that are currently merging/processing.
                                    </p>
                                </div>
                            </div>
                        ) : (
                            // BLITZ MODE SETTINGS
                            <div className="space-y-6">

                                <div className="space-y-4">
                                    <div className="flex justify-between items-center">
                                        <label className="text-sm font-medium text-zinc-300">Concurrent Fragments</label>
                                        <span className="text-amber-500 font-mono font-bold bg-amber-500/10 px-2 py-1 rounded border border-amber-500/20">
                                            {concurrentFragments}
                                        </span>
                                    </div>
                                    <input
                                        type="range"
                                        min="1"
                                        max="16"
                                        value={concurrentFragments}
                                        onChange={(e) => setFragmentSettings(true, parseInt(e.target.value))}
                                        className="w-full h-2 bg-zinc-800 rounded-lg appearance-none cursor-pointer accent-amber-500 hover:accent-amber-500/80 transition-all"
                                    />
                                    <p className="text-xs text-zinc-500">
                                        Number of parallel threads for the active download (passed as <code>-N</code>). Higher values utilize more bandwidth but may trigger server throttling.
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
                        <Button variant="secondary" size="sm" className="h-9 border-zinc-700 hover:border-zinc-500 hover:text-white" onClick={handleOpenEditor} disabled={isLoadingHistory || clearStatus === 'confirming'}>
                            {isLoadingHistory ? <Loader2 className="h-3.5 w-3.5 animate-spin mr-2" /> : <FileText className="h-3.5 w-3.5 mr-2" />} Edit Archive Manually
                        </Button>
                        <button disabled={clearStatus === 'cleared'} onClick={handleClearHistory} className={twMerge("relative h-9 rounded-md transition-all duration-300 flex items-center justify-center overflow-hidden border", clearStatus === 'idle' && "bg-zinc-900 border-zinc-800 text-zinc-400 hover:border-red-500/50 hover:text-red-400", clearStatus === 'confirming' && "bg-zinc-900 border-red-500 text-red-500", clearStatus === 'cleared' && "bg-emerald-500/10 border-emerald-500/50 text-emerald-500")}>
                            {clearStatus === 'confirming' && <div className="absolute inset-0 bg-red-600/20" style={{ width: `${clearTimer}%`, transition: 'width 16ms linear', left: 0 }} />}
                            <span className="relative z-10 flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest">
                                {clearStatus === 'idle' && <><Trash2 className="h-3.5 w-3.5" /> Clear History</>}
                                {clearStatus === 'confirming' && <><AlertTriangle className="h-3.5 w-3.5 animate-pulse" /> Click Again to Erase</>}
                                {clearStatus === 'cleared' && <><Check className="h-3.5 w-3.5" /> Database Cleared</>}
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
                            <div className="text-xs text-zinc-500 max-w-xs">Controls how much detail is written to the log files.</div>
                        </div>
                        <select value={logLevel} onChange={(e) => setLogLevel(e.target.value)} className="bg-zinc-900 border border-zinc-800 rounded-md px-3 py-1.5 text-sm text-zinc-200 focus:outline-none focus:border-theme-cyan/50 focus:ring-1 focus:ring-theme-cyan/50 transition-all">
                            <option value="off">Off</option>
                            <option value="error">Error</option>
                            <option value="warn">Warn</option>
                            <option value="info">Info</option>
                            <option value="debug">Debug</option>
                            <option value="trace">Trace</option>
                        </select>
                    </div>
                    {(logLevel === 'debug' || logLevel === 'trace') && (
                        <div className="flex items-center gap-3 text-xs text-amber-500 bg-amber-950/20 border border-amber-900/50 p-3 rounded-md animate-fade-in">
                            <AlertCircle className="h-4 w-4 flex-shrink-0" />
                            <span>High verbosity enabled. This will generate large log files and may impact performance.</span>
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
}