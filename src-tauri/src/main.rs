// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tauri::{Manager, WindowEvent};
use tokio::sync::mpsc;
use std::time::Duration;
use std::fs;

// --- WINDOWS SPECIFIC IMPORTS ---
#[cfg(target_os = "windows")]
use windows::{
    core::PCWSTR,
    Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID,
};

use crate::core::manager::JobManagerHandle;
use crate::config::ConfigManager;
use crate::core::logging::{LogManager, rotate_logs};

mod commands;
mod core;
mod models;
mod config;

fn main() {
    // Explicitly set the App User Model ID (AUMID) for the process.
    #[cfg(target_os = "windows")]
    unsafe {
        const APP_ID: &str = "net.zqil.multiyt-dlp";
        let wide_id: Vec<u16> = APP_ID.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = SetCurrentProcessExplicitAppUserModelID(PCWSTR(wide_id.as_ptr()));
    }
    // ---------------------------------------------------

    let home = dirs::home_dir().expect("Could not find home directory");
    let temp_dir = home.join(".multiyt-dlp").join("temp_downloads");
    if !temp_dir.exists() {
        let _ = fs::create_dir_all(&temp_dir);
    }

    // 1. Perform Log Rotation BEFORE logging initialization
    if let Err(e) = rotate_logs() {
        eprintln!("WARNING: Log rotation failed: {}", e);
    }

    // 2. Load Config & Init Logger
    let config_manager = Arc::new(ConfigManager::new());
    let initial_config = config_manager.get_config();
    let log_manager = LogManager::init(&initial_config.general.log_level);

    // Persistence config auto-save channel
    let config_manager_setup = config_manager.clone();
    let config_manager_event = config_manager.clone();
    let config_manager_saver = config_manager.clone();
    let (tx_save, mut rx_save) = mpsc::unbounded_channel::<()>();

    tauri::Builder::default()
        .manage(config_manager)
        .manage(log_manager)
        .setup(move |app| {
            // Initialize the Actor Handle here
            let job_manager_handle = JobManagerHandle::new(app.handle());
            app.manage(job_manager_handle);

            let main_window = app.get_window("main").unwrap();
            let config = config_manager_setup.get_config();
            
            let _ = main_window.set_size(tauri::Size::Physical(tauri::PhysicalSize {
                width: config.window.width as u32,
                height: config.window.height as u32,
            }));
            let _ = main_window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                x: config.window.x as i32,
                y: config.window.y as i32,
            }));
            
            tracing::info!("Application startup complete. Window initialized.");

            tauri::async_runtime::spawn(async move {
                while let Some(_) = rx_save.recv().await {
                    while let Ok(_) = rx_save.try_recv() {}
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    if let Err(e) = config_manager_saver.save() {
                        tracing::error!("Failed to auto-save window config: {}", e);
                    }
                }
            });

            Ok(())
        })
        .on_window_event(move |event| {
            if let WindowEvent::Destroyed = event.event() {
                let window_label = event.window().label();
                if window_label == "splashscreen" {
                    let app_handle = event.window().app_handle();
                    if let Some(main) = app_handle.get_window("main") {
                        if !main.is_visible().unwrap_or(false) {
                            app_handle.exit(0);
                        }
                    } else {
                        app_handle.exit(0);
                    }
                }
                if window_label == "main" {
                    event.window().app_handle().exit(0);
                }
            }

            if let WindowEvent::Moved(pos) = event.event() {
                let mut current_config = config_manager_event.get_config();
                current_config.window.x = pos.x as f64;
                current_config.window.y = pos.y as f64;
                config_manager_event.update_window(current_config.window);
                let _ = tx_save.send(());
            }
            
            if let WindowEvent::Resized(size) = event.event() {
                if size.width > 0 && size.height > 0 {
                    let mut current_config = config_manager_event.get_config();
                    current_config.window.width = size.width as f64;
                    current_config.window.height = size.height as f64;
                    config_manager_event.update_window(current_config.window);
                    let _ = tx_save.send(());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::check_dependencies,
            commands::system::install_dependency,
            commands::system::sync_dependencies,
            commands::system::open_external_link,
            commands::system::close_splash,
            commands::system::get_latest_app_version, 
            commands::system::show_in_folder, 
            commands::system::open_log_folder,
            commands::system::log_frontend_message, // REGISTER NEW COMMAND
            commands::downloader::start_download,
            commands::downloader::cancel_download,
            commands::downloader::expand_playlist,
            commands::downloader::get_pending_jobs,
            commands::downloader::resume_pending_jobs,
            commands::downloader::clear_pending_jobs,
            commands::config::get_app_config,
            commands::config::save_general_config,
            commands::config::save_preference_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}