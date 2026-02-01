pub mod engine;
pub mod retry;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager};
use self::engine::TransportEngine;
use self::retry::TransportError;
use serde::Serialize;

#[derive(Clone, Serialize)]
struct InstallProgressPayload {
    name: String,
    percentage: u64,
    status: String,
}

/// The public entry point for robust file downloading.
pub async fn download_file_robust(
    url: &str,
    destination: PathBuf,
    name: &str,
    app_handle: &AppHandle
) -> Result<(), TransportError> {
    let engine = TransportEngine::new(url, destination);
    let name_clone = name.to_string();
    let app_handle_clone = app_handle.clone();
    
    let last_percentage = AtomicU64::new(0);

    engine.execute(move |downloaded, total, speed| {
        let percentage = if total > 0 { (downloaded * 100) / total } else { 0 };
        let previous = last_percentage.load(Ordering::Relaxed);
        
        // "Blind Update" Fix: Even if total is 0 (unknown), show *some* activity or downloaded MB
        // However, for percentage-based UI, we just cap it or show indeterminate state in UI
        
        if percentage > previous || percentage == 100 || (total == 0 && downloaded % (1024*1024) == 0) {
            last_percentage.store(percentage, Ordering::Relaxed);
            
            let speed_mb = speed / 1_048_576.0;
            let status_msg = if percentage == 100 { 
                "Verifying...".to_string() 
            } else if total == 0 {
                // Chunked encoding UI feedback
                format!("{:.1} MB", downloaded as f64 / 1_048_576.0)
            } else { 
                format!("{:.1} MB/s", speed_mb) 
            };
            
            let _ = app_handle_clone.emit_all("install-progress", InstallProgressPayload {
                name: name_clone.clone(),
                percentage,
                status: status_msg
            });
        }
    }).await
}