use std::path::{ PathBuf};
use std::time::{Duration, Instant};
use reqwest::{Client, header};
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
        if let Some(file_name) = target_path.file_name() {
            let mut new_name = file_name.to_os_string();
            new_name.push(".part");
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
        let total_size = self.get_remote_size().await?;
        
        let current_size = if self.part_path.exists() {
            fs::metadata(&self.part_path).await?.len()
        } else {
            0
        };

        // Validate State
        if total_size > 0 && current_size == total_size {
            on_progress(total_size, total_size, 0.0);
            return Ok(());
        }

        if total_size > 0 && current_size > total_size {
            println!("[Transport] Corruption detected. Resetting.");
            fs::remove_file(&self.part_path).await?;
            return Box::pin(self.attempt_download(on_progress)).await;
        }

        // Avoid Resume overhead for tiny missing tails (< 100KB)
        // If we are missing just a tiny bit, it's safer to redownload to ensure integrity hash if we had one.
        // But for now, standard resume logic applies.

        let mut request = self.client.get(&self.url);

        if current_size > 0 {
            println!("[Transport] Resuming from byte {}", current_size);
            request = request.header(header::RANGE, format!("bytes={}-", current_size));
        }

        let response = request.send().await?;
        let status = response.status();

        let mut file;
        let starting_pos;

        if status.is_success() {
            if status == reqwest::StatusCode::PARTIAL_CONTENT {
                file = OpenOptions::new().create(true).append(true).open(&self.part_path).await?;
                starting_pos = current_size;
            } else {
                file = OpenOptions::new().create(true).write(true).truncate(true).open(&self.part_path).await?;
                starting_pos = 0;
            }
        } else {
            if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                 let remote_check = self.get_remote_size().await?;
                 if current_size == remote_check && remote_check > 0 {
                     return Ok(());
                 }
                 fs::remove_file(&self.part_path).await?;
                 return Err(TransportError::Validation("Range not satisfiable but size mismatch".into()));
            }
            return Err(TransportError::HttpStatus(status.as_u16()));
        }

        // Streaming Loop with Timeout and Speed Calc
        let mut stream = response.bytes_stream();
        let mut downloaded = starting_pos;
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
                    // Correctly construct a std::io::Error for the timeout
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