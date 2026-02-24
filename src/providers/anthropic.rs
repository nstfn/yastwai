use std::time::{Duration, Instant};
use serde::{de::DeserializeOwned, Serialize, Deserialize};
use anyhow::{Result, Context, anyhow};
use reqwest::Client;
use tokio::time::sleep;
use tokio::sync::Mutex;

use crate::errors::ProviderError;
use super::{Provider, Role, TranslationRequest, TranslationResponse};


/// Token bucket rate limiter implementation
///
/// This rate limiter implements the token bucket algorithm:
/// - A bucket holds tokens up to a maximum capacity
/// - Tokens are consumed when API requests are made
/// - Tokens are refilled at a steady rate over time
/// - If the bucket is empty, requests wait until tokens are available
///
/// This helps prevent rate limit errors from the Anthropic API, which has a
/// limit of 50 requests per minute for most accounts.
#[derive(Debug)]
struct TokenBucketRateLimiter {
    /// Maximum number of tokens in the bucket
    capacity: u32,
    
    /// Current number of tokens in the bucket
    tokens: u32,
    
    /// Time of last token refill
    last_refill: Instant,
    
    /// Refill rate in tokens per second
    refill_rate: f64,
}

impl TokenBucketRateLimiter {
    /// Create a new token bucket rate limiter
    fn new(requests_per_minute: u32) -> Self {
        // Calculate tokens per second from requests per minute
        let refill_rate = requests_per_minute as f64 / 60.0;
        
        Self {
            capacity: requests_per_minute,
            tokens: requests_per_minute, // Start with a full bucket
            last_refill: Instant::now(),
            refill_rate,
        }
    }
    
    /// Refill the token bucket based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let elapsed_secs = elapsed.as_secs_f64();
        
        // Calculate how many tokens to add based on elapsed time and refill rate
        let new_tokens = (elapsed_secs * self.refill_rate).floor() as u32;
        
        if new_tokens > 0 {
            // Add tokens up to capacity
            self.tokens = (self.tokens + new_tokens).min(self.capacity);
            self.last_refill = now;
        }
    }
    
    /// Try to consume a token from the bucket
    async fn consume(&mut self) -> bool {
        self.refill();
        
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }
    
    /// Wait until a token is available
    async fn wait_for_token(&mut self) {
        while !self.consume().await {
            // If no tokens are available, sleep for a short duration
            // Calculate time until next token is available
            let time_to_next_token_secs = 1.0 / self.refill_rate;
            let wait_ms = (time_to_next_token_secs * 1000.0).ceil() as u64;
            
            // Add small buffer to ensure token is ready
            sleep(Duration::from_millis(wait_ms + 10)).await;
            
            // Refill bucket after waiting
            self.refill();
        }
    }
}

/// Anthropic client for interacting with Anthropic API
#[derive(Debug)]
pub struct Anthropic {
    /// HTTP client for API requests
    client: Client,
    /// API key for authentication
    api_key: String,
    /// API endpoint URL (optional, defaults to public API)
    endpoint: String,
    /// Maximum number of retries for transient errors
    max_retries: u32,
    /// Initial backoff duration for retry in milliseconds
    initial_backoff_ms: u64,
    /// Rate limiter (optional)
    rate_limiter: Option<Mutex<TokenBucketRateLimiter>>,
}

/// Anthropic message request
#[derive(Debug, Serialize)]
pub struct AnthropicRequest {
    /// The model to use
    model: String,
    
    /// The messages for the conversation
    messages: Vec<AnthropicMessage>,
    
    /// System prompt to guide the AI
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    
    /// Temperature for generation
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    
    /// Maximum number of tokens to generate
    max_tokens: u32,
    
    /// Top probability mass to consider (nucleus sampling)
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    
    /// Top k tokens to consider
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
}

/// Anthropic message format
#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicMessage {
    /// Role of the message sender
    pub role: Role,

    /// Content of the message
    pub content: String,
}

/// Token usage information
#[derive(Debug, Deserialize)]
pub struct TokenUsage {
    /// Number of input tokens
    pub input_tokens: u32,
    /// Number of output tokens
    pub output_tokens: u32,
}

