use std::path::{ PathBuf};
use std::time::{Duration, Instant};
use reqwest::{Client};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncWriteExt};
use futures_util::StreamExt;
use crate::core::transport::retry::{RetryPolicy, TransportError};

// Kill connection if no bytes received for 15 seconds
const IO_TIMEOUT: Duration = Duration::from_secs(15);

pub struct TransportEngine {
    client: Client,
    target_path: PathBuf,
    part_path: PathBuf,
    url: String,
}

impl TransportEngine {
    pub fn new(url: &str, target_path: PathBuf) -> Self {
        // Construct paths
        let mut part_path = target_path.clone();
        
        // Defect Fix #4: Unique Part Filenames
        // We append a timestamp to ensure that if a previous zombie process is holding a lock 
        // on an old .part file, we don't crash or corrupt data by writing to the same one.
        if let Some(file_name) = target_path.file_name() {
            let mut new_name = file_name.to_os_string();
            let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            new_name.push(format!(".{}.part", timestamp));
            part_path.set_file_name(new_name);
        }

        let client = Client::builder()
            .user_agent("Multiyt-dlp/2.1 (Resumable-Engine)")
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            target_path,
            part_path,
            url: url.to_string(),
        }
    }

    /// The Main Execution Loop
    pub async fn execute<F>(&self, on_progress: F) -> Result<(), TransportError> 
    where F: Fn(u64, u64, f64) + Send + Sync + 'static 
    {
        let mut retry_policy = RetryPolicy::new(5);
        let on_progress_ref = &on_progress;

        loop {
            match self.attempt_download(on_progress_ref).await {
                Ok(_) => {
                    self.finalize().await?;
                    return Ok(());
                },
                Err(e) => {
                    // Cleanup partial on failure to avoid disk clutter, 
                    // since we use unique part names and don't support resumption across process restarts for deps.
                    let _ = fs::remove_file(&self.part_path).await;

                    // Fail fast on unrecoverable errors
                    if let TransportError::HttpStatus(404) = e {
                        return Err(e);
                    }

                    println!("[Transport] Download interrupted: {}. Checking retry policy...", e);
                    
                    match retry_policy.next_backoff() {
                        Some(delay) => {
                            println!("[Transport] Retrying in {}ms...", delay.as_millis());
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        None => {
                            return Err(TransportError::MaxRetriesExceeded);
                        }
                    }
                }
            }
        }
    }

    async fn attempt_download<F>(&self, on_progress: &F) -> Result<(), TransportError>
    where F: Fn(u64, u64, f64) + Send + Sync
    {
        // For dependencies, we don't strictly need resume logic if we use unique part files per attempt.
        // Simplified: Start fresh every attempt to avoid corruption from previous zombie writers.
        
        let total_size = self.get_remote_size().await?;
        
        let request = self.client.get(&self.url);
        let response = request.send().await?;
        let status = response.status();

        if !status.is_success() {
             return Err(TransportError::HttpStatus(status.as_u16()));
        }

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.part_path)
            .await?;

        // Streaming Loop with Timeout and Speed Calc
        let mut stream = response.bytes_stream();
        let mut downloaded = 0;
        let mut last_update = Instant::now();
        let mut bytes_since_update = 0;

        loop {
            // "Zombie Stream" Fix: Wrap poll in timeout
            let chunk_result = tokio::time::timeout(IO_TIMEOUT, stream.next()).await;

            match chunk_result {
                Ok(Some(res)) => {
                    let chunk = res?;
                    let len = chunk.len() as u64;
                    file.write_all(&chunk).await?;
                    downloaded += len;
                    bytes_since_update += len;

                    // Update stats every ~500ms
                    if last_update.elapsed().as_millis() >= 500 {
                         let secs = last_update.elapsed().as_secs_f64();
                         let speed = if secs > 0.0 { (bytes_since_update as f64) / secs } else { 0.0 };
                         
                         on_progress(downloaded, total_size, speed);
                         
                         last_update = Instant::now();
                         bytes_since_update = 0;
                    }
                },
                Ok(None) => break, // Stream finished
                Err(_) => {
                    return Err(TransportError::FileSystem(
                        std::io::Error::new(std::io::ErrorKind::TimedOut, "Stream timed out")
                    ));
                }
            }
        }
        
        file.flush().await?;

        if total_size > 0 && downloaded != total_size {
            return Err(TransportError::Validation(format!(
                "Stream ended prematurely. Expected {}, got {}", 
                total_size, downloaded
            )));
        }

        Ok(())
    }

    async fn get_remote_size(&self) -> Result<u64, TransportError> {
        let resp = self.client.head(&self.url).send().await?;
        if resp.status().is_success() {
            if let Some(len) = resp.content_length() {
                return Ok(len);
            }
        }
        Ok(0)
    }

    async fn finalize(&self) -> Result<(), TransportError> {
        // "Windows Access Denied" Fix
        if self.target_path.exists() {
            if let Err(e) = fs::remove_file(&self.target_path).await {
                // Check if PermissionDenied (OS error 5 on Windows, 13 on Unix)
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    println!("[Transport] Target file locked. Attempting rename-swap...");
                    let mut old_path = self.target_path.clone();
                    // Append .old.{timestamp} to avoid collisions
                    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                    let ext = old_path.extension().unwrap_or_default().to_string_lossy();
                    old_path.set_extension(format!("{}.old.{}", ext, timestamp));
                    
                    fs::rename(&self.target_path, &old_path).await?;
                    // We don't delete old_path immediately as it might still be locked. 
                    // It becomes garbage for next cleanup or OS restart.
                } else {
                    return Err(e.into());
                }
            }
        }
        
        fs::rename(&self.part_path, &self.target_path).await?;
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&self.target_path).await?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&self.target_path, perms).await?;
        }

        Ok(())
    }
}