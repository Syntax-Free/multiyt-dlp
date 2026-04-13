use thiserror::Error;
use std::time::Duration;
use tracing::debug;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("Network IO failed: {0}")]
    Network(#[from] reqwest::Error),
    
    #[error("File system failed: {0}")]
    FileSystem(#[from] std::io::Error),
    
    #[error("Validation failed: {0}")]
    Validation(String),
    
    #[error("Server returned error status: {0}")]
    HttpStatus(u16),

    #[error("Max retries exceeded")]
    MaxRetriesExceeded,
}

pub struct RetryPolicy {
    max_retries: u32,
    current_attempt: u32,
    base_delay_ms: u64,
}

impl RetryPolicy {
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            current_attempt: 0,
            base_delay_ms: 1000, // Start with 1 second
        }
    }

    /// Calculates the next backoff duration.
    /// Returns None if max retries have been exceeded.
    pub fn next_backoff(&mut self) -> Option<Duration> {
        if self.current_attempt >= self.max_retries {
            debug!(target: "core::transport::retry", "RetryPolicy: Max retries ({}) exhausted.", self.max_retries);
            return None;
        }

        let delay = self.base_delay_ms * 2_u64.pow(self.current_attempt);
        self.current_attempt += 1;

        // Cap delay at 10 seconds
        let capped_delay = std::cmp::min(delay, 10_000);
        
        debug!(target: "core::transport::retry", "RetryPolicy: Attempt {}/{}. Backing off for {}ms", self.current_attempt, self.max_retries, capped_delay);
        
        Some(Duration::from_millis(capped_delay))
    }
}
