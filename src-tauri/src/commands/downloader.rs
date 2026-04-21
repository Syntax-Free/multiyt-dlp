use tauri::{State, AppHandle};
use uuid::Uuid;
use std::sync::Arc;
use std::collections::HashSet;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, trace, warn};

use crate::config::ConfigManager;
use crate::core::{
    error::AppError,
    manager::JobManagerHandle,
    history::HistoryManager,
};
use crate::models::{DownloadFormatPreset, QueuedJob, PlaylistResult, PlaylistEntry, StartDownloadResponse};

static PROBE_SEMAPHORE: std::sync::OnceLock<Arc<Semaphore>> = std::sync::OnceLock::new();

fn get_probe_semaphore() -> Arc<Semaphore> {
    PROBE_SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(3))).clone()
}

async fn probe_url(url: &str, _app: &AppHandle, config_manager: &Arc<ConfigManager>) -> Result<Vec<PlaylistEntry>, AppError> {
    info!(target: "commands::downloader", "Starting playlist probe for URL: {}", url);
    let semaphore = get_probe_semaphore();
    trace!(target: "commands::downloader", "Waiting for probe semaphore permit...");
    let _permit = semaphore.acquire().await.map_err(|_| {
        error!(target: "commands::downloader", "Probe semaphore closed unexpectedly");
        AppError::ValidationFailed("Semaphore closed".into())
    })?;
    trace!(target: "commands::downloader", "Probe semaphore permit acquired");

    let config = config_manager.get_config().general;
    let bin_dir = crate::core::deps::get_common_bin_dir();
    
    let url_clone = url.to_string();
    
    let mut yt_dlp_cmd = "yt-dlp".to_string();
    let local_exe = bin_dir.join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" });
    if local_exe.exists() { 
        yt_dlp_cmd = local_exe.to_string_lossy().to_string(); 
        debug!(target: "commands::downloader", "Using local yt-dlp binary for probe: {}", yt_dlp_cmd);
    }

    let mut cmd = tokio::process::Command::new(&yt_dlp_cmd);

    if let Ok(current_path) = std::env::var("PATH") {
        let new_path = format!("{}{}{}", bin_dir.to_string_lossy(), if cfg!(windows) { ";" } else { ":" }, current_path);
        cmd.env("PATH", new_path);
    } else {
        cmd.env("PATH", bin_dir.to_string_lossy().to_string());
    }

    // Suppress config files and only probe for metadata
    cmd.arg("--ignore-config")
       .arg("--flat-playlist")
       .arg("--dump-single-json")
       .arg("--no-warnings")
       .arg(&url_clone);

    if let Some(path) = config.cookies_path {
        if !path.trim().is_empty() { 
            debug!(target: "commands::downloader", "Attaching cookies path to probe: {}", path);
            cmd.arg("--cookies").arg(path); 
        }
    } else if let Some(browser) = config.cookies_from_browser {
        if !browser.trim().is_empty() && browser != "none" { 
            debug!(target: "commands::downloader", "Attaching browser cookies to probe: {}", browser);
            cmd.arg("--cookies-from-browser").arg(browser); 
        }
    }

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    trace!(target: "commands::downloader", "Executing probe command: {:?}", cmd);
    let output_result = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output()).await;

    let output = match output_result {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            error!(target: "commands::downloader", "Probe process I/O error: {}", e);
            return Err(AppError::IoError(e.to_string()));
        },
        Err(_) => {
            error!(target: "commands::downloader", "Probe process timed out after 30 seconds");
            return Err(AppError::ValidationFailed("Probe timed out after 30 seconds".into()));
        },
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        warn!(target: "commands::downloader", "Probe process failed with exit code {:?}: {}", output.status.code(), stderr);
        return Err(AppError::ProcessFailed { 
            exit_code: output.status.code().unwrap_or(-1), 
            stderr
        });
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    trace!(target: "commands::downloader", "Probe output received ({} bytes)", json_str.len());

    let parsed: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| {
            error!(target: "commands::downloader", "Failed to parse probe JSON output: {}", e);
            AppError::ValidationFailed(format!("Failed to parse probe JSON: {}", e))
        })?;

    let mut entries = Vec::new();

    if let Some(entries_arr) = parsed.get("entries").and_then(|e| e.as_array()) {
        debug!(target: "commands::downloader", "Parsed probe output as a playlist containing {} items", entries_arr.len());
        for entry in entries_arr {
            if let Some(u) = entry.get("url").and_then(|s| s.as_str()) {
                entries.push(PlaylistEntry {
                    id: entry.get("id").and_then(|s| s.as_str()).map(|s| s.to_string()),
                    url: u.to_string(),
                    title: entry.get("title").and_then(|s| s.as_str()).unwrap_or("Unknown").to_string(),
                });
            }
        }
    } else {
        debug!(target: "commands::downloader", "Parsed probe output as a single video entity");
        entries.push(PlaylistEntry {
            id: parsed.get("id").and_then(|s| s.as_str()).map(|s| s.to_string()),
            url: parsed.get("webpage_url").and_then(|s| s.as_str()).unwrap_or(&url_clone).to_string(),
            title: parsed.get("title").and_then(|s| s.as_str()).unwrap_or("Unknown").to_string(),
        });
    }

    info!(target: "commands::downloader", "Probe completed successfully. Identified {} entries.", entries.len());
    Ok(entries)
}

