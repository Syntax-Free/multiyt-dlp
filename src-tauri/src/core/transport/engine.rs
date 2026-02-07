use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use reqwest::{Client, header};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncWriteExt};
use futures_util::StreamExt;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use crate::core::transport::retry::{RetryPolicy, TransportError};

// Constants
const IO_TIMEOUT: Duration = Duration::from_secs(15);
const CHUNK_THRESHOLD: u64 = 10 * 1024 * 1024; // 10 MB
const DEFAULT_CONCURRENCY: usize = 4;

#[derive(Debug, Clone)]
struct Chunk {
    index: usize,
    start: u64,
    end: u64,
    len: u64,
}

pub struct TransportEngine {
    client: Client,
    url: String,
    target_path: PathBuf,
    concurrency: usize,
    chunk_threshold: u64,
}

impl TransportEngine {
    pub fn new(url: &str, target_path: PathBuf) -> Self {
        let client = Client::builder()
            .user_agent("Multiyt-dlp/2.1 (Resumable-Engine)")
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            url: url.to_string(),
            target_path,
            concurrency: DEFAULT_CONCURRENCY,
            chunk_threshold: CHUNK_THRESHOLD,
        }
    }

    /// The Main Execution Loop
    pub async fn execute<F>(&self, on_progress: F) -> Result<(), TransportError>
    where
        F: Fn(u64, u64, f64) + Send + Sync + 'static + Clone,
    {
        // 1. Probe Phase: Check size and ranges support
        let (content_len, accepts_ranges) = self.probe().await?;

        // 2. Path Decision
        let validated_len = content_len.filter(|&s| s > 0);

        if let Some(total_size) = validated_len {
            if accepts_ranges && total_size >= self.chunk_threshold {
                return self.download_concurrent(total_size, on_progress).await;
            }
        }

        self.download_linear(validated_len, on_progress).await
    }

    async fn probe(&self) -> Result<(Option<u64>, bool), TransportError> {
        let resp = self.client.head(&self.url).send().await?;
        
        if !resp.status().is_success() {
             if resp.status() == reqwest::StatusCode::NOT_FOUND {
                 return Err(TransportError::HttpStatus(resp.status().as_u16()));
             }
             return Ok((None, false));
        }

        let len = resp.content_length();
        let accepts_ranges = if let Some(ranges) = resp.headers().get(header::ACCEPT_RANGES) {
            ranges.to_str().unwrap_or("").contains("bytes")
        } else {
            false
        };

        Ok((len, accepts_ranges))
    }

    fn calculate_deterministic_hash(&self) -> String {
        let mut hasher = DefaultHasher::new();
        self.target_path.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    // --- LINEAR DOWNLOAD ---
    
    async fn download_linear<F>(&self, total_size: Option<u64>, on_progress: F) -> Result<(), TransportError>
    where
        F: Fn(u64, u64, f64) + Send + Sync + 'static,
    {
        // Use hash-based naming to prevent zombies and allow resume attempt in future
        let hash = self.calculate_deterministic_hash();
        let part_path = self.target_path.with_extension(format!("part.linear.{}", hash));

        let mut retry_policy = RetryPolicy::new(5);

        loop {
            match self.attempt_linear(&part_path, total_size, &on_progress).await {
                Ok(_) => {
                    self.finalize(&part_path).await?;
                    return Ok(());
                },
                Err(e) => {
                    // For linear, we can't easily resume without range support logic here
                    // so we just truncate on retry (classic behavior for non-ranged)
                    let _ = fs::remove_file(&part_path).await;

                    if let TransportError::HttpStatus(404) = e {
                        return Err(e);
                    }

                    match retry_policy.next_backoff() {
                        Some(delay) => tokio::time::sleep(delay).await,
                        None => return Err(TransportError::MaxRetriesExceeded),
                    }
                }
            }
        }
    }

    async fn attempt_linear<F>(&self, path: &Path, total_size: Option<u64>, on_progress: &F) -> Result<(), TransportError>
    where F: Fn(u64, u64, f64) + Send + Sync
    {
        let response = self.client.get(&self.url).send().await?;
        if !response.status().is_success() {
             return Err(TransportError::HttpStatus(response.status().as_u16()));
        }

        let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(path).await?;
        let mut stream = response.bytes_stream();
        
        let mut downloaded = 0;
        let mut last_update = Instant::now();
        let mut bytes_since_update = 0;

        while let Some(chunk_result) = tokio::time::timeout(IO_TIMEOUT, stream.next()).await.ok() {
            match chunk_result {
                Some(Ok(chunk)) => {
                    let len = chunk.len() as u64;
                    file.write_all(&chunk).await?;
                    downloaded += len;
                    bytes_since_update += len;

                    if last_update.elapsed().as_millis() >= 500 {
                         let secs = last_update.elapsed().as_secs_f64();
                         let speed = if secs > 0.0 { (bytes_since_update as f64) / secs } else { 0.0 };
                         on_progress(downloaded, total_size.unwrap_or(0), speed);
                         last_update = Instant::now();
                         bytes_since_update = 0;
                    }
                },
                Some(Err(e)) => return Err(TransportError::Network(e)),
                None => break,
            }
        }

        file.flush().await?;

        if let Some(total) = total_size {
            if total > 0 && downloaded != total {
                return Err(TransportError::Validation(format!("Expected {}, got {}", total, downloaded)));
            }
        }

        Ok(())
    }

    // --- CONCURRENT DOWNLOAD ---

    async fn download_concurrent<F>(&self, total_size: u64, on_progress: F) -> Result<(), TransportError>
    where
        F: Fn(u64, u64, f64) + Send + Sync + 'static + Clone,
    {
        let chunk_size = total_size / (self.concurrency as u64);
        let mut chunks = Vec::new();

        for i in 0..self.concurrency {
            let start = i as u64 * chunk_size;
            let end = if i == self.concurrency - 1 {
                total_size - 1
            } else {
                (i as u64 + 1) * chunk_size - 1
            };
            chunks.push(Chunk { index: i, start, end, len: end - start + 1 });
        }

        // Initialize progress tracker with existing file sizes
        let bytes_downloaded = Arc::new(AtomicU64::new(0));
        let hash = self.calculate_deterministic_hash();
        
        let mut initial_progress = 0;
        for i in 0..self.concurrency {
            let p = self.target_path.with_extension(format!("part.{}.{}", hash, i));
            if let Ok(m) = fs::metadata(&p).await {
                initial_progress += m.len();
            }
        }
        bytes_downloaded.store(initial_progress, Ordering::Relaxed);

        let mut tasks = Vec::new();
        let bytes_downloaded_monitor = bytes_downloaded.clone();
        let on_progress_monitor = on_progress.clone();
        
        // Monitor Thread
        let monitor_handle = tokio::spawn(async move {
            let mut last_bytes = 0;
            let mut last_time = Instant::now();
            
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let current = bytes_downloaded_monitor.load(Ordering::Relaxed);
                if current >= total_size { break; }
                
                let now = Instant::now();
                let elapsed = now.duration_since(last_time).as_secs_f64();
                
                let speed = if elapsed > 0.0 {
                    (current.saturating_sub(last_bytes) as f64) / elapsed
                } else { 0.0 };

                on_progress_monitor(current, total_size, speed);
                
                last_bytes = current;
                last_time = now;
            }
        });

        // Worker Threads
        for chunk in chunks {
            let client = self.client.clone();
            let url = self.url.clone();
            let total_bytes_atomic = bytes_downloaded.clone();
            let part_path = self.target_path.with_extension(format!("part.{}.{}", hash, chunk.index));
            
            tasks.push(tokio::spawn(async move {
                let mut retry_policy = RetryPolicy::new(10); // More retries for chunks
                loop {
                    match Self::download_chunk_resumable(&client, &url, &part_path, &chunk, &total_bytes_atomic).await {
                        Ok(_) => return Ok(part_path),
                        Err(e) => {
                            match retry_policy.next_backoff() {
                                Some(delay) => tokio::time::sleep(delay).await,
                                None => return Err(e),
                            }
                        }
                    }
                }
            }));
        }

        let results = futures_util::future::join_all(tasks).await;
        monitor_handle.abort();

        let mut part_paths = Vec::new();
        let mut failed = false;

        for res in results {
            match res {
                Ok(Ok(path)) => part_paths.push(path),
                _ => failed = true,
            }
        }

        if failed {
            // Do NOT cleanup parts on failure, allowing resume next time
            return Err(TransportError::Validation("One or more chunks failed".to_string()));
        }

        // Merge Phase: Append to Part 0 to avoid double disk usage
        match self.merge_parts_optimized(&part_paths).await {
            Ok(_) => {
                on_progress(total_size, total_size, 0.0);
                Ok(())
            },
            Err(e) => Err(e)
        }
    }

    async fn download_chunk_resumable(
        client: &Client,
        url: &str,
        path: &Path,
        chunk: &Chunk,
        global_bytes: &AtomicU64
    ) -> Result<(), TransportError> {
        // Check for existing data
        let mut current_len = 0;
        if path.exists() {
            if let Ok(m) = fs::metadata(path).await {
                current_len = m.len();
            }
        }

        if current_len >= chunk.len {
            // Already downloaded
            return Ok(());
        }

        // Calculate Range
        let range_start = chunk.start + current_len;
        let range_end = chunk.end;

        let req = client.get(url).header(header::RANGE, format!("bytes={}-{}", range_start, range_end));
        let response = req.send().await?;
        
        if !response.status().is_success() {
            return Err(TransportError::HttpStatus(response.status().as_u16()));
        }

        // Open in Append Mode
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(path)
            .await?;
            
        let mut stream = response.bytes_stream();
        let mut downloaded_in_this_session = 0;

        while let Some(chunk_res) = tokio::time::timeout(IO_TIMEOUT, stream.next()).await.ok() {
            match chunk_res {
                Some(Ok(bytes)) => {
                    file.write_all(&bytes).await?;
                    let len = bytes.len() as u64;
                    downloaded_in_this_session += len;
                    global_bytes.fetch_add(len, Ordering::Relaxed);
                },
                Some(Err(e)) => return Err(TransportError::Network(e)),
                None => break,
            }
        }

        file.flush().await?;
        
        let final_len = current_len + downloaded_in_this_session;
        if final_len != chunk.len {
            // Adjust global stats back down if we failed
            global_bytes.fetch_sub(downloaded_in_this_session, Ordering::Relaxed);
            return Err(TransportError::Validation(format!("Chunk incomplete. Got {}, expected {}", final_len, chunk.len)));
        }

        Ok(())
    }

    // Optimized Merge: Append everything to the first part
    async fn merge_parts_optimized(&self, parts: &[PathBuf]) -> Result<(), TransportError> {
        if parts.is_empty() { return Ok(()); }

        let first_part = &parts[0];
        let mut target_file = OpenOptions::new().write(true).append(true).open(first_part).await?;

        // Append subsequent parts
        for part_path in parts.iter().skip(1) {
            let mut part_file = fs::File::open(part_path).await?;
            tokio::io::copy(&mut part_file, &mut target_file).await?;
            // Delete immediately after append to save space
            let _ = fs::remove_file(part_path).await;
        }
        
        target_file.flush().await?;
        self.finalize(first_part).await
    }

    async fn finalize(&self, source_path: &Path) -> Result<(), TransportError> {
        if self.target_path.exists() {
            let _ = fs::remove_file(&self.target_path).await;
        }
        
        fs::rename(source_path, &self.target_path).await?;
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = fs::metadata(&self.target_path).await {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&self.target_path, perms).await;
            }
        }

        Ok(())
    }
}