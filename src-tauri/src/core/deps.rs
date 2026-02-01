use std::fs::{self, File};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use serde::Serialize;
use std::process::Command;
use async_trait::async_trait;
use crate::core::transport::download_file_robust;

#[cfg(target_os = "windows")]
const YT_DLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
#[cfg(target_os = "macos")]
const YT_DLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos";
#[cfg(target_os = "linux")]
const YT_DLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";

#[cfg(target_os = "windows")]
const FFMPEG_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
#[cfg(target_os = "macos")]
const FFMPEG_URL: &str = "https://evermeet.cx/ffmpeg/ffmpeg-113374-g80f9281204.zip"; 
#[cfg(target_os = "linux")]
const FFMPEG_URL: &str = "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz";

#[cfg(target_os = "windows")]
const DENO_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
#[cfg(target_os = "macos")]
const DENO_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-apple-darwin.zip"; 
#[cfg(target_os = "linux")]
const DENO_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip";

#[derive(Clone, Serialize)]
struct InstallProgressPayload {
    name: String,
    percentage: u64,
    status: String,
}

#[async_trait]
pub trait DependencyProvider: Send + Sync {
    fn get_name(&self) -> String;
    fn get_binaries(&self) -> Vec<&str>;
    async fn install(&self, app_handle: AppHandle, target_dir: PathBuf) -> Result<(), String>;
}

// --- Helper Functions ---

