use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use crate::core::transport::retry::TransportError;
use regex::Regex;

pub struct AriaEngine {
    url: String,
    target_path: std::path::PathBuf,
    aria_bin: std::path::PathBuf,
    fallback_size: Option<u64>,
}

impl AriaEngine {
    pub fn new(url: &str, target_path: std::path::PathBuf, aria_bin: std::path::PathBuf, fallback_size: Option<u64>) -> Self {
        Self {
            url: url.to_string(),
            target_path,
            aria_bin,
            fallback_size,
        }
    }

    /// Parses Aria2 size strings (e.g., "53MiB", "5.9KiB", "100B") into bytes
    fn parse_aria_size(input: &str) -> Option<f64> {
        let clean = input.trim();
        let units = [
            ("GiB", 1024.0 * 1024.0 * 1024.0), 
            ("MiB", 1024.0 * 1024.0), 
            ("KiB", 1024.0),
            ("B", 1.0)
        ];

        for (unit, multiplier) in units {
            if clean.ends_with(unit) {
                let num_part = clean.trim_end_matches(unit);
                if let Ok(val) = num_part.parse::<f64>() {
                    return Some(val * multiplier);
                }
            }
        }
        
        // Fallback: try parsing as raw number
        clean.parse::<f64>().ok()
    }

    pub async fn execute<F>(&self, on_progress: F) -> Result<(), TransportError>
    where
        F: Fn(u64, u64, f64) + Send + Sync + 'static,
    {
        // Setup output directory and filename
        let dir = self.target_path.parent().ok_or(TransportError::Validation("Invalid path".into()))?;
        let filename = self.target_path.file_name().ok_or(TransportError::Validation("Invalid filename".into()))?;
        
        let tmp_filename = format!("{}.tmp", filename.to_string_lossy());
        let tmp_path = dir.join(&tmp_filename);
        
        // Ensure no leftover tmp
        let _ = tokio::fs::remove_file(&tmp_path).await;

        let mut cmd = Command::new(&self.aria_bin);
        
        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); 
        }

        cmd.arg(&self.url)
           .arg("-d").arg(dir)
           .arg("-o").arg(&tmp_filename)
           .arg("-s").arg("8") // 8 connections
           .arg("-x").arg("8") // 8 connections per server
           .arg("-j").arg("1") // 1 download at a time
           .arg("--min-split-size=1M")
           .arg("--allow-overwrite=true")
           .arg("--summary-interval=1") // Force periodic status lines (every 1s) to allow parsing
           .stdout(Stdio::piped())
           .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(TransportError::FileSystem)?;
        
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let mut reader = BufReader::new(stdout).lines();

        // Regex for Aria2 console output: 
        // Example: [#42b0a0 53MiB/691MiB(7%) CN:16 DL:5.9MiB ETA:1m47s]
        // Captures: Current Size, Total Size, Speed
        let re = Regex::new(r"(?P<current>[\d.]+[A-Za-z]+)/(?P<total>[\d.]+[A-Za-z]+)\((?P<percent>[\d.]+)%\).*?DL:(?P<speed>[\d.]+[A-Za-z]+)").unwrap();

        // Fallback Regex (Percentage only, if total size is unknown to aria2 initially)
        let re_fallback = Regex::new(r"\((?P<percent>[\d.]+)%\)").unwrap();

        while let Ok(Some(line)) = reader.next_line().await {
            // Try full parsing first
            if let Some(caps) = re.captures(&line) {
                let current_str = caps.name("current").map_or("", |m| m.as_str());
                let total_str = caps.name("total").map_or("", |m| m.as_str());
                let speed_str = caps.name("speed").map_or("", |m| m.as_str());

                let current_bytes = Self::parse_aria_size(current_str).unwrap_or(0.0) as u64;
                let total_bytes = Self::parse_aria_size(total_str).unwrap_or(0.0) as u64;
                let speed_bytes_sec = Self::parse_aria_size(speed_str).unwrap_or(0.0);

                // If aria2 reports 0 total (start up), use fallback
                let effective_total = if total_bytes > 0 { total_bytes } else { self.fallback_size.unwrap_or(0) };

                on_progress(current_bytes, effective_total, speed_bytes_sec);
            } 
            // Fallback parsing if formatting is different (e.g. unknown size)
            else if let Some(caps) = re_fallback.captures(&line) {
                if let Some(p_match) = caps.name("percent") {
                    if let Ok(p_val) = p_match.as_str().parse::<f64>() {
                        // We have percentage, but maybe not exact bytes.
                        // We can estimate bytes if we have a fallback size.
                        let total = self.fallback_size.unwrap_or(0);
                        let current = ((p_val / 100.0) * (total as f64)) as u64;
                        on_progress(current, total, 0.0);
                    }
                }
            }
        }

        let status = child.wait().await.map_err(TransportError::FileSystem)?;
        
        if status.success() {
            crate::core::deps::replace_dependency_robust_sync(&tmp_path, &self.target_path).map_err(TransportError::FileSystem)?;
            
            // Ensure 100% is reported on success
            let total = self.fallback_size.unwrap_or(0);
            on_progress(total, total, 0.0);
            Ok(())
        } else {
            // Cleanup partial tmp if failed
            let _ = tokio::fs::remove_file(&tmp_path).await;
            Err(TransportError::Validation(format!("Aria2 exited with code {:?}", status.code())))
        }
    }
}