/// Anthropic response
#[derive(Debug, Deserialize)]
pub struct AnthropicResponse {
    /// The content of the response
    pub content: Vec<AnthropicContent>,
    /// Token usage information
    pub usage: TokenUsage,
}

/// Individual content block in an Anthropic response
#[derive(Debug, Deserialize)]
pub struct AnthropicContent {
    /// The type of content
    #[serde(rename = "type")]
    pub content_type: String,
    
    /// The actual text content
    pub text: String,
}

impl Default for AnthropicRequest {
    fn default() -> Self {
        Self {
            model: String::new(),
            messages: Vec::new(),
            system: None,
            temperature: None,
            max_tokens: 4096,
            top_p: None,
            top_k: None,
        }
    }
}

/// Builder methods for AnthropicRequest
impl AnthropicRequest {
    /// Create a new Anthropic request
    pub fn new(model: impl Into<String>, max_tokens: u32) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            ..Default::default()
        }
    }
    
    /// Add a message to the request
    pub fn add_message(mut self, role: Role, content: impl Into<String>) -> Self {
        self.messages.push(AnthropicMessage {
            role,
            content: content.into(),
        });
        self
    }
    
    /// Set the system prompt
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }
    
    /// Set the temperature
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }
    
    /// Set the top_p (nucleus sampling)
    pub fn top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }
    
    /// Set the top_k
    pub fn top_k(mut self, top_k: u32) -> Self {
        self.top_k = Some(top_k);
        self
    }
}

impl Anthropic {
    /// Create a new Anthropic client with simple configuration
    pub fn new(api_key: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self::new_with_config(api_key, endpoint, 3, 1000, None)
    }
    
    /// Create a new Anthropic client with rate limiting
    pub fn new_with_rate_limit(
        api_key: impl Into<String>,
        endpoint: impl Into<String>,
        requests_per_minute: u32,
    ) -> Self {
        Self::new_with_config(api_key, endpoint, 3, 1000, Some(requests_per_minute))
    }
    
    /// Create a new Anthropic client with combined configuration
    /// 
    /// Uses connection pooling for better performance with concurrent requests.
    pub fn new_with_config(
        api_key: impl Into<String>,
        endpoint: impl Into<String>,
        max_retries: u32,
        initial_backoff_ms: u64,
        requests_per_minute: Option<u32>,
    ) -> Self {
        let rate_limiter = requests_per_minute
            .filter(|&rpm| rpm > 0)
            .map(|rpm| Mutex::new(TokenBucketRateLimiter::new(rpm)));
        
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                // Keep connections alive for better performance with concurrent requests
                .pool_idle_timeout(Duration::from_secs(90))
                .pool_max_idle_per_host(20)  // Allow more connections for parallel requests
                // Enable TCP keepalive
                .tcp_keepalive(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            api_key: api_key.into(),
            endpoint: endpoint.into(),
            max_retries,
            initial_backoff_ms,
            rate_limiter,
        }
    }
    
    /// Generate API URL based on configured endpoint
    fn api_url(&self) -> String {
        if self.endpoint.is_empty() {
            "https://api.anthropic.com/v1/messages".to_string()
        } else {
            format!("{}/v1/messages", self.endpoint.trim_end_matches('/'))
        }
    }
    
    /// Send a request to the Anthropic API with retry logic
    async fn send_request_with_retry(&self, request: &AnthropicRequest) -> Result<AnthropicResponse, ProviderError> {
        let api_url = self.api_url();
        let mut attempts = 0;
        let mut last_error = None;

        while attempts <= self.max_retries {
            if attempts > 0 {
                let backoff_ms = self.initial_backoff_ms * 2u64.pow(attempts - 1);
                sleep(Duration::from_millis(backoff_ms)).await;
            }

            // Apply rate limiting if configured
            if let Some(rate_limiter) = &self.rate_limiter {
                rate_limiter.lock().await.wait_for_token().await;
            }

            attempts += 1;

            match self.send_request(&api_url, request).await {
                Ok(response) => return Ok(response),
                Err(err) => {
                    // Only retry on connection errors, rate limit errors, and server errors
                    match &err {
                        ProviderError::ConnectionError(_) => {
                            last_error = Some(err);
                        },
                        ProviderError::RateLimitExceeded { retry_after_secs, .. } => {
                            // Use retry-after header when available, otherwise exponential backoff
                            let wait_ms = if let Some(secs) = retry_after_secs {
                                secs * 1000
                            } else {
                                self.initial_backoff_ms * 5 * 2u64.pow(attempts - 1)
                            };
                            sleep(Duration::from_millis(wait_ms)).await;
                            last_error = Some(err);
                        },
                        ProviderError::ApiError { status_code, .. } => {
                            // Retry on rate limiting (429), overloaded (529), and server errors (5xx)
                            if *status_code == 429 || *status_code >= 500 {
                                last_error = Some(err);
                            } else {
                                // Don't retry on client errors (4xx) except rate limiting
                                return Err(err);
                            }
                        },
                        _ => return Err(err), // Don't retry on other errors
                    }
                }
            }
        }

        // If we get here, all retries failed
        Err(last_error.unwrap_or_else(||
            ProviderError::ConnectionError("All retry attempts failed".to_string())))
    }
    
