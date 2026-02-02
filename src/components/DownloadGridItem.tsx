import { Download } from '@/types';
import { X, CheckCircle2, AlertTriangle, Hourglass, MonitorPlay, Headphones, Tags, FileOutput, Image as ImageIcon, Activity, FolderSearch, Trash2 } from 'lucide-react';
import { twMerge } from 'tailwind-merge';
import { showInFolder } from '@/api/invoke';
import { parseError } from '@/utils/errorRegistry';

interface DownloadGridItemProps {
  download: Download;
  onCancel: (jobId: string) => void;
}

export function DownloadGridItem({ download, onCancel }: DownloadGridItemProps) {
  const { jobId, status, progress, error, phase, preset, embedThumbnail, filename, url, outputPath, stderr } = download;

  const isAudio = preset?.startsWith('audio');
  const displayTitle = filename || url;
  
  // State Flags
  const isQueued = status === 'pending';
  const isActive = status === 'downloading'; 
  const isError = status === 'error';
  const isCompleted = status === 'completed';
  const isCancelled = status === 'cancelled';

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

  // Parse error for tooltip
  let friendlyError = error;
  if (isError) {
      const parsed = parseError(stderr, error);
      friendlyError = `${parsed.title}: ${parsed.description}`;
  }

  // --- Dynamic Styles ---

  const getContainerStyles = () => {
      if (isError) return "border-red-500/50 bg-red-950/20 shadow-[0_0_15px_-5px_rgba(239,68,68,0.3)]";
      if (isCompleted) return "border-emerald-500/50 bg-emerald-950/20 shadow-[0_0_15px_-5px_rgba(16,185,129,0.3)]";
      if (isProcessingPhase || isMetaPhase) return "border-amber-500/50 bg-amber-950/20 shadow-[0_0_15px_-5px_rgba(245,158,11,0.3)]";
      if (isActive) return "border-theme-cyan/50 bg-zinc-900 shadow-[0_0_15px_-5px_rgba(6,182,212,0.3)]";
      if (isCancelled) return "border-zinc-800 bg-zinc-950 opacity-60";
      return "border-zinc-800 bg-zinc-900"; // Queued
  };

  const IconComponent = () => {
    if (isError) return <AlertTriangle className="h-8 w-8 text-red-500 drop-shadow-lg" />;
    if (isCompleted) return <CheckCircle2 className="h-8 w-8 text-emerald-500 drop-shadow-lg" />;
    if (isCancelled) return <X className="h-8 w-8 text-zinc-600" />;
    if (isQueued) return <Hourglass className="h-8 w-8 text-zinc-500 animate-pulse" />;
    
    if (isMetaPhase) return <Tags className="h-8 w-8 text-amber-400 animate-pulse" />;
    if (isProcessingPhase) return <FileOutput className="h-8 w-8 text-amber-400 animate-pulse" />;
    if (embedThumbnail && phase?.includes('Thumbnail')) return <ImageIcon className="h-8 w-8 text-amber-400 animate-pulse" />;

    return isAudio 
        ? <Headphones className="h-8 w-8 text-theme-red" /> 
        : <MonitorPlay className="h-8 w-8 text-theme-cyan" />;
  };

  let badgeText = isAudio ? 'AUDIO' : 'VIDEO';
  if (preset) {
      const parts = preset.split('_');
      if (parts.length > 1 && parts[1] !== 'best') {
         badgeText = parts[1].toUpperCase();
      }
  }

  return (
    <div 
        className={twMerge(
            "group relative aspect-square w-full min-h-[140px] rounded-xl border-2 overflow-hidden transition-all duration-300 select-none flex flex-col",
            getContainerStyles()
        )}
    >
        {/* ERROR STATE: Striped Background Pattern */}
        {isError && (
            <div className="absolute inset-0 opacity-10 bg-[repeating-linear-gradient(45deg,transparent,transparent_10px,#ef4444_10px,#ef4444_20px)]" />
        )}

        {/* PROGRESS FILL (Active Only) */}
        {isActive && !isProcessingPhase && !isMetaPhase && (
            <div 
                className="absolute bottom-0 left-0 right-0 bg-theme-cyan/10 transition-all duration-300 ease-out z-0"
                style={{ height: `${progress}%` }}
            >
                {/* Glow line at the top of progress */}
                <div className="w-full h-[1px] bg-theme-cyan/50 shadow-[0_0_10px_rgba(6,182,212,0.8)]" />
            </div>
        )}

        {/* PROCESSING STRIPES */}
        {(isQueued || isProcessingPhase || isMetaPhase) && (
            <div className="absolute inset-0 w-full h-full bg-[linear-gradient(45deg,transparent_25%,rgba(255,255,255,0.03)_25%,rgba(255,255,255,0.03)_50%,transparent_50%,transparent_75%,rgba(255,255,255,0.03)_75%,rgba(255,255,255,0.03)_100%)] bg-[length:40px_40px] animate-[progress-stripes_1s_linear_infinite] pointer-events-none opacity-50" />
        )}

        {/* --- MAIN CONTENT CENTER --- */}
        <div className="relative z-10 flex-1 flex flex-col items-center justify-center p-4 group-hover:scale-105 transition-transform duration-300">
            {isActive && !isProcessingPhase && !isMetaPhase ? (
                <div className="flex flex-col items-center animate-fade-in space-y-1">
                    <span className="text-3xl font-black tracking-tighter text-zinc-100 tabular-nums drop-shadow-md">
                        {progress.toFixed(0)}<span className="text-sm font-medium text-zinc-500 align-top ml-0.5">%</span>
                    </span>
                    <div className="flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-black/20 backdrop-blur-sm border border-white/5">
                        <Activity className="h-3 w-3 text-theme-cyan animate-pulse" />
                        <span className="text-[10px] font-mono text-theme-cyan/80">DOWNLOADING</span>
                    </div>
                </div>
            ) : (
                <div className={twMerge("transition-transform duration-300 p-3 rounded-full bg-black/20 backdrop-blur-sm border border-white/5", isActive && "animate-pulse")}>
                    <IconComponent />
                </div>
            )}
            
            {/* Status Text (Non-Active states) */}
            {!isActive && (
                <div className={twMerge(
                    "mt-3 text-[10px] font-bold uppercase tracking-wider px-2 py-1 rounded border backdrop-blur-sm",
                    isError ? "text-red-400 border-red-500/30 bg-red-950/40" :
                    isCompleted ? "text-emerald-400 border-emerald-500/30 bg-emerald-950/40" :
                    isCancelled ? "text-zinc-500 border-zinc-700 bg-zinc-900" :
                    "text-zinc-400 border-zinc-700 bg-zinc-800"
                )}>
                    {isError ? 'FAILED' : isCompleted ? 'DONE' : isCancelled ? 'CANCELLED' : 'QUEUED'}
                </div>
            )}
        </div>

        {/* --- TITLE FOOTER --- */}
        <div className="relative z-10 w-full p-2 bg-black/40 backdrop-blur-sm border-t border-white/5">
            <div className="text-[10px] font-medium text-zinc-300 truncate text-center px-1" title={displayTitle}>
                {displayTitle}
            </div>
        </div>

        {/* --- HOVER OVERLAY ACTIONS --- */}
        <div className="absolute inset-0 z-20 bg-zinc-950/90 backdrop-blur-[2px] opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex flex-col p-3">
            {/* Top Bar Badges */}
            <div className="flex gap-1 mb-auto">
                <span className={twMerge(
                    "text-[9px] font-bold px-1.5 py-0.5 rounded border uppercase",
                    isAudio ? "bg-red-500/10 text-red-400 border-red-500/20" : "bg-cyan-500/10 text-cyan-400 border-cyan-500/20"
                )}>
                    {badgeText}
                </span>
                {(isProcessingPhase || isMetaPhase) && (
                     <span className="text-[9px] font-bold px-1.5 py-0.5 rounded border uppercase bg-amber-500/10 text-amber-400 border-amber-500/20 animate-pulse">
                        PROCESSING
                    </span>
                )}
            </div>

            {/* Error Details Preview */}
            {isError && (
                <div className="mb-auto text-[10px] text-red-300 leading-tight font-mono break-words line-clamp-3 bg-red-950/30 p-1.5 rounded border border-red-900/50">
                    {friendlyError}
                </div>
            )}

            {/* Action Buttons Container */}
            <div className="mt-auto grid grid-cols-1 gap-2">
                {isCompleted && outputPath ? (
                    <button
                        onClick={(e) => { e.stopPropagation(); showInFolder(outputPath); }}
                        className="flex items-center justify-center gap-2 w-full py-1.5 rounded bg-zinc-800 hover:bg-emerald-600 hover:text-white text-zinc-300 text-[10px] font-bold transition-colors border border-zinc-700 hover:border-emerald-500"
                    >
                        <FolderSearch className="h-3.5 w-3.5" />
                        OPEN FOLDER
                    </button>
                ) : null}

                {(isActive || isQueued || isError || isCancelled) && (
                     <button
                        onClick={(e) => { e.stopPropagation(); onCancel(jobId); }}
                        className={twMerge(
                            "flex items-center justify-center gap-2 w-full py-1.5 rounded text-[10px] font-bold transition-colors border",
                            isError 
                                ? "bg-zinc-800 hover:bg-red-600 text-zinc-300 hover:text-white border-zinc-700 hover:border-red-500"
                                : "bg-zinc-800 hover:bg-zinc-700 text-zinc-400 hover:text-zinc-200 border-zinc-700"
                        )}
                    >
                        {isError ? (
                            <>
                                <Trash2 className="h-3.5 w-3.5" /> DISMISS
                            </>
                        ) : (
                            <>
                                <X className="h-3.5 w-3.5" /> CANCEL
                            </>
                        )}
                    </button>
                )}
            </div>
        </div>
    </div>
  );
}