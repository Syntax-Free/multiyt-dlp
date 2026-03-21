use crate::config::{AppConfig, ConfigManager, GeneralConfig, PreferenceConfig};
use crate::core::logging::LogManager;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn get_app_config(config_manager: State<'_, Arc<ConfigManager>>) -> AppConfig {
    config_manager.get_config()
}

#[tauri::command]
pub fn save_general_config(
    config_manager: State<'_, Arc<ConfigManager>>,
    log_manager: State<'_, LogManager>, // NEW: Inject LogManager
    config: GeneralConfig,
) -> Result<(), String> {
    // 1. Update Log Level immediately
    if let Err(e) = log_manager.set_level(&config.log_level) {
        eprintln!("Failed to update log level: {}", e);
        // Don't fail the save just because logging failed to update, but warn
    }

    // 2. Save to Disk
    config_manager.update_general(config);
    config_manager.save()
}

#[tauri::command]
pub fn save_preference_config(
    config_manager: State<'_, Arc<ConfigManager>>,
    config: PreferenceConfig,
) -> Result<(), String> {
    config_manager.update_preferences(config);
    config_manager.save()
}
