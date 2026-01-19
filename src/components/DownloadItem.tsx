import { Download } from '@/types';
import { Progress } from './ui/Progress';
import { Button } from './ui/Button';
import { X, MonitorPlay, Clock, CheckCircle2, AlertCircle, Headphones, Activity, FileOutput, Tags, FileText, Image as ImageIcon, Hourglass, FolderSearch, FileCode, Copy, Trash2 } from 'lucide-react';
import { twMerge } from 'tailwind-merge';
import { showInFolder, openLogFolder } from '@/api/invoke';
import { useState } from 'react';
import { SmartError } from './ui/SmartError';

interface DownloadItemProps {
  download: Download;
  onCancel: (jobId: string) => void;
}

export function DownloadItem({ download, onCancel }: DownloadItemProps) {

  const { 
    jobId, url, status, progress, speed, eta, 
    error, filename, phase, preset, embedMetadata, 
    embedThumbnail, outputPath, stderr, logs, exit_code 
  } = download;

  const [showLogs, setShowLogs] = useState(false);
  const displayTitle = filename || url;
  const isAudio = preset?.startsWith('audio');

  const isQueued = status === 'pending';
  const isActive = status === 'downloading'; 
  const isError = status === 'error';
  const isCompleted = status === 'completed';
  const isCancelled = status === 'cancelled';

  const formatStat = (text?: string) => {
      if (!text || text === 'Unknown' || text === 'N/A') return <span className="animate-pulse text-zinc-600">--</span>;
      return text;
  };

  // Expanded check for various processing phases
  const isProcessingPhase = phase?.includes('Merging') 
    || phase?.includes('Extracting') 
    || phase?.includes('Fixing')
    || phase?.includes('Starting')
    || phase?.includes('Initializing')
    || phase?.includes('Moving')
    || phase?.includes('Finalizing')
    || phase?.includes('Processing');

  const isMetaPhase = phase?.includes('Metadata') || phase?.includes('Thumbnail');

  const getIcon = () => {
      if (isError) return <AlertCircle className="h-5 w-5 text-theme-red" />;
      if (isCompleted) return <CheckCircle2 className="h-5 w-5 text-theme-cyan" />;
      if (isCancelled) return <X className="h-5 w-5 text-zinc-600" />;
      if (isQueued) return <Hourglass className="h-5 w-5 text-zinc-500 animate-pulse" />;
      
      if (isMetaPhase) return <Tags className="h-5 w-5 text-yellow-400 animate-pulse" />;
      if (isProcessingPhase) return <FileOutput className="h-5 w-5 text-zinc-100 animate-pulse" />;

      return isAudio 
        ? <Headphones className="h-5 w-5 text-theme-red animate-pulse" /> 
        : <MonitorPlay className="h-5 w-5 text-theme-cyan animate-pulse" />;
  };

  let badgeText = isAudio ? 'AUDIO' : 'VIDEO';
  if (isActive || isQueued) {
      if (preset) {
          const parts = preset.split('_');
          if (parts.length > 1 && parts[1] !== 'best') {
             badgeText = parts[1].toUpperCase();
          }
      }
  }

  const handleOpenFolder = () => {
      if (outputPath) {
          showInFolder(outputPath).catch(console.error);
      }
  };

  const handleCopyLogs = () => {
      if (logs) {
          navigator.clipboard.writeText(logs);
      }
  };

  return (
    <div className={twMerge(
        "group animate-fade-in relative bg-surface border rounded-lg p-5 transition-all duration-300",
        isActive && "border-theme-cyan/30 shadow-[0_0_20px_-10px_rgba(0,242,234,0.1)]",
        (isProcessingPhase || isMetaPhase) && "border-yellow-500/30 shadow-[0_0_20px_-10px_rgba(234,179,8,0.2)]",
        isError && "border-theme-red/30",
        isQueued && "border-zinc-800/60 bg-zinc-900/30 opacity-80",
        isCancelled && "opacity-50 border-zinc-900 bg-zinc-950",
        (!isActive && !isError && !isQueued && !isCancelled) && "border-border"
    )}>
      
      <div className="flex items-start gap-5">
        <div className={twMerge(
            "h-12 w-12 flex-shrink-0 rounded-lg flex items-center justify-center border transition-colors duration-500",
            isActive && (isProcessingPhase || isMetaPhase) && "bg-yellow-500/10 border-yellow-500/30",
            isActive && !isProcessingPhase && !isMetaPhase && isAudio && "bg-theme-red/5 border-theme-red/20",
            isActive && !isProcessingPhase && !isMetaPhase && !isAudio && "bg-theme-cyan/5 border-theme-cyan/20",
            (!isActive || isCancelled) && "bg-zinc-900 border-zinc-800"
        )}>
          {getIcon()}
        </div>
        
        <div className="flex-grow min-w-0 space-y-3">
            <div className="flex justify-between items-start gap-4">
                 <div className="space-y-1 min-w-0">
                    <p className={twMerge(
                        "text-sm font-semibold truncate transition-colors",
                        isCancelled ? "text-zinc-500 line-through" : (isActive ? "text-zinc-100" : "text-zinc-400")
                    )} title={displayTitle}>
                        {displayTitle}
                    </p>
                    <div className="flex items-center gap-2 text-[10px] uppercase tracking-wider font-bold">
                        <span className={twMerge(
                            "px-1.5 py-0.5 rounded border transition-colors",
                            isCancelled
                                ? "border-zinc-800 text-zinc-700 bg-zinc-900"
                                : isQueued 
                                    ? "border-zinc-700 text-zinc-600 bg-zinc-800" 
                                    : isAudio 
                                        ? "border-theme-red/30 text-theme-red bg-theme-red/5" 
                                        : "border-theme-cyan/30 text-theme-cyan bg-theme-cyan/5"
                        )}>{badgeText}</span>
                        
                        {embedMetadata && (
                             <span className={twMerge("px-1.5 py-0.5 rounded border flex items-center gap-1", isCancelled ? "border-zinc-800 text-zinc-700 bg-zinc-900" : "border-zinc-700 text-zinc-400 bg-zinc-800/50")} title="Metadata Embedded">
                                <FileText className="h-3 w-3" /> TAGS
                             </span>
                        )}
                        
                        {embedThumbnail && (
                             <span className={twMerge("px-1.5 py-0.5 rounded border flex items-center gap-1", isCancelled ? "border-zinc-800 text-zinc-700 bg-zinc-900" : "border-zinc-700 text-zinc-400 bg-zinc-800/50")} title="Thumbnail Embedded">
                                <ImageIcon className="h-3 w-3" /> ART
                             </span>
                        )}

                        <span className={twMerge(
                            "flex items-center gap-1 transition-colors duration-300 ml-1",
                             isCancelled ? "text-zinc-700" : (isProcessingPhase || isMetaPhase) ? "text-yellow-400" : "text-zinc-500"
                        )}>
                            {isActive && <Activity className={twMerge("h-3 w-3", (isProcessingPhase || isMetaPhase) && "animate-spin")} />}
                            {phase || (isQueued ? "Waiting for slot..." : status)}
                        </span>
                    </div>
                 </div>

                 <div className="flex flex-col items-end gap-1">
                    {isActive && (
                         <span className="text-lg font-bold text-zinc-100 tabular-nums">
                            {progress.toFixed(0)}<span className="text-sm text-zinc-600">%</span>
                         </span>
                    )}
                    {isQueued && (
                        <span className="text-xs font-bold text-zinc-600 uppercase bg-zinc-900 border border-zinc-800 px-2 py-1 rounded">
                            Queued
                        </span>
                    )}
                    {isCancelled && (
                        <span className="text-xs font-bold text-zinc-700 uppercase bg-zinc-950 border border-zinc-900 px-2 py-1 rounded">
                            Cancelled
                        </span>
                    )}
                    {isError && exit_code !== undefined && (
                        <span className="text-[10px] font-mono text-theme-red bg-theme-red/10 border border-theme-red/20 px-1.5 py-0.5 rounded">
                            Exit Code: {exit_code}
                        </span>
                    )}
                 </div>
            </div>
            
            <div className="space-y-3">
                {isActive && (
                     <div className={twMerge("relative", (isProcessingPhase || isMetaPhase) && "opacity-80")}>
                        <Progress 
                            value={progress} 
                            variant={isError ? 'error' : 'default'} 
                            className={twMerge((isProcessingPhase || isMetaPhase) && "opacity-70")}
                        />
                        {(isProcessingPhase || isMetaPhase) && (
                            <div className="absolute inset-0 bg-yellow-400/20 animate-pulse rounded-full" />
                        )}
                     </div>
                )}
                
                {isQueued && (
                    <div className="w-full h-1 bg-zinc-900 rounded-full overflow-hidden relative">
                        <div className="absolute inset-0 w-full h-full bg-[linear-gradient(45deg,transparent_25%,rgba(255,255,255,0.05)_25%,rgba(255,255,255,0.05)_50%,transparent_50%,transparent_75%,rgba(255,255,255,0.05)_75%,rgba(255,255,255,0.05)_100%)] bg-[length:20px_20px] animate-[progress-stripes_1s_linear_infinite]" />
                    </div>
                )}
                
                {isError && (
                    <div className="space-y-2">
                        {/* New Smart Error Component */}
                        <SmartError error={error} stderr={stderr} />
                        
                        <div className="flex gap-2">
                            {logs && (
                                <Button 
                                    size="sm" 
                                    variant="outline" 
                                    className="h-7 text-xs border-theme-red/30 text-theme-red hover:bg-theme-red/10"
                                    onClick={() => setShowLogs(!showLogs)}
                                >
                                    <FileCode className="h-3 w-3 mr-2" />
                                    {showLogs ? 'Hide Logs' : 'View Logs'}
                                </Button>
                            )}
                             <Button 
                                size="sm" 
                                variant="outline" 
                                className="h-7 text-xs border-zinc-700 text-zinc-400 hover:text-zinc-200"
                                onClick={() => openLogFolder()}
                            >
                                Open Log Folder
                            </Button>
                        </div>
                        
                        {showLogs && logs && (
                             <div className="relative mt-2 p-3 bg-zinc-950 border border-zinc-800 rounded font-mono text-[10px] text-zinc-400 h-40 overflow-y-auto">
                                <pre>{logs}</pre>
                                <button 
                                    onClick={handleCopyLogs}
                                    className="absolute top-2 right-2 p-1.5 bg-zinc-800 hover:bg-zinc-700 rounded text-zinc-300 transition-colors"
                                    title="Copy to Clipboard"
                                >
                                    <Copy className="h-3 w-3" />
                                </button>
                             </div>
                        )}
                    </div>
                )}

                {isActive && !isProcessingPhase && !isMetaPhase && (
                    <div className="flex items-center justify-between text-xs text-zinc-500 font-mono">
                        <span title="Speed" className="text-zinc-400 min-w-[60px]">
                           {formatStat(speed)}
                        </span>
                        <span title="ETA" className="flex items-center gap-1 text-zinc-400">
                            <Clock className="h-3 w-3" /> {formatStat(eta)}
                        </span>
                    </div>
                )}
            </div>
        </div>

        {/* Actions Column */}
        <div className="flex flex-col justify-center pl-2 gap-2">
          {/* Active / Queued / Error / Cancelled states */}
          {(isActive || isQueued || isError || isCancelled) && (
             <Button 
                variant="ghost" 
                size="icon" 
                onClick={() => onCancel(jobId)} 
                className={twMerge(
                    "h-8 w-8 text-zinc-600 transition-all duration-300",
                    isError || isCancelled
                        ? "opacity-100 text-zinc-500 hover:bg-zinc-800 hover:text-red-400" // Trash icon style
                        : "opacity-0 group-hover:opacity-100 hover:bg-theme-red hover:text-white hover:scale-110 hover:shadow-glow-red" // Cancel icon style
                )}
                title={isError || isCancelled ? "Remove from List" : "Cancel Download"}
             >
                {/* Dynamically switch icon based on state */}
                {(isError || isCancelled) ? <Trash2 className="h-4 w-4" /> : <X className="h-4 w-4" />}
              </Button>
          )}

          {isCompleted && outputPath && (
              <Button
                variant="ghost"
                size="icon"
                onClick={handleOpenFolder}
                className="h-8 w-8 text-zinc-600 hover:text-theme-cyan hover:bg-theme-cyan/10 transition-all duration-300"
                title="Open File Location"
              >
                <FolderSearch className="h-4 w-4" />
              </Button>
          )}
        </div>
      </div>
    </div>
  );
}