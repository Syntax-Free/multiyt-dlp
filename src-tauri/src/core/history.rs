use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, oneshot};
use tokio::fs::{OpenOptions, File};
use tokio::io::{AsyncWriteExt, BufWriter};
use url::Url;
use tracing::{debug, error, info, trace, warn};

#[derive(Debug)]
enum HistoryMessage {
    Add(String),
    Replace(String, oneshot::Sender<Result<(), String>>),
    Clear(oneshot::Sender<Result<(), String>>),
    Get(oneshot::Sender<String>),
}

#[derive(Clone)]
pub struct HistoryManager {
    cache: Arc<RwLock<HashSet<String>>>,
    sender: mpsc::Sender<HistoryMessage>,
}

impl HistoryManager {
    pub fn new() -> Self {
        info!(target: "core::history", "Initializing HistoryManager");
        let home = dirs::home_dir().expect("Could not find home directory");
        let file_path = home.join(".multiyt-dlp").join("downloads.txt");

        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                trace!(target: "core::history", "Creating history directory at {:?}", parent);
                let _ = std::fs::create_dir_all(parent);
            }
        }

        let cache = Arc::new(RwLock::new(HashSet::new()));
        
        if file_path.exists() {
             debug!(target: "core::history", "Loading existing history from {:?}", file_path);
             if let Ok(file) = std::fs::File::open(&file_path) {
                let reader = std::io::BufReader::new(file);
                let mut c = cache.write().unwrap();
                use std::io::BufRead;
                let mut count = 0;
                for line in reader.lines() {
                    if let Ok(l) = line {
                        if !l.trim().is_empty() {
                            c.insert(Self::normalize_url(&l));
                            count += 1;
                        }
                    }
                }
                debug!(target: "core::history", "Loaded {} URLs into history cache", count);
             } else {
                 warn!(target: "core::history", "History file exists but could not be opened for read");
             }
        } else {
             trace!(target: "core::history", "No existing history file found");
        }

        let (tx, mut rx) = mpsc::channel(100);
        let actor_path = file_path.clone();
        let actor_cache = cache.clone();
        
        tauri::async_runtime::spawn(async move {
            debug!(target: "core::history", "History background actor started");
            let mut writer: Option<BufWriter<File>> = match OpenOptions::new()
                .create(true)
                .append(true)
                .open(&actor_path)
                .await 
            {
                Ok(f) => Some(BufWriter::with_capacity(8192, f)),
                Err(e) => {
                    error!(target: "core::history", "Failed to open persistent history handle: {}", e);
                    Option::None
                }
            };

            while let Some(msg) = rx.recv().await {
                trace!(target: "core::history", "Actor processing message: {:?}", msg);
                match msg {
                    HistoryMessage::Add(url) => {
                        if let Some(ref mut w) = writer {
                            if let Err(e) = w.write_all(format!("{}\n", url).as_bytes()).await {
                                error!(target: "core::history", "Failed to write history: {}", e);
                            } else {
                                let _ = w.flush().await; 
                                
                                let normalized = Self::normalize_url(&url);
                                if let Ok(mut c) = actor_cache.write() {
                                    c.insert(normalized);
                                }
                            }
                        } else {
                            warn!(target: "core::history", "Writer not available, attempting to reopen file");
                            if let Ok(f) = OpenOptions::new().create(true).append(true).open(&actor_path).await {
                                writer = Some(BufWriter::with_capacity(8192, f));
                            }
                        }
                    },
                    HistoryMessage::Replace(content, resp) => {
                         debug!(target: "core::history", "Replacing entire history file");
                         drop(writer.take());

                         match File::create(&actor_path).await {
                             Ok(mut file) => {
                                 if let Err(e) = file.write_all(content.as_bytes()).await {
                                     error!(target: "core::history", "Failed to overwrite history file: {}", e);
                                     let _ = resp.send(Err(e.to_string()));
                                 } else {
                                     let mut new_set = HashSet::new();
                                     for line in content.lines() {
                                         if !line.trim().is_empty() {
                                             new_set.insert(Self::normalize_url(line));
                                         }
                                     }
                                     if let Ok(mut c) = actor_cache.write() {
                                         *c = new_set;
                                     }
                                     let _ = resp.send(Ok(()));
                                 }
                             },
                             Err(e) => {
                                 error!(target: "core::history", "Failed to recreate history file: {}", e);
                                 let _ = resp.send(Err(e.to_string()));
                             }
                         }

                         if let Ok(f) = OpenOptions::new().append(true).open(&actor_path).await {
                             writer = Some(BufWriter::with_capacity(8192, f));
                         }
                    },
                    HistoryMessage::Clear(resp) => {
                        debug!(target: "core::history", "Clearing history file");
                        drop(writer.take());
                        match File::create(&actor_path).await {
                            Ok(_) => {
                                if let Ok(mut c) = actor_cache.write() {
                                    c.clear();
                                }
                                let _ = resp.send(Ok(()));
                            },
                            Err(e) => {
                                error!(target: "core::history", "Failed to clear history file: {}", e);
                                let _ = resp.send(Err(e.to_string()));
                            }
                        }
                        if let Ok(f) = OpenOptions::new().create(true).append(true).open(&actor_path).await {
                            writer = Some(BufWriter::with_capacity(8192, f));
                        }
                    },
                    HistoryMessage::Get(resp) => {
                        trace!(target: "core::history", "Reading entire history file for Get request");
                        let content = if actor_path.exists() {
                            match tokio::fs::read_to_string(&actor_path).await {
                                Ok(s) => s,
                                Err(e) => {
                                    warn!(target: "core::history", "Failed to read history content: {}", e);
                                    String::new()
                                }
                            }
                        } else {
                            String::new()
                        };
                        let _ = resp.send(content);
                    }
                }
            }
            debug!(target: "core::history", "History background actor shut down");
        });

        Self {
            cache,
            sender: tx
        }
    }

    pub fn normalize_url(raw_url: &str) -> String {
        let Ok(mut url) = Url::parse(raw_url) else {
            trace!(target: "core::history", "Failed to parse URL for normalization: {}", raw_url);
            return raw_url.trim().to_string();
        };

        let host_option = url.domain().map(|s| s.to_string());
        if let Some(host) = host_option {
            if host == "youtu.be" {
                let path = url.path().trim_start_matches('/');
                if !path.is_empty() {
                    let new_url = format!("https://youtube.com/watch?v={}", path);
                    if let Ok(u) = Url::parse(&new_url) {
                        url = u;
                    }
                }
            } else if host == "m.youtube.com" {
                let _ = url.set_host(Some("youtube.com"));
            } else if host.starts_with("www.") {
                let new_host = &host[4..];
                let _ = url.set_host(Some(new_host));
            }
        }

        let allowed_params: HashSet<&str> = ["v", "list", "id"].into_iter().collect();
        let current_params: Vec<(String, String)> = url.query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();

        let is_youtube = url.domain().map(|d| d.contains("youtube")).unwrap_or(false);

        if is_youtube {
            url.query_pairs_mut().clear();
            for (k, v) in current_params {
                if allowed_params.contains(k.as_str()) {
                    url.query_pairs_mut().append_pair(&k, &v);
                }
            }
        } else {
            let tracking = ["utm_source", "utm_medium", "utm_campaign", "si", "feature", "ab_channel"];
            url.query_pairs_mut().clear();
            for (k, v) in current_params {
                if !tracking.contains(&k.as_str()) {
                    url.query_pairs_mut().append_pair(&k, &v);
                }
            }
        }

        let as_str = url.to_string();
        let no_scheme = as_str.split("://").last().unwrap_or(&as_str);
        let normalized = no_scheme.trim_end_matches('/').to_string();
        
        trace!(target: "core::history", "Normalized URL: {} -> {}", raw_url, normalized);
        normalized
    }

    pub fn exists(&self, url: &str) -> bool {
        let normalized = Self::normalize_url(url);
        let cache = self.cache.read().unwrap();
        let hit = cache.contains(&normalized);
        trace!(target: "core::history", "Checked existence of {}: {}", normalized, hit);
        hit
    }

    pub async fn add(&self, url: &str) -> Result<(), String> {
        let normalized = Self::normalize_url(url);
        
        {
            let cache = self.cache.read().unwrap();
            if cache.contains(&normalized) {
                trace!(target: "core::history", "URL already in cache, ignoring Add: {}", normalized);
                return Ok(());
            }
        }

        debug!(target: "core::history", "Sending Add message to actor for {}", url);
        self.sender.send(HistoryMessage::Add(url.to_string())).await
            .map_err(|_| "History actor closed".to_string())
    }

    pub async fn get_content(&self) -> Result<String, String> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(HistoryMessage::Get(tx)).await.map_err(|_| "Actor closed".to_string())?;
        rx.await.map_err(|_| "Response failed".to_string())
    }

    pub async fn save_content(&self, content: String) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(HistoryMessage::Replace(content, tx)).await.map_err(|_| "Actor closed".to_string())?;
        rx.await.map_err(|_| "Response failed".to_string())?
    }

    pub async fn clear(&self) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(HistoryMessage::Clear(tx)).await.map_err(|_| "Actor closed".to_string())?;
        rx.await.map_err(|_| "Response failed".to_string())?
    }
}
