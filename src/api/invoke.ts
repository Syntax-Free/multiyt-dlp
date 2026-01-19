import { invoke } from "@tauri-apps/api/tauri";
import { open } from "@tauri-apps/api/dialog";
import { DownloadFormatPreset, AppDependencies, AppConfig, GeneralConfig, PreferenceConfig, PlaylistResult, QueuedJob, StartDownloadResponse, Download } from '@/types';

export async function checkDependencies(): Promise<AppDependencies> {
    return await invoke("check_dependencies");
}

export async function installDependency(name: string): Promise<void> {
    return await invoke("install_dependency", { name });
}

export async function syncDependencies(): Promise<AppDependencies> {
    return await invoke("sync_dependencies");
}

export async function openExternalLink(url: string): Promise<void> {
  return await invoke("open_external_link", { url });
}

export async function closeSplash(): Promise<void> {
  return await invoke("close_splash");
}

export async function getLatestAppVersion(): Promise<string> {
    return await invoke("get_latest_app_version");
}

export async function showInFolder(path: string): Promise<void> {
    return await invoke("show_in_folder", { path });
}

export async function openLogFolder(): Promise<void> {
    return await invoke("open_log_folder");
}

// --- Config API ---

export async function getAppConfig(): Promise<AppConfig> {
    return await invoke("get_app_config");
}

export async function saveGeneralConfig(config: GeneralConfig): Promise<void> {
    return await invoke("save_general_config", { config });
}

export async function savePreferenceConfig(config: PreferenceConfig): Promise<void> {
    return await invoke("save_preference_config", { config });
}

// --- Downloader API ---

export async function expandPlaylist(url: string): Promise<PlaylistResult> {
    return await invoke("expand_playlist", { url });
}

export async function startDownload(
  url: string, 
  downloadPath: string | undefined, 
  formatPreset: DownloadFormatPreset,
  videoResolution: string, 
  embedMetadata: boolean,
  embedThumbnail: boolean,
  filenameTemplate: string,
  restrictFilenames: boolean = false,
  forceDownload: boolean = false,
  urlWhitelist: string[] | undefined,
  liveFromStart: boolean = false
): Promise<StartDownloadResponse> { 
  return await invoke("start_download", { 
    url, 
    downloadPath, 
    formatPreset,
    videoResolution,
    embedMetadata,
    embedThumbnail,
    filenameTemplate,
    restrictFilenames,
    forceDownload,
    urlWhitelist,
    liveFromStart
  });
}

export async function cancelDownload(jobId: string): Promise<void> {
  return await invoke("cancel_download", { jobId });
}

export async function syncDownloadState(): Promise<Download[]> {
    return await invoke("sync_download_state");
}

// --- Persistence API ---

export async function getPendingJobs(): Promise<number> {
    return await invoke("get_pending_jobs");
}

export async function resumePendingJobs(): Promise<QueuedJob[]> {
    return await invoke("resume_pending_jobs");
}

export async function clearPendingJobs(): Promise<void> {
    return await invoke("clear_pending_jobs");
}

export async function selectDirectory(): Promise<string | null> {
    const selected = await open({
        directory: true,
        multiple: false,
    });
    
    if (Array.isArray(selected)) {
        return selected[0];
    }
    return selected;
}

// --- History API ---

export async function clearDownloadHistory(): Promise<void> {
    return await invoke("clear_download_history");
}

export async function getDownloadHistory(): Promise<string> {
    return await invoke("get_download_history");
}

export async function saveDownloadHistory(content: string): Promise<void> {
    return await invoke("save_download_history", { content });
}

// --- Logging API ---

export async function logFrontendMessage(level: 'Info' | 'Warn' | 'Error' | 'Debug', message: string, context?: string): Promise<void> {
    return await invoke("log_frontend_message", { level, message, context });
}