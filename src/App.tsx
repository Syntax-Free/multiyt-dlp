import { useEffect, useState, useRef } from 'react';
import { appWindow } from '@tauri-apps/api/window';
import { DownloadForm } from './components/DownloadForm';
import { DownloadQueue } from './components/DownloadQueue';
import { useDownloadManager } from './hooks/useDownloadManager';
import { Layout } from './components/Layout';
import { SplashWindow } from './components/SplashWindow';
import { Activity, CheckCircle2, AlertCircle, List, Database, Hourglass, LayoutGrid, Ban, Trash2, RefreshCw, Filter, X } from 'lucide-react';
import { twMerge } from 'tailwind-merge';
import { useAppContext } from './contexts/AppContext';
import { Button } from './components/ui/Button';

function App() {
  const { skipNotice, setSkipNotice, getTemplateString, preferences } = useAppContext();
  const [windowLabel, setWindowLabel] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<'list' | 'grid'>('list');
  const [isProcessingRetry, setIsProcessingRetry] = useState(false);
  
  // Bulk Cancel State
  const [cancelStatus, setCancelStatus] = useState<'idle' | 'confirming'>('idle');
  const [cancelTimer, setCancelTimer] = useState(100);
  const cancelTimerInterval = useRef<ReturnType<typeof setInterval> | null>(null);

  const { downloads, startDownload, cancelDownload, removeDownload, cancelAllDownloads } = useDownloadManager();
  const prevCountRef = useRef(0);

  useEffect(() => {
    setWindowLabel(appWindow.label);
  }, []);

  useEffect(() => {
    const count = downloads.size;
    const prevCount = prevCountRef.current;

    if (prevCount <= 20 && count > 20 && viewMode === 'list') {
        setViewMode('grid');
    }
    
    prevCountRef.current = count;
  }, [downloads.size, viewMode]);

  // Handle Global Cancel Confirmation Timer
  useEffect(() => {
    if (cancelStatus === 'confirming') {
        setCancelTimer(100);
        const startTime = Date.now();
        const duration = 4000; // 4 second window to confirm

        cancelTimerInterval.current = setInterval(() => {
            const elapsed = Date.now() - startTime;
            const remaining = Math.max(0, 100 - (elapsed / duration) * 100);
            setCancelTimer(remaining);

            if (remaining <= 0) {
                setCancelStatus('idle');
                if (cancelTimerInterval.current) clearInterval(cancelTimerInterval.current);
            }
        }, 16);
    } else {
        if (cancelTimerInterval.current) clearInterval(cancelTimerInterval.current);
    }
    return () => { if (cancelTimerInterval.current) clearInterval(cancelTimerInterval.current); };
  }, [cancelStatus]);

  if (!windowLabel) return null;

  if (windowLabel === 'splashscreen') {
      return <SplashWindow />;
  }

  // --- ACTIONS ---

  const handleClearCompleted = () => {
    const completedJobs = Array.from(downloads.values()).filter(d => d.status === 'completed');
    completedJobs.forEach(job => removeDownload(job.jobId));
  };

  const handleRetryFailed = () => {
    const failedJobs = Array.from(downloads.values()).filter(d => d.status === 'error');
    failedJobs.forEach(job => {
        removeDownload(job.jobId);
        startDownload(
            job.url,
            job.downloadPath,
            job.preset || 'best',
            job.videoResolution || 'best',
            job.embedMetadata || false,
            job.embedThumbnail || false,
            job.filenameTemplate || "%(title)s.%(ext)s",
            true, 
            true  
        );
    });
  };

  const handleBulkCancel = () => {
      if (cancelStatus === 'idle') {
          setCancelStatus('confirming');
      } else {
          cancelAllDownloads();
          setCancelStatus('idle');
      }
  };

  const handleRetrySkipped = async () => {
    if (!skipNotice) return;
    
    setIsProcessingRetry(true);
    try {
        await startDownload(
            skipNotice.url,
            undefined, 
            preferences.format_preset as any,
            preferences.video_resolution,
            preferences.embed_metadata,
            preferences.embed_thumbnail,
            getTemplateString(),
            false,
            true, 
            skipNotice.skippedUrls,
            preferences.live_from_start
        );
        setSkipNotice(null);
    } catch (e) {
        console.error("Retry failed", e);
    } finally {
        setIsProcessingRetry(false);
    }
  };

  const toggleViewMode = () => {
      setViewMode(prev => prev === 'list' ? 'grid' : 'list');
  };

  // Calculate Stats
  const total = downloads.size;
  const active = Array.from(downloads.values()).filter(d => d.status === 'downloading').length;
  const queued = Array.from(downloads.values()).filter(d => d.status === 'pending').length;
  const completed = Array.from(downloads.values()).filter(d => d.status === 'completed').length;
  const failed = Array.from(downloads.values()).filter(d => d.status === 'error').length;
  const hasActiveJobs = active > 0 || queued > 0;

  return (
      <Layout
        SidebarContent={
          <DownloadForm onDownload={startDownload} />
        }
        MainContent={
          <>
            <div className="flex items-center justify-between mb-4 bg-zinc-900/40 border border-zinc-800 rounded-lg p-4 transition-all">
                <div className="flex items-center gap-4">
                    <button 
                        onClick={toggleViewMode}
                        className="p-2 rounded-md bg-zinc-900 border border-zinc-800 text-zinc-400 hover:text-theme-cyan hover:border-theme-cyan/50 hover:bg-zinc-800 transition-all cursor-pointer group flex-shrink-0"
                        title={viewMode === 'list' ? "Switch to Grid View" : "Switch to List View"}
                    >
                         {viewMode === 'list' ? (
                             <List className="h-6 w-6 group-hover:scale-110 transition-transform" /> 
                         ) : (
                             <LayoutGrid className="h-6 w-6 group-hover:scale-110 transition-transform" />
                         )}
                    </button>

                    <div className="min-w-0">
                        <h2 className="text-lg font-semibold text-zinc-100 leading-tight truncate">
                            Download Queue
                        </h2>
                        <div className="text-xs text-zinc-500 font-mono mt-1 hidden sm:block">
                            SESSION: {Math.floor(Date.now() / 1000).toString(16).toUpperCase()}
                        </div>
                    </div>
                </div>

                <div className="flex items-center gap-6 overflow-x-auto overflow-y-hidden no-scrollbar pl-4">
                    <div className="flex items-center gap-6 text-sm">
                        
                        <div className="flex flex-col items-end flex-shrink-0">
                            <span className="text-[10px] text-zinc-600 uppercase tracking-wider font-bold">Total</span>
                            <div className="flex items-center gap-1.5 text-zinc-200 font-mono">
                                <Database className="h-3 w-3 text-zinc-500" />
                                {total}
                            </div>
                        </div>
                        
                        <div className="w-px h-8 bg-zinc-800 flex-shrink-0" />

                        {/* Combined Active/Queue Stats with Hover-to-Cancel */}
                        <div 
                            className={twMerge(
                                "relative flex items-center h-full group/stats py-2 -my-2 cursor-default",
                                hasActiveJobs ? "hover:cursor-pointer" : ""
                            )}
                            onMouseLeave={() => setCancelStatus('idle')}
                        >
                            {/* Standard Stats View (Hidden on Hover if jobs active) */}
                            <div className={twMerge(
                                "flex items-center gap-6 transition-opacity duration-200",
                                hasActiveJobs ? "group-hover/stats:opacity-0" : ""
                            )}>
                                <div className="flex flex-col items-end flex-shrink-0">
                                    <span className="text-[10px] text-zinc-600 uppercase tracking-wider font-bold">Queued</span>
                                    <div className="flex items-center gap-1.5 text-zinc-200 font-mono">
                                        <Hourglass className="h-3 w-3 text-amber-500/80" />
                                        {queued}
                                    </div>
                                </div>

                                <div className="w-px h-8 bg-zinc-800 flex-shrink-0" />
                                
                                <div className="flex flex-col items-end flex-shrink-0">
                                    <span className="text-[10px] text-zinc-600 uppercase tracking-wider font-bold">Active</span>
                                    <div className="flex items-center gap-1.5 text-zinc-200 font-mono">
                                        <Activity className="h-3 w-3 text-theme-cyan" />
                                        {active}
                                    </div>
                                </div>
                            </div>

                            {/* Hover Action Button (Cancel All) */}
                            {hasActiveJobs && (
                                <div className="absolute inset-0 flex items-center justify-center opacity-0 group-hover/stats:opacity-100 transition-opacity duration-200 z-10">
                                    <button
                                        onClick={handleBulkCancel}
                                        className={twMerge(
                                            "w-[140%] h-9 -ml-[20%] rounded-md border text-[10px] font-black uppercase tracking-widest transition-all duration-300 flex items-center justify-center gap-2 overflow-hidden shadow-lg",
                                            cancelStatus === 'idle' 
                                                ? "bg-zinc-900 border-zinc-700 text-zinc-400 hover:border-theme-red/50 hover:text-theme-red hover:bg-zinc-800"
                                                : "bg-theme-red/10 border-theme-red text-theme-red"
                                        )}
                                    >
                                        {cancelStatus === 'confirming' && (
                                            <div 
                                                className="absolute inset-0 bg-theme-red/10"
                                                style={{ width: `${cancelTimer}%`, transition: 'width 16ms linear' }}
                                            />
                                        )}
                                        <span className="relative z-10 flex items-center gap-2 whitespace-nowrap">
                                            <Ban className={twMerge("h-3 w-3", cancelStatus === 'confirming' && "animate-pulse")} />
                                            {cancelStatus === 'idle' ? "Cancel All" : "Confirm"}
                                        </span>
                                    </button>
                                </div>
                            )}
                        </div>
                        
                        <div className="w-px h-8 bg-zinc-800 flex-shrink-0" />
                        
                        <div className={twMerge("relative flex flex-col items-end min-w-[40px] flex-shrink-0", completed > 0 ? "group cursor-pointer" : "")}>
                            <div className={twMerge("flex flex-col items-end transition-opacity duration-200", completed > 0 && "group-hover:opacity-0")}>
                                <span className="text-[10px] text-zinc-600 uppercase tracking-wider font-bold">Done</span>
                                <div className="flex items-center gap-1.5 text-zinc-200 font-mono">
                                    <CheckCircle2 className="h-3 w-3 text-emerald-500" />
                                    {completed}
                                </div>
                            </div>
                            
                            {completed > 0 && (
                                <button 
                                    onClick={handleClearCompleted}
                                    className="absolute inset-0 hidden group-hover:flex items-center justify-center bg-zinc-900/90 backdrop-blur-sm border border-zinc-800 rounded shadow-lg animate-fade-in hover:border-emerald-500/50 hover:bg-zinc-800"
                                    title="Clear Completed"
                                >
                                    <Trash2 className="h-4 w-4 text-emerald-500" />
                                </button>
                            )}
                        </div>
                        
                        <div className="w-px h-8 bg-zinc-800 flex-shrink-0" />
                        
                        <div className={twMerge("relative flex flex-col items-end min-w-[40px] flex-shrink-0", failed > 0 ? "group cursor-pointer" : "")}>
                            <div className={twMerge("flex flex-col items-end transition-opacity duration-200", failed > 0 && "group-hover:opacity-0")}>
                                <span className="text-[10px] text-zinc-600 uppercase tracking-wider font-bold">Failed</span>
                                <div className="flex items-center gap-1.5 text-zinc-200 font-mono">
                                    <AlertCircle className="h-3 w-3 text-theme-red" />
                                    {failed}
                                </div>
                            </div>

                            {failed > 0 && (
                                <button 
                                    onClick={handleRetryFailed}
                                    className="absolute inset-0 hidden group-hover:flex items-center justify-center bg-zinc-900/90 backdrop-blur-sm border border-zinc-800 rounded shadow-lg animate-fade-in hover:border-theme-red/50 hover:bg-zinc-800"
                                    title="Retry Failed"
                                >
                                    <RefreshCw className="h-4 w-4 text-theme-red" />
                                </button>
                            )}
                        </div>
                    </div>
                </div>
            </div>

            {skipNotice && (
                <div className="mb-4 animate-fade-in relative p-4 rounded-lg border bg-zinc-900/60 border-theme-cyan/20 backdrop-blur-sm flex items-center justify-between gap-4 shadow-lg shadow-theme-cyan/5">
                    <div className="flex items-center gap-4">
                        <div className="p-2.5 bg-theme-cyan/10 rounded-full text-theme-cyan ring-1 ring-theme-cyan/20">
                             <Filter className="h-5 w-5" />
                        </div>
                        <div>
                            <div className="text-sm font-black text-zinc-100 uppercase tracking-wider">
                                {skipNotice.skipped === skipNotice.total 
                                    ? "Duplicate Blocked" 
                                    : "Partial Duplicate Filter"}
                            </div>
                            <div className="text-xs text-zinc-400 mt-0.5">
                                {skipNotice.skipped === skipNotice.total 
                                    ? "The requested URL already exists in your download history."
                                    : `Filtered ${skipNotice.skipped} duplicates. ${skipNotice.total - skipNotice.skipped} new items were added to the queue.`
                                }
                            </div>
                        </div>
                    </div>
                    
                    <div className="flex items-center gap-3">
                         <Button
                            variant="neon"
                            size="sm"
                            className="h-8 px-4 text-[10px] font-black"
                            disabled={isProcessingRetry}
                            onClick={handleRetrySkipped}
                        >
                            {isProcessingRetry ? <RefreshCw className="h-3 w-3 animate-spin mr-2" /> : <RefreshCw className="h-3 w-3 mr-2" />}
                            FORCE DOWNLOAD
                        </Button>
                        <button 
                            onClick={() => setSkipNotice(null)}
                            className="p-1 text-zinc-600 hover:text-white transition-colors"
                        >
                            <X className="h-4 w-4" />
                        </button>
                    </div>
                </div>
            )}

            <DownloadQueue 
                downloads={downloads} 
                onCancel={cancelDownload} 
                viewMode={viewMode}
            />
          </>
        }
      />
  );
}

export default App;