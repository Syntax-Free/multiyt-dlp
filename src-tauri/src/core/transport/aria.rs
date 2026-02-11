use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use crate::core::transport::retry::TransportError;
use regex::Regex;

pub struct AriaEngine {
    url: String,
    target_path: std::path::PathBuf,
    aria_bin: std::path::PathBuf,
}

impl AriaEngine {
    pub fn new(url: &str, target_path: std::path::PathBuf, aria_bin: std::path::PathBuf) -> Self {
        Self {
            url: url.to_string(),
            target_path,
            aria_bin,
        }
    }

    pub async fn execute<F>(&self, on_progress: F) -> Result<(), TransportError>
    where
        F: Fn(u64, u64, f64) + Send + Sync + 'static,
    {
        // Setup output directory and filename
        let dir = self.target_path.parent().ok_or(TransportError::Validation("Invalid path".into()))?;
        let filename = self.target_path.file_name().ok_or(TransportError::Validation("Invalid filename".into()))?;
        
        let mut cmd = Command::new(&self.aria_bin);
        
        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); 
        }

        cmd.arg(&self.url)
           .arg("-d").arg(dir)
           .arg("-o").arg(filename)
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

        // Regex for Aria2 console output: [#2089b0 400.0KiB/30.0MiB(1%) CN:1 ...]
        // We really just need the percentage or bytes to calculate progress
        let re = Regex::new(r"\((?P<percent>[\d.]+)%\)").unwrap();

        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(caps) = re.captures(&line) {
                if let Some(p_match) = caps.name("percent") {
                    if let Ok(p_val) = p_match.as_str().parse::<f64>() {
                        // Aria2 gives percentage. We mock total bytes for the callback contract
                        // logic in mod.rs converts this back to percentage anyway.
                        on_progress(p_val as u64, 100, 0.0);
                    }
                }
            }
        }

        let status = child.wait().await.map_err(TransportError::FileSystem)?;
        
        if status.success() {
            on_progress(100, 100, 0.0);
            Ok(())
        } else {
            Err(TransportError::Validation(format!("Aria2 exited with code {:?}", status.code())))
        }
    }
}