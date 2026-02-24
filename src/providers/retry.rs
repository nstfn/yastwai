/*!
 * Shared retry policy for provider requests.
 *
 * Provides exponential backoff with configurable parameters,
 * used by all provider implementations.
 */

use std::future::Future;
use std::time::Duration;

use log::error;
use tokio::time::sleep;

use crate::errors::ProviderError;

/// Retry policy with exponential backoff.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (0 = no retries)
    pub max_retries: u32,
    /// Base backoff time in milliseconds
    pub backoff_base_ms: u64,
}

impl RetryPolicy {
    pub fn new(max_retries: u32, backoff_base_ms: u64) -> Self {
        Self { max_retries, backoff_base_ms }
    }

    /// Execute an async operation with retry logic.
    ///
    /// Retries on connection errors, rate limits, and server errors (5xx).
    /// Does NOT retry on client errors (4xx except 429) or auth errors.
    pub async fn execute<T, F, Fut>(&self, mut operation: F) -> Result<T, ProviderError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, ProviderError>>,
    {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts <= self.max_retries {
            if attempts > 0 {
                let backoff_ms = self.backoff_base_ms * (1u64 << (attempts - 1));
                sleep(Duration::from_millis(backoff_ms)).await;
            }

            attempts += 1;

            match operation().await {
                Ok(result) => return Ok(result),
                Err(err) => {
                    if is_retryable(&err) {
                        error!("Provider request failed (attempt {}/{}): {}",
                               attempts, self.max_retries + 1, err);

                        // Extra wait for rate limits
                        if let ProviderError::RateLimitExceeded { retry_after_secs, .. } = &err {
                            let wait_ms = retry_after_secs
                                .map(|s| s * 1000)
                                .unwrap_or(self.backoff_base_ms * 5 * (1u64 << (attempts - 1)));
                            sleep(Duration::from_millis(wait_ms)).await;
                        }

                        last_error = Some(err);
                    } else {
                        return Err(err);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(||
            ProviderError::ConnectionError("All retry attempts failed".to_string())))
    }
}

fn is_retryable(err: &ProviderError) -> bool {
    match err {
        ProviderError::ConnectionError(_) => true,
        ProviderError::RateLimitExceeded { .. } => true,
        ProviderError::ApiError { status_code, .. } => *status_code == 429 || *status_code >= 500,
        _ => false,
    }
}
