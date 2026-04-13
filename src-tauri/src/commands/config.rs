use tauri::State;
use std::sync::Arc;
use crate::config::{AppConfig, ConfigManager, GeneralConfig, PreferenceConfig};
use crate::core::logging::LogManager;
use tracing::{debug, error, info, trace};

#[tauri::command]
pub fn get_app_config(config_manager: State<'_, Arc<ConfigManager>>) -> AppConfig {
    trace!(target: "commands::config", "Frontend requested AppConfig");
    config_manager.get_config()
}

#[tauri::command]
pub fn save_general_config(
    config_manager: State<'_, Arc<ConfigManager>>,
    log_manager: State<'_, LogManager>,
    config: GeneralConfig
) -> Result<(), String> {
    info!(target: "commands::config", "Saving general configuration");

    // 1. Update Log Level immediately
    debug!(target: "commands::config", "Attempting to update log level to: {}", config.log_level);
    if let Err(e) = log_manager.set_level(&config.log_level) {
        error!(target: "commands::config", "Failed to update log level: {}", e);
    }

    // 2. Save to Disk
    config_manager.update_general(config);
    match config_manager.save() {
        Ok(_) => {
            debug!(target: "commands::config", "General config saved successfully");
            Ok(())
        },
        Err(e) => {
            error!(target: "commands::config", "Failed to save general config: {}", e);
            Err(e)
        }
    }
}

#[tauri::command]
pub fn save_preference_config(
    config_manager: State<'_, Arc<ConfigManager>>,
    config: PreferenceConfig
) -> Result<(), String> {
    info!(target: "commands::config", "Saving preference configuration");
    config_manager.update_preferences(config);
    match config_manager.save() {
        Ok(_) => {
            debug!(target: "commands::config", "Preference config saved successfully");
            Ok(())
        },
        Err(e) => {
            error!(target: "commands::config", "Failed to save preference config: {}", e);
            Err(e)
        }
    }
}
