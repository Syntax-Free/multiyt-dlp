use std::fs::{self, File};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use serde::Serialize;
use std::process::Command;
use async_trait::async_trait;
use crate::core::transport::download_file_robust;
use regex::Regex;
use tokio::time::{timeout, Duration, sleep};

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

#[cfg(target_os = "windows")]
const BUN_URL: &str = "https://github.com/oven-sh/bun/releases/latest/download/bun-windows-x64.zip";
#[cfg(target_os = "macos")]
const BUN_URL: &str = "https://github.com/oven-sh/bun/releases/latest/download/bun-darwin-aarch64.zip";
#[cfg(target_os = "linux")]
const BUN_URL: &str = "https://github.com/oven-sh/bun/releases/latest/download/bun-linux-x64.zip";

#[cfg(target_os = "windows")]
const ARIA2_URL: &str = "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip";
#[cfg(target_os = "macos")]
const ARIA2_URL: &str = "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-osx-darwin.tar.bz2";
#[cfg(target_os = "linux")]
const ARIA2_URL: &str = "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-linux-gnu-64bit-build1.tar.bz2";

// Static Fallback Sizes (in bytes)
const YT_DLP_SIZE: u64 = 17_500_000;
const FFMPEG_SIZE: u64 = 94_600_000;
const DENO_SIZE: u64 = 116_000_000;
const BUN_SIZE: u64 = 97_700_000;
const ARIA2_SIZE: u64 = 5_380_000;

#[derive(Clone, Serialize)]
pub struct InstallProgressPayload {
    pub name: String,
    pub percentage: u64,
    pub status: String,
}

#[async_trait]
pub trait DependencyProvider: Send + Sync {
    fn get_name(&self) -> String;
    fn get_binaries(&self) -> Vec<&str>;
    async fn install(&self, app_handle: AppHandle, target_dir: PathBuf) -> Result<(), String>;
    async fn check_update_available(&self, bin_dir: &PathBuf) -> Result<bool, String>;
}

// --- GitHub Helper with Robust Retries & Timeout ---

pub async fn get_latest_github_tag(repo: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("Multiyt-dlp/2.2")
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    let mut last_error = String::new();
    let max_retries = 3;

    for attempt in 0..max_retries {
        match timeout(Duration::from_secs(5), client.get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
            .send()).await 
        {
            Ok(Ok(resp)) => {
                if resp.status().is_success() {
                    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
                    if let Some(tag) = json.get("tag_name").and_then(|v| v.as_str()) {
                        return Ok(tag.to_string());
                    }
                    return Err("Malformed GitHub response: tag_name missing".to_string());
                } else if resp.status() == reqwest::StatusCode::FORBIDDEN || resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    return Err("GitHub API Rate Limited. Please try again later.".to_string());
                }
                last_error = format!("HTTP Status {}", resp.status());
            },
            Ok(Err(e)) => last_error = format!("Network Error: {}", e),
            Err(_) => last_error = "Connection Timeout".to_string(),
        }

        if attempt < max_retries - 1 {
            sleep(Duration::from_secs((attempt + 1) as u64)).await;
        }
    }

    Err(format!("Update check failed after {} retries. Last error: {}", max_retries, last_error))
}

// --- Version Comparison Helpers ---

pub fn compare_semver(current: &str, required: &str) -> bool {
    let re = Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap();
    let c = re.captures(current);
    let r = re.captures(required);

    if let (Some(cc), Some(rc)) = (c, r) {
        let cv = (cc[1].parse::<u32>().unwrap(), cc[2].parse::<u32>().unwrap(), cc[3].parse::<u32>().unwrap());
        let rv = (rc[1].parse::<u32>().unwrap(), rc[2].parse::<u32>().unwrap(), rc[3].parse::<u32>().unwrap());
        return cv >= rv;
    }
    false
}

pub fn compare_date(current: &str, required: &str) -> bool {
    let re = Regex::new(r"(\d{4})-(\d{2})-(\d{2})").unwrap();
    let c = re.captures(current);
    let r = re.captures(required);

    if let (Some(cc), Some(rc)) = (c, r) {
        let cv = (cc[1].parse::<u32>().unwrap(), cc[2].parse::<u32>().unwrap(), cc[3].parse::<u32>().unwrap());
        let rv = (rc[1].parse::<u32>().unwrap(), rc[2].parse::<u32>().unwrap(), rc[3].parse::<u32>().unwrap());
        return cv >= rv;
    }
    false
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

pub fn get_local_version(path: &PathBuf, arg: &str) -> Option<String> {
    if !path.exists() { return None; }
    let output = new_silent_command(path.to_str()?).arg(arg).output().ok()?;
    if !output.status.success() { return None; }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// --- Extraction Helpers ---

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
            }
        }
    }
    Ok(())
}

// --- Dependency Providers ---

pub struct YtDlpProvider;
#[async_trait]
impl DependencyProvider for YtDlpProvider {
    fn get_name(&self) -> String { "yt-dlp".to_string() }
    fn get_binaries(&self) -> Vec<&str> { if cfg!(windows) { vec!["yt-dlp.exe"] } else { vec!["yt-dlp"] } }
    async fn install(&self, app_handle: AppHandle, target_dir: PathBuf) -> Result<(), String> {
        let target_path = target_dir.join(self.get_binaries()[0]);
        download_file_robust(YT_DLP_URL, target_path, &self.get_name(), &app_handle, Some(YT_DLP_SIZE)).await.map_err(|e| e.to_string())
    }
    async fn check_update_available(&self, bin_dir: &PathBuf) -> Result<bool, String> {
        let local_path = bin_dir.join(self.get_binaries()[0]);
        if !local_path.exists() { return Ok(true); }
        let remote_tag = get_latest_github_tag("yt-dlp/yt-dlp").await?;
        Ok(get_local_version(&local_path, "--version").map_or(true, |v| v.trim() != remote_tag.trim()))
    }
}

