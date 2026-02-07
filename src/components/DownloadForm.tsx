import React, { useState, useRef, useEffect } from 'react';
import { Button } from './ui/Button';
import { Card, CardContent } from './ui/Card';
import { Download, FolderOpen, Link2, MonitorPlay, Headphones, FileText, Image as ImageIcon, AlertTriangle, Loader2, ChevronDown, Radio } from 'lucide-react';
import { selectDirectory, expandPlaylist } from '@/api/invoke';
import { DownloadFormatPreset, PreferenceConfig, StartDownloadResponse, PlaylistEntry } from '@/types';
import { useAppContext } from '@/contexts/AppContext';
import { twMerge } from 'tailwind-merge';
import { SmartError } from './ui/SmartError';
import { extractErrorDetails } from '@/utils/errorRegistry';
import { PlaylistSelectionModal } from './PlaylistSelectionModal';

interface DownloadFormProps {
  onDownload: (
      url: string, 
      downloadPath: string | undefined, 
      formatPreset: DownloadFormatPreset, 
      videoResolution: string,
      embedMeta: boolean,
      embedThumbnail: boolean,
      filenameTemplate: string,
      restrictFilenames: boolean,
      forceDownload: boolean,
      urlWhitelist?: string[],
      liveFromStart?: boolean
    ) => Promise<StartDownloadResponse>; 
}

type DownloadMode = 'video' | 'audio';

const formatPresets: {
  label: string;
  value: DownloadFormatPreset;
  mode: DownloadMode;
}[] = [
  { label: 'Best Quality', value: 'best', mode: 'video' },
  { label: 'Best MP4', value: 'best_mp4', mode: 'video' },
  { label: 'Best MKV', value: 'best_mkv', mode: 'video' },
  { label: 'Best WebM', value: 'best_webm', mode: 'video' },
  { label: 'Best Audio', value: 'audio_best', mode: 'audio' },
  { label: 'MP3 Audio', value: 'audio_mp3', mode: 'audio' },
  { label: 'FLAC (Lossless)', value: 'audio_flac', mode: 'audio' },
  { label: 'M4A Audio', value: 'audio_m4a', mode: 'audio' },
];

const resolutionOptions = [
    { label: 'Best Available', value: 'best' },
    { label: '4K (2160p)', value: '2160p' },
    { label: '2K (1440p)', value: '1440p' },
    { label: 'Full HD (1080p)', value: '1080p' },
    { label: 'HD (720p)', value: '720p' },
    { label: 'SD (480p)', value: '480p' },
    { label: 'Low (360p)', value: '360p' },
    { label: 'Lowest (240p)', value: '240p' },
];

interface ModeButtonProps {
    mode: DownloadMode;
    currentMode: DownloadMode;
    icon: React.ElementType;
    label: string;
    onClick: (mode: DownloadMode) => void;
    accentColor: string;
}

const ModeButton: React.FC<ModeButtonProps> = ({ mode, currentMode, icon: Icon, label, onClick }) => {
    const isActive = mode === currentMode;
    const activeClass = mode === 'video' 
        ? 'bg-theme-cyan/10 text-theme-cyan border-theme-cyan/50 shadow-glow-cyan' 
        : 'bg-theme-red/10 text-theme-red border-theme-red/50 shadow-glow-red';

    return (
        <button
            type="button"
            onClick={() => onClick(mode)}
            className={twMerge(
                'flex flex-1 items-center justify-center gap-2 py-2.5 text-xs uppercase tracking-wider font-bold rounded-md transition-all border',
                isActive
                    ? activeClass
                    : 'bg-zinc-900/50 border-zinc-800 text-zinc-500 hover:text-zinc-300 hover:border-zinc-700'
            )}
        >
            <Icon className="h-4 w-4" />
            {label}
        </button>
    );
};

