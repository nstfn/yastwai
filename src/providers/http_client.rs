/*!
 * Shared HTTP client builder for providers.
 *
 * Centralizes reqwest client configuration to ensure consistent
 * connection pooling, timeouts, and keepalive settings.
 */

use std::time::Duration;
use reqwest::Client;

/// Build an HTTP client tuned for LLM provider requests.
///
/// # Arguments
/// * `timeout_secs` - Request timeout in seconds
/// * `pool_max_idle_per_host` - Max idle connections per host
/// * `http1_only` - Force HTTP/1.1 (needed for many local servers)
pub fn build_provider_client(
    timeout_secs: u64,
    pool_max_idle_per_host: usize,
    http1_only: bool,
) -> Client {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(pool_max_idle_per_host)
        .tcp_keepalive(Duration::from_secs(60));

    if http1_only {
        builder = builder.http1_only();
    }

    builder.build().unwrap_or_default()
}
