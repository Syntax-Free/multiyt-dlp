use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use arc_swap::ArcSwap;
use tracing::{debug, error, info, trace, warn};

// --- Configuration Structs ---

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)] 
pub struct WindowConfig {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
    pub is_maximized: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1200.0,
            height: 800.0,
            x: 100.0,
            y: 100.0,
            is_maximized: false,
        }
    }
}

impl WindowConfig {
    /// Validates and fixes invalid coordinates (e.g. minimized state -32000)
    pub fn sanitize(&mut self) {
        let original_x = self.x;
        let original_y = self.y;
        
        if self.x <= -10000.0 || self.y <= -10000.0 {
            self.x = 100.0;
            self.y = 100.0;
            debug!(target: "config", "Sanitized window coordinates from ({}, {}) to (100, 100)", original_x, original_y);
        }

        if self.width < 400.0 { 
            debug!(target: "config", "Sanitized window width from {} to 1200", self.width);
            self.width = 1200.0; 
        }
        if self.height < 300.0 { 
            debug!(target: "config", "Sanitized window height from {} to 800", self.height);
            self.height = 800.0; 
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct GeneralConfig {
    pub download_path: Option<String>,
    pub filename_template: String,
    pub template_blocks_json: Option<String>,
    pub max_concurrent_downloads: u32,
    pub max_total_instances: u32,
    pub log_level: String, 
    pub check_for_updates: bool,
    pub cookies_path: Option<String>,
    pub cookies_from_browser: Option<String>,
    pub aria2_prompt_dismissed: bool,
    pub use_concurrent_fragments: bool,
    pub concurrent_fragments: u32,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            download_path: None, 
            filename_template: "%(title)s.%(ext)s".to_string(),
            template_blocks_json: None,
            max_concurrent_downloads: 4,
            max_total_instances: 10,
            log_level: "info".to_string(),
            check_for_updates: true,
            cookies_path: None,
            cookies_from_browser: None,
            aria2_prompt_dismissed: false,
            use_concurrent_fragments: false,
            concurrent_fragments: 4,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct PreferenceConfig {
    pub mode: String,
    pub format_preset: String, 
    pub video_preset: String,  
    pub audio_preset: String,  
    pub video_resolution: String, 
    pub embed_metadata: bool,
    pub embed_thumbnail: bool,
    pub live_from_start: bool,
    pub enable_playlist_selection: bool,
}

impl Default for PreferenceConfig {
    fn default() -> Self {
        Self {
            mode: "video".to_string(),
            format_preset: "best".to_string(),
            video_preset: "best".to_string(),        
            audio_preset: "audio_best".to_string(),  
            video_resolution: "best".to_string(),
            embed_metadata: false,
            embed_thumbnail: false,
            live_from_start: false,
            enable_playlist_selection: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub preferences: PreferenceConfig,
    pub window: WindowConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            preferences: PreferenceConfig::default(),
            window: WindowConfig::default(),
        }
    }
}

// --- Manager (Lock‑Free via arc_swap) ---

pub struct ConfigManager {
    config: ArcSwap<AppConfig>,
    file_path: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Self {
        info!(target: "config", "Initializing ConfigManager");
        let home = dirs::home_dir().expect("Could not find home directory");
        let config_dir = home.join(".multiyt-dlp");
        let file_path = config_dir.join("config.json");

        if !config_dir.exists() {
            trace!(target: "config", "Creating config directory at {:?}", config_dir);
            let _ = fs::create_dir_all(&config_dir);
        }

        let mut config = Self::load_robustly(&file_path)
            .or_else(|| {
                warn!(target: "config", "Failed to load primary config. Attempting backup load.");
                let bak_path = file_path.with_extension("json.bak");
                Self::load_robustly(&bak_path)
            })
            .unwrap_or_else(|| {
                warn!(target: "config", "Failed to load config entirely. Falling back to defaults.");
                AppConfig::default()
            });

        config.window.sanitize();

        let manager = Self {
            config: ArcSwap::from_pointee(config),
            file_path,
        };
        
        if let Err(e) = manager.save() {
            error!(target: "config", "Failed to perform initial config save: {}", e);
        } else {
            debug!(target: "config", "Initial configuration saved successfully");
        }
        
        manager
    }

    fn load_robustly(path: &PathBuf) -> Option<AppConfig> {
        if !path.exists() {
            debug!(target: "config", "Config file does not exist at {:?}", path);
            return None;
        }

        let content = fs::read_to_string(path).ok()?;
        trace!(target: "config", "Read {} bytes from config file", content.len());

        match serde_json::from_str::<AppConfig>(&content) {
            Ok(cfg) => {
                debug!(target: "config", "Config successfully parsed directly");
                Some(cfg)
            },
            Err(e) => {
                warn!(target: "config", "Direct config parse failed ({}). Attempting tolerant merge.", e);
                let disk_json: Value = serde_json::from_str(&content).ok()?; 
                let default_config = AppConfig::default();
                let mut merged_json = serde_json::to_value(&default_config).unwrap();
                Self::tolerant_merge(&mut merged_json, &disk_json);
                let res = serde_json::from_value(merged_json).ok();
                if res.is_some() {
                    debug!(target: "config", "Tolerant merge successful");
                } else {
                    error!(target: "config", "Tolerant merge failed to produce valid config");
                }
                res
            }
        }
    }

    fn tolerant_merge(base: &mut Value, overlay: &Value) {
        match (base, overlay) {
            (Value::Object(base_map), Value::Object(overlay_map)) => {
                for (k, v) in overlay_map {
                    if let Some(base_val) = base_map.get_mut(k) {
                        Self::tolerant_merge(base_val, v);
                    }
                }
            }
            (base_val, overlay_val) => {
                if std::mem::discriminant(base_val) == std::mem::discriminant(overlay_val) {
                    *base_val = overlay_val.clone();
                } else if base_val.is_null() {
                    *base_val = overlay_val.clone();
                } else if base_val.is_f64() && overlay_val.is_i64() {
                    if let Some(n) = overlay_val.as_f64() {
                        *base_val = Value::from(n);
                    }
                } else if base_val.is_boolean() && overlay_val.is_boolean() {
                    *base_val = overlay_val.clone();
                }
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        trace!(target: "config", "Acquiring current config Arc for saving");
        let config_arc = self.config.load_full();
        
        let json = serde_json::to_string_pretty(config_arc.as_ref())
            .map_err(|e| {
                error!(target: "config", "Serialization error during save: {}", e);
                format!("Serialization error: {}", e)
            })?;

        let main_path = &self.file_path;
        let tmp_path = main_path.with_extension("tmp");
        let bak_path = main_path.with_extension("json.bak");

        trace!(target: "config", "Writing temporary config file to {:?}", tmp_path);
        fs::write(&tmp_path, json)
            .map_err(|e| {
                error!(target: "config", "Failed to write temp config: {}", e);
                format!("Failed to write temp config: {}", e)
            })?;

        if main_path.exists() {
            trace!(target: "config", "Backing up current config to {:?}", bak_path);
            let _ = fs::copy(main_path, &bak_path); 
        }

        trace!(target: "config", "Renaming temp config to main config path");
        fs::rename(&tmp_path, main_path)
            .map_err(|e| {
                error!(target: "config", "Failed to commit config file: {}", e);
                format!("Failed to commit config file: {}", e)
            })?;

        debug!(target: "config", "Config successfully flushed to disk");
        Ok(())
    }

    /// Returns an `Arc<AppConfig>` – cheap, lock‑free, and wait‑free.
    pub fn get_config(&self) -> Arc<AppConfig> {
        self.config.load_full()
    }

    pub fn update_general(&self, general: GeneralConfig) {
        debug!(target: "config", "Updating General Configuration");
        let current = self.config.load_full();
        let mut new_cfg = (*current).clone();
        new_cfg.general = general;
        self.config.store(Arc::new(new_cfg));
    }

    pub fn update_preferences(&self, prefs: PreferenceConfig) {
        debug!(target: "config", "Updating Preference Configuration");
        let current = self.config.load_full();
        let mut new_cfg = (*current).clone();
        new_cfg.preferences = prefs;
        self.config.store(Arc::new(new_cfg));
    }

    pub fn update_window(&self, mut window: WindowConfig) {
        trace!(target: "config", "Updating Window Configuration");
        window.sanitize(); 
        let current = self.config.load_full();
        let mut new_cfg = (*current).clone();
        new_cfg.window = window;
        self.config.store(Arc::new(new_cfg));
    }
}