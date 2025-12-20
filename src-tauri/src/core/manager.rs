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
    DownloadCancelledPayload
};
use crate::config::ConfigManager;
use crate::core::process::run_download_process;
use crate::core::native;

/// The "Handle" is what we pass around in the Tauri state.
/// It sends messages to the running Actor loop.
#[derive(Clone)]
pub struct JobManagerHandle {
    sender: mpsc::Sender<JobMessage>,
}

impl JobManagerHandle {
    pub fn new(app_handle: AppHandle) -> Self {
        // Increased channel capacity from 100 to 1000 to prevent backpressure 
        // during high-frequency log updates from multiple concurrent downloads.
        let (sender, receiver) = mpsc::channel(1000);
        let actor = JobManagerActor::new(app_handle, receiver, sender.clone());
        // FIX: Use tauri::async_runtime::spawn instead of tokio::spawn to ensure runtime context
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
        // Clone the data needed for saving so we can move it into the async block.
        // This prevents blocking the main actor loop with file I/O.
        let jobs: Vec<QueuedJob> = self.persistence_registry.values().cloned().collect();
        
        tauri::async_runtime::spawn(async move {
            if let Ok(json) = serde_json::to_string_pretty(&jobs) {
                 let _ = tokio::fs::write(path, json).await;
            }
        });
    }

    async fn run(mut self) {
        // Tick for UI updates (200ms) to prevent frontend flooding
        let mut interval = time::interval(Duration::from_millis(200));

        loop {
            tokio::select! {
                // 1. Handle Messages
                Some(msg) = self.receiver.recv() => {
                    self.handle_message(msg).await;
                }

                // 2. Batch Emit Tick
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
                    let j = Job::new(job.id, job.url.clone());
                    self.jobs.insert(job.id, j);
                    self.persistence_registry.insert(job.id, job.clone());
                    self.queue.push_back(job);
                    self.save_state();
                    self.process_queue();
                    let _ = resp.send(Ok(()));
                }
            },
            JobMessage::CancelJob { id } => {
                // 1. Kill Process
                if let Some(job) = self.jobs.get(&id) {
                    if let Some(pid) = job.pid {
                        self.kill_process(pid);
                    }
                }
                
                // 2. Update Status internally
                if let Some(job) = self.jobs.get_mut(&id) {
                    job.status = JobStatus::Cancelled;
                }

                // 3. Clean Persistence
                self.persistence_registry.remove(&id);
                self.save_state();

                // 4. Notify Frontend of Cancellation (Distinct from Error)
                let _ = self.app_handle.emit_all("download-cancelled", DownloadCancelledPayload {
                    job_id: id
                });
            },
            JobMessage::ProcessStarted { id, pid } => {
                if let Some(job) = self.jobs.get_mut(&id) {
                    // Double check cancellation race condition
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
                    // If cancelled, stop processing updates
                    if job.status == JobStatus::Cancelled { return; }

                    job.progress = percentage;
                    // We don't emit here. We push to buffer.
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
                    // CRITICAL: If the job was cancelled by the user, IGNORE the error.
                    // The kill command causes the process to exit with a non-zero code,
                    // which triggers this message. We suppress it here.
                    if job.status == JobStatus::Cancelled {
                        return;
                    }
                    job.status = JobStatus::Error;
                }
                // Persistence kept for retry
                let _ = self.app_handle.emit_all("download-error", payload);
            },
            JobMessage::WorkerFinished => {
                if self.active_process_instances > 0 {
                    self.active_process_instances -= 1;
                    self.completed_session_count += 1;
                }
                
                // Release network slot conservatively
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
                                // Re-inject into state
                                if !self.jobs.contains_key(&job.id) {
                                    self.jobs.insert(job.id, Job::new(job.id, job.url.clone()));
                                    self.persistence_registry.insert(job.id, job.clone());
                                    // Important: Queue it!
                                    self.queue.push_back(job.clone());
                                    resumed.push(job);
                                }
                            }
                        }
                    }
                }
                self.process_queue(); // Kickstart
                let _ = tx.send(resumed);
            },
            JobMessage::ClearPending => {
                let path = Self::get_persistence_path();
                if path.exists() { let _ = fs::remove_file(path); }
                self.clean_temp_directory();
            }
        }
    }

    fn flush_updates(&mut self) {
        if self.pending_updates.is_empty() { return; }

        let updates: Vec<DownloadProgressPayload> = self.pending_updates.values().cloned().collect();
        self.pending_updates.clear();

        // Emit Single Batch Event
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
                 
                 // FIX: Use tauri::async_runtime::spawn
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
        #[cfg(not(windows))]
        {
            use nix::sys::signal::{self, Signal};
            use nix::unistd::Pid;
            // Just fire and forget
            let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGINT);
        }

        #[cfg(windows)]
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