use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use tokio::fs::{OpenOptions, File};
use tokio::io::{AsyncWriteExt, AsyncReadExt, BufReader, AsyncBufReadExt};
use url::Url;

enum HistoryMessage {
    Write(String),
    Clear,
    Save(String),
}

#[derive(Clone)]
pub struct HistoryManager {
    cache: Arc<RwLock<HashSet<String>>>,
    file_path: PathBuf,
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
        
        // Load sync
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

        // --- Serial Actor for File Writes ---
        let (tx, mut rx) = mpsc::channel(100);
        let actor_path = file_path.clone();
        
        tauri::async_runtime::spawn(async move {
            while let Some(msg) = rx.recv().await {
                match msg {
                    HistoryMessage::Write(url) => {
                        if let Ok(mut file) = OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&actor_path)
                            .await 
                        {
                            let _ = file.write_all(format!("{}\n", url).as_bytes()).await;
                        }
                    },
                    HistoryMessage::Clear => {
                        let _ = File::create(&actor_path).await;
                    },
                    HistoryMessage::Save(content) => {
                         if let Ok(mut file) = File::create(&actor_path).await {
                             let _ = file.write_all(content.as_bytes()).await;
                         }
                    }
                }
            }
        });

        Self {
            cache,
            file_path,
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

    pub async fn reload(&self) -> Result<(), String> {
        let file = File::open(&self.file_path).await.map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut new_set = HashSet::new();

        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                new_set.insert(Self::normalize_url(&line));
            }
        }

        let mut cache = self.cache.write().unwrap();
        *cache = new_set;
        Ok(())
    }

    pub fn exists(&self, url: &str) -> bool {
        let normalized = Self::normalize_url(url);
        let cache = self.cache.read().unwrap();
        cache.contains(&normalized)
    }

    pub async fn add(&self, url: &str) -> Result<(), String> {
        let normalized = Self::normalize_url(url);
        
        {
            let mut cache = self.cache.write().unwrap();
            if cache.contains(&normalized) {
                return Ok(());
            }
            cache.insert(normalized);
        }

        let _ = self.sender.send(HistoryMessage::Write(url.to_string())).await;
        Ok(())
    }

    pub async fn get_content(&self) -> Result<String, String> {
        if !self.file_path.exists() { return Ok(String::new()); }
        let mut file = File::open(&self.file_path).await.map_err(|e| e.to_string())?;
        let mut content = String::new();
        file.read_to_string(&mut content).await.map_err(|e| e.to_string())?;
        Ok(content)
    }

    pub async fn save_content(&self, content: String) -> Result<(), String> {
        let _ = self.sender.send(HistoryMessage::Save(content)).await;
        // Wait briefly for file to write before reloading (imperfect but functional for manual edits)
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        self.reload().await
    }

    pub async fn clear(&self) -> Result<(), String> {
        let _ = self.sender.send(HistoryMessage::Clear).await;
        let mut cache = self.cache.write().unwrap();
        cache.clear();
        Ok(())
    }
}