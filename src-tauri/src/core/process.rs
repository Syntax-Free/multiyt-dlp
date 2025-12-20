use std::process::Stdio;
use std::sync::Arc;
use once_cell::sync::Lazy;
use regex::Regex;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use std::path::{Path, PathBuf};
use std::fs;
use serde::Deserialize;

use crate::config::ConfigManager;
use crate::models::{DownloadFormatPreset, QueuedJob, JobMessage, DownloadErrorPayload};
use crate::commands::system::get_js_runtime_info;

// --- Regex Definitions ---
static DESTINATION_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[download\]\s+Destination:\s+(?P<filename>.+)$").unwrap());
static ALREADY_DOWNLOADED_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[download\]\s+(?:Destination:\s+)?(?P<filename>.+?)\s+has already been downloaded").unwrap());
static MERGER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\[Merger\]\s+Merging formats into\s+"?(?P<filename>.+?)"?$"#).unwrap());
static EXTRACT_AUDIO_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[ExtractAudio\]\s+Destination:\s+(?P<filename>.+)$").unwrap());
static METADATA_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[Metadata\]\s+Adding metadata to:\s+(?P<filename>.+)$").unwrap());
static THUMBNAIL_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[(?:Thumbnails|EmbedThumbnail)\]").unwrap());
static FIXUP_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[(?:Fixup\w+)\]").unwrap());
static TITLE_CLEANER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s\[[a-zA-Z0-9_-]{11}\]\.(?:f[0-9]+\.)?[a-z0-9]+$").unwrap());
static FILESYSTEM_ERROR_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(No such file|Invalid argument|cannot be written|WinError 123|Postprocessing: Error opening input files)").unwrap());

#[derive(Deserialize, Debug)]
struct YtDlpJsonProgress {
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
    total_bytes_estimate: Option<u64>,
    speed: Option<f64>, // bytes per second
    eta: Option<u64>,   // seconds
    filename: Option<String>,
}

// --- Helpers ---

fn robust_move_file(src: &Path, dest: &Path) -> Result<(), std::io::Error> {
    if let Err(_) = fs::rename(src, dest) {
        fs::copy(src, dest)?;
        fs::remove_file(src)?;
    }
    Ok(())
}

fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec.is_nan() || bytes_per_sec.is_infinite() { return "N/A".to_string(); }
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    if bytes_per_sec >= GIB { format!("{:.2} GiB/s", bytes_per_sec / GIB) }
    else if bytes_per_sec >= MIB { format!("{:.2} MiB/s", bytes_per_sec / MIB) }
    else if bytes_per_sec >= KIB { format!("{:.2} KiB/s", bytes_per_sec / KIB) }
    else { format!("{:.0} B/s", bytes_per_sec) }
}

fn format_eta(seconds: u64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 { format!("{:02}:{:02}:{:02}", h, m, s) }
    else { format!("{:02}:{:02}", m, s) }
}

fn construct_error(job_id: uuid::Uuid, msg: String, exit_code: Option<i32>, stderr: String, logs: Vec<String>) -> JobMessage {
    JobMessage::JobError {
        id: job_id,
        payload: DownloadErrorPayload {
            job_id,
            error: msg,
            exit_code,
            stderr,
            logs: logs.join("\n"),
        }
    }
}

// --- Main Process Logic ---