export function DownloadForm({ onDownload }: DownloadFormProps) {
  const { 
    getTemplateString, 
    isJsRuntimeMissing, 
    preferences, 
    updatePreferences, 
    defaultDownloadPath, 
    setDefaultDownloadPath,
    setSkipNotice
  } = useAppContext();
  
  const [url, setUrl] = useState('');
  const [isProcessing, setIsProcessing] = useState(false);
  const [showForceOptions, setShowForceOptions] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const [errorDetails, setErrorDetails] = useState<{ message: string, stderr?: string } | null>(null);

  // Playlist state
  const [playlistEntries, setPlaylistEntries] = useState<PlaylistEntry[]>([]);
  const [isPlaylistModalOpen, setIsPlaylistModalOpen] = useState(false);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setShowForceOptions(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [dropdownRef]);

  /**
   * Finalizes the download process by invoking the manager
   */
  const triggerDownload = async (targetUrl: string, force: boolean = false, whitelist?: string[]) => {
      setIsProcessing(true);
      try {
          const template = getTemplateString();
          const response = await onDownload(
              targetUrl, 
              defaultDownloadPath || undefined, 
              preferences.format_preset as DownloadFormatPreset,
              preferences.video_resolution,
              preferences.embed_metadata, 
              preferences.embed_thumbnail, 
              template,
              false, 
              force, 
              whitelist, 
              preferences.live_from_start 
          );

          if (response.skipped_count > 0) {
              setSkipNotice({
                  skipped: response.skipped_count,
                  total: response.total_found,
                  url: targetUrl,
                  skippedUrls: response.skipped_urls
              });
          }

          if (response.job_ids.length > 0 || response.skipped_count === response.total_found) {
              setUrl('');
          }
      } catch (err: any) {
          console.error("Failed to start download", err);
          const extracted = extractErrorDetails(err);
          setErrorDetails(extracted);
      } finally {
          setIsProcessing(false);
      }
  };

  /**
   * Primary form submission handler
   */
  const handleSubmit = async (e: React.FormEvent | React.MouseEvent, force: boolean = false) => {
    if (e) e.preventDefault();
    if (!url.trim()) return;

    const isYoutube = url.includes('youtube.com') || url.includes('youtu.be');
    if (isYoutube && isJsRuntimeMissing) {
        const confirmed = window.confirm(
            "JavaScript Runtime Missing\n\nYouTube downloads rely heavily on a JS runtime for extraction. Proceed anyway?"
        );
        if (!confirmed) return;
    }

    const currentUrl = url;

    setIsProcessing(true);
    setErrorDetails(null);
    setSkipNotice(null);
    setShowForceOptions(false);

    try {
        // If selection modal is enabled, probe first
        if (preferences.enable_playlist_selection && !force) {
            const result = await expandPlaylist(currentUrl);
            if (result.entries.length > 1) {
                setPlaylistEntries(result.entries);
                setIsPlaylistModalOpen(true);
                // Note: We DO NOT set isProcessing to false here. 
                // We want the button behind the modal to stay in the "Analyzing" state.
                return;
            }
        }

        // Standard flow for single video or disabled selection
        await triggerDownload(currentUrl, force);
    } catch (err: any) {
        console.error("Failed to expand playlist", err);
        const extracted = extractErrorDetails(err);
        setErrorDetails(extracted);
        setIsProcessing(false);
    }
  };

  /**
   * Handler for when user confirms selection in the playlist modal
   */
  const handlePlaylistConfirm = (selectedUrls: string[]) => {
      setIsPlaylistModalOpen(false);
      triggerDownload(url, false, selectedUrls);
  };

  /**
   * Handler for when the user closes the playlist modal without selecting
   */
  const handlePlaylistCancel = () => {
      setIsPlaylistModalOpen(false);
      setIsProcessing(false);
  };

  const handleSelectDirectory = async () => {
    try {
        const selected = await selectDirectory();
        if (selected) {
            setDefaultDownloadPath(selected);
        }
    } catch (err) {
        console.error("Failed to select directory", err);
    }
  };
  
  const handleModeChange = (newMode: DownloadMode) => {
    let targetPreset = '';
    
    if (newMode === 'video') {
        targetPreset = preferences.video_preset || 'best';
    } else {
        targetPreset = preferences.audio_preset || 'audio_best';
    }

    updatePreferences({ mode: newMode, format_preset: targetPreset });
  };

  const handlePresetChange = (newValue: string) => {
      const updates: Partial<PreferenceConfig> = { format_preset: newValue };
      
      if (currentMode === 'video') {
          updates.video_preset = newValue;
      } else {
          updates.audio_preset = newValue;
      }
      
      updatePreferences(updates);
  };

  const isValidUrl = url.startsWith('http://') || url.startsWith('https://');
  const isYoutube = url.includes('youtube.com') || url.includes('youtu.be');
  const currentMode = preferences.mode as DownloadMode;
  
  const filteredPresets = formatPresets.filter(p => p.mode === currentMode);
  // Button is disabled if URL is invalid OR we are currently processing (probing or downloading)
  const isSubmitDisabled = !isValidUrl || isProcessing;

  return (
    <Card className="bg-transparent border-0 shadow-none p-0">
      <CardContent className="p-0">
        <PlaylistSelectionModal 
            isOpen={isPlaylistModalOpen}
            onClose={handlePlaylistCancel}
            entries={playlistEntries}
            onConfirm={handlePlaylistConfirm}
            title="Configure Playlist Items"
        />

        <form onSubmit={(e) => handleSubmit(e, false)} className="flex flex-col gap-6">
          
          {/* URL Input */}
          <div className="space-y-2">
            <div className="flex justify-between">
                <label className="text-[11px] uppercase tracking-wider font-bold text-zinc-500 ml-1">Target URL</label>
                {isYoutube && isJsRuntimeMissing && (
                     <span className="flex items-center gap-1 text-[10px] font-bold text-amber-500 bg-amber-500/10 px-2 py-0.5 rounded animate-pulse">
                        <AlertTriangle className="h-3 w-3" />
                        JS RUNTIME NEEDED
                     </span>
                )}
            </div>
            <div className="relative group">
                <div className="absolute inset-0 bg-theme-cyan/20 blur-md rounded-lg opacity-0 group-focus-within:opacity-100 transition-opacity duration-500"></div>
                <Link2 className="absolute left-3 top-3 h-4 w-4 text-zinc-500 group-focus-within:text-theme-cyan transition-colors" />
                <input
                    type="text"
                    value={url}
                    onChange={(e) => { 
                        setUrl(e.target.value); 
                        setErrorDetails(null); 
                    }}
                    disabled={isProcessing}
                    placeholder="https://youtube.com/watch?v=... or Playlist URL"
                    className={twMerge(
                        "relative w-full bg-surfaceHighlight border rounded-md pl-10 pr-4 py-2.5 text-sm text-zinc-100 placeholder-zinc-700 focus:outline-none focus:ring-1 transition-all",
                        errorDetails
                            ? "border-theme-red focus:ring-theme-red focus:border-theme-red"
                            : isYoutube && isJsRuntimeMissing 
                                ? "border-amber-500/50 focus:border-amber-500 focus:ring-amber-500" 
                                : "border-border focus:ring-theme-cyan focus:border-theme-cyan"
                    )}
                />
            </div>
            
            {errorDetails && (
                <div className="animate-fade-in">
                    <SmartError 
                        error={errorDetails.message} 
                        stderr={errorDetails.stderr} 
                    />
                </div>
            )}
          </div>
          
          <div className="grid grid-cols-1 gap-5">
              
              {/* Mode & Format */}
              <div className="space-y-2">
                 <label className="text-[11px] uppercase tracking-wider font-bold text-zinc-500 ml-1">Configuration</label>
                 <div className="flex gap-2 mb-3">
                    <ModeButton 
                        mode="video" 
                        currentMode={currentMode} 
                        onClick={handleModeChange} 
                        icon={MonitorPlay} 
                        label="Video" 
                        accentColor="cyan"
                    />
                    <ModeButton 
                        mode="audio" 
                        currentMode={currentMode} 
                        onClick={handleModeChange} 
                        icon={Headphones} 
                        label="Audio" 
                        accentColor="red"
                    />
                 </div>
                 
                 <div className="space-y-3">
                     <div className="flex gap-3">
                         <div className="relative flex-1">
                            <select
                                value={preferences.format_preset}
                                onChange={(e) => handlePresetChange(e.target.value)}
                                className="w-full appearance-none bg-surfaceHighlight border border-border rounded-md pl-3 pr-10 py-2.5 text-sm text-zinc-300 focus:outline-none focus:ring-1 focus:ring-theme-cyan/50 focus:border-theme-cyan/50"
                            >
                                {filteredPresets.map(p => (
                                    <option key={p.value} value={p.value}>
                                        {p.label}
                                    </option>
                                ))}
                            </select>
                            <ChevronDown className="absolute right-3 top-1/2 -translate-y-1/2 h-4 w-4 text-zinc-500 pointer-events-none" />
                         </div>

                         {currentMode === 'video' && (
                             <div className="relative flex-1">
                                <select
                                    value={preferences.video_resolution}
                                    onChange={(e) => updatePreferences({ video_resolution: e.target.value })}
                                    className="w-full appearance-none bg-surfaceHighlight border border-border rounded-md pl-3 pr-10 py-2.5 text-sm text-zinc-300 focus:outline-none focus:ring-1 focus:ring-theme-cyan/50 focus:border-theme-cyan/50"
                                >
                                    {resolutionOptions.map(r => (
                                        <option key={r.value} value={r.value}>
                                            {r.label}
                                        </option>
                                    ))}
                                </select>
                                <ChevronDown className="absolute right-3 top-1/2 -translate-y-1/2 h-4 w-4 text-zinc-500 pointer-events-none" />
                             </div>
                         )}
                     </div>

                     <div className="flex gap-2">
                         <button
                            type="button"
                            onClick={() => updatePreferences({ embed_metadata: !preferences.embed_metadata })}
                            className={twMerge(
                                "flex-1 flex items-center justify-center gap-2 px-2 py-2.5 rounded-md border transition-all text-xs font-medium",
                                preferences.embed_metadata 
                                    ? "bg-zinc-800 border-theme-cyan/50 text-theme-cyan"
                                    : "bg-surfaceHighlight border-border text-zinc-500 hover:text-zinc-300"
                            )}
                            title="Embed Metadata"
                         >
                            <FileText className="h-3.5 w-3.5" />
                            Metadata
                         </button>

                         <button
                            type="button"
                            onClick={() => updatePreferences({ embed_thumbnail: !preferences.embed_thumbnail })}
                            className={twMerge(
                                "flex-1 flex items-center justify-center gap-2 px-2 py-2.5 rounded-md border transition-all text-xs font-medium",
                                preferences.embed_thumbnail 
                                    ? "bg-zinc-800 border-theme-cyan/50 text-theme-cyan"
                                    : "bg-surfaceHighlight border-border text-zinc-500 hover:text-zinc-300"
                            )}
                            title="Embed Thumbnail"
                         >
                            <ImageIcon className="h-3.5 w-3.5" />
                            Thumbnail
                         </button>

                         <button
                            type="button"
                            onClick={() => updatePreferences({ live_from_start: !preferences.live_from_start })}
                            className={twMerge(
                                "flex-1 flex items-center justify-center gap-2 px-2 py-2.5 rounded-md border transition-all text-xs font-medium",
                                preferences.live_from_start
                                    ? "bg-zinc-800 border-theme-red/50 text-theme-red" 
                                    : "bg-surfaceHighlight border-border text-zinc-500 hover:text-zinc-300"
                            )}
                            title="Download Livestreams from Start"
                         >
                            <Radio className="h-3.5 w-3.5" />
                            Live
                         </button>
                     </div>
                 </div>
              </div>

              {/* Directory */}
              <div className="space-y-2">
                  <label className="text-[11px] uppercase tracking-wider font-bold text-zinc-500 ml-1">Save Location</label>
                  <div className="flex gap-2">
                     <input
                        type="text"
                        value={defaultDownloadPath || ''}
                        readOnly
                        placeholder="Downloads Folder (System Default)"
                        className="flex-grow bg-surfaceHighlight border border-border rounded-md px-3 py-2.5 text-sm text-zinc-500 cursor-not-allowed"
                     />
                     <Button 
                        type="button" 
                        variant="secondary" 
                        onClick={handleSelectDirectory} 
                        className="px-4 border-zinc-700 hover:border-zinc-500"
                        title="Choose Folder"
                     >
                        <FolderOpen className="h-4 w-4" />
                     </Button>
                  </div>
              </div>
          </div>

          <div className="pt-2 relative" ref={dropdownRef}>
            <div className={twMerge(
                "flex w-full h-12 rounded-md overflow-hidden transition-shadow",
                isSubmitDisabled 
                    ? "shadow-none" 
                    : "shadow-lg shadow-theme-cyan/20 hover:shadow-theme-cyan/40"
            )}>
                <Button 
                    type="submit" 
                    variant="default"
                    disabled={isSubmitDisabled} 
                    className={twMerge(
                        "flex-grow h-full text-base uppercase tracking-wide font-black rounded-r-none border-r border-black/20",
                        isProcessing 
                            ? "cursor-wait opacity-80" 
                            : ""
                    )}
                >
                    {isProcessing ? (
                        <>
                            <Loader2 className="mr-2 h-5 w-5 animate-spin" />
                            Analyzing...
                        </>
                    ) : (
                        <>
                            <Download className="mr-2 h-5 w-5" />
                            Initialize Download
                        </>
                    )}
                </Button>
                <Button
                    type="button"
                    variant="default"
                    disabled={isSubmitDisabled}
                    onClick={() => setShowForceOptions(!showForceOptions)}
                    className="w-12 h-full rounded-l-none px-0 flex items-center justify-center bg-theme-cyan/90 hover:bg-theme-cyan/80"
                >
                    <ChevronDown className={twMerge("h-5 w-5 transition-transform", showForceOptions ? "rotate-180" : "")} />
                </Button>
            </div>

            {showForceOptions && (
                <div className="absolute top-full left-0 right-0 mt-2 z-50 bg-zinc-900 border border-zinc-700 rounded-lg shadow-xl overflow-hidden animate-fade-in">
                    <button
                        type="button"
                        onClick={(e) => handleSubmit(e, true)}
                        className="w-full text-left px-4 py-3 flex items-start gap-3 hover:bg-zinc-800 transition-colors group"
                    >
                        <div className="p-2 rounded bg-zinc-800 group-hover:bg-zinc-700 text-theme-cyan mt-0.5">
                            <Download className="h-4 w-4" />
                        </div>
                        <div>
                            <div className="text-sm font-bold text-zinc-200">Force Download</div>
                            <div className="text-xs text-zinc-500 mt-0.5">
                                Bypass history check and download immediately.
                            </div>
                        </div>
                    </button>
                </div>
            )}
          </div>

        </form>
      </CardContent>
    </Card>
  );
}