#[tauri::command]
pub async fn expand_playlist(
    app: AppHandle,
    url: String,
    config: State<'_, Arc<ConfigManager>>,
) -> Result<PlaylistResult, AppError> {
    info!(target: "commands::downloader", "Frontend requested playlist expansion for: {}", url);
    let app_handle = app.clone();
    let config_manager = config.inner().clone();
    let entries = probe_url(&url, &app_handle, &config_manager).await?;
    Ok(PlaylistResult { entries })
}

#[tauri::command]
pub async fn start_download(
    app: AppHandle,
    url: String,
    download_path: Option<String>,
    format_preset: DownloadFormatPreset,
    video_resolution: String, 
    embed_metadata: bool,
    embed_thumbnail: bool,
    filename_template: String,
    restrict_filenames: Option<bool>,
    force_download: Option<bool>,
    live_from_start: Option<bool>,
    url_whitelist: Option<Vec<String>>,
    download_sections: Option<String>,
    config: State<'_, Arc<ConfigManager>>,
    manager: State<'_, JobManagerHandle>, 
    history: State<'_, HistoryManager>, 
) -> Result<StartDownloadResponse, AppError> { 
    
    info!(target: "commands::downloader", "Initializing download sequence for URL: {}", url);
    
    if !url.starts_with("http://") && !url.starts_with("https://") {
        warn!(target: "commands::downloader", "Rejected invalid URL: {}", url);
        return Err(AppError::ValidationFailed("Invalid URL provided.".into()));
    }

    let config_manager = config.inner().clone();
    let general_config = config_manager.get_config().general;

    let final_download_path = download_path
        .or(general_config.download_path)
        .or_else(|| tauri::api::path::download_dir().map(|p| p.to_string_lossy().to_string()));

    if final_download_path.is_none() {
        error!(target: "commands::downloader", "Could not resolve a valid destination directory");
        return Err(AppError::ValidationFailed("Could not determine a valid download directory.".into()));
    }
    
    debug!(target: "commands::downloader", "Resolved output directory: {:?}", final_download_path);

    let safe_template = if filename_template.trim().is_empty() {
        "%(title)s.%(ext)s".to_string()
    } else {
        filename_template
    };

    let app_handle = app.clone();
    let url_clone = url.clone();
    let is_forced = force_download.unwrap_or(false);

    let entries = probe_url(&url_clone, &app_handle, &config_manager).await?;
    
    let whitelist_set: Option<HashSet<String>> = url_whitelist.map(|list| list.into_iter().collect());
    
    let total_found = if let Some(ref wl) = whitelist_set {
        wl.len() as u32
    } else {
        entries.len() as u32
    };

    let mut created_job_ids = Vec::new();
    let mut skipped_urls = Vec::new();
    let mut urls_to_add = Vec::new();

    for entry in entries {
        if let Some(ref wl) = whitelist_set {
            if !wl.contains(&entry.url) {
                trace!(target: "commands::downloader", "Entry {} filtered out by whitelist", entry.url);
                continue;
            }
        }

        if !is_forced && history.exists(&entry.url) {
            debug!(target: "commands::downloader", "Entry {} skipped due to history duplication", entry.url);
            skipped_urls.push(entry.url.clone());
            continue;
        }

        let job_id = Uuid::new_v4();
        trace!(target: "commands::downloader", "Generating job ID {} for {}", job_id, entry.url);
        
        let job_data = QueuedJob {
            id: job_id,
            url: entry.url.clone(),
            download_path: final_download_path.clone(),
            format_preset: format_preset.clone(),
            video_resolution: video_resolution.clone(),
            embed_metadata,
            embed_thumbnail,
            restrict_filenames: restrict_filenames.unwrap_or(false),
            filename_template: safe_template.clone(),
            live_from_start: live_from_start.unwrap_or(false),
            download_sections: download_sections.clone(),
            status: None,
            error: None,
            stderr: None,
        };

        match manager.add_job(job_data).await {
            Ok(_) => {
                created_job_ids.push(job_id);
                urls_to_add.push(entry.url);
            },
            Err(e) => {
                error!(target: "commands::downloader", "Failed to add job to manager queue: {}", e);
                return Err(AppError::ValidationFailed(e));
            }
        }
    }

    if !urls_to_add.is_empty() {
        debug!(target: "commands::downloader", "Submitting {} URLs to history archiver", urls_to_add.len());
        let history_handle = history.inner().clone();
        tauri::async_runtime::spawn(async move {
            for url in urls_to_add {
                let _ = history_handle.add(&url).await;
            }
        });
    }

    info!(target: "commands::downloader", "Download initialization complete. Created {} jobs, skipped {}.", created_job_ids.len(), skipped_urls.len());

    Ok(StartDownloadResponse {
        job_ids: created_job_ids,
        skipped_count: skipped_urls.len() as u32,
        total_found,
        skipped_urls,
    })
}

