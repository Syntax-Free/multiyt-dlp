use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use once_cell::sync::Lazy;
use regex::Regex;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use std::path::{Path, PathBuf};
use std::fs;
use serde::Deserialize;
use std::time::{Duration, Instant};
use tracing::{debug, error, warn, trace, info};
use walkdir::WalkDir;
use std::collections::VecDeque;

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

fn construct_error(
    job_id: uuid::Uuid, 
    msg: String, 
    exit_code: Option<i32>, 
    stderr: String, 
    logs: VecDeque<String>
) -> JobMessage {
    error!(target: "core::process", job_id = ?job_id, exit_code = ?exit_code, "Job failed: {}", msg);
    
    let flattened_logs = Vec::from(logs).join("\n");

    JobMessage::JobError {
        id: job_id,
        payload: DownloadErrorPayload {
            job_id,
            error: msg,
            exit_code,
            stderr,
            logs: flattened_logs,
        }
    }
}

async fn robust_move_file(src: &Path, dest: &Path) -> Result<(), std::io::Error> {
    let mut attempts = 0;
    loop {
        if dest.exists() {
             warn!(target: "core::process", "Destination file already exists during robust move: {:?}", dest);
             return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "Destination file already exists"));
        }

        match fs::rename(src, dest) {
            Ok(_) => {
                trace!(target: "core::process", "Successfully moved file {:?} -> {:?}", src, dest);
                return Ok(())
            },
            Err(e) => {
                attempts += 1;
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    return Err(e);
                }
                warn!(target: "core::process", "Rename failed (Attempt {}). Error: {}. Retrying...", attempts, e);
                if attempts > 3 {
                    warn!(target: "core::process", "Rename exhausted retries, falling back to copy+delete.");
                    if let Ok(_) = fs::copy(src, dest) {
                        let _ = fs::remove_file(src);
                        return Ok(());
                    }
                    error!(target: "core::process", "Copy+delete fallback failed for {:?} -> {:?}", src, dest);
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
    cancel_flag: Arc<AtomicBool>,
) {
    let _guard = WorkerGuard { tx: tx_actor.clone() };

    let job_id = job_data.id;
    let url = job_data.url.clone();
    
    let mut preserve_temp_file = false;
    let mut fallback_level = 0;

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
        if cancel_flag.load(Ordering::Relaxed) {
            debug!(target: "core::process", job_id = ?job_id, "Job cancellation detected. Aborting outer process loop.");
            break;
        }

        info!(target: "core::process", job_id = ?job_id, "Preparing execution environment for URL (Fallback Level {})", fallback_level);
        let general_config = config_manager.get_config().general.clone();
        let bin_dir = crate::core::deps::get_common_bin_dir();
        
        let target_dir = if let Some(ref path) = job_data.download_path {
            PathBuf::from(path)
        } else {
            match tauri::api::path::download_dir() {
                Some(path) => path,
                None => {
                    error!(target: "core::process", job_id = ?job_id, "Failed to resolve system download directory");
                    let _ = tx_actor.send(construct_error(job_id, "Critical Error: Could not determine save directory.".into(), None, String::new(), VecDeque::new())).await;
                    return; 
                }
            }
        };
        
        if !target_dir.exists() { 
            trace!(target: "core::process", job_id = ?job_id, "Creating target directory: {:?}", target_dir);
            let _ = std::fs::create_dir_all(&target_dir); 
        }
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let base_temp_dir = home.join(".multiyt-dlp").join("temp_downloads");
        let unique_temp_dir = base_temp_dir.join(job_id.to_string());

        if unique_temp_dir.exists() { 
            trace!(target: "core::process", job_id = ?job_id, "Wiping existing unique temp directory");
            let _ = std::fs::remove_dir_all(&unique_temp_dir); 
        }
        let _ = std::fs::create_dir_all(&unique_temp_dir);

        let mut yt_dlp_cmd = "yt-dlp".to_string();
        let local_exe = bin_dir.join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" });
        if local_exe.exists() { yt_dlp_cmd = local_exe.to_string_lossy().to_string(); }

        let mut cmd = Command::new(&yt_dlp_cmd);
        
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
            debug!(target: "core::process", job_id = ?job_id, "Injecting JS Runtime: {}:{}", ytdlp_runtime_name, path);
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

        cmd.arg("--ignore-config");

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

        if let Some(ref sections) = job_data.download_sections {
            if !sections.trim().is_empty() {
                cmd.arg("--download-sections").arg(sections);
            }
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

        let args: Vec<String> = cmd.as_std().get_args().map(|s| s.to_string_lossy().to_string()).collect();
        let used_command = format!("{} {}", yt_dlp_cmd, args.join(" "));

        info!(target: "core::process", job_id = ?job_id, "Spawning yt-dlp: {}", used_command);

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                error!(target: "core::process", job_id = ?job_id, "Failed to spawn process: {}", e);
                let _ = tx_actor.send(construct_error(job_id, format!("Failed to spawn process: {}", e), None, e.to_string(), VecDeque::new())).await;
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
                warn!(target: "core::process", job_id = ?job_id, "Failed to create Windows Job Object for subprocess management");
                None
            }
        };

        if let Some(pid) = child.id() {
             let _ = tx_actor.send(JobMessage::ProcessStarted { id: job_id, pid }).await;
        }

        if job_data.restrict_filenames && fallback_level == 0 {
            let _ = tx_actor.send(JobMessage::UpdateProgress {
                id: job_id, percentage: 0.0, speed: "Retrying...".to_string(), eta: "--".to_string(), filename: None,
                phase: "Sanitizing Filenames (Retry)".to_string(),
            }).await;
        }

        trace!(target: "core::process", job_id = ?job_id, "Connecting to subprocess stdout/stderr pipes");
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
        
        let mut last_ipc_update = Instant::now();
        let mut last_emitted_phase = state_phase.clone();
        
        let mut captured_logs = VecDeque::with_capacity(100);
        let mut captured_stderr = VecDeque::with_capacity(50);
        
        while let Some((line, is_stderr)) = rx.recv().await {
            if line.len() > 2048 { 
                trace!(target: "core::process", job_id = ?job_id, "Skipped extremely long line (>2048 chars)");
                continue; 
            }
            
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            
            captured_logs.push_back(trimmed.to_string());
            if captured_logs.len() > 100 { 
                captured_logs.pop_front(); 
            }
            
            if is_stderr {
                trace!(target: "core::process::stderr", job_id = ?job_id, "{}", trimmed);
                captured_stderr.push_back(trimmed.to_string());
                if captured_stderr.len() > 50 { 
                    captured_stderr.pop_front(); 
                }
            } else {
                trace!(target: "core::process::stdout", job_id = ?job_id, "{}", trimmed);
            }

            if !is_stderr {
                let potential_path = PathBuf::from(trimmed);
                if potential_path.is_absolute() && potential_path.starts_with(&unique_temp_dir) {
                    debug!(target: "core::process", job_id = ?job_id, "Detected output path match: {}", trimmed);
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
                        if state_phase != "Downloading" {
                            trace!(target: "core::process", job_id = ?job_id, "Phase changed implicitly to Downloading via JSON telemetry");
                        }
                        state_phase = "Downloading".to_string();
                    }
                    emit_update = true;
                }
            } else {
                if trimmed.starts_with("[download]") {
                     if DOWNLOAD_START_REGEX.is_match(trimmed) {
                        trace!(target: "core::process", job_id = ?job_id, "Regex matched: DOWNLOAD_START_REGEX");
                        state_phase = "Starting Download".to_string();
                        emit_update = true;
                    }
                }
                else if trimmed.starts_with("[Metadata]") {
                    trace!(target: "core::process", job_id = ?job_id, "Matched Metadata phase string");
                    state_phase = "Writing Metadata".to_string();
                    state_percentage = 99.0;
                    emit_update = true;
                }
                else if trimmed.starts_with("[Thumbnails]") || trimmed.starts_with("[EmbedThumbnail]") {
                    trace!(target: "core::process", job_id = ?job_id, "Matched Thumbnail phase string");
                    state_phase = "Embedding Thumbnail".to_string();
                    state_percentage = 99.0;
                    emit_update = true;
                }
                else if trimmed.starts_with("[Merger]") {
                    trace!(target: "core::process", job_id = ?job_id, "Matched Merger phase string");
                    state_phase = "Merging Formats".to_string();
                    state_percentage = 100.0;
                    eta_str = "Done".to_string();
                    emit_update = true;
                }
                else if trimmed.starts_with("[ExtractAudio]") {
                    trace!(target: "core::process", job_id = ?job_id, "Matched ExtractAudio phase string");
                    state_phase = "Extracting Audio".to_string();
                    state_percentage = 100.0;
                    eta_str = "Done".to_string();
                    emit_update = true;
                }
                else if trimmed.starts_with("[Fixup") {
                    if FIXUP_REGEX.is_match(trimmed) {
                        trace!(target: "core::process", job_id = ?job_id, "Regex matched: FIXUP_REGEX");
                        state_phase = "Fixing Container".to_string();
                        state_percentage = 100.0;
                        emit_update = true;
                    }
                }
                else if trimmed.starts_with("[MoveFiles]") {
                    trace!(target: "core::process", job_id = ?job_id, "Matched MoveFiles phase string");
                    state_phase = "Finalizing".to_string();
                    state_percentage = 100.0;
                    emit_update = true;
                }
                else if trimmed.starts_with("[ffmpeg]") {
                     if !state_phase.contains("Merging") && !state_phase.contains("Extracting") {
                         trace!(target: "core::process", job_id = ?job_id, "Matched generic ffmpeg phase string");
                         state_phase = "Processing (FFmpeg)".to_string();
                         emit_update = true;
                    }
                }
            }

            if emit_update {
                 let phase_changed = state_phase != last_emitted_phase;
                 let time_elapsed = last_ipc_update.elapsed().as_millis() >= 500;
                 let is_terminal = state_percentage >= 100.0;
                 
                 if time_elapsed || phase_changed || is_terminal {
                     let _ = tx_actor.send(JobMessage::UpdateProgress {
                        id: job_id,
                        percentage: state_percentage,
                        speed: speed_str,
                        eta: eta_str,
                        filename: detected_filename_only.clone(),
                        phase: state_phase.clone()
                     }).await;
                     
                     last_ipc_update = Instant::now();
                     last_emitted_phase = state_phase.clone();
                 }
            }
        }

        let status = child.wait().await.expect("Child process error");

        if cancel_flag.load(Ordering::Relaxed) {
            debug!(target: "core::process", job_id = ?job_id, "Job cancellation detected. Aborting outer process loop.");
            break;
        }

        if status.success() {
            debug!(target: "core::process", job_id = ?job_id, "Subprocess returned success exit code (0)");
            let mut final_src_path: Option<PathBuf> = None;

            if let Some(p) = detected_output_path {
                let path = PathBuf::from(p);
                if path.exists() {
                    trace!(target: "core::process", job_id = ?job_id, "Validated explicitly detected output path: {:?}", path);
                    final_src_path = Some(path);
                } else {
                    warn!(target: "core::process", job_id = ?job_id, "Explicit output path was detected but file does not exist: {:?}", path);
                }
            }

            if final_src_path.is_none() {
                 if let Some(ref fname) = detected_filename_only {
                     let path = unique_temp_dir.join(fname);
                     if path.exists() { 
                         trace!(target: "core::process", job_id = ?job_id, "Validated fallback filename matching path: {:?}", path);
                         final_src_path = Some(path); 
                     }
                 }
            }

            if final_src_path.is_none() {
                debug!(target: "core::process", job_id = ?job_id, "Initiating deep temp dir scan for valid media file...");
                for entry in WalkDir::new(&unique_temp_dir).min_depth(1).max_depth(3) {
                    if let Ok(e) = entry {
                        if e.file_type().is_file() {
                             if let Some(ext) = e.path().extension() {
                                let ext_str = ext.to_string_lossy();
                                if ["mp4", "mkv", "webm", "mp3", "flac", "m4a", "wav"].contains(&ext_str.as_ref()) {
                                    debug!(target: "core::process", job_id = ?job_id, "Scan matched valid media file: {:?}", e.path());
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
                
                let _ = tx_actor.send(JobMessage::UpdateProgress {
                    id: job_id,
                    percentage: 100.0,
                    speed: "Finalizing".to_string(),
                    eta: "00:00".to_string(),
                    filename: detected_filename_only.clone(),
                    phase: "Moving to Library".to_string()
                }).await;

                tokio::time::sleep(Duration::from_millis(50)).await;
                
                let is_modified = fallback_level > 0;

                match robust_move_file(&src_path, &dest_path).await {
                    Ok(_) => {
                        info!(target: "core::process", job_id = ?job_id, "Successfully moved completed file to target directory: {:?}", dest_path);
                        let _ = tx_actor.send(JobMessage::JobCompleted { 
                            id: job_id, 
                            output_path: dest_path.to_string_lossy().to_string(),
                            is_modified,
                            used_command,
                        }).await;
                        break;
                    },
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::AlreadyExists {
                            warn!(target: "core::process", job_id = ?job_id, "Conflict block: File already exists at {:?}", dest_path);
                            let _ = tx_actor.send(JobMessage::FileConflict { 
                                id: job_id, 
                                temp_path: src_path.to_string_lossy().to_string(),
                                output_path: dest_path.to_string_lossy().to_string(),
                                is_modified,
                                used_command,
                            }).await;
                            preserve_temp_file = true;
                            break;
                        } else {
                            error!(target: "core::process", job_id = ?job_id, "Catastrophic file move failure: {}", e);
                            let _ = tx_actor.send(construct_error(job_id, format!("File move failed: {}", e), status.code(), e.to_string(), captured_logs)).await;
                            break;
                        }
                    }
                }
            } else {
                error!(target: "core::process", job_id = ?job_id, "Download claimed success, but no matching output file found in {:?}", unique_temp_dir);
                let _ = tx_actor.send(construct_error(job_id, "Download succeeded but file not found".into(), status.code(), "Could not locate output file in temp dir".into(), captured_logs)).await;
                break;
            }
        } else {
            let log_blob = Vec::from(captured_logs.clone()).join("\n");
            let stderr_blob = Vec::from(captured_stderr.clone()).join("\n");
            
            warn!(target: "core::process", job_id = ?job_id, exit_code = ?status.code(), "Process exited with error status");
            
            let is_filesystem_error = FILESYSTEM_ERROR_REGEX.is_match(&log_blob);
            if !job_data.restrict_filenames && is_filesystem_error {
                warn!(target: "core::process", job_id = ?job_id, "Filesystem error detected in logs. Enabling restrict_filenames and retrying.");
                job_data.restrict_filenames = true;
                continue; 
            }

            let is_fatal_auth_js = stderr_blob.contains("No supported JavaScript runtime") 
                || stderr_blob.contains("Sign in to confirm") 
                || stderr_blob.contains("confirm you're not a bot");

            if !is_fatal_auth_js {
                if fallback_level == 0 {
                    warn!(target: "core::process", job_id = ?job_id, "Download failed natively, escalating to Fallback Level 1 (Loose Format)");
                    fallback_level = 1;
                    job_data.video_resolution = "best".to_string();
                    job_data.embed_metadata = false;
                    job_data.embed_thumbnail = false;
                    job_data.live_from_start = false;
                    
                    let _ = tx_actor.send(JobMessage::UpdateProgress {
                        id: job_id, percentage: 0.0, speed: "Retrying...".to_string(), eta: "--".to_string(), filename: None,
                        phase: "Fallback Level 1 (Loose Format)".to_string(),
                    }).await;
                    continue;
                } else if fallback_level == 1 {
                    warn!(target: "core::process", job_id = ?job_id, "Download failed at Level 1, escalating to Fallback Level 2 (Any Format)");
                    fallback_level = 2;
                    job_data.format_preset = DownloadFormatPreset::Best;
                    
                    let _ = tx_actor.send(JobMessage::UpdateProgress {
                        id: job_id, percentage: 0.0, speed: "Retrying...".to_string(), eta: "--".to_string(), filename: None,
                        phase: "Fallback Level 2 (Any Format)".to_string(),
                    }).await;
                    continue;
                }
            } else {
                error!(target: "core::process", job_id = ?job_id, "Fatal unrecoverable error detected in logs (Auth or Runtime requirement)");
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
    
    if !preserve_temp_file {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let base_temp_dir = home.join(".multiyt-dlp").join("temp_downloads");
        let unique_temp_dir = base_temp_dir.join(job_id.to_string());
        
        async fn robust_remove_dir_internal(path: &Path) {
            for i in 0..5 {
                match fs::remove_dir_all(path) {
                    Ok(_) => {
                        trace!(target: "core::process", "Cleaned up unique temp dir: {:?}", path);
                        return;
                    },
                    Err(_) => { tokio::time::sleep(Duration::from_millis(100 * 2u64.pow(i))).await; }
                }
            }
            warn!(target: "core::process", "Failed to clean unique temp dir {:?} after retries", path);
            let _ = fs::remove_dir_all(path);
        }

        if unique_temp_dir.exists() {
            robust_remove_dir_internal(&unique_temp_dir).await;
        }
    }
}