pub struct FfmpegProvider;
#[async_trait]
impl DependencyProvider for FfmpegProvider {
    fn get_name(&self) -> String { "FFmpeg".to_string() }
    fn get_binaries(&self) -> Vec<&str> { if cfg!(windows) { vec!["ffmpeg.exe"] } else { vec!["ffmpeg"] } }
    async fn install(&self, app_handle: AppHandle, target_dir: PathBuf) -> Result<(), String> {
        let archive_path = std::env::temp_dir().join("ffmpeg_tmp");
        download_file_robust(FFMPEG_URL, archive_path.clone(), &self.get_name(), &app_handle, Some(FFMPEG_SIZE)).await.map_err(|e| e.to_string())?;
        let _ = app_handle.emit_all("install-progress", InstallProgressPayload {
            name: self.get_name(),
            percentage: 100,
            status: "Extracting FFmpeg...".to_string()
        });
        extract_zip_finding_binary(&archive_path, &target_dir, &self.get_binaries())?;
        let _ = fs::remove_file(archive_path);
        Ok(())
    }
    async fn check_update_available(&self, _bin_dir: &PathBuf) -> Result<bool, String> { Ok(false) }
}

pub struct DenoProvider;
#[async_trait]
impl DependencyProvider for DenoProvider {
    fn get_name(&self) -> String { "Deno".to_string() }
    fn get_binaries(&self) -> Vec<&str> { if cfg!(windows) { vec!["deno.exe"] } else { vec!["deno"] } }
    async fn install(&self, app_handle: AppHandle, target_dir: PathBuf) -> Result<(), String> {
        let archive_path = std::env::temp_dir().join("deno.zip");
        download_file_robust(DENO_URL, archive_path.clone(), &self.get_name(), &app_handle, Some(DENO_SIZE)).await.map_err(|e| e.to_string())?;
        extract_zip_finding_binary(&archive_path, &target_dir, &self.get_binaries())?;
        let _ = fs::remove_file(archive_path);
        Ok(())
    }
    async fn check_update_available(&self, bin_dir: &PathBuf) -> Result<bool, String> {
        let local_path = bin_dir.join(self.get_binaries()[0]);
        if !local_path.exists() { return Ok(true); }
        let remote_tag = get_latest_github_tag("denoland/deno").await?;
        let clean_remote = remote_tag.replace('v', "");
        Ok(get_local_version(&local_path, "--version").map_or(true, |v| !v.contains(&clean_remote)))
    }
}

pub struct BunProvider;
#[async_trait]
impl DependencyProvider for BunProvider {
    fn get_name(&self) -> String { "Bun".to_string() }
    fn get_binaries(&self) -> Vec<&str> { if cfg!(windows) { vec!["bun.exe"] } else { vec!["bun"] } }
    async fn install(&self, app_handle: AppHandle, target_dir: PathBuf) -> Result<(), String> {
        let archive_path = std::env::temp_dir().join("bun.zip");
        download_file_robust(BUN_URL, archive_path.clone(), &self.get_name(), &app_handle, Some(BUN_SIZE)).await.map_err(|e| e.to_string())?;
        extract_zip_finding_binary(&archive_path, &target_dir, &self.get_binaries())?;
        let _ = fs::remove_file(archive_path);
        Ok(())
    }
    async fn check_update_available(&self, bin_dir: &PathBuf) -> Result<bool, String> {
        let local_path = bin_dir.join(self.get_binaries()[0]);
        if !local_path.exists() { return Ok(true); }
        let remote_tag = get_latest_github_tag("oven-sh/bun").await?;
        let clean_remote = remote_tag.replace('v', "");
        Ok(get_local_version(&local_path, "--version").map_or(true, |v| !v.contains(&clean_remote)))
    }
}

pub struct Aria2Provider;
#[async_trait]
impl DependencyProvider for Aria2Provider {
    fn get_name(&self) -> String { "Aria2".to_string() }
    fn get_binaries(&self) -> Vec<&str> { if cfg!(windows) { vec!["aria2c.exe"] } else { vec!["aria2c"] } }
    async fn install(&self, app_handle: AppHandle, target_dir: PathBuf) -> Result<(), String> {
        let archive_path = std::env::temp_dir().join("aria2_tmp.zip");
        download_file_robust(ARIA2_URL, archive_path.clone(), &self.get_name(), &app_handle, Some(ARIA2_SIZE)).await.map_err(|e| e.to_string())?;
        extract_zip_finding_binary(&archive_path, &target_dir, &self.get_binaries())?;
        let _ = fs::remove_file(archive_path);
        Ok(())
    }
    async fn check_update_available(&self, _bin_dir: &PathBuf) -> Result<bool, String> { Ok(false) }
}

pub fn get_provider(name: &str) -> Option<Box<dyn DependencyProvider>> {
    match name.to_lowercase().as_str() {
        "yt-dlp" => Some(Box::new(YtDlpProvider)),
        "ffmpeg" => Some(Box::new(FfmpegProvider)),
        "deno" => Some(Box::new(DenoProvider)),
        "bun" => Some(Box::new(BunProvider)),
        "aria2" | "aria2c" => Some(Box::new(Aria2Provider)),
        _ => None
    }
}

pub async fn install_dep(name: String, app_handle: AppHandle) -> Result<(), String> {
    let provider = get_provider(&name).ok_or("Unknown dependency")?;
    let app_dir = app_handle.path_resolver().app_data_dir().ok_or("AppData dir not found")?;
    let bin_dir = app_dir.join("bin");
    if !bin_dir.exists() { fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?; }
    provider.install(app_handle, bin_dir).await
}