use std::fs;
use std::path::PathBuf;
use tracing::{info};
use tracing_subscriber::{
    fmt, 
    prelude::*, 
    reload, 
    Registry, 
    EnvFilter
};
use tracing_appender::non_blocking::WorkerGuard;
use chrono::Local;

// --- Structs ---

pub type LogHandle = reload::Handle<EnvFilter, Registry>;

pub struct LogManager {
    // We must keep the guard alive, otherwise file logging stops immediately
    _guard: WorkerGuard,
    // The handle allows us to swap the filter (log level) at runtime
    reload_handle: LogHandle,
}

pub struct LogPaths {
    pub log_dir: PathBuf,
    pub latest_log: PathBuf,
    pub archive_dir: PathBuf,
}

impl LogPaths {
    pub fn new() -> Option<Self> {
        let home = dirs::home_dir()?;
        let log_dir = home.join(".multiyt-dlp").join("logs");
        let latest_log = log_dir.join("latest.log");
        let archive_dir = log_dir.join("archive");
        
        Some(Self {
            log_dir,
            latest_log,
            archive_dir,
        })
    }
}

// --- Rotation Logic ---

/// Rotates 'latest.log' to 'archive/...' and cleans up old files.
/// This must be called BEFORE LogManager::init to ensure the file isn't locked.
pub fn rotate_logs() -> Result<(), String> {
    let paths = LogPaths::new().ok_or("Could not determine home directory")?;

    // 1. Ensure directories exist
    if !paths.log_dir.exists() {
        fs::create_dir_all(&paths.log_dir).map_err(|e| e.to_string())?;
    }
    if !paths.archive_dir.exists() {
        fs::create_dir_all(&paths.archive_dir).map_err(|e| e.to_string())?;
    }

    // 2. Rotate latest.log if it exists
    if paths.latest_log.exists() {
        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
        let archive_name = format!("app-{}.log", timestamp);
        let archive_path = paths.archive_dir.join(archive_name);

        // Attempt rename
        if let Err(e) = fs::rename(&paths.latest_log, &archive_path) {
            eprintln!("Failed to rotate log file: {}", e);
            // If rename fails (e.g. cross-device link), try copy-delete
            if let Err(copy_err) = fs::copy(&paths.latest_log, &archive_path) {
                 eprintln!("Failed to copy log file to archive: {}", copy_err);
            } else {
                 let _ = fs::remove_file(&paths.latest_log);
            }
        }
    }

    // 3. Cleanup old archives (Keep last 10)
    cleanup_archives(&paths.archive_dir).map_err(|e| format!("Cleanup failed: {}", e))?;

    Ok(())
}

fn cleanup_archives(archive_dir: &PathBuf) -> std::io::Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(archive_dir)?
        .filter_map(|res| res.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();

    // Sort by modification time (Newest first)
    entries.sort_by(|a, b| {
        let meta_a = fs::metadata(a).and_then(|m| m.modified());
        let meta_b = fs::metadata(b).and_then(|m| m.modified());
        
        // If we can't get metadata, rely on name (which has timestamp)
        // Reverse sort because we want newest (largest timestamp/time) first
        match (meta_a, meta_b) {
            (Ok(time_a), Ok(time_b)) => time_b.cmp(&time_a), 
            _ => b.cmp(a), // Fallback to filename reverse sort
        }
    });

    // Keep top 10, delete the rest
    if entries.len() > 10 {
        for path in entries.iter().skip(10) {
            if let Err(e) = fs::remove_file(path) {
                eprintln!("Failed to delete old log {:?}: {}", path, e);
            }
        }
    }

    Ok(())
}

// --- Manager Implementation ---

impl LogManager {
    pub fn init(log_level: &str) -> Self {
        // 1. Get Paths
        let paths = LogPaths::new().expect("Could not determine log paths during init");
        
        // 2. Create/Truncate "latest.log"
        // Since we rotated beforehand, this creates a fresh file. 
        // If rotation failed, this overwrites/truncates the existing mess, effectively restarting the log.
        let file = std::fs::File::create(&paths.latest_log).expect("Failed to create latest.log");

        // 3. Non-blocking Writer
        let (non_blocking, guard) = tracing_appender::non_blocking(file);

        // 4. Layers
        
        // Layer A: JSON File Output
        let file_layer = fmt::layer()
            .json()
            .with_writer(non_blocking)
            .with_target(true)
            .with_file(true)
            .with_line_number(true);

        // Layer B: Pretty Console Output
        let stdout_layer = fmt::layer()
            .pretty()
            .with_writer(std::io::stdout);

        // 5. Filter (Reloadable)
        let filter_str = Self::get_filter_string(log_level);
        let initial_filter = EnvFilter::try_new(&filter_str)
            .unwrap_or_else(|_| EnvFilter::new(Self::get_filter_string("info")));
            
        let (filter_layer, reload_handle) = reload::Layer::new(initial_filter);

        // 6. Registry Construction
        tracing_subscriber::registry()
            .with(filter_layer)
            .with(file_layer)
            .with(stdout_layer)
            .init();

        info!("Logging initialized. Writing to: {:?}", paths.latest_log);
        
        Self {
            _guard: guard,
            reload_handle,
        }
    }

    pub fn set_level(&self, level: &str) -> Result<(), String> {
        let filter_str = Self::get_filter_string(level);
        let new_filter = EnvFilter::try_new(&filter_str)
            .map_err(|e| format!("Invalid log level '{}': {}", filter_str, e))?;
        
        self.reload_handle.reload(new_filter)
            .map_err(|e| format!("Failed to reload log level: {}", e))?;
            
        info!("Log level dynamically changed to: {}", level);
        Ok(())
    }

    fn get_filter_string(level: &str) -> String {
        // Silence noisy libraries
        format!("{},tao=error,wry=error,hyper=error", level)
    }
}