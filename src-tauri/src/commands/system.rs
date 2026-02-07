use std::process::Command;
use tauri::{AppHandle, Manager};
use serde::{Serialize, Deserialize};
use regex::Regex;
use crate::core::deps;
use std::path::PathBuf;
use tracing::{info, warn, error, debug};
use tokio::time::{timeout, Duration};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::collections::HashSet;

// GLOBAL LOCK to prevent concurrent dependency installs
static INSTALL_LOCKS: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

#[derive(Serialize, Clone, Debug)]
pub struct DependencyInfo {
    pub name: String,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub is_supported: bool,
    pub is_recommended: bool,
    pub is_latest: bool,
}

#[derive(Serialize)]
pub struct AppDependencies {
    pub yt_dlp: DependencyInfo,
    pub ffmpeg: DependencyInfo,
    pub js_runtime: DependencyInfo,
    pub aria2: DependencyInfo,
}

#[derive(Deserialize)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

#[tauri::command]
pub fn log_frontend_message(level: LogLevel, message: String, context: Option<String>) {
    let ctx = context.unwrap_or_else(|| "frontend".to_string());
    match level {
        LogLevel::Info => info!(target: "frontend", context = %ctx, "{}", message),
        LogLevel::Warn => warn!(target: "frontend", context = %ctx, "{}", message),
        LogLevel::Error => error!(target: "frontend", context = %ctx, "{}", message),
        LogLevel::Debug => debug!(target: "frontend", context = %ctx, "{}", message),
    }
}

fn new_silent_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); 
    }
    cmd
}

pub fn resolve_binary_info(bin_name: &str, version_flag: &str, local_bin_path: &PathBuf) -> DependencyInfo {
    let local_path = local_bin_path.join(bin_name);
    let local_available = local_path.exists();

    let final_path = if local_available {
        Some(local_path.to_string_lossy().to_string())
    } else {
        let path_cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
        new_silent_command(path_cmd)
            .arg(bin_name)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.lines().next().unwrap_or("").trim().to_string())
    };

    let available = final_path.is_some();
    let mut version = None;

    if let Some(ref p) = final_path {
        if let Ok(output) = new_silent_command(p).arg(version_flag).output() {
             if output.status.success() {
                 let out_str = String::from_utf8_lossy(&output.stdout).to_string();
                 let first_line = out_str.lines().next().unwrap_or("").trim().to_string();
                 version = Some(first_line);
             }
        }
    }

    DependencyInfo {
        name: bin_name.to_string(),
        available,
        version,
        path: final_path,
        is_supported: true,
        is_recommended: true,
        is_latest: true,
    }
}

pub fn get_js_runtime_info(bin_path: &PathBuf) -> Option<(String, String)> {
    let providers = [
        ("deno", "deno"),
        ("node", "node"),
        ("bun", "bun"),
        ("qjs", "quickjs"),
        ("qjs-ng", "quickjs-ng"),
    ];

    for (exec_base, engine_name) in providers {
        let exec = if cfg!(windows) { format!("{}.exe", exec_base) } else { exec_base.to_string() };
        let local = bin_path.join(&exec);
        if local.exists() {
            return Some((engine_name.to_string(), local.to_string_lossy().to_string()));
        }

        let path_cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
        let found = new_silent_command(path_cmd)
            .arg(&exec)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.lines().next().unwrap_or("").trim().to_string());

        if let Some(p) = found {
            return Some((engine_name.to_string(), p));
        }
    }
    None
}

pub async fn analyze_js_runtime(_app_handle: &AppHandle, bin_path: &PathBuf) -> DependencyInfo {
    let providers = [
        ("deno", "Deno", "--version"),
        ("node", "Node.js", "--version"),
        ("bun", "Bun", "--version"),
        ("qjs", "QuickJS", "--version"),
        ("qjs-ng", "QuickJS", "--version"),
    ];

    for (exec_base, label, flag) in providers {
        let exec = if cfg!(windows) { format!("{}.exe", exec_base) } else { exec_base.to_string() };
        let mut info = resolve_binary_info(&exec, flag, bin_path);
        
        if !info.available { continue; }

        info.name = label.to_string();
        let version_str = info.version.clone().unwrap_or_default();
        
        let (supported, recommended) = match exec_base {
            "deno" => (deps::compare_semver(&version_str, "2.0.0"), true),
            "node" => (deps::compare_semver(&version_str, "20.0.0"), deps::compare_semver(&version_str, "22.0.0")),
            "bun" => (deps::compare_semver(&version_str, "1.0.31"), true),
            _ => (deps::compare_date(&version_str, "2023-12-09"), true),
        };

        info.is_supported = supported;
        info.is_recommended = recommended;
        return info;
    }

    DependencyInfo {
        name: "None".to_string(),
        available: false,
        version: None,
        path: None,
        is_supported: false,
        is_recommended: false,
        is_latest: false,
    }
}

