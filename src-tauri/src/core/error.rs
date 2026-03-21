use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
pub enum AppError {
    #[error("I/O Error: {0}")]
    IoError(String),

    #[error("yt-dlp process failed with exit code {exit_code}: {stderr}")]
    #[allow(dead_code)]
    ProcessFailed { exit_code: i32, stderr: String },

    #[error("Validation failed: {0}")]
    ValidationFailed(String),
}

// Required to convert from std::io::Error
impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::IoError(err.to_string())
    }
}
