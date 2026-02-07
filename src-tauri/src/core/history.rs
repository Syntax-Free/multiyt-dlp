use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, oneshot};
use tokio::fs::{OpenOptions, File};
use tokio::io::{AsyncWriteExt};
use url::Url;

#[derive(Debug)]
enum HistoryMessage {
    Add(String),
    Replace(String, oneshot::Sender<Result<(), String>>),
    Clear(oneshot::Sender<Result<(), String>>),
    Get(oneshot::Sender<String>),
}

#[derive(Clone)]
pub struct HistoryManager {
    // Cache is now strictly a read-replica updated by the actor
    cache: Arc<RwLock<HashSet<String>>>,
    sender: mpsc::Sender<HistoryMessage>,
}

impl HistoryManager {
    pub fn new() -> Self {
        let home = dirs::home_dir().expect("Could not find home directory");
        let file_path = home.join(".multiyt-dlp").join("downloads.txt");

        // Ensure directory exists
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        let cache = Arc::new(RwLock::new(HashSet::new()));
        
        // Initial Load
        if file_path.exists() {
             if let Ok(file) = std::fs::File::open(&file_path) {
                let reader = std::io::BufReader::new(file);
                let mut c = cache.write().unwrap();
                use std::io::BufRead;
                for line in reader.lines() {
                    if let Ok(l) = line {
                        if !l.trim().is_empty() {
                            c.insert(Self::normalize_url(&l));
                        }
                    }
                }
             }
        }

        let (tx, mut rx) = mpsc::channel(100);
        let actor_path = file_path.clone();
        let actor_cache = cache.clone();
        
        // Actor Loop: Serializes all file access
        tauri::async_runtime::spawn(async move {
            while let Some(msg) = rx.recv().await {
                match msg {
                    HistoryMessage::Add(url) => {
                        // Append to file
                        if let Ok(mut file) = OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&actor_path)
                            .await 
                        {
                            if let Err(e) = file.write_all(format!("{}\n", url).as_bytes()).await {
                                eprintln!("Failed to write history: {}", e);
                            } else {
                                // Update Read Replica only on success
                                let normalized = Self::normalize_url(&url);
                                if let Ok(mut c) = actor_cache.write() {
                                    c.insert(normalized);
                                }
                            }
                        }
                    },
                    HistoryMessage::Replace(content, resp) => {
                         // Atomic overwrite
                         match File::create(&actor_path).await {
                             Ok(mut file) => {
                                 if let Err(e) = file.write_all(content.as_bytes()).await {
                                     let _ = resp.send(Err(e.to_string()));
                                 } else {
                                     // Rebuild Cache completely
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
                                 let _ = resp.send(Err(e.to_string()));
                             }
                         }
                    },
                    HistoryMessage::Clear(resp) => {
                        match File::create(&actor_path).await {
                            Ok(_) => {
                                if let Ok(mut c) = actor_cache.write() {
                                    c.clear();
                                }
                                let _ = resp.send(Ok(()));
                            },
                            Err(e) => {
                                let _ = resp.send(Err(e.to_string()));
                            }
                        }
                    },
                    HistoryMessage::Get(resp) => {
                        let content = if actor_path.exists() {
                            match tokio::fs::read_to_string(&actor_path).await {
                                Ok(s) => s,
                                Err(_) => String::new(),
                            }
                        } else {
                            String::new()
                        };
                        let _ = resp.send(content);
                    }
                }
            }
        });

        Self {
            cache,
            sender: tx
        }
    }

    pub fn normalize_url(raw_url: &str) -> String {
        let Ok(mut url) = Url::parse(raw_url) else {
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
        no_scheme.trim_end_matches('/').to_string()
    }

    // Fast path: Read from RAM cache
    pub fn exists(&self, url: &str) -> bool {
        let normalized = Self::normalize_url(url);
        let cache = self.cache.read().unwrap();
        cache.contains(&normalized)
    }

    // Slow path: Send message to actor
    pub async fn add(&self, url: &str) -> Result<(), String> {
        let normalized = Self::normalize_url(url);
        
        // Optimistic check to avoid channel traffic
        {
            let cache = self.cache.read().unwrap();
            if cache.contains(&normalized) {
                return Ok(());
            }
        }

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