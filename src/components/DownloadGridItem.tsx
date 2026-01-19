import { Download } from '@/types';
import { X, CheckCircle2, AlertCircle, Hourglass, MonitorPlay, Headphones, Tags, FileOutput, Image as ImageIcon, Activity, FolderSearch, Trash2 } from 'lucide-react';
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

  const isProcessingPhase = phase?.includes('Merging') 
    || phase?.includes('Extracting') 
    || phase?.includes('Fixing')
    || phase?.includes('Starting')
    || phase?.includes('Initializing')
    || phase?.includes('Moving')
    || phase?.includes('Finalizing')
    || phase?.includes('Processing');

  const isMetaPhase = phase?.includes('Metadata') || phase?.includes('Thumbnail');

  // Friendly error summary for tooltip
  let friendlyError = error;
  if (isError) {
      const parsed = parseError(stderr, error);
      friendlyError = `${parsed.title}: ${parsed.description}`;
  }

  // Determine base color based on type/status
  const getThemeColorClass = () => {
      if (isError) return "text-theme-red border-theme-red shadow-glow-red";
      if (isCompleted) return "text-emerald-500 border-emerald-500";
      if (isProcessingPhase || isMetaPhase) return "text-yellow-500 border-yellow-500";
      if (isAudio) return "text-theme-red border-theme-red shadow-glow-red";
      return "text-theme-cyan border-theme-cyan shadow-glow-cyan"; 
  };
  
  const getBgFillColor = () => {
      if (isError) return "bg-theme-red";
      if (isCompleted) return "bg-emerald-500";
      if (isProcessingPhase || isMetaPhase) return "bg-yellow-500";
      if (isAudio) return "bg-theme-red";
      return "bg-theme-cyan";
  };

  const IconComponent = () => {
    if (isError) return <AlertCircle className="h-6 w-6" />;
    if (isCompleted) return <CheckCircle2 className="h-6 w-6" />;
    if (isCancelled) return <X className="h-6 w-6" />;
    if (isQueued) return <Hourglass className="h-6 w-6 animate-pulse" />;
    
    if (isMetaPhase) return <Tags className="h-6 w-6 animate-pulse" />;
    if (isProcessingPhase) return <FileOutput className="h-6 w-6 animate-pulse" />;
    if (embedThumbnail && phase?.includes('Thumbnail')) return <ImageIcon className="h-6 w-6 animate-pulse" />;

    return isAudio ? <Headphones className="h-6 w-6" /> : <MonitorPlay className="h-6 w-6" />;
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
            "group relative h-28 w-full rounded-xl border bg-zinc-900/40 overflow-hidden transition-all duration-300 select-none flex items-center justify-center",
            isActive ? getThemeColorClass() : "border-zinc-800 text-zinc-600",
            (isQueued || isCancelled) && "opacity-60 bg-zinc-900/20 border-zinc-800/60"
        )}
    >
        {/* Progress Fill Layer */}
        {isActive && (
            <div 
                className={twMerge("absolute bottom-0 left-0 right-0 transition-all duration-300 opacity-20", getBgFillColor())}
                style={{ height: `${progress}%` }}
            />
        )}
        
        {(isQueued || isProcessingPhase) && (
            <div className="absolute inset-0 w-full h-full bg-[linear-gradient(45deg,transparent_25%,rgba(255,255,255,0.03)_25%,rgba(255,255,255,0.03)_50%,transparent_50%,transparent_75%,rgba(255,255,255,0.03)_75%,rgba(255,255,255,0.03)_100%)] bg-[length:20px_20px] animate-[progress-stripes_2s_linear_infinite] pointer-events-none" />
        )}

        {/* Content Layer */}
        <div className="z-10 relative flex flex-col items-center justify-center group-hover:opacity-0 transition-opacity duration-200">
            {isActive && !isProcessingPhase && !isMetaPhase ? (
                <div className="flex flex-col items-center animate-fade-in">
                    <span className="text-xl font-black tracking-tighter tabular-nums">
                        {progress.toFixed(0)}<span className="text-xs font-normal opacity-70">%</span>
                    </span>
                    <Activity className="h-3 w-3 mt-1 animate-pulse opacity-50" />
                </div>
            ) : (
                <div className={twMerge("transition-transform duration-300", isActive && "animate-pulse")}>
                    <IconComponent />
                </div>
            )}
        </div>

        {/* HOVER OVERLAY */}
        <div className="absolute inset-0 bg-zinc-950/90 backdrop-blur-sm opacity-0 group-hover:opacity-100 transition-all duration-200 z-20 flex flex-col p-3 text-left">
            
            {/* Top Right: Actions */}
            <div className="absolute top-2 right-2 flex gap-2 z-30">
                {isCompleted && outputPath && (
                    <button
                        onClick={(e) => { e.stopPropagation(); showInFolder(outputPath); }}
                        className="p-1.5 rounded-full bg-zinc-800 hover:bg-theme-cyan hover:text-black text-zinc-400 transition-colors shadow-lg"
                        title="Open File Location"
                    >
                        <FolderSearch className="h-3 w-3" />
                    </button>
                )}

                {/* Cancel or Remove Button */}
                {(isActive || isQueued || isError || isCancelled) && (
                    <button 
                        onClick={(e) => { e.stopPropagation(); onCancel(jobId); }}
                        className={twMerge(
                            "p-1.5 rounded-full bg-zinc-800 transition-colors shadow-lg",
                            isError || isCancelled 
                                ? "hover:bg-zinc-700 hover:text-red-400 text-zinc-400" 
                                : "hover:bg-theme-red hover:text-white text-zinc-400"
                        )}
                        title={isError || isCancelled ? "Remove from Grid" : "Cancel Download"}
                    >
                        {(isError || isCancelled) ? <Trash2 className="h-3 w-3" /> : <X className="h-3 w-3" />}
                    </button>
                )}
            </div>

            {/* Title */}
            <div className="text-[10px] font-bold text-zinc-100 leading-tight line-clamp-2 pr-12 mb-auto break-all">
                {displayTitle}
            </div>

            {/* Error Message specific display */}
            {isError ? (
                 <div className="text-[9px] text-red-400 font-mono leading-tight mt-1 line-clamp-3">
                    {friendlyError}
                 </div>
            ) : (
                <>
                    {/* Badges */}
                    <div className="flex flex-wrap gap-1 mt-2 mb-1">
                        <span className={twMerge(
                            "px-1 py-0.5 text-[9px] font-bold rounded uppercase",
                            isCancelled ? "bg-zinc-800 text-zinc-600" : (isAudio ? "bg-theme-red/20 text-theme-red" : "bg-theme-cyan/20 text-theme-cyan")
                        )}>
                            {isCancelled ? 'CANCELLED' : badgeText}
                        </span>
                    </div>
                    
                    <div className={twMerge(
                        "text-[9px] font-mono truncate",
                        isCancelled ? "text-zinc-600 line-through" : (isProcessingPhase || isMetaPhase) ? "text-yellow-500" : "text-zinc-500"
                    )}>
                        {phase || status}
                    </div>
                </>
            )}
        </div>
    </div>
  );
}