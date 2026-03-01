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
use std::time::Duration;
use tracing::{debug, error, warn, trace};
use walkdir::WalkDir;

use crate::config::ConfigManager;
use crate::models::{DownloadFormatPreset, QueuedJob, JobMessage, DownloadErrorPayload};
use crate::commands::system::get_js_runtime_info;

static FIXUP_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[(?:Fixup\w+)\]").unwrap());
static DOWNLOAD_START_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[download\]\s+Destination:").unwrap());
static FILESYSTEM_ERROR_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(No such file|Invalid argument|cannot be written|WinError 123|Postprocessing: Error opening input files)").unwrap());

#[derive(Deserialize, Debug)]
struct YtDlpJsonProgress {
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
    total_bytes_estimate: Option<u64>,
    speed: Option<f64>,
    eta: Option<u64>, 
    filename: Option<String>,
}

#[cfg(target_os = "windows")]
mod win_job {
    use windows::Win32::System::JobObjects::{
        CreateJobObjectW, AssignProcessToJobObject, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::Foundation::{HANDLE, CloseHandle};

    pub struct JobObject(HANDLE);

    impl JobObject {
        pub fn new() -> Result<Self, String> {
            unsafe {
                let job = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let success = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if !success.as_bool() {
                    let _ = CloseHandle(job);
                    return Err("Failed to set job object information".to_string());
                }
                Ok(Self(job))
            }
        }
        pub fn assign_process(&self, process_handle: std::os::windows::io::RawHandle) -> Result<(), String> {
            unsafe {
                let success = AssignProcessToJobObject(self.0, HANDLE(process_handle as isize));
                if !success.as_bool() {
                    return Err("Failed to assign process to job object".to_string());
                }
                Ok(())
            }
        }
    }
    impl Drop for JobObject {
        fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } }
    }
}

struct WorkerGuard {
    tx: mpsc::Sender<JobMessage>,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(JobMessage::WorkerFinished).await;
        });
    }
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
    error!(target: "core::process", job_id = ?job_id, exit_code = ?exit_code, "Job failed: {}", msg);
    
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