    /// Send a single request to the Anthropic API
    async fn send_request(&self, api_url: &str, request: &AnthropicRequest) -> Result<AnthropicResponse, ProviderError> {
        // Add timeout to prevent hanging HTTP requests
        let request_future = self.client.post(api_url)
            .header("Content-Type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(request)
            .send();
        
        let timeout_duration = std::time::Duration::from_secs(60); // 1 minute timeout
        let response = tokio::select! {
            result = request_future => {
                result.map_err(|e| {
                    if e.is_timeout() {
                        ProviderError::ConnectionError(format!("Request timed out: {}", e))
                    } else if e.is_connect() {
                        ProviderError::ConnectionError(format!("Connection failed: {}", e))
                    } else {
                        ProviderError::RequestFailed(e.to_string())
                    }
                })?
            },
            _ = tokio::time::sleep(timeout_duration) => {
                return Err(ProviderError::ConnectionError("Anthropic API request timed out after 60 seconds".to_string()));
            }
        };
        
        let status = response.status();
        if !status.is_success() {
            // Parse retry-after header before consuming the response body
            let retry_after_secs = response.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());

            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to get error response text".to_string());

            // Try to extract a structured error message from the JSON response body
            let message = Self::extract_api_error_message(&error_text)
                .unwrap_or(error_text);

            return match status.as_u16() {
                429 => Err(ProviderError::RateLimitExceeded { message, retry_after_secs }),
                401 | 403 => Err(ProviderError::AuthenticationError(message)),
                529 => Err(ProviderError::ApiError {
                    status_code: 529,
                    message: format!("Anthropic API overloaded: {}", message),
                }),
                _ => Err(ProviderError::ApiError {
                    status_code: status.as_u16(),
                    message,
                })
            };
        }
        
        response.json::<AnthropicResponse>().await
            .map_err(|e| ProviderError::ParseError(e.to_string()))
    }
}

/// Structured error response from the Anthropic API
#[derive(Debug, Deserialize)]
struct AnthropicErrorResponse {
    #[serde(rename = "type")]
    _response_type: Option<String>,
    error: AnthropicErrorDetail,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorDetail {
    #[serde(rename = "type")]
    _error_type: Option<String>,
    message: String,
}

impl Anthropic {
    /// Try to extract a human-readable error message from the API's JSON error body.
    /// Returns `None` if the body isn't valid structured JSON error.
    fn extract_api_error_message(body: &str) -> Option<String> {
        serde_json::from_str::<AnthropicErrorResponse>(body)
            .ok()
            .map(|e| e.error.message)
    }
}

impl Anthropic {
    /// Complete a request and parse the response as JSON.
    ///
    /// Anthropic doesn't have a native JSON mode, but this method helps
    /// ensure the response is parsed as JSON. The system prompt should
    /// instruct the model to output valid JSON only.
    ///
    /// # Type Parameters
    /// * `T` - The type to deserialize the JSON response into
    ///
    /// # Arguments
    /// * `model` - The model to use
    /// * `system_prompt` - System message instructing JSON output format
    /// * `user_prompt` - The user message (data to process)
    /// * `temperature` - Temperature for generation (0.0-1.0)
    /// * `max_tokens` - Maximum tokens to generate
    ///
    /// # Returns
    /// The parsed response of type T, or an error if parsing fails
    pub async fn complete_json<T: DeserializeOwned>(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        temperature: f32,
        max_tokens: u32,
    ) -> Result<T> {
        // Enhance the system prompt to emphasize JSON-only output
        let json_system_prompt = format!(
            "{}\n\nIMPORTANT: Respond with valid JSON only. Do not include any text before or after the JSON.",
            system_prompt
        );

        let request = AnthropicRequest::new(model, max_tokens)
            .system(&json_system_prompt)
            .add_message(Role::User, user_prompt)
            .temperature(temperature);

        let response = self.send_request_with_retry(&request).await
            .map_err(|e| anyhow!("Anthropic API error: {:?}", e))?;

        // Extract the text content
        let content = Self::extract_text(&response);

        // Try to find JSON in the response (in case there's extra text)
        let json_content = Self::extract_json_from_text(&content)
            .ok_or_else(|| anyhow!("No valid JSON found in response: {}", content))?;

        // Parse the JSON response
        serde_json::from_str(&json_content)
            .with_context(|| format!("Failed to parse JSON response: {}", json_content))
    }

