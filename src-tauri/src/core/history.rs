use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::fs::{OpenOptions, File};
use tokio::io::{AsyncWriteExt, AsyncReadExt, BufReader, AsyncBufReadExt};
use url::Url;

#[derive(Clone)]
pub struct HistoryManager {
    cache: Arc<RwLock<HashSet<String>>>,
    file_path: PathBuf,
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

        let manager = Self {
            cache: Arc::new(RwLock::new(HashSet::new())),
            file_path,
        };

        // Initial synchronous load to populate cache before app starts serving requests
        manager.load_sync();
        
        manager
    }

    /// Canonicalizes URLs to ensure duplicates are detected regardless of:
    /// - Protocol (http vs https)
    /// - Subdomain (www vs non-www, m.youtube vs youtube)
    /// - Tracking parameters (utm_*, feature, si)
    /// - Shortened links (youtu.be vs youtube.com)
    pub fn normalize_url(raw_url: &str) -> String {
        let Ok(mut url) = Url::parse(raw_url) else {
            // If it's not a valid URL (e.g. just a filename?), trim and return
            return raw_url.trim().to_string();
        };

        // Capture host as owned String to avoid borrowing `url` during mutation checks
        let host_option = url.domain().map(|s| s.to_string());

        if let Some(host) = host_option {
            if host == "youtu.be" {
                // Shortened URL logic
                // Borrow path momentarily to create new string
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

        // 2. Filter Query Parameters
        let allowed_params: HashSet<&str> = ["v", "list", "id"].into_iter().collect();
        // Collect params to owned Vector to drop borrow on `url`
        let current_params: Vec<(String, String)> = url.query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();

        // If it's YouTube, be strict. If generic, be lenient but strip specific tracking.
        let is_youtube = url.domain().map(|d| d.contains("youtube")).unwrap_or(false);

        if is_youtube {
            url.query_pairs_mut().clear();
            for (k, v) in current_params {
                if allowed_params.contains(k.as_str()) {
                    url.query_pairs_mut().append_pair(&k, &v);
                }
            }
        } else {
            // Generic: Remove known tracking params
            let tracking = ["utm_source", "utm_medium", "utm_campaign", "si", "feature", "ab_channel"];
            url.query_pairs_mut().clear();
            for (k, v) in current_params {
                if !tracking.contains(&k.as_str()) {
                    url.query_pairs_mut().append_pair(&k, &v);
                }
            }
        }

        // 3. Remove Scheme for pure string comparison
        let as_str = url.to_string();
        // Strip https:// or http://
        let no_scheme = as_str.split("://").last().unwrap_or(&as_str);
        
        // Remove trailing slash
        no_scheme.trim_end_matches('/').to_string()
    }

    fn load_sync(&self) {
        if !self.file_path.exists() { return; }
        
        if let Ok(file) = std::fs::File::open(&self.file_path) {
            let reader = std::io::BufReader::new(file);
            let mut cache = self.cache.write().unwrap();
            
            use std::io::BufRead;
            for line in reader.lines() {
                if let Ok(l) = line {
                    if !l.trim().is_empty() {
                        cache.insert(Self::normalize_url(&l));
                    }
                }
            }
        }
    }

    pub async fn reload(&self) -> Result<(), String> {
        let file = File::open(&self.file_path).await.map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        
        // Prepare new set
        let mut new_set = HashSet::new();

        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                new_set.insert(Self::normalize_url(&line));
            }
        }

        // Atomic Swap
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
        
        // 1. Check & Insert to Cache (Short Critical Section)
        {
            let mut cache = self.cache.write().unwrap();
            if cache.contains(&normalized) {
                return Ok(());
            }
            cache.insert(normalized);
        }

        // 2. Append to File (Async I/O)
        // We append the ORIGINAL url to the file for readability, 
        // but the cache only cares about the normalized version.
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .await
            .map_err(|e| e.to_string())?;

        file.write_all(format!("{}\n", url).as_bytes())
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    // --- Raw File Operations for Editor ---

    pub async fn get_content(&self) -> Result<String, String> {
        if !self.file_path.exists() { return Ok(String::new()); }
        let mut file = File::open(&self.file_path).await.map_err(|e| e.to_string())?;
        let mut content = String::new();
        file.read_to_string(&mut content).await.map_err(|e| e.to_string())?;
        Ok(content)
    }

    pub async fn save_content(&self, content: String) -> Result<(), String> {
        let mut file = File::create(&self.file_path).await.map_err(|e| e.to_string())?;
        file.write_all(content.as_bytes()).await.map_err(|e| e.to_string())?;
        
        // Reload cache after manual edit
        self.reload().await
    }

    pub async fn clear(&self) -> Result<(), String> {
        // Truncate file
        let _ = File::create(&self.file_path).await.map_err(|e| e.to_string())?;
        
        // Clear cache
        let mut cache = self.cache.write().unwrap();
        cache.clear();
        
        Ok(())
    }
}