#[tauri::command]
pub async fn check_dependencies(app_handle: AppHandle) -> AppDependencies {
    let app_dir = app_handle.path_resolver().app_data_dir().unwrap();
    let bin_dir = app_dir.join("bin");

    let (yt_res, ff_res, aria_res, js_res) = tokio::join!(
        async {
            let exec_name = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };
            let mut info = resolve_binary_info(exec_name, "--version", &bin_dir);
            info.name = "yt-dlp".to_string();
            info
        },
        async {
            let exec_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
            let mut info = resolve_binary_info(exec_name, "-version", &bin_dir);
            info.name = "FFmpeg".to_string();
            if let Some(ref v) = info.version {
                let re = Regex::new(r"ffmpeg version ([^\s]+)").unwrap();
                if let Some(caps) = re.captures(v) {
                    info.version = Some(caps[1].to_string());
                }
            }
            info
        },
        async {
            let exec_name = if cfg!(windows) { "aria2c.exe" } else { "aria2c" };
            let mut info = resolve_binary_info(exec_name, "--version", &bin_dir);
            info.name = "aria2c".to_string();
            if let Some(ref v) = info.version {
                let re = Regex::new(r"aria2 version ([^\s]+)").unwrap();
                if let Some(caps) = re.captures(v) {
                    info.version = Some(caps[1].to_string());
                }
            }
            info
        },
        analyze_js_runtime(&app_handle, &bin_dir)
    );

    AppDependencies {
        yt_dlp: yt_res,
        ffmpeg: ff_res,
        js_runtime: js_res,
        aria2: aria_res,
    }
}

#[tauri::command]
pub async fn install_dependency(app_handle: AppHandle, name: String) -> Result<(), String> {
    {
        let mut locks = INSTALL_LOCKS.lock().unwrap();
        if locks.contains(&name) {
            return Err(format!("Installation of {} is already in progress", name));
        }
        locks.insert(name.clone());
    }

    let result = deps::install_dep(name.clone(), app_handle).await;

    {
        let mut locks = INSTALL_LOCKS.lock().unwrap();
        locks.remove(&name);
    }
    
    result
}

#[tauri::command]
pub async fn sync_dependencies(app_handle: AppHandle) -> Result<AppDependencies, String> {
    let app_dir = app_handle.path_resolver().app_data_dir().ok_or("Failed to get app dir")?;
    let bin_dir = app_dir.join("bin");

    if !bin_dir.exists() {
        std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
    }

    let yt_exists = bin_dir.join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" }).exists();
    let ff_exists = bin_dir.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }).exists();

    if !yt_exists || !ff_exists {
        let _ = timeout(Duration::from_secs(60), async {
            let _ = tokio::join!(
                deps::auto_update_yt_dlp(app_handle.clone(), bin_dir.clone()),
                deps::install_missing_ffmpeg(app_handle.clone(), bin_dir.clone()),
            );
        }).await;
    } else {
        let app_bg = app_handle.clone();
        let bin_bg = bin_dir.clone();
        tauri::async_runtime::spawn(async move {
            let _ = deps::auto_update_yt_dlp(app_bg, bin_bg).await;
        });
    }

    Ok(check_dependencies(app_handle).await)
}

#[tauri::command]
pub fn open_external_link(app_handle: AppHandle, url: String) -> Result<(), String> {
    tauri::api::shell::open(&app_handle.shell_scope(), url, None)
        .map_err(|e| format!("Failed to open URL: {}", e))
}

#[tauri::command]
pub fn close_splash(app_handle: AppHandle) {
    if let Some(splash) = app_handle.get_window("splashscreen") {
        let _ = splash.close();
    }
    if let Some(main) = app_handle.get_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
}

#[tauri::command]
pub async fn get_latest_app_version() -> Result<String, String> {
    match timeout(Duration::from_secs(3), deps::get_latest_github_tag("zqily/multiyt-dlp")).await {
        Ok(res) => res,
        Err(_) => Err("Request timed out".into())
    }
}

#[tauri::command]
pub fn request_attention(app_handle: AppHandle) {
    if let Some(window) = app_handle.get_window("splashscreen") {
        let _ = window.request_user_attention(Some(tauri::UserAttentionType::Informational));
    }
}

#[tauri::command]
pub fn show_in_folder(path: String) -> Result<(), String> {
    let path_obj = std::path::Path::new(&path);
    if !path_obj.exists() {
        return Err(format!("File not found: {}", path));
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt; 
        let normalized_path = path.replace("/", "\\");
        let _ = Command::new("explorer")
            .arg("/select,")
            .raw_arg(format!("\"{}\"", normalized_path))
            .spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").args(["-R", &path]).spawn();
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = path_obj.parent() {
             let _ = Command::new("xdg-open").arg(parent).spawn();
        }
    }
    Ok(())
}

#[tauri::command]
pub fn open_log_folder() -> Result<(), String> {
    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    let log_dir = home.join(".multiyt-dlp").join("logs");

    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    }

    let cmd = if cfg!(target_os = "windows") { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = Command::new(cmd).arg(&log_dir).spawn();
    
    Ok(())
}