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
    app_handle: &AppHandle,
    fallback_size: Option<u64>
) -> Result<(), TransportError> {
    
    let name_arc = Arc::new(name.to_string());
    let app_handle_clone = app_handle.clone();
    
    // OPTIMISTIC UPDATE: Emit initializing immediately
    let _ = app_handle_clone.emit_all("install-progress", InstallProgressPayload {
        name: name_arc.to_string(),
        percentage: 0,
        status: "Initializing...".to_string()
    });

    // 1. Check if Aria2 is available and if this isn't a download for Aria2 itself
    let bin_dir = crate::core::deps::get_common_bin_dir();
    let aria_exe = if cfg!(windows) { "aria2c.exe" } else { "aria2c" };
    let aria_path = bin_dir.join(aria_exe);
    
    let use_aria = aria_path.exists() && name.to_lowercase() != "aria2";

    // Shared state for the progress closure
    let last_percentage = Arc::new(AtomicU64::new(0));
    
    let callback = move |downloaded: u64, total: u64, speed: f64| {
        // Fallback Logic: If engine reports 0 total, use fallback if available
        let effective_total = if total == 0 {
             fallback_size.unwrap_or(0)
        } else {
             total
        };

        let percentage = if effective_total > 0 { (downloaded * 100) / effective_total } else { 0 };
        let previous = last_percentage.load(Ordering::Relaxed);
        
        // Update UI if percentage changed, is complete, or periodically for indeterminate progress
        if percentage > previous || percentage == 100 || (effective_total == 0 && downloaded % (1024*1024) == 0) {
            last_percentage.store(percentage, Ordering::Relaxed);
            
            let speed_mb = speed / 1_048_576.0;
            let status_msg = if percentage == 100 { 
                "Verifying...".to_string() 
            } else if effective_total == 0 {
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
        // Pass fallback_size to AriaEngine
        let engine = AriaEngine::new(url, destination.clone(), aria_path, fallback_size);
        
        // Fallback to standard engine if Aria2 fails
        match engine.execute(callback.clone()).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                warn!("Aria2 failed, falling back to internal engine: {}", e);
                // Clean up partial files from aria2 if any
                let _ = std::fs::remove_file(&destination);
            }
        }
    } else {
        // NATIVE ENGINE STRATEGY: Indeterminate Loading State
        // We explicitly tell frontend we are in native mode by keeping percentage at 0 
        // and setting a specific status text that the frontend looks for to trigger spinner mode.
        let _ = app_handle.emit_all("install-progress", InstallProgressPayload {
            name: name.to_string(),
            percentage: 0,
            status: "Downloading (Native)...".to_string()
        });

        // Use a dummy callback that basically ignores updates to prevent "broken" progress bars
        // The transport engine will still do its job, but we won't stream granular updates.
        let dummy_callback = |_: u64, _: u64, _: f64| {};
        
        let mut engine = TransportEngine::new(url, destination);
        if let Some(s) = fallback_size {
            engine = engine.with_fallback_size(s);
        }
        
        // Execute with the dummy callback
        return engine.execute(dummy_callback).await;
    }

    Ok(())
}