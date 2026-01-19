use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, Duration};
use tauri::{AppHandle, Manager};
use uuid::Uuid;
use std::fs;
use std::path::PathBuf;

use crate::models::{
    Job, JobStatus, QueuedJob, JobMessage, 
    DownloadProgressPayload, BatchProgressPayload, 
    DownloadCompletePayload,
    DownloadCancelledPayload,
    Download
};
use crate::config::ConfigManager;
use crate::core::process::run_download_process;
use crate::core::native;

#[derive(Clone)]
pub struct JobManagerHandle {
    sender: mpsc::Sender<JobMessage>,
}

impl JobManagerHandle {
    pub fn new(app_handle: AppHandle) -> Self {
        let (sender, receiver) = mpsc::channel(1000);
        let actor = JobManagerActor::new(app_handle, receiver, sender.clone());
        tauri::async_runtime::spawn(actor.run());
        
        Self { sender }
    }

    pub async fn add_job(&self, job: QueuedJob) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let _ = self.sender.send(JobMessage::AddJob { job, resp: tx }).await;
        rx.await.map_err(|_| "Actor closed".to_string())?
    }

    pub async fn cancel_job(&self, id: Uuid) {
        let _ = self.sender.send(JobMessage::CancelJob { id }).await;
    }

    pub async fn get_pending_count(&self) -> u32 {
        let (tx, rx) = oneshot::channel();
        let _ = self.sender.send(JobMessage::GetPendingCount(tx)).await;
        rx.await.unwrap_or(0)
    }

    pub async fn resume_pending(&self) -> Vec<QueuedJob> {
        let (tx, rx) = oneshot::channel();
        let _ = self.sender.send(JobMessage::ResumePending(tx)).await;
        rx.await.unwrap_or_default()
    }

    pub async fn clear_pending(&self) {
        let _ = self.sender.send(JobMessage::ClearPending).await;
    }

    pub async fn sync_state(&self) -> Vec<Download> {
        let (tx, rx) = oneshot::channel();
        let _ = self.sender.send(JobMessage::SyncState(tx)).await;
        rx.await.unwrap_or_default()
    }
}

struct JobManagerActor {
    app_handle: AppHandle,
    receiver: mpsc::Receiver<JobMessage>,
    self_sender: mpsc::Sender<JobMessage>, // To pass to workers

    // State
    jobs: HashMap<Uuid, Job>,
    queue: VecDeque<QueuedJob>,
    persistence_registry: HashMap<Uuid, QueuedJob>,

    // Concurrency
    active_network_jobs: u32,
    active_process_instances: u32,
    completed_session_count: u32,

    // Batching Buffer
    pending_updates: HashMap<Uuid, DownloadProgressPayload>,
}

impl JobManagerActor {
    fn new(app_handle: AppHandle, receiver: mpsc::Receiver<JobMessage>, self_sender: mpsc::Sender<JobMessage>) -> Self {
        Self {
            app_handle,
            receiver,
            self_sender,
            jobs: HashMap::new(),
            queue: VecDeque::new(),
            persistence_registry: HashMap::new(),
            active_network_jobs: 0,
            active_process_instances: 0,
            completed_session_count: 0,
            pending_updates: HashMap::new(),
        }
    }

    fn get_persistence_path() -> PathBuf {
        let home = dirs::home_dir().expect("Could not find home directory");
        home.join(".multiyt-dlp").join("jobs.json")
    }

    fn save_state(&self) {
        let path = Self::get_persistence_path();
        let jobs: Vec<QueuedJob> = self.persistence_registry.values().cloned().collect();
        
        tauri::async_runtime::spawn(async move {
            if let Ok(json) = serde_json::to_string_pretty(&jobs) {
                // Atomic Write: Write to temp then rename
                let tmp_path = path.with_extension("tmp");
                if tokio::fs::write(&tmp_path, json).await.is_ok() {
                    let _ = tokio::fs::rename(tmp_path, path).await;
                }
            }
        });
    }

