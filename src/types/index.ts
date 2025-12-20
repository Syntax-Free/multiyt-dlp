export interface GeneralConfig {
  download_path: string | null;
  filename_template: string;
  template_blocks_json: string | null;
  max_concurrent_downloads: number;
  max_total_instances: number;
  log_level: string;
  check_for_updates: boolean;
  cookies_path: string | null;
  cookies_from_browser: string | null;
}

export interface PreferenceConfig {
  mode: string;
  format_preset: string;
  video_preset: string; 
  audio_preset: string; 
  video_resolution: string; 
  embed_metadata: boolean;
  embed_thumbnail: boolean;
}

export interface WindowConfig {
  width: number;
  height: number;
  x: number;
  y: number;
}

export interface AppConfig {
  general: GeneralConfig;
  preferences: PreferenceConfig;
  window: WindowConfig;
}

export interface DependencyInfo {
    name: string;
    available: boolean;
    version: string | null;
    path: string | null;
}

export interface AppDependencies {
  yt_dlp: DependencyInfo;
  ffmpeg: DependencyInfo;
  js_runtime: DependencyInfo;
}

export type AppError = {
  IoError?: string;
  ProcessFailed?: { exit_code: number; stderr: string };
  ValidationFailed?: string;
  JobAlreadyExists?: string;
};

export type DownloadFormatPreset = 
  | 'best' 
  | 'best_mp4' 
  | 'best_mkv'
  | 'best_webm'
  | 'audio_best' 
  | 'audio_mp3'
  | 'audio_flac'
  | 'audio_m4a';

export interface DownloadProgressPayload {
  jobId: string;
  percentage: number;
  speed: string;
  eta: string;
  filename?: string; 
  phase?: string;    
}

export interface BatchProgressPayload {
    updates: DownloadProgressPayload[];
}

export interface DownloadCompletePayload {
  jobId: string;
  outputPath: string;
}

export interface DownloadErrorPayload {
  jobId: string;
  error: string;
  exit_code?: number;
  stderr: string;
  logs: string;
}

export type DownloadStatus = 'pending' | 'downloading' | 'completed' | 'error' | 'cancelled';

export interface Download {
  jobId: string;
  url: string;
  status: DownloadStatus;
  progress: number;
  speed?: string;
  eta?: string;
  outputPath?: string;
  error?: string;
  filename?: string;
  phase?: string;
  
  // Error Details
  exit_code?: number;
  stderr?: string;
  logs?: string;
  
  preset?: DownloadFormatPreset; 
  videoResolution?: string;
  downloadPath?: string;
  filenameTemplate?: string;
  embedMetadata?: boolean; 
  embedThumbnail?: boolean;
  restrictFilenames?: boolean;
}

export interface QueuedJob {
  id: string; 
  url: string;
  download_path?: string;
  format_preset: DownloadFormatPreset;
  video_resolution: string;
  embed_metadata: boolean;
  embed_thumbnail: boolean;
  filename_template: string;
  restrict_filenames: boolean;
}

export type TemplateBlockType = 'variable' | 'separator' | 'text';

export interface TemplateBlock {
  id: string;
  type: TemplateBlockType;
  value: string; 
  label: string; 
}

export interface PlaylistEntry {
    id?: string;
    url: string;
    title: string;
}

export interface PlaylistResult {
    entries: PlaylistEntry[];
}