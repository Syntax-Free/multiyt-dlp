pub mod engine;
pub mod retry;
pub mod aria;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager};
use self::engine::TransportEngine;
use self::aria::AriaEngine;
use self::retry::TransportError;
use serde::Serialize;
use tracing::{info, warn};

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
    
    // 1. Check if Aria2 is available and if this isn't a download for Aria2 itself
    let app_dir = app_handle.path_resolver().app_data_dir().unwrap();
    let bin_dir = app_dir.join("bin");
    let aria_exe = if cfg!(windows) { "aria2c.exe" } else { "aria2c" };
    let aria_path = bin_dir.join(aria_exe);
    
    let use_aria = aria_path.exists() && name.to_lowercase() != "aria2";

    // Shared state for the progress closure
    let last_percentage = Arc::new(AtomicU64::new(0));
    let name_arc = Arc::new(name.to_string());
    let app_handle_clone = app_handle.clone();
    
    let callback = move |downloaded: u64, total: u64, speed: f64| {
        let percentage = if total > 0 { (downloaded * 100) / total } else { 0 };
        let previous = last_percentage.load(Ordering::Relaxed);
        
        // Update UI if percentage changed, is complete, or periodically for indeterminate progress
        if percentage > previous || percentage == 100 || (total == 0 && downloaded % (1024*1024) == 0) {
            last_percentage.store(percentage, Ordering::Relaxed);
            
            let speed_mb = speed / 1_048_576.0;
            let status_msg = if percentage == 100 { 
                "Verifying...".to_string() 
            } else if total == 0 {
                format!("{:.1} MB", downloaded as f64 / 1_048_576.0)
            } else { 
                format!("{:.1} MB/s", speed_mb) 
            };
            
            let _ = app_handle_clone.emit_all("install-progress", InstallProgressPayload {
                name: name_arc.to_string(),
                percentage,
                status: status_msg
            });
        }
    };

    if use_aria {
        info!("Using Aria2 for download: {}", name);
        let engine = AriaEngine::new(url, destination.clone(), aria_path);
        // Fallback to standard engine if Aria2 fails
        match engine.execute(callback.clone()).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                warn!("Aria2 failed, falling back to internal engine: {}", e);
                // Clean up partial files from aria2 if any
                let _ = std::fs::remove_file(&destination);
            }
        }
    }

    // Native Rust fallback/default
    let engine = TransportEngine::new(url, destination);
    engine.execute(callback).await
}