    async fn run(mut self) {
        let mut interval = time::interval(Duration::from_millis(100));

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    self.handle_message(msg).await;
                }
                _ = interval.tick() => {
                    self.flush_updates();
                    self.update_native_ui();
                }
            }
        }
    }

    async fn handle_message(&mut self, msg: JobMessage) {
        match msg {
            JobMessage::AddJob { job, resp } => {
                if self.jobs.contains_key(&job.id) {
                    let _ = resp.send(Err("Job already exists".into()));
                } else {
                    let is_duplicate_active = self.jobs.values().any(|j| {
                        j.url == job.url && (j.status == JobStatus::Pending || j.status == JobStatus::Downloading)
                    });

                    if is_duplicate_active {
                        let _ = resp.send(Err("URL is already in queue".into()));
                    } else {
                        // Inherit original QueuedJob props into Job model for UI sync
                        let mut j = Job::new(job.id, job.url.clone());
                        j.preset = Some(job.format_preset.clone());
                        j.video_resolution = Some(job.video_resolution.clone());
                        j.download_path = job.download_path.clone();
                        j.filename_template = Some(job.filename_template.clone());
                        j.embed_metadata = Some(job.embed_metadata);
                        j.embed_thumbnail = Some(job.embed_thumbnail);
                        j.restrict_filenames = Some(job.restrict_filenames);
                        j.live_from_start = Some(job.live_from_start);

                        self.jobs.insert(job.id, j);
                        self.persistence_registry.insert(job.id, job.clone());
                        self.queue.push_back(job);
                        self.save_state();
                        self.process_queue();
                        let _ = resp.send(Ok(()));
                    }
                }
            },
            JobMessage::CancelJob { id } => {
                if let Some(job) = self.jobs.get(&id) {
                    if let Some(pid) = job.pid {
                        self.kill_process(pid);
                    }
                }
                if let Some(job) = self.jobs.get_mut(&id) {
                    job.status = JobStatus::Cancelled;
                }
                self.persistence_registry.remove(&id);
                self.save_state();

                let _ = self.app_handle.emit_all("download-cancelled", DownloadCancelledPayload {
                    job_id: id
                });
            },
            JobMessage::ProcessStarted { id, pid } => {
                if let Some(job) = self.jobs.get_mut(&id) {
                    if job.status == JobStatus::Cancelled {
                        self.kill_process(pid);
                    } else {
                        job.pid = Some(pid);
                        job.status = JobStatus::Downloading;
                    }
                }
            },
            JobMessage::UpdateProgress { id, percentage, speed, eta, filename, phase } => {
                if let Some(job) = self.jobs.get_mut(&id) {
                    if job.status == JobStatus::Cancelled { return; }

                    job.progress = percentage;
                    job.speed = Some(speed.clone());
                    job.eta = Some(eta.clone());
                    if filename.is_some() { job.filename = filename.clone(); }
                    job.phase = Some(phase.clone());

                    self.pending_updates.insert(id, DownloadProgressPayload {
                        job_id: id,
                        percentage,
                        speed,
                        eta,
                        filename,
                        phase: Some(phase)
                    });
                }
            },
            JobMessage::JobCompleted { id, output_path } => {
                if let Some(job) = self.jobs.get_mut(&id) {
                    if job.status == JobStatus::Cancelled { return; }
                    job.status = JobStatus::Completed;
                    job.progress = 100.0;
                    job.output_path = Some(output_path.clone());
                    job.phase = Some("Done".to_string());
                }
                self.persistence_registry.remove(&id);
                self.save_state();

                let _ = self.app_handle.emit_all("download-complete", DownloadCompletePayload {
                    job_id: id,
                    output_path,
                });
            },
            JobMessage::JobError { id, payload } => {
                if let Some(job) = self.jobs.get_mut(&id) {
                    if job.status == JobStatus::Cancelled { return; }
                    job.status = JobStatus::Error;
                    job.error = Some(payload.error.clone());
                    job.stderr = Some(payload.stderr.clone());
                    job.logs = Some(payload.logs.clone());
                    job.exit_code = payload.exit_code;
                }
                let _ = self.app_handle.emit_all("download-error", payload);
            },
            JobMessage::WorkerFinished => {
                if self.active_process_instances > 0 {
                    self.active_process_instances -= 1;
                    self.completed_session_count += 1;
                }
                if self.active_network_jobs > 0 {
                    self.active_network_jobs -= 1;
                }
                if self.active_process_instances == 0 {
                    self.trigger_finished_notification();
                    self.clean_temp_directory();
                }
                self.process_queue();
            },
            JobMessage::GetPendingCount(tx) => {
                let path = Self::get_persistence_path();
                if path.exists() {
                     if let Ok(content) = fs::read_to_string(path) {
                         if let Ok(jobs) = serde_json::from_str::<Vec<QueuedJob>>(&content) {
                             let _ = tx.send(jobs.len() as u32);
                             return;
                         }
                     }
                }
                let _ = tx.send(0);
            },
            JobMessage::ResumePending(tx) => {
                let path = Self::get_persistence_path();
                let mut resumed = Vec::new();
                if path.exists() {
                    if let Ok(content) = fs::read_to_string(path) {
                        if let Ok(jobs) = serde_json::from_str::<Vec<QueuedJob>>(&content) {
                            for job in jobs {
                                if !self.jobs.contains_key(&job.id) {
                                    // Reconstruct Job
                                    let mut j = Job::new(job.id, job.url.clone());
                                    j.preset = Some(job.format_preset.clone());
                                    j.video_resolution = Some(job.video_resolution.clone());
                                    j.download_path = job.download_path.clone();
                                    j.filename_template = Some(job.filename_template.clone());
                                    j.embed_metadata = Some(job.embed_metadata);
                                    j.embed_thumbnail = Some(job.embed_thumbnail);
                                    j.restrict_filenames = Some(job.restrict_filenames);
                                    j.live_from_start = Some(job.live_from_start);

                                    self.jobs.insert(job.id, j);
                                    self.persistence_registry.insert(job.id, job.clone());
                                    self.queue.push_back(job.clone());
                                    resumed.push(job);
                                }
                            }
                        }
                    }
                }
                self.process_queue(); 
                let _ = tx.send(resumed);
            },
            JobMessage::ClearPending => {
                let path = Self::get_persistence_path();
                if path.exists() { let _ = fs::remove_file(path); }
                self.clean_temp_directory();
            },
            JobMessage::SyncState(tx) => {
                // Convert internal HashMap<Uuid, Job> to Vec<Download> equivalent
                let mut downloads: Vec<Download> = Vec::new();
                for job in self.jobs.values() {
                    downloads.push(Download {
                        job_id: job.id,
                        url: job.url.clone(),
                        status: job.status.clone(),
                        progress: job.progress,
                        speed: job.speed.clone(),
                        eta: job.eta.clone(),
                        output_path: job.output_path.clone(),
                        error: job.error.clone(),
                        filename: job.filename.clone(),
                        phase: job.phase.clone(),
                        exit_code: job.exit_code,
                        stderr: job.stderr.clone(),
                        logs: job.logs.clone(),
                        preset: job.preset.clone(),
                        video_resolution: job.video_resolution.clone(),
                        download_path: job.download_path.clone(),
                        filename_template: job.filename_template.clone(),
                        embed_metadata: job.embed_metadata,
                        embed_thumbnail: job.embed_thumbnail,
                        restrict_filenames: job.restrict_filenames,
                        live_from_start: job.live_from_start,
                    });
                }
                let _ = tx.send(downloads);
            }
        }
    }

    fn flush_updates(&mut self) {
        if self.pending_updates.is_empty() { return; }

        let updates: Vec<DownloadProgressPayload> = self.pending_updates.values().cloned().collect();
        self.pending_updates.clear();
        let _ = self.app_handle.emit_all("download-progress-batch", BatchProgressPayload { updates });
    }

    fn process_queue(&mut self) {
        let config_manager = self.app_handle.state::<Arc<ConfigManager>>();
        let config = config_manager.get_config().general;

        while self.active_network_jobs < config.max_concurrent_downloads 
           && self.active_process_instances < config.max_total_instances 
        {
            if let Some(next_job) = self.queue.pop_front() {
                 if let Some(job) = self.jobs.get(&next_job.id) {
                     if job.status == JobStatus::Cancelled { continue; }
                 }

                 self.active_network_jobs += 1;
                 self.active_process_instances += 1;
                 
                 let tx = self.self_sender.clone();
                 let app = self.app_handle.clone();
                 
                 tauri::async_runtime::spawn(async move {
                    run_download_process(next_job, app, tx).await;
                 });
            } else {
                break;
            }
        }
    }

    fn update_native_ui(&self) {
        let active_jobs: Vec<&Job> = self.jobs.values()
            .filter(|j| j.status == JobStatus::Downloading || j.status == JobStatus::Pending)
            .collect();
        
        let active_count = active_jobs.len();

        if active_count == 0 {
            native::clear_taskbar_progress(&self.app_handle);
            return;
        }

        let total_progress: f32 = active_jobs.iter().map(|j| j.progress).sum();
        let aggregated = total_progress / (active_count as f32);
        let has_error = self.jobs.values().any(|j| j.status == JobStatus::Error);

        let app_handle_for_closure = self.app_handle.clone();
        
        let _ = self.app_handle.run_on_main_thread(move || {
            native::set_taskbar_progress(&app_handle_for_closure, (aggregated / 100.0) as f64, has_error);
        });
    }

    fn kill_process(&self, pid: u32) {
        #[cfg(not(target_os = "windows"))]
        {
            use nix::sys::signal::{self, Signal};
            use nix::unistd::Pid;
            // Send SIGTERM to the process group (-pid)
            // This ensures we kill yt-dlp AND its children (ffmpeg)
            let _ = signal::kill(Pid::from_raw(-(pid as i32)), Signal::SIGTERM);
        }

        #[cfg(target_os = "windows")]
        {
            // With Job Objects implemented in process.rs, we just need to kill the parent.
            // But taskkill /T /F is a safe fallback if Job Object failed.
            let mut cmd = std::process::Command::new("taskkill");
            cmd.args(&["/F", "/T", "/PID", &pid.to_string()]);
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); 
            let _ = cmd.spawn();
        }
    }

    fn trigger_finished_notification(&mut self) {
        use tauri::api::notification::Notification;
        let count = self.completed_session_count;
        if count == 0 { return; }

        let _ = Notification::new(self.app_handle.config().tauri.bundle.identifier.clone())
            .title("Downloads Finished")
            .body(format!("Queue processed. {} files handled.", count))
            .icon("icons/128x128.png") 
            .show();

        self.completed_session_count = 0;
    }

    fn clean_temp_directory(&self) {
        if !self.queue.is_empty() || !self.persistence_registry.is_empty() { return; }

        let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let temp_dir = home.join(".multiyt-dlp").join("temp_downloads");
        
        if temp_dir.exists() {
            if let Ok(entries) = fs::read_dir(&temp_dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() { let _ = fs::remove_dir_all(entry.path()); }
                    else { let _ = fs::remove_file(entry.path()); }
                }
            }
        }
    }
}