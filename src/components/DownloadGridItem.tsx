import { Download } from '@/types';
import { X, CheckCircle2, AlertTriangle, Hourglass, MonitorPlay, Headphones, Tags, FileOutput, Image as ImageIcon, Activity, FolderOpen, Trash2, FileWarning, RefreshCw } from 'lucide-react';
import { twMerge } from 'tailwind-merge';
import { showInFolder } from '@/api/invoke';
import { parseError } from '@/utils/errorRegistry';
import { useDownloadManager } from '@/hooks/useDownloadManager';

interface DownloadGridItemProps {
  download: Download;
  onCancel: (jobId: string) => void;
}

function middleTruncate(str: string, maxLength: number) {
    if (str.length <= maxLength) return str;
    const partLen = Math.floor((maxLength - 3) / 2);
    return str.substring(0, partLen) + '...' + str.substring(str.length - partLen);
}

export function DownloadGridItem({ download, onCancel }: DownloadGridItemProps) {
  const { resolveConflict } = useDownloadManager();
  const { jobId, status, progress, error, phase, preset, embedThumbnail, filename, url, outputPath, stderr } = download;

  const isAudio = preset?.startsWith('audio');
  const rawTitle = filename || url;
  const displayTitle = middleTruncate(rawTitle, 40);
  
  // State Flags
  const isQueued = status === 'pending';
  const isActive = status === 'downloading'; 
  const isError = status === 'error';
  const isCompleted = status === 'completed';
  const isCancelled = status === 'cancelled';
  const isConflict = status === 'file_conflict';

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
      if (isConflict) return "border-amber-500/60 bg-amber-950/20 shadow-[0_0_15px_-5px_rgba(245,158,11,0.3)] ring-1 ring-amber-500/30";
      if (isCompleted) return "border-emerald-500/50 bg-emerald-950/20 shadow-[0_0_15px_-5px_rgba(16,185,129,0.3)]";
      if (isProcessingPhase || isMetaPhase) return "border-amber-500/50 bg-amber-950/20 shadow-[0_0_15px_-5px_rgba(245,158,11,0.3)]";
      if (isActive) return "border-theme-cyan/50 bg-zinc-900 shadow-[0_0_15px_-5px_rgba(6,182,212,0.3)]";
      if (isCancelled) return "border-zinc-800 bg-zinc-950 opacity-60";
      return "border-zinc-800 bg-zinc-900"; 
  };

  const IconComponent = () => {
    if (isError) return <AlertTriangle className="h-7 w-7 text-red-500 drop-shadow-lg" />;
    if (isConflict) return <FileWarning className="h-8 w-8 text-amber-500 drop-shadow-lg animate-pulse" />;
    if (isCompleted) return <CheckCircle2 className="h-7 w-7 text-emerald-500 drop-shadow-lg" />;
    if (isCancelled) return <X className="h-7 w-7 text-zinc-600" />;
    if (isQueued) return <Hourglass className="h-7 w-7 text-zinc-500 animate-pulse" />;
    
    if (isMetaPhase) return <Tags className="h-7 w-7 text-amber-400 animate-pulse" />;
    if (isProcessingPhase) return <FileOutput className="h-7 w-7 text-amber-400 animate-pulse" />;
    if (embedThumbnail && phase?.includes('Thumbnail')) return <ImageIcon className="h-7 w-7 text-amber-400 animate-pulse" />;

    return isAudio 
        ? <Headphones className="h-7 w-7 text-theme-red" /> 
        : <MonitorPlay className="h-7 w-7 text-theme-cyan" />;
  };

  let badgeText = isAudio ? 'AUDIO' : 'VIDEO';
  if (preset) {
      const parts = preset.split('_');
      if (parts.length > 1 && parts[1] !== 'best') {
         badgeText = parts[1].toUpperCase();
      }
  }

  const handleCardClick = (e: React.MouseEvent) => {
      if (isCompleted && outputPath) {
          e.stopPropagation();
          showInFolder(outputPath);
      }
  };

  return (
    <div 
        onClick={handleCardClick}
        className={twMerge(
            "group relative aspect-square w-full min-h-[160px] rounded-xl border-2 overflow-hidden transition-all duration-300 select-none flex flex-col",
            getContainerStyles(),
            isCompleted && outputPath ? "cursor-pointer hover:border-emerald-500/80 hover:shadow-emerald-500/20" : ""
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
                <div className="w-full h-[1px] bg-theme-cyan/50 shadow-[0_0_10px_rgba(6,182,212,0.8)]" />
            </div>
        )}

        {/* --- TITLE HEADER --- */}
        <div className="relative z-10 w-full p-2.5 bg-black/40 backdrop-blur-md border-b border-white/5 h-[52px] flex items-center">
            <div className="text-[11px] font-black leading-tight text-zinc-100 line-clamp-2 text-center w-full" title={rawTitle}>
                {displayTitle}
            </div>
        </div>

        {/* --- MAIN CONTENT CENTER --- */}
        <div className="relative z-10 flex-1 flex flex-col items-center justify-center p-3 group-hover:scale-105 transition-transform duration-300">
            {isActive && !isProcessingPhase && !isMetaPhase ? (
                <div className="flex flex-col items-center animate-fade-in">
                    <span className="text-2xl font-black tracking-tighter text-zinc-100 tabular-nums">
                        {progress.toFixed(0)}<span className="text-[10px] font-medium text-zinc-500 align-top ml-0.5">%</span>
                    </span>
                    <div className="mt-1 flex items-center gap-1 px-1.5 py-0.5 rounded-full bg-theme-cyan/10 border border-theme-cyan/20">
                         <Activity className="h-2.5 w-2.5 text-theme-cyan animate-pulse" />
                         <span className="text-[9px] font-black tracking-widest text-theme-cyan">DL</span>
                    </div>
                </div>
            ) : (
                <div className={twMerge("transition-transform duration-300", isActive && "animate-pulse")}>
                    <IconComponent />
                </div>
            )}
            
            {/* Phase Text */}
            {(isActive || isQueued || isConflict) && (
                <div className={twMerge(
                    "mt-2 text-[8px] font-black uppercase tracking-[0.2em]",
                    isConflict ? "text-amber-400" : "text-zinc-500"
                )}>
                    {isConflict ? 'CONFLICT' : (phase || (isQueued ? 'Queued' : 'Init'))}
                </div>
            )}
        </div>

        {/* --- HOVER OVERLAY ACTIONS --- */}
        <div className={twMerge(
            "absolute inset-0 z-20 transition-all duration-300 flex flex-col p-3",
            // Special styling for Completed State: Full overlay acts as button
            isCompleted ? "bg-black/60 backdrop-blur-sm opacity-0 group-hover:opacity-100 items-center justify-center" : "bg-zinc-950/95 backdrop-blur-[2px] opacity-0 group-hover:opacity-100",
            isConflict ? "opacity-100" : "" 
        )}>
            {/* COMPLETED STATE: Big Folder Icon */}
            {isCompleted && outputPath ? (
                <>
                    <div className="absolute top-3 left-3 flex gap-1">
                        <span className={twMerge(
                            "text-[9px] font-bold px-1.5 py-0.5 rounded border uppercase shadow-lg",
                            isAudio ? "bg-red-500/20 text-red-300 border-red-500/30" : "bg-cyan-500/20 text-cyan-300 border-cyan-500/30"
                        )}>
                            {badgeText}
                        </span>
                    </div>
                    <FolderOpen className="h-16 w-16 text-emerald-400 drop-shadow-[0_0_15px_rgba(52,211,153,0.5)] transform group-hover:scale-110 transition-transform duration-300" strokeWidth={1.5} />
                </>
            ) : (
                // STANDARD OVERLAY FOR OTHER STATES
                <>
                    <div className="flex gap-1 mb-auto">
                        <span className={twMerge(
                            "text-[9px] font-bold px-1.5 py-0.5 rounded border uppercase",
                            isAudio ? "bg-red-500/10 text-red-400 border-red-500/20" : "bg-cyan-500/10 text-cyan-400 border-cyan-500/20"
                        )}>
                            {badgeText}
                        </span>
                    </div>

                    {isError && (
                        <div className="mb-auto mt-2 text-[10px] text-red-300 leading-tight font-mono break-words line-clamp-4 bg-red-950/30 p-2 rounded border border-red-900/50">
                            {friendlyError}
                        </div>
                    )}

                    {isConflict && (
                        <div className="mb-auto mt-2 text-[10px] text-amber-200 leading-tight font-bold text-center">
                            File already exists.
                            <br/>Overwrite?
                        </div>
                    )}

                    <div className="mt-auto grid grid-cols-1 gap-2">
                        {isConflict && (
                             <div className="flex gap-2">
                                <button
                                     onClick={(e) => { e.stopPropagation(); resolveConflict(jobId, 'overwrite'); }}
                                     className="flex-1 flex items-center justify-center gap-1 py-1.5 rounded bg-amber-500/20 hover:bg-amber-500 text-amber-500 hover:text-black text-[9px] font-black transition-all border border-amber-500/30"
                                >
                                    <RefreshCw className="h-3 w-3" /> REPLACE
                                </button>
                                 <button
                                     onClick={(e) => { e.stopPropagation(); resolveConflict(jobId, 'discard'); }}
                                     className="flex-1 flex items-center justify-center gap-1 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-400 hover:text-white text-[9px] font-black transition-all border border-zinc-700"
                                >
                                    <Trash2 className="h-3 w-3" /> NO
                                </button>
                             </div>
                        )}

                        {(isActive || isQueued || isError || isCancelled) && !isConflict && (
                             <button
                                onClick={(e) => { e.stopPropagation(); onCancel(jobId); }}
                                className={twMerge(
                                    "flex items-center justify-center gap-2 w-full py-1.5 rounded text-[10px] font-black transition-all border",
                                    isError 
                                        ? "bg-red-600/10 hover:bg-red-600 text-red-500 hover:text-white border-red-500/20"
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
                </>
            )}
        </div>
    </div>
  );
}