pub async fn get_latest_github_tag(repo: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("Multiyt-dlp/2.1")
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    
    let resp = client.get(&url)
        .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API Error: {}", resp.status()));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    
    json.get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Could not find tag_name in response".to_string())
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

fn get_local_version(path: &PathBuf, arg: &str) -> Option<String> {
    if !path.exists() { return None; }
    
    let output = new_silent_command(path.to_str()?)
        .arg(arg)
        .output()
        .ok()?;

    if !output.status.success() { return None; }
    
    let out_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(out_str)
}

// --- Extraction Logic ---

fn extract_zip_finding_binary(zip_path: &PathBuf, target_dir: &PathBuf, binary_names: &[&str]) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let outpath = match file.enclosed_name() {
            Some(path) => path.to_owned(),
            None => continue,
        };

        if let Some(file_name) = outpath.file_name() {
            let file_name_str = file_name.to_string_lossy();
            if binary_names.contains(&file_name_str.as_ref()) {
                let mut out_file = File::create(target_dir.join(file_name)).map_err(|e| e.to_string())?;
                std::io::copy(&mut file, &mut out_file).map_err(|e| e.to_string())?;
                
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = out_file.metadata().map_err(|e| e.to_string())?.permissions();
                    perms.set_mode(0o755);
                    out_file.set_permissions(perms).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    Ok(())
}

fn extract_tar_xz_finding_binary(tar_path: &PathBuf, target_dir: &PathBuf, binary_names: &[&str]) -> Result<(), String> {
    let tar_gz = File::open(tar_path).map_err(|e| e.to_string())?;
    let tar = xz2::read::XzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        
        if let Some(file_name) = path.file_name() {
            let file_name_str = file_name.to_string_lossy();
            if binary_names.contains(&file_name_str.as_ref()) {
                entry.unpack(target_dir.join(file_name)).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

// --- Providers ---

pub struct YtDlpProvider;
#[async_trait]
impl DependencyProvider for YtDlpProvider {
    fn get_name(&self) -> String { "yt-dlp".to_string() }
    fn get_binaries(&self) -> Vec<&str> {
        if cfg!(windows) { vec!["yt-dlp.exe"] } else { vec!["yt-dlp"] }
    }
    async fn install(&self, app_handle: AppHandle, target_dir: PathBuf) -> Result<(), String> {
        let filename = self.get_binaries()[0];
        let target_path = target_dir.join(filename);
        
        // Use Robust Transport
        download_file_robust(YT_DLP_URL, target_path, "yt-dlp", &app_handle)
            .await
            .map_err(|e| format!("Transport Error: {}", e))
    }
}

pub struct FfmpegProvider;
#[async_trait]
impl DependencyProvider for FfmpegProvider {
    fn get_name(&self) -> String { "ffmpeg".to_string() }
    fn get_binaries(&self) -> Vec<&str> {
        if cfg!(windows) { vec!["ffmpeg.exe", "ffprobe.exe"] } else { vec!["ffmpeg", "ffprobe"] }
    }
    async fn install(&self, app_handle: AppHandle, target_dir: PathBuf) -> Result<(), String> {
        let archive_name = if cfg!(windows) || cfg!(target_os = "macos") { "ffmpeg.zip" } else { "ffmpeg.tar.xz" };
        let temp_dir = std::env::temp_dir();
        let archive_path = temp_dir.join(archive_name);

        // Use Robust Transport
        download_file_robust(FFMPEG_URL, archive_path.clone(), "ffmpeg", &app_handle)
            .await
            .map_err(|e| format!("Transport Error: {}", e))?;

        let _ = app_handle.emit_all("install-progress", InstallProgressPayload {
            name: "ffmpeg".to_string(), percentage: 100, status: "Extracting...".to_string()
        });

        if archive_path.extension().unwrap_or_default() == "zip" {
            extract_zip_finding_binary(&archive_path, &target_dir, &self.get_binaries())?;
        } else {
            extract_tar_xz_finding_binary(&archive_path, &target_dir, &self.get_binaries())?;
        }

        let _ = fs::remove_file(archive_path);
        Ok(())
    }
}

pub struct DenoProvider;
#[async_trait]
impl DependencyProvider for DenoProvider {
    fn get_name(&self) -> String { "js_runtime".to_string() }
    fn get_binaries(&self) -> Vec<&str> {
        if cfg!(windows) { vec!["deno.exe"] } else { vec!["deno"] }
    }
    async fn install(&self, app_handle: AppHandle, target_dir: PathBuf) -> Result<(), String> {
        let archive_path = std::env::temp_dir().join("deno.zip");

        // Use Robust Transport
        download_file_robust(DENO_URL, archive_path.clone(), "js_runtime", &app_handle)
            .await
            .map_err(|e| format!("Transport Error: {}", e))?;

        let _ = app_handle.emit_all("install-progress", InstallProgressPayload {
            name: "js_runtime".to_string(), percentage: 100, status: "Extracting...".to_string()
        });

        extract_zip_finding_binary(&archive_path, &target_dir, &self.get_binaries())?;
        let _ = fs::remove_file(archive_path);
        Ok(())
    }
}

// --- High Level Functions ---

pub async fn auto_update_yt_dlp(app_handle: AppHandle, bin_dir: PathBuf) -> Result<(), String> {
    let provider = YtDlpProvider;
    let binary_name = provider.get_binaries()[0];
    let local_path = bin_dir.join(binary_name);

    // Try to get remote tag. If network fails, proceed silently if local exists.
    let remote_tag = match get_latest_github_tag("yt-dlp/yt-dlp").await {
        Ok(t) => t,
        Err(e) => {
            if !local_path.exists() { return Err(e); }
            return Ok(());
        }
    };

    if let Some(local_ver) = get_local_version(&local_path, "--version") {
        if local_ver.trim() == remote_tag.trim() {
            return Ok(());
        }
    }

    let _ = app_handle.emit_all("install-progress", InstallProgressPayload {
        name: "yt-dlp".to_string(),
        percentage: 0,
        status: format!("Updating to {}...", remote_tag)
    });
    
    provider.install(app_handle, bin_dir).await
}

pub async fn manage_js_runtime(app_handle: AppHandle, bin_dir: PathBuf) -> Result<(), String> {
    if new_silent_command("deno").arg("--version").output().is_ok() { return Ok(()); }
    if new_silent_command("bun").arg("--version").output().is_ok() { return Ok(()); }
    if new_silent_command("node").arg("--version").output().is_ok() { return Ok(()); }

    let provider = DenoProvider;
    let binary_name = provider.get_binaries()[0];
    let local_path = bin_dir.join(binary_name);

    // Offline Handling
    let remote_tag = match get_latest_github_tag("denoland/deno").await {
        Ok(t) => t,
        Err(e) => {
             if !local_path.exists() { return Err(e); }
             return Ok(());
        }
    };
    
    let clean_remote = remote_tag.replace("v", ""); 

    if let Some(local_ver_raw) = get_local_version(&local_path, "--version") {
        if local_ver_raw.contains(&clean_remote) {
            return Ok(());
        }
    }

    let _ = app_handle.emit_all("install-progress", InstallProgressPayload {
        name: "Portable Runtime".to_string(),
        percentage: 0,
        status: format!("Syncing Deno {}...", clean_remote)
    });

    provider.install(app_handle, bin_dir).await
}

pub async fn install_missing_ffmpeg(app_handle: AppHandle, bin_dir: PathBuf) -> Result<(), String> {
    let provider = FfmpegProvider;
    let binary_name = provider.get_binaries()[0]; 
    let local_path = bin_dir.join(binary_name);
    
    let exec_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if new_silent_command(exec_name).arg("-version").output().is_ok() {
        return Ok(()); 
    }

    if !local_path.exists() {
         let _ = app_handle.emit_all("install-progress", InstallProgressPayload {
            name: "ffmpeg".to_string(), percentage: 0, status: "Installing...".to_string()
        });
        provider.install(app_handle, bin_dir).await?;
    }
    Ok(())
}

pub fn get_provider(name: &str) -> Option<Box<dyn DependencyProvider>> {
    match name {
        "yt-dlp" => Some(Box::new(YtDlpProvider)),
        "ffmpeg" => Some(Box::new(FfmpegProvider)),
        "js_runtime" => Some(Box::new(DenoProvider)),
        _ => None
    }
}

pub async fn install_dep(name: String, app_handle: AppHandle) -> Result<(), String> {
    let provider = get_provider(&name).ok_or("Unknown dependency")?;
    
    let app_dir = app_handle.path_resolver().app_data_dir().ok_or("Failed to resolve app data dir")?;
    let bin_dir = app_dir.join("bin");
    
    if !bin_dir.exists() {
        fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
    }

    provider.install(app_handle.clone(), bin_dir).await?;

    let installed_name = provider.get_name();
    let _ = app_handle.emit_all("install-progress", InstallProgressPayload {
        name: installed_name, 
        percentage: 100, 
        status: "Installed".to_string()
    });

    Ok(())
}