import { Download } from '@/types';
import { Progress } from './ui/Progress';
import { Button } from './ui/Button';
import { X, MonitorPlay, Clock, CheckCircle2, AlertTriangle, Headphones, Activity, FileOutput, Tags, FileText, Image as ImageIcon, Hourglass, FolderSearch, Copy, Trash2, ChevronDown, ChevronUp } from 'lucide-react';
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
    embedThumbnail, outputPath, stderr, logs 
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

  const isProcessingPhase = isActive && (
       phase?.includes('Merging') 
    || phase?.includes('Extracting') 
    || phase?.includes('Fixing')
    || phase?.includes('Moving')
    || phase?.includes('Finalizing')
    || phase?.includes('Processing')
  );

  const isMetaPhase = isActive && (
       phase?.includes('Metadata') 
    || phase?.includes('Thumbnail')
  );

  const getStatusColor = () => {
      if (isError) return "text-red-500 bg-red-500/10 border-red-500/20";
      if (isCompleted) return "text-emerald-500 bg-emerald-500/10 border-emerald-500/20";
      if (isCancelled) return "text-zinc-500 bg-zinc-800/50 border-zinc-700/50";
      if (isProcessingPhase || isMetaPhase) return "text-amber-500 bg-amber-500/10 border-amber-500/20";
      if (isActive) return "text-theme-cyan bg-theme-cyan/10 border-theme-cyan/20";
      return "text-zinc-400 bg-zinc-800 border-zinc-700"; // Queued
  };

  const getIcon = () => {
      if (isError) return <AlertTriangle className="h-5 w-5 text-red-500" />;
      if (isCompleted) return <CheckCircle2 className="h-5 w-5 text-emerald-500" />;
      if (isCancelled) return <X className="h-5 w-5 text-zinc-500" />;
      if (isQueued) return <Hourglass className="h-5 w-5 text-zinc-500 animate-pulse" />;
      
      if (isMetaPhase) return <Tags className="h-5 w-5 text-amber-500 animate-pulse" />;
      if (isProcessingPhase) return <FileOutput className="h-5 w-5 text-amber-500 animate-pulse" />;

      return isAudio 
        ? <Headphones className="h-5 w-5 text-theme-red animate-pulse" /> 
        : <MonitorPlay className="h-5 w-5 text-theme-cyan animate-pulse" />;
  };

  let badgeText = isAudio ? 'AUDIO' : 'VIDEO';
  if (preset) {
      const parts = preset.split('_');
      if (parts.length > 1 && parts[1] !== 'best') {
         badgeText = parts[1].toUpperCase();
      }
  }

  const handleOpenFolder = () => outputPath && showInFolder(outputPath).catch(console.error);
  const handleCopyLogs = () => logs && navigator.clipboard.writeText(logs);

  return (
    <div className={twMerge(
        "group animate-fade-in relative bg-zinc-900/40 border rounded-lg p-4 transition-all duration-300 hover:bg-zinc-900/60",
        isError ? "border-red-900/30 hover:border-red-900/50" : 
        isCompleted ? "border-emerald-900/30 hover:border-emerald-900/50" : 
        isActive ? "border-theme-cyan/20 shadow-[0_0_15px_-10px_rgba(6,182,212,0.1)] hover:border-theme-cyan/40" : 
        "border-zinc-800 hover:border-zinc-700"
    )}>
      
      <div className="flex gap-4">
        {/* ICON BOX */}
        <div className={twMerge(
            "h-12 w-12 flex-shrink-0 rounded-lg flex items-center justify-center border transition-colors duration-500",
            isActive && !isProcessingPhase ? "bg-theme-cyan/5 border-theme-cyan/20" : "bg-zinc-950 border-zinc-800",
            (isProcessingPhase || isMetaPhase) && "bg-amber-500/5 border-amber-500/20",
            isError && "bg-red-500/5 border-red-500/20",
            isCompleted && "bg-emerald-500/5 border-emerald-500/20"
        )}>
          {getIcon()}
        </div>
        
        {/* MAIN CONTENT */}
        <div className="flex-grow min-w-0 flex flex-col justify-between gap-2">
            
            {/* Header Row */}
            <div className="flex justify-between items-start gap-4">
                 <div className="min-w-0">
                    <p className={twMerge(
                        "text-sm font-semibold truncate mb-1",
                        isCancelled ? "text-zinc-500 line-through decoration-zinc-700" : "text-zinc-200"
                    )} title={displayTitle}>
                        {displayTitle}
                    </p>
                    
                    <div className="flex flex-wrap items-center gap-2">
                        {/* Format Badge */}
                        <span className={twMerge(
                            "text-[10px] font-bold px-1.5 py-0.5 rounded border uppercase",
                            isAudio ? "text-red-400 bg-red-400/10 border-red-400/20" : "text-cyan-400 bg-cyan-400/10 border-cyan-400/20",
                            isCancelled && "text-zinc-500 bg-zinc-800 border-zinc-700"
                        )}>
                            {badgeText}
                        </span>

                        {/* Status Badge */}
                        <span className={twMerge(
                            "text-[10px] font-bold px-1.5 py-0.5 rounded border uppercase flex items-center gap-1.5",
                            getStatusColor()
                        )}>
                            {isActive && <Activity className={twMerge("h-3 w-3", (isProcessingPhase || isMetaPhase) && "animate-spin")} />}
                            {phase || (isQueued ? "Waiting" : status)}
                        </span>

                        {/* Extra Flags */}
                        {(embedMetadata || embedThumbnail) && !isCancelled && (
                            <div className="flex gap-1">
                                {embedMetadata && (
                                    <span title="Metadata">
                                        <FileText className="h-3 w-3 text-zinc-500" />
                                    </span>
                                )}
                                {embedThumbnail && (
                                    <span title="Thumbnail">
                                        <ImageIcon className="h-3 w-3 text-zinc-500" />
                                    </span>
                                )}
                            </div>
                        )}
                    </div>
                 </div>

                 {/* Percentage / Status Right Side */}
                 <div className="text-right flex-shrink-0">
                    {isActive ? (
                         <span className="text-xl font-black text-zinc-100 tabular-nums tracking-tight">
                            {progress.toFixed(0)}<span className="text-xs font-medium text-zinc-600 ml-0.5">%</span>
                         </span>
                    ) : (
                        <div className="h-6" /> // spacer
                    )}
                 </div>
            </div>
            
            {/* Progress Bar Row */}
            <div className="w-full">
                {isActive && (
                     <div className={twMerge("relative", (isProcessingPhase || isMetaPhase) && "opacity-80")}>
                        <Progress 
                            value={progress} 
                            variant={isError ? 'error' : isCompleted ? 'success' : 'default'} 
                            className="h-1.5"
                        />
                        {(isProcessingPhase || isMetaPhase) && (
                            <div className="absolute inset-0 bg-amber-400/30 animate-pulse rounded-full" />
                        )}
                     </div>
                )}
                
                {isQueued && (
                    <div className="w-full h-1.5 bg-zinc-900 rounded-full overflow-hidden relative border border-zinc-800/50">
                        <div className="absolute inset-0 w-full h-full bg-[linear-gradient(45deg,transparent_25%,rgba(255,255,255,0.05)_25%,rgba(255,255,255,0.05)_50%,transparent_50%,transparent_75%,rgba(255,255,255,0.05)_75%,rgba(255,255,255,0.05)_100%)] bg-[length:16px_16px] animate-[progress-stripes_1s_linear_infinite]" />
                    </div>
                )}

                {/* Active Stats Footer */}
                {isActive && !isProcessingPhase && !isMetaPhase && (
                    <div className="flex items-center justify-between text-[10px] text-zinc-500 font-mono mt-1.5 px-0.5">
                        <span title="Download Speed">{formatStat(speed)}</span>
                        <span title="ETA" className="flex items-center gap-1">
                            <Clock className="h-3 w-3" /> {formatStat(eta)}
                        </span>
                    </div>
                )}
            </div>
            
            {/* Error Details Section */}
            {isError && (
                <div className="mt-2 animate-fade-in">
                    <SmartError error={error} stderr={stderr} />
                    
                    <div className="flex items-center gap-2 mt-2">
                         <button 
                            onClick={() => setShowLogs(!showLogs)}
                            className="flex items-center text-[10px] text-zinc-400 hover:text-zinc-200 transition-colors"
                        >
                            {showLogs ? <ChevronUp className="h-3 w-3 mr-1" /> : <ChevronDown className="h-3 w-3 mr-1" />}
                            {showLogs ? 'Hide Raw Logs' : 'View Raw Logs'}
                        </button>
                        <div className="h-3 w-px bg-zinc-800" />
                        <button 
                             onClick={() => openLogFolder()}
                             className="text-[10px] text-zinc-400 hover:text-zinc-200 transition-colors"
                        >
                            Open Log Folder
                        </button>
                    </div>

                    {showLogs && logs && (
                         <div className="relative mt-2 p-3 bg-zinc-950 border border-zinc-800 rounded font-mono text-[10px] text-zinc-400 h-32 overflow-y-auto custom-scrollbar">
                            <pre className="whitespace-pre-wrap break-all">{logs}</pre>
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
        </div>

        {/* ACTIONS COLUMN */}
        <div className="flex flex-col justify-start gap-2 pt-1 pl-2 border-l border-zinc-800/50">
          {(isActive || isQueued || isError || isCancelled) && (
             <Button 
                variant="ghost" 
                size="icon" 
                onClick={() => onCancel(jobId)} 
                className={twMerge(
                    "h-8 w-8 transition-all duration-200",
                    isError || isCancelled
                        ? "text-zinc-500 hover:bg-red-500/10 hover:text-red-400"
                        : "text-zinc-400 hover:bg-zinc-800 hover:text-white"
                )}
                title={isError || isCancelled ? "Dismiss" : "Cancel"}
             >
                {(isError || isCancelled) ? <Trash2 className="h-4 w-4" /> : <X className="h-4 w-4" />}
              </Button>
          )}

          {isCompleted && outputPath && (
              <Button
                variant="ghost"
                size="icon"
                onClick={handleOpenFolder}
                className="h-8 w-8 text-emerald-500 hover:bg-emerald-500/10"
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