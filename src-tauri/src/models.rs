use serde::{Deserialize, Serialize};
use uuid::Uuid;
use tokio::sync::oneshot;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum JobStatus {
    Pending,
    Downloading,
    Completed,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadFormatPreset {
    Best,
    BestMp4,
    BestMkv,
    BestWebm,
    AudioBest,
    AudioMp3,
    AudioFlac,
    AudioM4a,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: Uuid,
    pub url: String,
    pub pid: Option<u32>,
    pub status: JobStatus,
    pub progress: f32,
    pub output_path: Option<String>,
}

impl Job {
    pub fn new(id: Uuid, url: String) -> Self {
        Self {
            id,
            url,
            pid: None,
            status: JobStatus::Pending,
            progress: 0.0,
            output_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedJob {
    pub id: Uuid,
    pub url: String,
    pub download_path: Option<String>,
    pub format_preset: DownloadFormatPreset,
    pub video_resolution: String,
    pub embed_metadata: bool,
    pub embed_thumbnail: bool,
    pub filename_template: String,
    pub restrict_filenames: bool,
}

// --- Playlist Expansion ---

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaylistResult {
    pub entries: Vec<PlaylistEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlaylistEntry {
    pub id: Option<String>,
    pub url: String,
    pub title: String,
}

// --- Event Payloads ---

#[derive(Clone, serde::Serialize)]
pub struct DownloadProgressPayload {
    #[serde(rename = "jobId")]
    pub job_id: Uuid,
    pub percentage: f32,
    pub speed: String,
    pub eta: String,
    pub filename: Option<String>,
    pub phase: Option<String>,
}

#[derive(Clone, serde::Serialize)]
pub struct BatchProgressPayload {
    pub updates: Vec<DownloadProgressPayload>,
}

#[derive(Clone, serde::Serialize)]
pub struct DownloadCompletePayload {
    #[serde(rename = "jobId")]
    pub job_id: Uuid,
    #[serde(rename = "outputPath")]
    pub output_path: String,
}

#[derive(Clone, serde::Serialize)]
pub struct DownloadCancelledPayload {
    #[serde(rename = "jobId")]
    pub job_id: Uuid,
}

#[derive(Clone, serde::Serialize)]
pub struct DownloadErrorPayload {
    #[serde(rename = "jobId")]
    pub job_id: Uuid,
    pub error: String,
    pub exit_code: Option<i32>,
    pub stderr: String,
    pub logs: String,
}

// --- Actor Messages ---

pub enum JobMessage {
    /// Add a new job to the queue
    AddJob { job: QueuedJob, resp: oneshot::Sender<Result<(), String>> },
    
    /// User requested cancellation
    CancelJob { id: Uuid },

    /// Update status/progress from the process thread
    UpdateProgress { 
        id: Uuid, 
        percentage: f32, 
        speed: String, 
        eta: String, 
        filename: Option<String>, 
        phase: String 
    },

    /// Process started, link PID
    ProcessStarted { id: Uuid, pid: u32 },

    /// Process finished successfully
    JobCompleted { id: Uuid, output_path: String },

    /// Process failed or error occurred
    JobError { id: Uuid, payload: DownloadErrorPayload },

    /// Worker thread finished (cleanup slot)
    WorkerFinished,

    /// Request a snapshot of pending jobs (for persistence check)
    GetPendingCount(oneshot::Sender<u32>),

    /// Request resume of all persistence jobs
    ResumePending(oneshot::Sender<Vec<QueuedJob>>),

    /// Clear persistence
    ClearPending,
}