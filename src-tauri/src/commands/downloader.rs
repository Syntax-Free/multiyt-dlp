use tauri::{State, AppHandle};
use uuid::Uuid;
use std::process::Command;
use std::sync::Arc;
use std::collections::HashSet;
use tokio::sync::Semaphore;

use crate::config::ConfigManager;
use crate::core::{
    error::AppError,
    manager::JobManagerHandle,
    history::HistoryManager,
};
use crate::models::{DownloadFormatPreset, QueuedJob, PlaylistResult, PlaylistEntry, StartDownloadResponse};

// Limits concurrent probing tasks to avoid system freezing on large playlists
static PROBE_SEMAPHORE: std::sync::OnceLock<Arc<Semaphore>> = std::sync::OnceLock::new();

fn get_probe_semaphore() -> Arc<Semaphore> {
    PROBE_SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(3))).clone()
}

async fn probe_url(url: &str, app: &AppHandle, config_manager: &Arc<ConfigManager>) -> Result<Vec<PlaylistEntry>, AppError> {
    let semaphore = get_probe_semaphore();
    let _permit = semaphore.acquire().await.map_err(|_| AppError::ValidationFailed("Semaphore closed".into()))?;

    let config = config_manager.get_config().general;
    let app_dir = app.path_resolver().app_data_dir().unwrap();
    let bin_dir = app_dir.join("bin");
    
    let url_clone = url.to_string();
    
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut yt_dlp_cmd = "yt-dlp".to_string();
        let local_exe = bin_dir.join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" });
        if local_exe.exists() { 
            yt_dlp_cmd = local_exe.to_string_lossy().to_string(); 
        }

        let mut cmd = Command::new(yt_dlp_cmd);

        if let Ok(current_path) = std::env::var("PATH") {
            let new_path = format!("{}{}{}", bin_dir.to_string_lossy(), if cfg!(windows) { ";" } else { ":" }, current_path);
            cmd.env("PATH", new_path);
        } else {
            cmd.env("PATH", bin_dir.to_string_lossy().to_string());
        }

        cmd.arg("--flat-playlist")
        .arg("--dump-single-json")
        .arg("--no-warnings")
        .arg(&url_clone);

        if let Some(path) = config.cookies_path {
            if !path.trim().is_empty() { cmd.arg("--cookies").arg(path); }
        } else if let Some(browser) = config.cookies_from_browser {
            if !browser.trim().is_empty() && browser != "none" { cmd.arg("--cookies-from-browser").arg(browser); }
        }

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        let output = cmd.output().map_err(|e| AppError::IoError(e.to_string()))?;

        if !output.status.success() {
            return Err(AppError::ProcessFailed { 
                exit_code: output.status.code().unwrap_or(-1), 
                stderr: String::from_utf8_lossy(&output.stderr).to_string() 
            });
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| AppError::ValidationFailed(format!("Failed to parse probe JSON: {}", e)))?;

        let mut entries = Vec::new();

        if let Some(entries_arr) = parsed.get("entries").and_then(|e| e.as_array()) {
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
            entries.push(PlaylistEntry {
                id: parsed.get("id").and_then(|s| s.as_str()).map(|s| s.to_string()),
                url: parsed.get("webpage_url").and_then(|s| s.as_str()).unwrap_or(&url_clone).to_string(),
                title: parsed.get("title").and_then(|s| s.as_str()).unwrap_or("Unknown").to_string(),
            });
        }

        Ok(entries)
    }).await.map_err(|e| AppError::IoError(e.to_string()))??;

    Ok(result)
}

#[tauri::command]
pub async fn expand_playlist(
    app: AppHandle,
    url: String,
    config: State<'_, Arc<ConfigManager>>,
) -> Result<PlaylistResult, AppError> {
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
    config: State<'_, Arc<ConfigManager>>,
    manager: State<'_, JobManagerHandle>, 
    history: State<'_, HistoryManager>, 
) -> Result<StartDownloadResponse, AppError> { 
    
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(AppError::ValidationFailed("Invalid URL provided.".into()));
    }

    let safe_template = if filename_template.trim().is_empty() {
        "%(title)s.%(ext)s".to_string()
    } else {
        filename_template
    };

    let app_handle = app.clone();
    let config_manager = config.inner().clone();
    let url_clone = url.clone();
    let is_forced = force_download.unwrap_or(false);

    // 1. Probe for entries
    let entries = probe_url(&url_clone, &app_handle, &config_manager).await?;
    let total_found = entries.len() as u32;

    let mut created_job_ids = Vec::new();
    let mut skipped_urls = Vec::new();
    let mut urls_to_add = Vec::new();

    let whitelist_set: Option<HashSet<String>> = url_whitelist.map(|list| list.into_iter().collect());

    // 2. Process entries
    for entry in entries {
        // If whitelist is provided (manual retry), skip anything not in the list
        if let Some(ref wl) = whitelist_set {
            if !wl.contains(&entry.url) {
                continue;
            }
        }

        // Check history if not forced
        if !is_forced && history.exists(&entry.url) {
            skipped_urls.push(entry.url.clone());
            continue;
        }

        let job_id = Uuid::new_v4();
        
        let job_data = QueuedJob {
            id: job_id,
            url: entry.url.clone(),
            download_path: download_path.clone(),
            format_preset: format_preset.clone(),
            video_resolution: video_resolution.clone(),
            embed_metadata,
            embed_thumbnail,
            restrict_filenames: restrict_filenames.unwrap_or(false),
            filename_template: safe_template.clone(),
            live_from_start: live_from_start.unwrap_or(false),
        };

        match manager.add_job(job_data).await {
            Ok(_) => {
                created_job_ids.push(job_id);
                // Queue for background history addition once download starts
                urls_to_add.push(entry.url);
            },
            Err(e) => {
                // This handles "URL already in queue" errors
                // We don't count these as "History Skips" but rather as "Active Conflicts"
                // The frontend will handle this via errorDetails.
                return Err(AppError::ValidationFailed(e));
            }
        }
    }

    // 3. Update History for the new URLs
    if !urls_to_add.is_empty() {
        let history_handle = history.inner().clone();
        tauri::async_runtime::spawn(async move {
            for url in urls_to_add {
                let _ = history_handle.add(&url).await;
            }
        });
    }

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
    manager.cancel_job(job_id).await;
    Ok(())
}

#[tauri::command]
pub async fn get_pending_jobs(manager: State<'_, JobManagerHandle>) -> Result<u32, String> {
    Ok(manager.get_pending_count().await)
}

#[tauri::command]
pub async fn resume_pending_jobs(
    manager: State<'_, JobManagerHandle>
) -> Result<Vec<QueuedJob>, String> {
    Ok(manager.resume_pending().await)
}

#[tauri::command]
pub async fn clear_pending_jobs(manager: State<'_, JobManagerHandle>) -> Result<(), String> {
    manager.clear_pending().await;
    Ok(())
}

#[tauri::command]
pub async fn sync_download_state(
    manager: State<'_, JobManagerHandle>
) -> Result<Vec<crate::models::Download>, String> {
    Ok(manager.sync_state().await)
}