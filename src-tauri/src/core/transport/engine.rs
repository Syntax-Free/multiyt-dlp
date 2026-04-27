use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use reqwest::{Client, header};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter}; 
use futures_util::StreamExt;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tracing::{debug, error, info, trace, warn};
use crate::core::transport::retry::{RetryPolicy, TransportError};

// Constants
const IO_TIMEOUT: Duration = Duration::from_secs(15);
const CHUNK_THRESHOLD: u64 = 10 * 1024 * 1024; // 10 MB
const DEFAULT_CONCURRENCY: usize = 4;
const PROGRESS_INTERVAL_MS: u128 = 100; 

// Kernel-friendly buffer size: 4MB
const IO_BUFFER_SIZE: usize = 4 * 1024 * 1024;

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
    fallback_size: Option<u64>,
    cancel_flag: Arc<AtomicBool>,
}

impl TransportEngine {
    pub fn new(url: &str, target_path: PathBuf, cancel_flag: Arc<AtomicBool>) -> Self {
        trace!(target: "core::transport", "Building HTTP client for Native Transport Engine");
        let client = Client::builder()
            .user_agent("Multiyt-dlp/2.2 (Resumable-Engine)")
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10)) 
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            url: url.to_string(),
            target_path,
            concurrency: DEFAULT_CONCURRENCY,
            chunk_threshold: CHUNK_THRESHOLD,
            fallback_size: Option::None,
            cancel_flag,
        }
    }

    pub fn with_fallback_size(mut self, size: u64) -> Self {
        trace!(target: "core::transport", "Applying fallback total size constraint: {}", size);
        self.fallback_size = Some(size);
        self
    }

    pub async fn execute<F>(&self, on_progress: F) -> Result<(), TransportError>
    where
        F: Fn(u64, u64, f64) + Send + Sync + 'static + Clone,
    {
        info!(target: "core::transport", "Initiating Native Transport Engine execution for URL: {}", self.url);
        let (content_len, accepts_ranges) = self.probe().await?;

        let effective_len = content_len.or(self.fallback_size);
        let validated_len = effective_len.filter(|&s| s > 0);

        if let Some(total_size) = validated_len {
            if accepts_ranges && total_size >= self.chunk_threshold {
                info!(target: "core::transport", "Target supports ranges and size ({} bytes) meets threshold. Dispatching Concurrent Downloader.", total_size);
                return self.download_concurrent(total_size, on_progress).await;
            }
        }

        info!(target: "core::transport", "Target lacks range support or size is below threshold. Dispatching Linear Downloader.");
        self.download_linear(validated_len, on_progress).await
    }

    async fn probe(&self) -> Result<(Option<u64>, bool), TransportError> {
        debug!(target: "core::transport", "Probing target server capabilities using HEAD request...");
        let head_resp = self.client.head(&self.url).send().await;

        let resp = match head_resp {
            Ok(r) if r.status().is_success() => {
                trace!(target: "core::transport", "HEAD request succeeded");
                r
            },
            _ => {
                debug!(target: "core::transport", "HEAD request failed or invalid, falling back to ranged GET request");
                self.client.get(&self.url)
                    .header(header::RANGE, "bytes=0-0")
                    .send()
                    .await?
            }
        };

        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
             if resp.status() == reqwest::StatusCode::NOT_FOUND {
                 error!(target: "core::transport", "Probe encountered 404 NOT FOUND");
                 return Err(TransportError::HttpStatus(resp.status().as_u16()));
             }
             warn!(target: "core::transport", "Probe received non-success HTTP status {}", resp.status());
             return Ok((Option::None, false));
        }

        let len = resp.content_length()
            .or_else(|| {
                resp.headers()
                    .get(header::CONTENT_RANGE)
                    .and_then(|val| val.to_str().ok())
                    .and_then(|s| s.split('/').last())
                    .and_then(|s| s.parse::<u64>().ok())
            });

        let accepts_ranges = if let Some(ranges) = resp.headers().get(header::ACCEPT_RANGES) {
            ranges.to_str().unwrap_or("").contains("bytes")
        } else {
            resp.status() == reqwest::StatusCode::PARTIAL_CONTENT
        };

        debug!(target: "core::transport", "Probe Result: Content Length = {:?}, Accepts Ranges = {}", len, accepts_ranges);
        Ok((len, accepts_ranges))
    }

    fn calculate_deterministic_hash(&self) -> String {
        let mut hasher = DefaultHasher::new();
        self.target_path.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    async fn download_linear<F>(&self, total_size: Option<u64>, on_progress: F) -> Result<(), TransportError>
    where
        F: Fn(u64, u64, f64) + Send + Sync + 'static,
    {
        let hash = self.calculate_deterministic_hash();
        let part_path = self.target_path.with_extension(format!("part.linear.{}", hash));
        trace!(target: "core::transport", "Linear target scratch path: {:?}", part_path);

        let mut retry_policy = RetryPolicy::new(10); // Elevated linear retries

        loop {
            match self.attempt_linear(&part_path, total_size, &on_progress).await {
                Ok(_) => {
                    debug!(target: "core::transport", "Linear download successfully completed");
                    self.finalize(&part_path).await?;
                    if let Some(total) = total_size {
                        on_progress(total, total, 0.0);
                    }
                    return Ok(());
                },
                Err(e) => {
                    error!(target: "core::transport", "Linear download chunk attempt failed: {}", e);
                    let _ = fs::remove_file(&part_path).await;
                    
                    if let TransportError::Cancelled = e { return Err(e); }
                    if let TransportError::HttpStatus(404) = e { return Err(e); }
                    
                    match retry_policy.next_backoff() {
                        Some(delay) => {
                            warn!(target: "core::transport", "Retrying linear download after delay of {:?}", delay);
                            tokio::time::sleep(delay).await;
                        },
                        Option::None => {
                            error!(target: "core::transport", "Maximum linear retries exhausted");
                            return Err(TransportError::MaxRetriesExceeded);
                        }
                    }
                }
            }
        }
    }

    async fn attempt_linear<F>(&self, path: &Path, total_size: Option<u64>, on_progress: &F) -> Result<(), TransportError>
    where F: Fn(u64, u64, f64) + Send + Sync
    {
        trace!(target: "core::transport", "Executing HTTP GET for linear mode");
        let response = self.client.get(&self.url).send().await?;
        if !response.status().is_success() {
             warn!(target: "core::transport", "HTTP status rejection: {}", response.status());
             return Err(TransportError::HttpStatus(response.status().as_u16()));
        }

        let raw_file = OpenOptions::new().create(true).write(true).truncate(true).open(path).await?;
        let mut file = BufWriter::with_capacity(IO_BUFFER_SIZE, raw_file);
        
        let mut stream = response.bytes_stream();
        
        let mut downloaded = 0;
        let mut last_update = Instant::now();
        let mut bytes_since_update = 0;
        
        on_progress(0, total_size.unwrap_or(0), 0.0);

        loop {
            if self.cancel_flag.load(Ordering::Relaxed) {
                return Err(TransportError::Cancelled);
            }

            let chunk_fut = tokio::time::timeout(IO_TIMEOUT, stream.next());
            let sleep_fut = tokio::time::sleep(Duration::from_millis(500));

            tokio::select! {
                chunk_result = chunk_fut => {
                    match chunk_result {
                        Ok(Some(Ok(chunk))) => {
                            let len = chunk.len() as u64;
                            trace!(target: "core::transport", "Writing {} bytes to linear output buffer", len);
                            file.write_all(&chunk).await?;
                            downloaded += len;
                            bytes_since_update += len;

                            if last_update.elapsed().as_millis() >= PROGRESS_INTERVAL_MS {
                                 let secs = last_update.elapsed().as_secs_f64();
                                 let speed = if secs > 0.0 { (bytes_since_update as f64) / secs } else { 0.0 };
                                 on_progress(downloaded, total_size.unwrap_or(0), speed);
                                 last_update = Instant::now();
                                 bytes_since_update = 0;
                            }
                        },
                        Ok(Some(Err(e))) => {
                            error!(target: "core::transport", "Network stream error during linear read: {}", e);
                            return Err(TransportError::Network(e))
                        },
                        Ok(None) => break,
                        Err(_) => {
                            error!(target: "core::transport", "Network stream read timed out");
                            return Err(TransportError::Validation("Connection timed out".into()));
                        }
                    }
                }
                _ = sleep_fut => {
                    // Just unblocks to re-check cancel_flag loop condition
                }
            }
        }

        trace!(target: "core::transport", "Flushing I/O buffer to disk");
        file.flush().await?;

        if let Some(total) = total_size {
            if total > 0 && downloaded != total {
                error!(target: "core::transport", "Linear byte mismatch. Expected {}, got {}", total, downloaded);
                return Err(TransportError::Validation(format!("Expected {}, got {}", total, downloaded)));
            }
        }

        Ok(())
    }

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
            trace!(target: "core::transport", "Defined Chunk {}: Start={}, End={}, Length={}", i, start, end, end - start + 1);
        }

        let bytes_downloaded = Arc::new(AtomicU64::new(0));
        let hash = self.calculate_deterministic_hash();
        
        let mut initial_progress = 0;
        for i in 0..self.concurrency {
            let p = self.target_path.with_extension(format!("part.{}.{}", hash, i));
            if let Ok(m) = fs::metadata(&p).await {
                initial_progress += m.len();
                debug!(target: "core::transport", "Resuming Chunk {} from offset {}", i, m.len());
            }
        }
        bytes_downloaded.store(initial_progress, Ordering::Relaxed);

        let mut tasks = Vec::new();
        let bytes_downloaded_monitor = bytes_downloaded.clone();
        let on_progress_monitor = on_progress.clone();
        let cancel_flag_monitor = self.cancel_flag.clone();
        
        on_progress(initial_progress, total_size, 0.0);

        let monitor_handle = tokio::spawn(async move {
            let mut last_bytes = initial_progress;
            let mut last_time = Instant::now();
            
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if cancel_flag_monitor.load(Ordering::Relaxed) { break; }

                let current = bytes_downloaded_monitor.load(Ordering::Relaxed);
                
                let now = Instant::now();
                let elapsed = now.duration_since(last_time).as_secs_f64();
                
                let speed = if elapsed > 0.0 {
                    (current.saturating_sub(last_bytes) as f64) / elapsed
                } else { 0.0 };

                on_progress_monitor(current, total_size, speed);
                
                last_bytes = current;
                last_time = now;
                
                if current >= total_size { break; }
            }
        });

        for chunk in chunks {
            let client = self.client.clone();
            let url = self.url.clone();
            let total_bytes_atomic = bytes_downloaded.clone();
            let part_path = self.target_path.with_extension(format!("part.{}.{}", hash, chunk.index));
            let cancel_flag_task = self.cancel_flag.clone();
            
            tasks.push(tokio::spawn(async move {
                let mut retry_policy = RetryPolicy::new(15); // Elevated chunk retries
                loop {
                    match Self::download_chunk_resumable(&client, &url, &part_path, &chunk, &total_bytes_atomic, &cancel_flag_task).await {
                        Ok(_) => {
                            debug!(target: "core::transport", "Chunk {} completed successfully", chunk.index);
                            return Ok(part_path)
                        },
                        Err(e) => {
                            error!(target: "core::transport", "Chunk {} failed with error: {}", chunk.index, e);
                            if let TransportError::Cancelled = e { return Err(e); }
                            
                            match retry_policy.next_backoff() {
                                Some(delay) => {
                                    warn!(target: "core::transport", "Retrying Chunk {} after delay of {:?}", chunk.index, delay);
                                    tokio::time::sleep(delay).await;
                                },
                                Option::None => {
                                    error!(target: "core::transport", "Maximum retries exhausted for Chunk {}", chunk.index);
                                    return Err(e);
                                }
                            }
                        }
                    }
                }
            }));
        }

        let results = futures_util::future::join_all(tasks).await;
        let _ = monitor_handle.await; 

        let mut part_paths = Vec::new();
        let mut failed = false;
        let mut cancelled = false;

        for res in results {
            match res {
                Ok(Ok(path)) => part_paths.push(path),
                Ok(Err(TransportError::Cancelled)) => cancelled = true,
                _ => failed = true,
            }
        }

        if cancelled || self.cancel_flag.load(Ordering::Relaxed) {
            error!(target: "core::transport", "Concurrent download aborted due to cancellation");
            for p in &part_paths {
                let _ = fs::remove_file(p).await;
            }
            for i in 0..self.concurrency {
                let p = self.target_path.with_extension(format!("part.{}.{}", hash, i));
                let _ = fs::remove_file(p).await;
            }
            return Err(TransportError::Cancelled);
        }

        if failed {
            error!(target: "core::transport", "Concurrent download aborted due to chunk failures");
            return Err(TransportError::Validation("One or more chunks failed".to_string()));
        }

        info!(target: "core::transport", "All chunks complete. Merging parts.");
        match self.merge_parts_optimized(&part_paths).await {
            Ok(_) => {
                debug!(target: "core::transport", "Merge complete");
                on_progress(total_size, total_size, 0.0); 
                Ok(())
            },
            Err(e) => {
                error!(target: "core::transport", "Merge failed: {}", e);
                Err(e)
            }
        }
    }

    async fn download_chunk_resumable(
        client: &Client,
        url: &str,
        path: &Path,
        chunk: &Chunk,
        global_bytes: &AtomicU64,
        cancel_flag: &AtomicBool
    ) -> Result<(), TransportError> {
        let mut current_len = 0;
        if path.exists() {
            if let Ok(m) = fs::metadata(path).await {
                current_len = m.len();
            }
        }

        if current_len >= chunk.len {
            trace!(target: "core::transport", "Chunk {} already strictly complete, skipping network.", chunk.index);
            return Ok(());
        }

        let range_start = chunk.start + current_len;
        let range_end = chunk.end;

        trace!(target: "core::transport", "Chunk {} requesting HTTP RANGE bytes={}-{}", chunk.index, range_start, range_end);
        let req = client.get(url).header(header::RANGE, format!("bytes={}-{}", range_start, range_end));
        let response = req.send().await?;
        
        if !response.status().is_success() {
            return Err(TransportError::HttpStatus(response.status().as_u16()));
        }

        let raw_file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(path)
            .await?;
            
        let mut file = BufWriter::with_capacity(IO_BUFFER_SIZE, raw_file);

        let mut stream = response.bytes_stream();
        let mut downloaded_in_this_session = 0;
        let remaining_for_chunk = chunk.len.saturating_sub(current_len);

        loop {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(TransportError::Cancelled);
            }

            let chunk_fut = tokio::time::timeout(IO_TIMEOUT, stream.next());
            let sleep_fut = tokio::time::sleep(Duration::from_millis(500));

            tokio::select! {
                chunk_res = chunk_fut => {
                    match chunk_res {
                        Ok(Some(Ok(bytes))) => {
                            let len = bytes.len() as u64;
                            if downloaded_in_this_session + len > remaining_for_chunk {
                                global_bytes.fetch_sub(downloaded_in_this_session, Ordering::Relaxed);
                                error!(target: "core::transport", "Chunk {} received out-of-bounds bytes from server", chunk.index);
                                return Err(TransportError::Validation("Server exceeded requested byte range".into()));
                            }
                            trace!(target: "core::transport", "Chunk {} writing {} bytes", chunk.index, len);
                            file.write_all(&bytes).await?;
                            downloaded_in_this_session += len;
                            global_bytes.fetch_add(len, Ordering::Relaxed);
                        },
                        Ok(Some(Err(e))) => return Err(TransportError::Network(e)),
                        Ok(None) => break,
                        Err(_) => {
                            error!(target: "core::transport", "Chunk stream read timed out");
                            return Err(TransportError::Validation("Connection timed out".into()));
                        }
                    }
                }
                _ = sleep_fut => {
                    // Unblock to recheck cancel_flag
                }
            }
        }

        file.flush().await?;
        
        let final_len = current_len + downloaded_in_this_session;
        if final_len != chunk.len {
            global_bytes.fetch_sub(downloaded_in_this_session, Ordering::Relaxed);
            return Err(TransportError::Validation(format!("Chunk {} incomplete. Got {}, expected {}", chunk.index, final_len, chunk.len)));
        }

        Ok(())
    }

    async fn merge_parts_optimized(&self, parts: &[PathBuf]) -> Result<(), TransportError> {
        if parts.is_empty() { return Ok(()); }
        
        let hash = self.calculate_deterministic_hash();
        let final_tmp_path = self.target_path.with_extension(format!("final.{}", hash));
        
        if final_tmp_path.exists() {
            let _ = fs::remove_file(&final_tmp_path).await;
        }

        // Clone parts for the blocking closure – they are needed by value.
        let parts_clone = parts.to_vec();
        let final_tmp_path_clone = final_tmp_path.clone();

        // Offload the heavy merge to a blocking thread to leverage kernel‑space copy.
        tokio::task::spawn_blocking(move || -> Result<(), TransportError> {
            use std::io::Write;
            
            let mut target_file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&final_tmp_path_clone)?;

            for part in &parts_clone {
                let mut source_file = std::fs::File::open(part)?;
                // std::io::copy uses OS‑specific optimisations (e.g. sendfile on Linux)
                std::io::copy(&mut source_file, &mut target_file)?;
            }
            
            target_file.flush()?;
            Ok(())
        })
        .await
        .map_err(|_| TransportError::Validation("Blocking merge task panicked".into()))??;

        // Clean up the chunk parts after successful merge.
        for part in parts {
            let _ = fs::remove_file(part).await;
        }
        
        self.finalize(&final_tmp_path).await
    }

    async fn finalize(&self, source_path: &Path) -> Result<(), TransportError> {
        debug!(target: "core::transport", "Finalizing TransportEngine payload to destination: {:?}", self.target_path);
        crate::core::deps::replace_dependency_robust_sync(source_path, &self.target_path).map_err(TransportError::FileSystem)?;
        Ok(())
    }
}