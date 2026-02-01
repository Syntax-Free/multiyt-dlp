use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{PathBuf};
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

impl WindowConfig {
    /// Validates and fixes invalid coordinates (e.g. minimized state -32000)
    pub fn sanitize(&mut self) {
        // Windows minimizes to -32000. Mac/Linux behave differently but large negatives are bad.
        // We also ensure the window isn't 0x0 size.
        if self.x <= -10000.0 || self.y <= -10000.0 {
            // Reset to safe default
            self.x = 100.0;
            self.y = 100.0;
        }

        // Prevent tiny unusable windows
        if self.width < 400.0 { self.width = 1200.0; }
        if self.height < 300.0 { self.height = 800.0; }
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

        // Try load main -> Try load backup -> Default
        let mut config = Self::load_robustly(&file_path)
            .or_else(|| {
                let bak_path = file_path.with_extension("json.bak");
                println!("Main config failed. Attempting backup: {:?}", bak_path);
                Self::load_robustly(&bak_path)
            })
            .unwrap_or_else(AppConfig::default);

        // Sanitize immediately upon load to fix any existing bad states
        config.window.sanitize();

        let manager = Self {
            config: Mutex::new(config),
            file_path,
        };
        
        // Save immediately to ensure file structure is up to date (schema migration)
        let _ = manager.save();
        
        manager
    }

    /// Robust loader that handles missing fields and partial corruption
    fn load_robustly(path: &PathBuf) -> Option<AppConfig> {
        if !path.exists() { return None; }

        let content = fs::read_to_string(path).ok()?;

        // 1. Attempt strict typed deserialization first
        match serde_json::from_str::<AppConfig>(&content) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                println!("Direct config load failed ({}). Attempting repair merge...", e);
                
                // 2. Fallback: Generic JSON Merge
                let disk_json: Value = serde_json::from_str(&content).ok()?; // If valid JSON but invalid schema
                
                let default_config = AppConfig::default();
                let mut merged_json = serde_json::to_value(&default_config).unwrap();

                Self::tolerant_merge(&mut merged_json, &disk_json);

                serde_json::from_value(merged_json).ok()
            }
        }
    }

    /// Recursively merges `overlay` into `base` while preserving types.
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
                // Only overwrite if types match or if base is null
                if std::mem::discriminant(base_val) == std::mem::discriminant(overlay_val) {
                    *base_val = overlay_val.clone();
                } else if base_val.is_null() {
                    *base_val = overlay_val.clone();
                } else if base_val.is_f64() && overlay_val.is_i64() {
                    // Handle float/int mismatch (common with coords)
                    if let Some(n) = overlay_val.as_f64() {
                        *base_val = Value::from(n);
                    }
                }
            }
        }
    }

    /// Atomic Save with Backup
    pub fn save(&self) -> Result<(), String> {
        let config_guard = self.config.lock().unwrap();
        
        let json = serde_json::to_string_pretty(&*config_guard)
            .map_err(|e| format!("Serialization error: {}", e))?;

        // Paths
        let main_path = &self.file_path;
        let tmp_path = main_path.with_extension("tmp");
        let bak_path = main_path.with_extension("json.bak");

        // 1. Write to .tmp first (Atomic prep)
        fs::write(&tmp_path, json)
            .map_err(|e| format!("Failed to write temp config: {}", e))?;

        // 2. If main exists, copy it to .bak (Backup)
        if main_path.exists() {
            let _ = fs::copy(main_path, &bak_path); 
        }

        // 3. Rename .tmp to main (Atomic commit)
        fs::rename(&tmp_path, main_path)
            .map_err(|e| format!("Failed to commit config file: {}", e))?;

        Ok(())
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

    pub fn update_window(&self, mut window: WindowConfig) {
        window.sanitize(); // Always sanitize before setting memory state
        let mut cfg = self.config.lock().unwrap();
        cfg.window = window;
    }
}