    /// Extract JSON content from text that may contain non-JSON prefix/suffix.
    fn extract_json_from_text(text: &str) -> Option<String> {
        let trimmed = text.trim();

        // If it starts with { or [, try to parse as-is first
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            // Find the matching closing bracket
            if let Some(json) = Self::find_balanced_json(trimmed) {
                return Some(json);
            }
        }

        // Try to find JSON block in the text
        if let Some(start) = trimmed.find('{') {
            let from_start = &trimmed[start..];
            if let Some(json) = Self::find_balanced_json(from_start) {
                return Some(json);
            }
        }

        if let Some(start) = trimmed.find('[') {
            let from_start = &trimmed[start..];
            if let Some(json) = Self::find_balanced_json(from_start) {
                return Some(json);
            }
        }

        None
    }

    /// Find a balanced JSON object or array starting from the beginning of the string.
    fn find_balanced_json(text: &str) -> Option<String> {
        let mut chars = text.chars();
        let open_char = chars.next()?;
        let close_char = match open_char {
            '{' => '}',
            '[' => ']',
            _ => return None,
        };

        let mut depth = 1;
        let mut in_string = false;
        let mut escape_next = false;

        for (byte_idx, c) in text.char_indices().skip(1) {
            if escape_next {
                escape_next = false;
                continue;
            }

            if c == '\\' && in_string {
                escape_next = true;
                continue;
            }

            if c == '"' {
                in_string = !in_string;
                continue;
            }

            if !in_string {
                if c == open_char {
                    depth += 1;
                } else if c == close_char {
                    depth -= 1;
                    if depth == 0 {
                        return Some(text[..byte_idx + c.len_utf8()].to_string());
                    }
                }
            }
        }

        None
    }
}

impl Anthropic {
    /// Complete a messages request (convenience wrapper)
    pub async fn complete(&self, request: AnthropicRequest) -> Result<AnthropicResponse, ProviderError> {
        self.send_request_with_retry(&request).await
    }

    /// Extract text from Anthropic response
    pub fn extract_text(response: &AnthropicResponse) -> String {
        response.content.iter()
            .filter(|c| c.content_type == "text")
            .map(|c| c.text.clone())
            .collect()
    }
}

impl Provider for Anthropic {
    fn translate<'a>(
        &'a self,
        request: &'a TranslationRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TranslationResponse, ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            let anthropic_request = AnthropicRequest::new(&request.model, request.max_tokens)
                .system(&request.system_prompt)
                .add_message(Role::User, &request.user_prompt)
                .temperature(request.temperature);

            let response = self.send_request_with_retry(&anthropic_request).await?;

            let text = Self::extract_text(&response);

            Ok(TranslationResponse {
                text,
                input_tokens: Some(response.usage.input_tokens as u64),
                output_tokens: Some(response.usage.output_tokens as u64),
            })
        })
    }

    fn test_connection<'a>(
        &'a self,
        model: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            let request = AnthropicRequest::new(model, 1)
                .add_message(Role::User, "Hi");

            self.send_request_with_retry(&request).await.map(|_| ())
        })
    }

    fn provider_name(&self) -> &'static str {
        "Anthropic"
    }
}