/// Robustly moves a file, handling potential cross-device errors or locks.
/// Returns std::io::ErrorKind::AlreadyExists if destination exists.
async fn robust_move_file(src: &Path, dest: &Path) -> Result<(), std::io::Error> {
    let mut attempts = 0;
    loop {
        // Explicit check for destination existence to avoid implicit overwrites or failures on Windows
        // This ensures the Conflict Handler in manager.rs gets triggered.
        if dest.exists() {
             return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "Destination file already exists"));
        }

        match fs::rename(src, dest) {
            Ok(_) => return Ok(()),
            Err(e) => {
                attempts += 1;
                // If the error was specifically "AlreadyExists", bail immediately
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    return Err(e);
                }
                
                // Retry a few times for transient locks
                if attempts > 3 {
                    // Fallback to Copy + Delete for cross-filesystem moves
                    if let Ok(_) = fs::copy(src, dest) {
                        let _ = fs::remove_file(src);
                        return Ok(());
                    }
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

pub async fn run_download_process(
    mut job_data: QueuedJob,
    app_handle: AppHandle,
    tx_actor: mpsc::Sender<JobMessage>,
) {
    let _guard = WorkerGuard { tx: tx_actor.clone() };

    let job_id = job_data.id;
    let url = job_data.url.clone();
    
    // Flag to skip cleanup if we enter Conflict state
    let mut preserve_temp_file = false;

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
        let general_config = config_manager.get_config().general;
        let bin_dir = crate::core::deps::get_common_bin_dir();
        
        let target_dir = if let Some(ref path) = job_data.download_path {
            PathBuf::from(path)
        } else {
            match tauri::api::path::download_dir() {
                Some(path) => path,
                None => {
                    let _ = tx_actor.send(construct_error(job_id, "Missing download dir".into(), None, String::new(), vec![])).await;
                    return; 
                }
            }
        };
        
        if !target_dir.exists() { let _ = std::fs::create_dir_all(&target_dir); }
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let base_temp_dir = home.join(".multiyt-dlp").join("temp_downloads");
        let unique_temp_dir = base_temp_dir.join(job_id.to_string());

        if unique_temp_dir.exists() { let _ = std::fs::remove_dir_all(&unique_temp_dir); }
        let _ = std::fs::create_dir_all(&unique_temp_dir);

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
        cmd.current_dir(&unique_temp_dir);

        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        if let Some((name, path)) = get_js_runtime_info(&bin_dir) {
            let ytdlp_runtime_name = match name.as_str() {
                "quickjs" | "quickjs-ng" => "quickjs",
                "node" => "node",
                "deno" => "deno",
                "bun" => "bun",
                _ => &name
            };
            cmd.arg("--js-runtimes").arg(format!("{}:{}", ytdlp_runtime_name, path));
        }

        if let Some(cookie_path) = &general_config.cookies_path {
            if !cookie_path.trim().is_empty() { cmd.arg("--cookies").arg(cookie_path); }
        } else if let Some(browser) = &general_config.cookies_from_browser {
            if !browser.trim().is_empty() && browser != "none" { cmd.arg("--cookies-from-browser").arg(browser); }
        }

        if general_config.use_concurrent_fragments {
            cmd.arg("-N").arg(general_config.concurrent_fragments.to_string());
        } else {
            cmd.arg("-N").arg("1");
        }

        cmd.arg(&url)
            .arg("-o").arg(&job_data.filename_template) 
            .arg("--no-playlist")
            .arg("--no-simulate") 
            .arg("--newline")
            .arg("--windows-filenames")
            .arg("--encoding").arg("utf-8")
            .arg("--progress") 
            .arg("--progress-template").arg("download:%(progress)j")
            .arg("--print").arg("after_move:filepath");

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        { cmd.creation_flags(0x08000000); } 

        if job_data.restrict_filenames {
            cmd.arg("--restrict-filenames").arg("--trim-filenames").arg("200");
        }

        if job_data.embed_metadata { cmd.arg("--embed-metadata"); }
        if job_data.embed_thumbnail { cmd.arg("--embed-thumbnail"); }

        if job_data.live_from_start {
            cmd.arg("--live-from-start");
        }

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

        debug!(target: "core::process", job_id = ?job_id, "Spawning process: {:?}", cmd.as_std());

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let _ = tx_actor.send(construct_error(job_id, format!("Failed to spawn process: {}", e), None, e.to_string(), vec![])).await;
                let _ = std::fs::remove_dir_all(&unique_temp_dir);
                return;
            }
        };

        #[cfg(target_os = "windows")]
        let _job_object = {
            if let Ok(job) = win_job::JobObject::new() {
                if let Some(handle) = child.raw_handle() {
                     let _ = job.assign_process(handle);
                }
                Some(job)
            } else {
                None
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
        
        let (tx, mut rx) = mpsc::channel::<(String, bool)>(1000);

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

        let mut state_percentage: f32 = 0.0;
        let mut state_phase: String = "Initializing".to_string();
        let mut detected_output_path: Option<String> = None;
        let mut detected_filename_only: Option<String> = None;
        
        let mut captured_logs = Vec::new();
        let mut captured_stderr = Vec::new();
        
        while let Some((line, is_stderr)) = rx.recv().await {
            if line.len() > 2048 { continue; }
            
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            
            captured_logs.push(trimmed.to_string());
            if captured_logs.len() > 100 { captured_logs.remove(0); }
            
            if is_stderr {
                warn!(target: "core::process::stderr", job_id = ?job_id, "{}", trimmed);
                captured_stderr.push(trimmed.to_string());
                if captured_stderr.len() > 50 { captured_stderr.remove(0); }
            } else {
                trace!(target: "core::process::stdout", job_id = ?job_id, "{}", trimmed);
            }

            if !is_stderr {
                let potential_path = PathBuf::from(trimmed);
                if potential_path.is_absolute() && potential_path.starts_with(&unique_temp_dir) {
                    detected_output_path = Some(trimmed.to_string());
                    if let Some(name) = potential_path.file_name() {
                        detected_filename_only = Some(name.to_string_lossy().to_string());
                    }
                    continue; 
                }
            }

            let mut emit_update = false;
            let mut speed_str = "N/A".to_string();
            let mut eta_str = "N/A".to_string();

            if trimmed.starts_with('{') {
                if let Ok(progress_json) = serde_json::from_str::<YtDlpJsonProgress>(trimmed) {
                    if let Some(d) = progress_json.downloaded_bytes {
                         let t = progress_json.total_bytes.or(progress_json.total_bytes_estimate);
                         if let Some(total) = t { 
                             if total > 0 {
                                 state_percentage = (d as f32 / total as f32) * 100.0; 
                             }
                         }
                    }
                    if let Some(s) = progress_json.speed { speed_str = format_speed(s); }
                    if let Some(e) = progress_json.eta { eta_str = format_eta(e); }
                    if let Some(f) = progress_json.filename {
                         if let Some(n) = Path::new(&f).file_name() {
                             detected_filename_only = detected_filename_only.or(Some(n.to_string_lossy().to_string()));
                         }
                    }
                    
                    if !state_phase.contains("Merging") && !state_phase.contains("Extracting") 
                       && !state_phase.contains("Writing") && !state_phase.contains("Embedding") 
                       && !state_phase.contains("Fixing") && !state_phase.contains("Moving") {
                        state_phase = "Downloading".to_string();
                    }
                    emit_update = true;
                }
            } else {
                if trimmed.starts_with("[download]") {
                     if DOWNLOAD_START_REGEX.is_match(trimmed) {
                        state_phase = "Starting Download".to_string();
                        emit_update = true;
                    }
                }
                else if trimmed.starts_with("[Metadata]") {
                    state_phase = "Writing Metadata".to_string();
                    state_percentage = 99.0;
                    emit_update = true;
                }
                else if trimmed.starts_with("[Thumbnails]") || trimmed.starts_with("[EmbedThumbnail]") {
                    state_phase = "Embedding Thumbnail".to_string();
                    state_percentage = 99.0;
                    emit_update = true;
                }
                else if trimmed.starts_with("[Merger]") {
                    state_phase = "Merging Formats".to_string();
                    state_percentage = 100.0;
                    eta_str = "Done".to_string();
                    emit_update = true;
                }
                else if trimmed.starts_with("[ExtractAudio]") {
                    state_phase = "Extracting Audio".to_string();
                    state_percentage = 100.0;
                    eta_str = "Done".to_string();
                    emit_update = true;
                }
                else if trimmed.starts_with("[Fixup") {
                    if FIXUP_REGEX.is_match(trimmed) {
                        state_phase = "Fixing Container".to_string();
                        state_percentage = 100.0;
                        emit_update = true;
                    }
                }
                else if trimmed.starts_with("[MoveFiles]") {
                    state_phase = "Finalizing".to_string();
                    state_percentage = 100.0;
                    emit_update = true;
                }
                else if trimmed.starts_with("[ffmpeg]") {
                     if !state_phase.contains("Merging") && !state_phase.contains("Extracting") {
                         state_phase = "Processing (FFmpeg)".to_string();
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
                    filename: detected_filename_only.clone(),
                    phase: state_phase.clone()
                }).await;
            }
        }

        let status = child.wait().await.expect("Child process error");

        if status.success() {
            let mut final_src_path: Option<PathBuf> = None;

            if let Some(p) = detected_output_path {
                let path = PathBuf::from(p);
                if path.exists() {
                    final_src_path = Some(path);
                }
            }

            if final_src_path.is_none() {
                 if let Some(ref fname) = detected_filename_only {
                     let path = unique_temp_dir.join(fname);
                     if path.exists() { final_src_path = Some(path); }
                 }
            }

            if final_src_path.is_none() {
                for entry in WalkDir::new(&unique_temp_dir).min_depth(1).max_depth(3) {
                    if let Ok(e) = entry {
                        if e.file_type().is_file() {
                             if let Some(ext) = e.path().extension() {
                                let ext_str = ext.to_string_lossy();
                                if ["mp4", "mkv", "webm", "mp3", "flac", "m4a", "wav"].contains(&ext_str.as_ref()) {
                                    final_src_path = Some(e.path().to_path_buf());
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            if let Some(src_path) = final_src_path {
                if !target_dir.exists() { let _ = std::fs::create_dir_all(&target_dir); }

                let file_name = src_path.file_name().unwrap();
                let dest_path = target_dir.join(file_name);
                
                // Notify "Moving" phase before attempting move
                let _ = tx_actor.send(JobMessage::UpdateProgress {
                    id: job_id,
                    percentage: 100.0,
                    speed: "Finalizing".to_string(),
                    eta: "00:00".to_string(),
                    filename: detected_filename_only.clone(),
                    phase: "Moving to Library".to_string()
                }).await;

                // Sync wait to ensure the message is processed by actor buffer before conflict logic fires
                tokio::time::sleep(Duration::from_millis(50)).await;
                
                match robust_move_file(&src_path, &dest_path).await {
                    Ok(_) => {
                        let _ = tx_actor.send(JobMessage::JobCompleted { id: job_id, output_path: dest_path.to_string_lossy().to_string() }).await;
                        break;
                    },
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::AlreadyExists {
                            // CONFLICT: Notify manager and preserve temp file
                            let _ = tx_actor.send(JobMessage::FileConflict { 
                                id: job_id, 
                                temp_path: src_path.to_string_lossy().to_string(),
                                output_path: dest_path.to_string_lossy().to_string()
                            }).await;
                            preserve_temp_file = true;
                            break;
                        } else {
                            let _ = tx_actor.send(construct_error(job_id, format!("File move failed: {}", e), status.code(), e.to_string(), captured_logs)).await;
                            break;
                        }
                    }
                }
            } else {
                let _ = tx_actor.send(construct_error(job_id, "Download succeeded but file not found".into(), status.code(), "Could not locate output file in temp dir".into(), captured_logs)).await;
                break;
            }
        } else {
            let log_blob = captured_logs.join("\n");
            let stderr_blob = captured_stderr.join("\n");
            
            warn!(target: "core::process", job_id = ?job_id, exit_code = ?status.code(), "Process exited with error");
            
            let is_filesystem_error = FILESYSTEM_ERROR_REGEX.is_match(&log_blob);
            if !job_data.restrict_filenames && is_filesystem_error {
                warn!(target: "core::process", job_id = ?job_id, "Retrying with restricted filenames");
                job_data.restrict_filenames = true;
                continue; 
            }

            let short_msg = if stderr_blob.contains("No supported JavaScript runtime") {
                "Missing compliant JS Runtime".to_string()
            } else if stderr_blob.contains("Sign in to confirm") {
                "Authentication Required".to_string()
            } else {
                format!("Process Failed (Exit Code {})", status.code().unwrap_or(-1))
            };

            let _ = tx_actor.send(construct_error(job_id, short_msg, status.code(), stderr_blob, captured_logs)).await;
            break;
        }
    }
    
    // Robust cleanup on exit, UNLESS we are in a conflict state
    if !preserve_temp_file {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let base_temp_dir = home.join(".multiyt-dlp").join("temp_downloads");
        let unique_temp_dir = base_temp_dir.join(job_id.to_string());
        
        async fn robust_remove_dir_internal(path: &Path) {
            for i in 0..5 {
                match fs::remove_dir_all(path) {
                    Ok(_) => return,
                    Err(_) => { tokio::time::sleep(Duration::from_millis(100 * 2u64.pow(i))).await; }
                }
            }
            let _ = fs::remove_dir_all(path);
        }

        if unique_temp_dir.exists() {
            robust_remove_dir_internal(&unique_temp_dir).await;
        }
    }
}