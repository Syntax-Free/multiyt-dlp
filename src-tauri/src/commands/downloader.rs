use tauri::{State, AppHandle};
use uuid::Uuid;
use std::process::Command;
use std::sync::Arc;
use std::collections::HashSet;
use std::fs::{OpenOptions, File};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::config::ConfigManager;
use crate::core::{
    error::AppError,
    manager::{JobManagerHandle},
};
use crate::models::{DownloadFormatPreset, QueuedJob, PlaylistResult, PlaylistEntry, StartDownloadResponse};

// --- History Management Helpers ---

fn get_history_file_path() -> PathBuf {
    let home = dirs::home_dir().expect("Could not find home directory");
    home.join(".multiyt-dlp").join("downloads.txt")
}

fn load_history() -> HashSet<String> {
    let path = get_history_file_path();
    let mut set = HashSet::new();
    if path.exists() {
        if let Ok(file) = File::open(path) {
            let reader = BufReader::new(file);
            for line in reader.lines() {
                if let Ok(l) = line {
                    if !l.trim().is_empty() {
                        set.insert(l.trim().to_string());
                    }
                }
            }
        }
    }
    set
}

fn append_history(urls: Vec<String>) {
    let path = get_history_file_path();
    // Ensure directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        for url in urls {
            let _ = writeln!(file, "{}", url);
        }
    }
}

// --- Probe Helper ---

fn probe_url(url: &str, app: &AppHandle, config_manager: &Arc<ConfigManager>) -> Result<Vec<PlaylistEntry>, AppError> {
    let config = config_manager.get_config().general;

    // 1. Resolve Binary Path
    let app_dir = app.path_resolver().app_data_dir().unwrap();
    let bin_dir = app_dir.join("bin");
    
    let mut yt_dlp_cmd = "yt-dlp".to_string();
    let local_exe = bin_dir.join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" });
    if local_exe.exists() { 
        yt_dlp_cmd = local_exe.to_string_lossy().to_string(); 
    }

    let mut cmd = Command::new(yt_dlp_cmd);

    // 2. Inject Environment Path (for dependencies)
    if let Ok(current_path) = std::env::var("PATH") {
        let new_path = format!("{}{}{}", bin_dir.to_string_lossy(), if cfg!(windows) { ";" } else { ":" }, current_path);
        cmd.env("PATH", new_path);
    } else {
        cmd.env("PATH", bin_dir.to_string_lossy().to_string());
    }

    // 3. Configure Command
    cmd.arg("--flat-playlist")
       .arg("--dump-single-json")
       .arg("--no-warnings")
       .arg(url);

    // 4. Inject Cookies
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
        .map_err(|e| AppError::ValidationFailed(format!("Failed to parse JSON: {}", e)))?;

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
            url: parsed.get("webpage_url").and_then(|s| s.as_str()).unwrap_or(url).to_string(),
            title: parsed.get("title").and_then(|s| s.as_str()).unwrap_or("Unknown").to_string(),
        });
    }

    Ok(entries)
}

#[tauri::command]
pub async fn expand_playlist(
    app: AppHandle,
    url: String,
    config: State<'_, Arc<ConfigManager>>,
) -> Result<PlaylistResult, AppError> {
    let app_handle = app.clone();
    let config_manager = config.inner().clone();
    
    // Spawn blocking to avoid freezing the async runtime
    let entries = tauri::async_runtime::spawn_blocking(move || {
        probe_url(&url, &app_handle, &config_manager)
    }).await.map_err(|e| AppError::IoError(e.to_string()))??;

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
    url_whitelist: Option<Vec<String>>,
    config: State<'_, Arc<ConfigManager>>,
    manager: State<'_, JobManagerHandle>, 
) -> Result<StartDownloadResponse, AppError> { 
    
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(AppError::ValidationFailed("Invalid URL provided.".into()));
    }

    let safe_template = if filename_template.trim().is_empty() {
        "%(title)s.%(ext)s".to_string()
    } else {
        if filename_template.contains("..") || filename_template.starts_with("/") || filename_template.starts_with("\\") {
             return Err(AppError::ValidationFailed("Invalid characters in filename template.".into()));
        }
        filename_template
    };

    let app_handle = app.clone();
    let config_manager = config.inner().clone();
    let url_clone = url.clone();
    let is_forced = force_download.unwrap_or(false);

    // 1. Probe URL (Blocking Operation)
    let entries = tauri::async_runtime::spawn_blocking(move || {
        probe_url(&url_clone, &app_handle, &config_manager)
    }).await.map_err(|e| AppError::IoError(e.to_string()))??;

    let total_found = entries.len() as u32;

    // 2. Load History for Deduplication (Blocking)
    let history = tauri::async_runtime::spawn_blocking(load_history).await.map_err(|e| AppError::IoError(e.to_string()))?;

    let mut created_job_ids = Vec::new();
    let mut urls_to_archive = Vec::new();
    let mut skipped_urls = Vec::new();

    let whitelist_set: Option<HashSet<String>> = url_whitelist.map(|list| list.into_iter().collect());

    // 3. Filter and Queue Jobs
    for entry in entries {
        // WHITELIST CHECK: If whitelist is active, skip anything NOT in it
        if let Some(ref wl) = whitelist_set {
            if !wl.contains(&entry.url) {
                continue;
            }
        }

        // HISTORY CHECK:
        // If not forced AND not whitelisted (assuming whitelist implies intent), check history
        if !is_forced && history.contains(&entry.url) {
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
            filename_template: safe_template.clone(),
            restrict_filenames: restrict_filenames.unwrap_or(false),
        };

        match manager.add_job(job_data).await {
            Ok(_) => {
                created_job_ids.push(job_id);
                // Only prepare for history append if it wasn't there
                if !history.contains(&entry.url) {
                    urls_to_archive.push(entry.url);
                }
            },
            Err(e) => {
                // If duplicate active job, just ignore
                println!("Job ignored (Duplicate/Error): {}", e);
            }
        }
    }

    // 4. Append New URLs to History (Blocking)
    if !urls_to_archive.is_empty() {
        tauri::async_runtime::spawn_blocking(move || {
            append_history(urls_to_archive);
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