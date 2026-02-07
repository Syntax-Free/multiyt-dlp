use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, Duration};
use tauri::{AppHandle, Manager};
use uuid::Uuid;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn, error, debug};

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
    
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        let _ = self.sender.send(JobMessage::Shutdown(tx)).await;
        let _ = rx.await;
    }
}

enum PersistenceMsg {
    Save(Vec<QueuedJob>),
    Clear
}

struct JobManagerActor {
    app_handle: AppHandle,
    receiver: mpsc::Receiver<JobMessage>,
    self_sender: mpsc::Sender<JobMessage>,

    jobs: HashMap<Uuid, Job>,
    queue: VecDeque<QueuedJob>,
    persistence_registry: HashMap<Uuid, QueuedJob>,
    persistence_tx: mpsc::Sender<PersistenceMsg>,
    
    // Dirty flag for coalesced persistence updates
    dirty_persistence: bool,

    active_network_jobs: u32,
    active_process_instances: u32,
    completed_session_count: u32,

    pending_updates: HashMap<Uuid, DownloadProgressPayload>,
}

impl JobManagerActor {
    fn new(app_handle: AppHandle, receiver: mpsc::Receiver<JobMessage>, self_sender: mpsc::Sender<JobMessage>) -> Self {
        
        let (ptx, mut prx) = mpsc::channel(100);
        tauri::async_runtime::spawn(async move {
            let path = Self::get_persistence_path();
            while let Some(msg) = prx.recv().await {
                match msg {
                    PersistenceMsg::Save(jobs) => {
                        if let Ok(json) = serde_json::to_string_pretty(&jobs) {
                            let tmp_path = path.with_extension("tmp");
                            if tokio::fs::write(&tmp_path, json).await.is_ok() {
                                let _ = tokio::fs::rename(tmp_path, &path).await;
                            } else {
                                warn!(target: "core::persistence", "Failed to write persistence file");
                            }
                        }
                    },
                    PersistenceMsg::Clear => {
                        if path.exists() { 
                            let _ = tokio::fs::remove_file(&path).await; 
                        }
                    }
                }
            }
        });

        Self {
            app_handle,
            receiver,
            self_sender,
            jobs: HashMap::new(),
            queue: VecDeque::new(),
            persistence_registry: HashMap::new(),
            persistence_tx: ptx,
            dirty_persistence: false,
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

    fn mark_dirty(&mut self) {
        self.dirty_persistence = true;
    }

    async fn run(mut self) {
        info!(target: "core::manager", "JobManagerActor started");
        let mut interval = time::interval(Duration::from_millis(100));

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    if let JobMessage::Shutdown(tx) = msg {
                        info!(target: "core::manager", "Shutdown requested");
                        self.handle_shutdown().await;
                        let _ = tx.send(());
                        break;
                    }
                    self.handle_message(msg).await;
                }
                _ = interval.tick() => {
                    self.flush_updates();
                    self.update_native_ui();
                    
                    if self.dirty_persistence {
                        let jobs: Vec<QueuedJob> = self.persistence_registry.values().cloned().collect();
                        if let Ok(_) = self.persistence_tx.try_send(PersistenceMsg::Save(jobs)) {
                            self.dirty_persistence = false;
                        }
                    }
                }
            }
        }
    }

    async fn handle_shutdown(&mut self) {
        let pids: Vec<u32> = self.jobs.values()
            .filter_map(|j| j.pid)
            .collect();
        
        info!(target: "core::manager", "Killing {} active processes", pids.len());

        for pid in pids {
            self.kill_process(pid);
        }

        // Wait for workers to drop with a timeout
        let deadline = time::Instant::now() + Duration::from_secs(3);
        while self.active_process_instances > 0 {
             if time::Instant::now() > deadline {
                 warn!("Shutdown timeout reached with {} active processes", self.active_process_instances);
                 break;
             }
             if let Ok(msg) = self.receiver.try_recv() {
                 if matches!(msg, JobMessage::WorkerFinished) {
                     self.handle_message(msg).await;
                 }
             }
             time::sleep(Duration::from_millis(50)).await;
        }

        self.clean_temp_directory().await;
    }

    fn is_fatal_error(err_msg: &str) -> bool {
        let msg = err_msg.to_lowercase();
        msg.contains("video unavailable") || 
        msg.contains("this video has been removed") ||
        (msg.contains("fragment") && msg.contains("not received")) ||
        msg.contains("http error 404")
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
                        info!(target: "core::manager", job_id = ?job.id, url = %job.url, "Job added to queue");
                        
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
                        self.mark_dirty();
                        self.process_queue();
                        let _ = resp.send(Ok(()));
                    }
                }
            },
            JobMessage::CancelJob { id } => {
                info!(target: "core::manager", job_id = ?id, "Cancelling job");
                if let Some(job) = self.jobs.get(&id) {
                    if let Some(pid) = job.pid {
                        self.kill_process(pid);
                    }
                }
                if let Some(job) = self.jobs.get_mut(&id) {
                    job.status = JobStatus::Cancelled;
                    job.sequence_id += 1; // Increment Seq
                }
                self.persistence_registry.remove(&id);
                self.mark_dirty();

                let _ = self.app_handle.emit_all("download-cancelled", DownloadCancelledPayload {
                    job_id: id
                });
            },
            JobMessage::ProcessStarted { id, pid } => {
                debug!(target: "core::manager", job_id = ?id, pid = pid, "Process started");
                if let Some(job) = self.jobs.get_mut(&id) {
                    if job.status == JobStatus::Cancelled {
                        self.kill_process(pid);
                    } else {
                        job.pid = Some(pid);
                        job.status = JobStatus::Downloading;
                        job.sequence_id += 1; // Increment Seq
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
                    job.sequence_id += 1; // Increment Seq

                    self.pending_updates.insert(id, DownloadProgressPayload {
                        job_id: id,
                        percentage,
                        sequence_id: job.sequence_id, // Pass to payload
                        speed,
                        eta,
                        filename,
                        phase: Some(phase)
                    });
                }
            },
            JobMessage::JobCompleted { id, output_path } => {
                info!(target: "core::manager", job_id = ?id, path = %output_path, "Job completed");
                if let Some(job) = self.jobs.get_mut(&id) {
                    if job.status == JobStatus::Cancelled { return; }
                    job.status = JobStatus::Completed;
                    job.progress = 100.0;
                    job.output_path = Some(output_path.clone());
                    job.phase = Some("Done".to_string());
                    job.sequence_id += 1;
                }
                self.persistence_registry.remove(&id);
                self.mark_dirty();

                let _ = self.app_handle.emit_all("download-complete", DownloadCompletePayload {
                    job_id: id,
                    output_path,
                });
            },
            JobMessage::JobError { id, payload } => {
                error!(target: "core::manager", job_id = ?id, error = %payload.error, "Job failed");
                if let Some(job) = self.jobs.get_mut(&id) {
                    if job.status == JobStatus::Cancelled { return; }
                    job.status = JobStatus::Error;
                    job.error = Some(payload.error.clone());
                    job.stderr = Some(payload.stderr.clone());
                    job.logs = Some(payload.logs.clone());
                    job.exit_code = payload.exit_code;
                    job.sequence_id += 1;
                }
                
                if Self::is_fatal_error(&payload.error) || Self::is_fatal_error(&payload.stderr) {
                    self.persistence_registry.remove(&id);
                } else {
                    if let Some(reg_entry) = self.persistence_registry.get_mut(&id) {
                        reg_entry.status = Some("error".to_string());
                        reg_entry.error = Some(payload.error.clone());
                        reg_entry.stderr = Some(payload.stderr.clone());
                    }
                }
                self.mark_dirty();

                let _ = self.app_handle.emit_all("download-error", payload);
            },
            JobMessage::WorkerFinished => {
                debug!(target: "core::manager", "Worker finished signal received");
                if self.active_process_instances > 0 {
                    self.active_process_instances -= 1;
                    self.completed_session_count += 1;
                }
                if self.active_network_jobs > 0 {
                    self.active_network_jobs -= 1;
                }
                if self.active_process_instances == 0 {
                    self.trigger_finished_notification();
                    self.clean_temp_directory().await;
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
                info!(target: "core::manager", "Resuming pending jobs from disk");
                let path = Self::get_persistence_path();
                let mut resumed = Vec::new();
                if path.exists() {
                    if let Ok(content) = fs::read_to_string(path) {
                        if let Ok(jobs) = serde_json::from_str::<Vec<QueuedJob>>(&content) {
                            for job in jobs {
                                if !self.jobs.contains_key(&job.id) {
                                    let mut j = Job::new(job.id, job.url.clone());
                                    j.preset = Some(job.format_preset.clone());
                                    j.video_resolution = Some(job.video_resolution.clone());
                                    j.download_path = job.download_path.clone();
                                    j.filename_template = Some(job.filename_template.clone());
                                    j.embed_metadata = Some(job.embed_metadata);
                                    j.embed_thumbnail = Some(job.embed_thumbnail);
                                    j.restrict_filenames = Some(job.restrict_filenames);
                                    j.live_from_start = Some(job.live_from_start);
                                    
                                    if let Some(st) = &job.status {
                                        if st == "error" {
                                            j.status = JobStatus::Error;
                                            j.error = job.error.clone();
                                            j.stderr = job.stderr.clone();
                                        }
                                    }

                                    self.jobs.insert(job.id, j.clone());
                                    self.persistence_registry.insert(job.id, job.clone());
                                    
                                    if j.status != JobStatus::Error {
                                        self.queue.push_back(job.clone());
                                    }
                                    
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
                info!(target: "core::manager", "Clearing pending jobs");
                let _ = self.persistence_tx.try_send(PersistenceMsg::Clear);
                self.clean_temp_directory().await;
            },
            JobMessage::SyncState(tx) => {
                let mut downloads: Vec<Download> = Vec::new();
                for job in self.jobs.values() {
                    downloads.push(Download {
                        job_id: job.id,
                        url: job.url.clone(),
                        status: job.status.clone(),
                        progress: job.progress,
                        sequence_id: job.sequence_id, // Pass to sync
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
            },
            JobMessage::Shutdown(_) => {}
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

                 debug!(target: "core::manager", job_id = ?next_job.id, "Spawning worker for job");
                 
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
        debug!(target: "core::manager", pid = pid, "Terminating process");
        #[cfg(not(target_os = "windows"))]
        {
            use nix::sys::signal::{self, Signal};
            use nix::unistd::Pid;
            let _ = signal::kill(Pid::from_raw(-(pid as i32)), Signal::SIGTERM);
        }

        #[cfg(target_os = "windows")]
        {
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

    async fn clean_temp_directory(&self) {
        if !self.queue.is_empty() || !self.persistence_registry.is_empty() { return; }

        debug!(target: "core::manager", "Cleaning temp directory");
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let temp_dir = home.join(".multiyt-dlp").join("temp_downloads");
        
        if temp_dir.exists() {
            // Helper for robust recursive deletion
            async fn robust_remove_dir(path: &Path) -> std::io::Result<()> {
                for i in 0..5 {
                    match fs::remove_dir_all(path) {
                        Ok(_) => return Ok(()),
                        Err(_) => {
                            time::sleep(Duration::from_millis(100 * 2u64.pow(i))).await;
                        }
                    }
                }
                fs::remove_dir_all(path)
            }

            if let Ok(entries) = fs::read_dir(&temp_dir) {
                for entry in entries.flatten() {
                     let path = entry.path();
                     if path.is_dir() {
                         let _ = robust_remove_dir(&path).await;
                     } else {
                         let _ = fs::remove_file(&path);
                     }
                }
            }
        }
    }
}