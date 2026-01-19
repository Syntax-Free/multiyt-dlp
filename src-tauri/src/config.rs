use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

// --- Configuration Structs ---

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)] 
pub struct WindowConfig {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1200.0,
            height: 800.0,
            x: 100.0,
            y: 100.0,
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
    // NEW: Cookies
    pub cookies_path: Option<String>,
    pub cookies_from_browser: Option<String>, // "chrome", "firefox", etc. or None
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

// --- Manager ---

pub struct ConfigManager {
    config: Mutex<AppConfig>,
    file_path: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Self {
        let home = dirs::home_dir().expect("Could not find home directory");
        let config_dir = home.join(".multiyt-dlp");
        let file_path = config_dir.join("config.json");

        if !config_dir.exists() {
            let _ = fs::create_dir_all(&config_dir);
        }

        let config = Self::load_robustly(&file_path);

        // Immediately save back to disk to ensure the file 
        // has any new fields that might have been added in this version.
        let manager = Self {
            config: Mutex::new(config),
            file_path,
        };
        let _ = manager.save();
        
        manager
    }

    /// Robust loader that attempts to preserve user data even across schema changes
    fn load_robustly(path: &PathBuf) -> AppConfig {
        if !path.exists() {
            return AppConfig::default();
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return AppConfig::default(),
        };

        // 1. Attempt direct deserialization
        // Thanks to #[serde(default)], this handles missing fields gracefully.
        match serde_json::from_str::<AppConfig>(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                println!("Direct config load failed ({}). Attempting repair merge...", e);
                
                // 2. Fallback: Type-Safe Merge
                // If direct load failed (e.g. type mismatch), load as generic JSON
                // and merge valid fields into the Default config.
                let disk_json: Value = match serde_json::from_str(&content) {
                    Ok(v) => v,
                    Err(_) => {
                        println!("Config file is strictly invalid JSON. Backing up and resetting.");
                        let _ = fs::rename(path, path.with_extension("corrupt.json"));
                        return AppConfig::default();
                    }
                };

                let final_config = AppConfig::default();
                let mut default_json = serde_json::to_value(&final_config).unwrap();

                // Merge disk values into default values
                Self::tolerant_merge(&mut default_json, &disk_json);

                // Deserialize the merged result
                match serde_json::from_value(default_json) {
                    Ok(recovered) => {
                        println!("Config recovered successfully.");
                        recovered
                    },
                    Err(_) => {
                        println!("Recovery failed. Resetting to defaults.");
                        let _ = fs::rename(path, path.with_extension("json.bak"));
                        AppConfig::default()
                    }
                }
            }
        }
    }

    /// Recursively merges `overlay` into `base`.
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
                if base_val.is_number() && overlay_val.is_number() {
                    *base_val = overlay_val.clone();
                } 
                else if std::mem::discriminant(base_val) == std::mem::discriminant(overlay_val) {
                    *base_val = overlay_val.clone();
                }
                else if base_val.is_null() {
                    *base_val = overlay_val.clone();
                }
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let json = {
            let config = self.config.lock().unwrap();
            serde_json::to_string_pretty(&*config)
                .map_err(|e| format!("Serialization error: {}", e))?
        };
        
        fs::write(&self.file_path, json)
            .map_err(|e| format!("Failed to write config file: {}", e))
    }

    pub fn get_config(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn update_general(&self, general: GeneralConfig) {
        let mut cfg = self.config.lock().unwrap();
        cfg.general = general;
    }

    pub fn update_preferences(&self, prefs: PreferenceConfig) {
        let mut cfg = self.config.lock().unwrap();
        cfg.preferences = prefs;
    }

    pub fn update_window(&self, window: WindowConfig) {
        let mut cfg = self.config.lock().unwrap();
        cfg.window = window;
    }
}