pub async fn run_download_process(
    mut job_data: QueuedJob,
    app_handle: AppHandle,
    tx_actor: mpsc::Sender<JobMessage>,
) {
    let job_id = job_data.id;
    let url = job_data.url.clone();

    // Notify Start
    let _ = tx_actor.send(JobMessage::UpdateProgress {
        id: job_id,
        percentage: 0.0,
        speed: "Starting...".to_string(),
        eta: "Calculating...".to_string(),
        filename: None,
        phase: "Initializing Process...".to_string(),
    }).await;

    let config_manager = app_handle.state::<Arc<ConfigManager>>();

    loop {
        // Refresh config on retry
        let general_config = config_manager.get_config().general;

        let app_dir = app_handle.path_resolver().app_data_dir().unwrap();
        let bin_dir = app_dir.join("bin");
        
        let target_dir = if let Some(ref path) = job_data.download_path {
            PathBuf::from(path)
        } else {
            match tauri::api::path::download_dir() {
                Some(path) => path,
                None => {
                    let _ = tx_actor.send(construct_error(job_id, "Missing download dir".into(), None, String::new(), vec![])).await;
                    let _ = tx_actor.send(JobMessage::WorkerFinished).await;
                    return;
                }
            }
        };
        
        if !target_dir.exists() { let _ = std::fs::create_dir_all(&target_dir); }
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let temp_dir = home.join(".multiyt-dlp").join("temp_downloads");
        if !temp_dir.exists() { let _ = std::fs::create_dir_all(&temp_dir); }

        let mut yt_dlp_cmd = "yt-dlp".to_string();
        let local_exe = bin_dir.join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" });
        if local_exe.exists() { yt_dlp_cmd = local_exe.to_string_lossy().to_string(); }

        let mut cmd = Command::new(yt_dlp_cmd);
        
        if let Ok(current_path) = std::env::var("PATH") {
            let new_path = format!("{}{}{}", bin_dir.to_string_lossy(), if cfg!(windows) { ";" } else { ":" }, current_path);
            cmd.env("PATH", new_path);
        } else {
            cmd.env("PATH", bin_dir.to_string_lossy().to_string());
        }
        
        cmd.env("PYTHONUTF8", "1");
        cmd.env("PYTHONIOENCODING", "utf-8");
        cmd.current_dir(&temp_dir);

        if let Some((name, path)) = get_js_runtime_info(&bin_dir) {
            cmd.arg("--js-runtimes").arg(format!("{}:{}", name, path));
        }

        if let Some(cookie_path) = &general_config.cookies_path {
            if !cookie_path.trim().is_empty() { cmd.arg("--cookies").arg(cookie_path); }
        } else if let Some(browser) = &general_config.cookies_from_browser {
            if !browser.trim().is_empty() && browser != "none" { cmd.arg("--cookies-from-browser").arg(browser); }
        }

        cmd.arg(&url)
            .arg("-o").arg(&job_data.filename_template) 
            .arg("--no-playlist")
            .arg("--no-simulate") 
            .arg("--newline")
            .arg("--windows-filenames")
            .arg("--encoding").arg("utf-8")
            .arg("--progress-template").arg("download:%(progress)j");

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        { cmd.creation_flags(0x08000000); }

        if job_data.restrict_filenames {
            cmd.arg("--restrict-filenames").arg("--trim-filenames").arg("200");
        }

        if job_data.embed_metadata { cmd.arg("--embed-metadata"); }
        if job_data.embed_thumbnail { cmd.arg("--embed-thumbnail"); }

        let height_filter = if job_data.video_resolution != "best" {
            let number_part: String = job_data.video_resolution.chars().filter(|c| c.is_numeric()).collect();
            if !number_part.is_empty() { format!("[height<={}]", number_part) } else { String::new() }
        } else { String::new() };

        match job_data.format_preset {
            DownloadFormatPreset::Best => {
                if !height_filter.is_empty() { cmd.arg("-f").arg(format!("bestvideo{}+bestaudio/best{}", height_filter, height_filter)); }
            }
            DownloadFormatPreset::BestMp4 => {
                cmd.arg("-f").arg(format!("bestvideo{}+bestaudio", height_filter));
                cmd.args(["--merge-output-format", "mp4"]);
            }
            DownloadFormatPreset::BestMkv => {
                cmd.arg("-f").arg(format!("bestvideo{}+bestaudio", height_filter));
                cmd.args(["--merge-output-format", "mkv"]);
            }
            DownloadFormatPreset::BestWebm => {
                cmd.arg("-f").arg(format!("bestvideo{}+bestaudio", height_filter));
                cmd.args(["--merge-output-format", "webm"]);
            }
            DownloadFormatPreset::AudioBest => { cmd.arg("-x").args(["-f", "bestaudio/best"]); }
            DownloadFormatPreset::AudioMp3 => { cmd.arg("-x").args(["--audio-format", "mp3", "--audio-quality", "0"]); }
            DownloadFormatPreset::AudioFlac => { cmd.arg("-x").args(["--audio-format", "flac", "--audio-quality", "0"]); }
            DownloadFormatPreset::AudioM4a => { cmd.arg("-x").args(["--audio-format", "m4a", "--audio-quality", "0"]); }
        }

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let _ = tx_actor.send(construct_error(job_id, format!("Failed to spawn process: {}", e), None, e.to_string(), vec![])).await;
                let _ = tx_actor.send(JobMessage::WorkerFinished).await;
                return;
            }
        };

        if let Some(pid) = child.id() {
             let _ = tx_actor.send(JobMessage::ProcessStarted { id: job_id, pid }).await;
        }

        if job_data.restrict_filenames {
            let _ = tx_actor.send(JobMessage::UpdateProgress {
                id: job_id, percentage: 0.0, speed: "Retrying...".to_string(), eta: "--".to_string(), filename: None,
                phase: "Sanitizing Filenames (Retry)".to_string(),
            }).await;
        }

        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");
        
        // Channel sends (line, is_stderr)
        let (tx, mut rx) = mpsc::channel::<(String, bool)>(100);

        let tx_out = tx.clone();
        tauri::async_runtime::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await { if tx_out.send((line, false)).await.is_err() { break; } }
        });

        let tx_err = tx.clone();
        tauri::async_runtime::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await { if tx_err.send((line, true)).await.is_err() { break; } }
        });
        drop(tx);

        let mut state_clean_title: Option<String> = None;
        let mut state_final_filename: Option<String> = None; 
        let mut state_percentage: f32 = 0.0;
        let mut state_phase: String = "Initializing".to_string();
        
        // Logs
        let mut captured_logs = Vec::new(); // Combined Logs
        let mut captured_stderr = Vec::new(); // Pure Stderr for error reporting
        
        let extract_filename_from_path = |path_str: &str| -> Option<String> {
            Path::new(path_str).file_name().map(|os| os.to_string_lossy().to_string())
        };
        let extract_clean_title = |path_str: &str| -> Option<String> {
             if let Some(fname) = extract_filename_from_path(path_str) {
                let cleaned = TITLE_CLEANER_REGEX.replace(&fname, "");
                return Some(cleaned.to_string());
             }
             None
        };

        while let Some((line, is_stderr)) = rx.recv().await {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            
            // Keep track of logs
            captured_logs.push(trimmed.to_string());
            if captured_logs.len() > 100 { captured_logs.remove(0); }
            
            if is_stderr {
                captured_stderr.push(trimmed.to_string());
                if captured_stderr.len() > 50 { captured_stderr.remove(0); }
            }

            let mut emit_update = false;
            let mut speed_str = "N/A".to_string();
            let mut eta_str = "N/A".to_string();

            if let Ok(progress_json) = serde_json::from_str::<YtDlpJsonProgress>(trimmed) {
                if let Some(d) = progress_json.downloaded_bytes {
                     let t = progress_json.total_bytes.or(progress_json.total_bytes_estimate);
                     if let Some(total) = t { state_percentage = (d as f32 / total as f32) * 100.0; }
                }
                if let Some(s) = progress_json.speed { speed_str = format_speed(s); }
                if let Some(e) = progress_json.eta { eta_str = format_eta(e); }
                if let Some(f) = progress_json.filename {
                     if let Some(n) = extract_filename_from_path(&f) {
                         if state_clean_title.is_none() { state_clean_title = extract_clean_title(&n); }
                         state_final_filename = Some(n);
                     }
                }
                
                if !state_phase.contains("Merging") && !state_phase.contains("Extracting") && !state_phase.contains("Writing") && !state_phase.contains("Embedding") {
                    state_phase = "Downloading".to_string();
                }
                emit_update = true;
            } else {
                if let Some(caps) = METADATA_REGEX.captures(trimmed) {
                    if let Some(f) = caps.name("filename") { state_final_filename = extract_filename_from_path(f.as_str()); }
                    state_phase = "Writing Metadata".to_string();
                    state_percentage = 99.0;
                    emit_update = true;
                }
                else if THUMBNAIL_REGEX.is_match(trimmed) {
                    state_phase = "Embedding Thumbnail".to_string();
                    state_percentage = 99.0;
                    emit_update = true;
                }
                else if let Some(caps) = MERGER_REGEX.captures(trimmed) {
                    if let Some(f) = caps.name("filename") {
                        state_final_filename = extract_filename_from_path(f.as_str());
                        state_clean_title = extract_clean_title(f.as_str()).or(state_clean_title);
                    }
                    state_phase = "Merging Formats".to_string();
                    state_percentage = 100.0;
                    eta_str = "Done".to_string();
                    emit_update = true;
                }
                else if let Some(caps) = EXTRACT_AUDIO_REGEX.captures(trimmed) {
                    if let Some(f) = caps.name("filename") {
                        state_final_filename = extract_filename_from_path(f.as_str());
                        state_clean_title = extract_clean_title(f.as_str()).or(state_clean_title);
                    }
                    state_phase = "Extracting Audio".to_string();
                    state_percentage = 100.0;
                    eta_str = "Done".to_string();
                    emit_update = true;
                }
                else if FIXUP_REGEX.is_match(trimmed) {
                    state_phase = "Fixing Container".to_string();
                    emit_update = true;
                }
                else if let Some(caps) = ALREADY_DOWNLOADED_REGEX.captures(trimmed) {
                    if let Some(f) = caps.name("filename") {
                        state_final_filename = extract_filename_from_path(f.as_str());
                        state_clean_title = extract_clean_title(f.as_str()).or(state_clean_title);
                    }
                    state_phase = "Finished".to_string();
                    state_percentage = 100.0;
                    eta_str = "Done".to_string();
                    emit_update = true;
                }
                else if let Some(caps) = DESTINATION_REGEX.captures(trimmed) {
                    if let Some(f) = caps.name("filename") {
                        let full_path_str = f.as_str();
                        if state_clean_title.is_none() { state_clean_title = extract_clean_title(full_path_str); }
                        state_final_filename = extract_filename_from_path(full_path_str);
                        state_phase = "Downloading".to_string();
                        emit_update = true;
                    }
                }
            }

            if emit_update {
                 let _ = tx_actor.send(JobMessage::UpdateProgress {
                    id: job_id,
                    percentage: state_percentage,
                    speed: speed_str,
                    eta: eta_str,
                    filename: state_clean_title.clone(),
                    phase: state_phase.clone()
                }).await;
            }
        }

        let status = child.wait().await.expect("Child process error");

        if status.success() {
            if let Some(filename) = state_final_filename {
                let src_path = temp_dir.join(&filename);
                let dest_path = target_dir.join(&filename);
                
                if src_path.exists() {
                    match robust_move_file(&src_path, &dest_path) {
                        Ok(_) => {
                            let _ = tx_actor.send(JobMessage::JobCompleted { id: job_id, output_path: dest_path.to_string_lossy().to_string() }).await;
                            break;
                        },
                        Err(e) => {
                            let _ = tx_actor.send(construct_error(job_id, format!("File move failed: {}", e), status.code(), e.to_string(), captured_logs)).await;
                            break;
                        }
                    }
                } else {
                     let _ = tx_actor.send(construct_error(job_id, "Output file missing in temp directory".into(), status.code(), "File not found at destination".into(), captured_logs)).await;
                     break;
                }
            } else {
                let _ = tx_actor.send(construct_error(job_id, "Could not determine filename".into(), status.code(), "Filename parser failed".into(), captured_logs)).await;
                break;
            }
        } else {
            let log_blob = captured_logs.join("\n");
            let stderr_blob = captured_stderr.join("\n");
            
            // Auto Retry logic for filename issues
            let is_filesystem_error = FILESYSTEM_ERROR_REGEX.is_match(&log_blob);
            if !job_data.restrict_filenames && is_filesystem_error {
                job_data.restrict_filenames = true;
                continue; // Retry Loop
            }

            let short_msg = if stderr_blob.contains("No supported JavaScript runtime") {
                "Missing JS Runtime".to_string()
            } else if stderr_blob.contains("Sign in to confirm") {
                "Authentication Required".to_string()
            } else {
                format!("Process Failed (Exit Code {})", status.code().unwrap_or(-1))
            };

            let _ = tx_actor.send(construct_error(job_id, short_msg, status.code(), stderr_blob, captured_logs)).await;
            break;
        }
    }
    
    let _ = tx_actor.send(JobMessage::WorkerFinished).await;
}