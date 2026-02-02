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
    
    // Add missing fields for UI Sync
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub filename: Option<String>,
    pub phase: Option<String>,
    pub error: Option<String>,
    pub exit_code: Option<i32>,
    pub stderr: Option<String>,
    pub logs: Option<String>,
    pub preset: Option<DownloadFormatPreset>,
    
    #[serde(rename = "videoResolution")]
    pub video_resolution: Option<String>,
    
    #[serde(rename = "downloadPath")]
    pub download_path: Option<String>,
    
    #[serde(rename = "filenameTemplate")]
    pub filename_template: Option<String>,
    
    #[serde(rename = "embedMetadata")]
    pub embed_metadata: Option<bool>,
    
    #[serde(rename = "embedThumbnail")]
    pub embed_thumbnail: Option<bool>,
    
    #[serde(rename = "restrictFilenames")]
    pub restrict_filenames: Option<bool>,
    
    #[serde(rename = "liveFromStart")]
    pub live_from_start: Option<bool>,
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
            speed: None,
            eta: None,
            filename: None,
            phase: None,
            error: None,
            exit_code: None,
            stderr: None,
            logs: None,
            preset: None,
            video_resolution: None,
            download_path: None,
            filename_template: None,
            embed_metadata: None,
            embed_thumbnail: None,
            restrict_filenames: None,
            live_from_start: None,
        }
    }
}

// Used for API response of sync_download_state
#[derive(Serialize)]
pub struct Download {
    pub job_id: Uuid,
    pub url: String,
    pub status: JobStatus,
    pub progress: f32,
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub output_path: Option<String>,
    pub error: Option<String>,
    pub filename: Option<String>,
    pub phase: Option<String>,
    pub exit_code: Option<i32>,
    pub stderr: Option<String>,
    pub logs: Option<String>,
    pub preset: Option<DownloadFormatPreset>,
    
    #[serde(rename = "videoResolution")]
    pub video_resolution: Option<String>,
    
    #[serde(rename = "downloadPath")]
    pub download_path: Option<String>,
    
    #[serde(rename = "filenameTemplate")]
    pub filename_template: Option<String>,
    
    #[serde(rename = "embedMetadata")]
    pub embed_metadata: Option<bool>,
    
    #[serde(rename = "embedThumbnail")]
    pub embed_thumbnail: Option<bool>,
    
    #[serde(rename = "restrictFilenames")]
    pub restrict_filenames: Option<bool>,
    
    #[serde(rename = "liveFromStart")]
    pub live_from_start: Option<bool>,
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
    pub live_from_start: bool,
    
    // Persistence fields added for Defect #1
    pub status: Option<String>,
    pub error: Option<String>,
    pub stderr: Option<String>,
}

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

#[derive(Debug, Serialize)]
pub struct StartDownloadResponse {
    pub job_ids: Vec<Uuid>,
    pub skipped_count: u32,
    pub total_found: u32,
    pub skipped_urls: Vec<String>,
}

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

pub enum JobMessage {
    AddJob { job: QueuedJob, resp: oneshot::Sender<Result<(), String>> },
    CancelJob { id: Uuid },
    UpdateProgress { 
        id: Uuid, 
        percentage: f32, 
        speed: String, 
        eta: String, 
        filename: Option<String>, 
        phase: String 
    },
    ProcessStarted { id: Uuid, pid: u32 },
    JobCompleted { id: Uuid, output_path: String },
    JobError { id: Uuid, payload: DownloadErrorPayload },
    WorkerFinished,
    GetPendingCount(oneshot::Sender<u32>),
    ResumePending(oneshot::Sender<Vec<QueuedJob>>),
    ClearPending,
    SyncState(oneshot::Sender<Vec<Download>>),
    Shutdown(oneshot::Sender<()>),
}