#[tauri::command]
pub async fn cancel_download(
    job_id: Uuid,
    manager: State<'_, JobManagerHandle>,
) -> Result<(), AppError> {
    info!(target: "commands::downloader", "Cancellation requested for Job ID: {}", job_id);
    manager.cancel_job(job_id).await;
    Ok(())
}

#[tauri::command]
pub async fn resolve_file_conflict(
    job_id: Uuid,
    resolution: String,
    manager: State<'_, JobManagerHandle>,
) -> Result<(), AppError> {
    info!(target: "commands::downloader", "Resolving file conflict for Job ID: {}, strategy: {}", job_id, resolution);
    if resolution != "overwrite" && resolution != "discard" {
        warn!(target: "commands::downloader", "Invalid conflict resolution string provided: {}", resolution);
        return Err(AppError::ValidationFailed("Invalid resolution".into()));
    }
    manager.resolve_conflict(job_id, resolution).await.map_err(|e| {
        error!(target: "commands::downloader", "Failed to apply conflict resolution: {}", e);
        AppError::ValidationFailed(e)
    })?;
    Ok(())
}

#[tauri::command]
pub async fn get_pending_jobs(manager: State<'_, JobManagerHandle>) -> Result<u32, String> {
    trace!(target: "commands::downloader", "Fetching pending jobs count");
    Ok(manager.get_pending_count().await)
}

#[tauri::command]
pub async fn resume_pending_jobs(
    manager: State<'_, JobManagerHandle>
) -> Result<Vec<QueuedJob>, String> {
    info!(target: "commands::downloader", "Resuming pending jobs requested");
    Ok(manager.resume_pending().await)
}

#[tauri::command]
pub async fn clear_pending_jobs(manager: State<'_, JobManagerHandle>) -> Result<(), String> {
    info!(target: "commands::downloader", "Clearing pending jobs requested");
    manager.clear_pending().await;
    Ok(())
}

#[tauri::command]
pub async fn sync_download_state(
    manager: State<'_, JobManagerHandle>
) -> Result<Vec<crate::models::Download>, String> {
    trace!(target: "commands::downloader", "Frontend syncing download state");
    Ok(manager.sync_state().await)
}
