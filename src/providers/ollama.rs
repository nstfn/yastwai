use anyhow::{Result, anyhow, Context};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;
use log::error;

use crate::errors::ProviderError;
use super::{Provider, Role, TranslationRequest, TranslationResponse};

/// Ollama client for interacting with Ollama API
pub struct Ollama {
    /// Base URL of the Ollama API
    base_url: String,
    /// HTTP client for making requests
    client: Client,
    /// Maximum number of retry attempts
    max_retries: u32,
    /// Base backoff time in milliseconds for exponential backoff
    backoff_base_ms: u64,
    /// Optional rate limit in requests per minute
    rate_limit: Option<u32>,
}

/// Generate request for the Ollama API
#[derive(Debug, Serialize, Deserialize)]
pub struct GenerationRequest {
    /// Model name to use for generation
    model: String,
    /// Prompt to generate from
    prompt: String,
    /// System message to guide the model
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    /// Additional model parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<GenerationOptions>,
    /// Format to return a response in
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    /// Context from previous generations
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<Vec<i32>>,
    /// Whether to stream the response
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    /// Whether to use raw prompting
    #[serde(skip_serializing_if = "Option::is_none")]
    raw: Option<bool>,
    /// How long to keep the model loaded in memory
    #[serde(skip_serializing_if = "Option::is_none")]
    keep_alive: Option<String>,
}

/// Generation options for the Ollama API
#[derive(Debug, Serialize, Deserialize)]
pub struct GenerationOptions {
    /// Temperature for generation (default: 0.8)
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    /// Top-p sampling (default: 0.9)
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    /// Top-k sampling (default: 40)
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
    /// Random seed for generation
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
    /// Maximum number of tokens to generate
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

/// Generation response from the Ollama API
#[derive(Debug, Serialize, Deserialize)]
pub struct GenerationResponse {
    /// Model name
    pub model: String,
    /// Creation timestamp
    pub created_at: String,
    /// Generated text
    pub response: String,
    /// Whether the generation is complete
    pub done: bool,
    /// Context for future generations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Vec<i32>>,
    /// Total duration of the request in nanoseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration: Option<u64>,
    /// Duration of loading the model in nanoseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_duration: Option<u64>,
    /// Number of prompt tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_count: Option<u64>,
    /// Duration of prompt evaluation in nanoseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_duration: Option<u64>,
    /// Number of generated tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_count: Option<u64>,
    /// Duration of generation in nanoseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_duration: Option<u64>,
}

/// Chat message object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role of the message sender
    pub role: Role,
    /// Content of the message
    pub content: String,
}

/// Chat request for the Ollama API
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatRequest {
    /// Model name to use for generation
    model: String,
    /// Messages of the conversation
    messages: Vec<ChatMessage>,
    /// System message to guide the model
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    /// Additional model parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<GenerationOptions>,
    /// Format to return a response in
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    /// Whether to stream the response
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    /// How long to keep the model loaded in memory
    #[serde(skip_serializing_if = "Option::is_none")]
    keep_alive: Option<String>,
}

/// Chat response from the Ollama API
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatResponse {
    /// Model name
    pub model: String,
    /// Creation timestamp
    pub created_at: String,
    /// Response message
    pub message: ChatMessage,
    /// Whether the generation is complete
    pub done: bool,
    /// Total duration of the request in nanoseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration: Option<u64>,
    /// Duration of loading the model in nanoseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_duration: Option<u64>,
    /// Number of prompt tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_count: Option<u64>,
    /// Duration of prompt evaluation in nanoseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_duration: Option<u64>,
    /// Number of generated tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_count: Option<u64>,
    /// Duration of generation in nanoseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_duration: Option<u64>,
}

/// Embeddings request for the Ollama API
#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    /// Model name to use for generation
    model: String,
    /// Prompt to generate embeddings for
    prompt: String,
    /// Additional model parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<GenerationOptions>,
    /// How long to keep the model loaded in memory
    #[serde(skip_serializing_if = "Option::is_none")]
    keep_alive: Option<String>,
}

/// Embeddings response from the Ollama API
#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    /// Embedding vector
    pub embedding: Vec<f32>,
}

/// Builder methods for GenerationRequest
impl GenerationRequest {
    /// Create a new generation request
    pub fn new(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            prompt: prompt.into(),
            system: None,
            options: None,
            format: None,
            context: None,
            stream: Some(false),
            raw: None,
            keep_alive: None,
        }
    }

    /// Set the system prompt
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set the temperature
    pub fn temperature(mut self, temperature: f32) -> Self {
        if self.options.is_none() {
            self.options = Some(GenerationOptions {
                temperature: Some(temperature),
                top_p: None,
                top_k: None,
                seed: None,
                num_predict: None,
            });
        } else if let Some(options) = &mut self.options {
            options.temperature = Some(temperature);
        }
        self
    }

    /// Set the format
    pub fn format(mut self, format: impl Into<String>) -> Self {
        self.format = Some(format.into());
        self
    }

    /// Set the keep-alive duration
    pub fn keep_alive(mut self, keep_alive: impl Into<String>) -> Self {
        self.keep_alive = Some(keep_alive.into());
        self
    }

    /// Disable streaming for this request
    pub fn no_stream(mut self) -> Self {
        self.stream = Some(false);
        self
    }
}

/// Builder methods for ChatRequest
impl ChatRequest {
    /// Create a new chat request
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            system: None,
            options: None,
            format: None,
            stream: Some(false),
            keep_alive: None,
        }
    }

    /// Set the system prompt
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set the temperature
    pub fn temperature(mut self, temperature: f32) -> Self {
        if self.options.is_none() {
            self.options = Some(GenerationOptions {
                temperature: Some(temperature),
                top_p: None,
                top_k: None,
                seed: None,
                num_predict: None,
            });
        } else if let Some(options) = &mut self.options {
            options.temperature = Some(temperature);
        }
        self
    }

    /// Set the format
    pub fn format(mut self, format: impl Into<String>) -> Self {
        self.format = Some(format.into());
        self
    }

    /// Set the keep-alive duration
    pub fn keep_alive(mut self, keep_alive: impl Into<String>) -> Self {
        self.keep_alive = Some(keep_alive.into());
        self
    }

    /// Disable streaming
    pub fn no_stream(mut self) -> Self {
        self.stream = Some(false);
        self
    }
}

impl Ollama {
    /// Create a new Ollama client with the specified base URL
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        let host = host.into();
        
        // Construct a proper URL with scheme and port
        let base_url = if host.starts_with("http://") || host.starts_with("https://") {
            // If the host already has a scheme
            let url_parts: Vec<&str> = host.split("://").collect();
            if url_parts.len() == 2 {
                let scheme = url_parts[0];
                let host_part = url_parts[1];
                
                // Check if host_part already contains a port
                if host_part.contains(":") {
                    // Already has a port, use as is
                    host
                } else {
                    // No port, append it
                    format!("{}://{}:{}", scheme, host_part, port)
                }
            } else {
                // Malformed URL, fallback to safe default
                format!("http://localhost:{}", port)
            }
        } else {
            // No scheme, add http:// and port
            format!("http://{}:{}", host, port)
        };
        
        Self {
            base_url,
            client: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            max_retries: 3,
            backoff_base_ms: 1000,
            rate_limit: None,
        }
    }
    
    /// Create a new Ollama client with configuration
    /// 
    /// Uses connection pooling for better performance with concurrent requests.
    /// Note: Ollama typically uses HTTP/1.1, so we don't force HTTP/2.
    pub fn new_with_config(
        host: impl Into<String>, 
        port: u16,
        max_retries: u32,
        backoff_base_ms: u64,
        rate_limit: Option<u32>
    ) -> Self {
        let host = host.into();
        
        // Construct a proper URL with scheme and port (same logic as new())
        let base_url = if host.starts_with("http://") || host.starts_with("https://") {
            let url_parts: Vec<&str> = host.split("://").collect();
            if url_parts.len() == 2 {
                let scheme = url_parts[0];
                let host_part = url_parts[1];
                
                if host_part.contains(":") {
                    host
                } else {
                    format!("{}://{}:{}", scheme, host_part, port)
                }
            } else {
                format!("http://localhost:{}", port)
            }
        } else {
            format!("http://{}:{}", host, port)
        };
        
        Self {
            base_url,
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                // Force HTTP/1.1 - Ollama uses HTTP/1.1
                .http1_only()
                // Keep connections alive for better performance
                .pool_idle_timeout(Duration::from_secs(90))
                .pool_max_idle_per_host(20)  // Allow more connections for parallel requests
                // Enable TCP keepalive
                .tcp_keepalive(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            max_retries,
            backoff_base_ms,
            rate_limit,
        }
    }
    
    /// Create a new Ollama client from a complete URL
    pub fn from_url(url: impl Into<String>) -> Self {
        Self {
            base_url: url.into(),
            client: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            max_retries: 3,
            backoff_base_ms: 1000,
            rate_limit: None,
        }
    }
    
    /// Generate text from the Ollama API with retry logic
    pub async fn generate(&self, request: GenerationRequest) -> Result<GenerationResponse> {
        let url = format!("{}/api/generate", self.base_url);
        
        let mut attempt = 0;
        let mut last_error = None;
        
        while attempt <= self.max_retries {
            // Add rate limiting if configured
            if let Some(rate_limit) = self.rate_limit {
                let delay_ms = 60_000 / rate_limit as u64; // Convert requests per minute to milliseconds
                if attempt > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
            
            let response_result = self.client.post(&url)
                .json(&request)
                .send()
                .await;
            
            match response_result {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
        
        // Get the raw response text first
        let response_text = response.text().await
            .map_err(|e| anyhow!("Failed to get response text from Ollama API: {}", e))?;
        
                        // Try to parse as single JSON object first
                        match serde_json::from_str::<GenerationResponse>(&response_text) {
                            Ok(generated_response) => {
                                return Ok(generated_response);
                            },
                            Err(e) => {
                                // Log the raw response for debugging
                                error!("Failed to parse Ollama API response: {}. Raw response (first 500 chars): {}", 
                                      e, if response_text.chars().count() > 500 { 
                                          response_text.chars().take(500).collect::<String>() 
                                      } else { 
                                          response_text.clone() 
                                      });
                
                // The response might be in JSONL format (streaming response)
                // Split by lines and try to parse each as a JSON object
                let lines: Vec<&str> = response_text.lines().collect();
                
                                if !lines.is_empty() {
                                    // Try to parse the last line, which should contain the final state
                                    for line in lines.iter().rev() {
                                        if line.is_empty() {
                                            continue;
                                        }
                                        
                                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                                            // Check if it's a "done" message
                                            if value.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                                                // Found the final response
                                                let model = value.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                                let created_at = value.get("created_at").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                
                                                // For streaming responses, we need to concatenate all the pieces
                                                let mut full_response = String::new();
                                                for line in lines.iter() {
                                                    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
                                                        if let Some(part) = obj.get("response").and_then(|v| v.as_str()) {
                                                            full_response.push_str(part);
                                                        }
                                                    }
                                                }
                                                
                                                // Extract optional numeric fields if available
                                                let prompt_eval_count = value.get("prompt_eval_count").and_then(|v| v.as_u64());
                                                let eval_count = value.get("eval_count").and_then(|v| v.as_u64());
                                                
                                                return Ok(GenerationResponse {
                                                    model,
                                                    created_at,
                                                    response: full_response,
                                                    done: true,
                                                    context: None,
                                                    total_duration: None,
                                                    load_duration: None,
                                                    prompt_eval_count,
                                                    prompt_eval_duration: None,
                                                    eval_count,
                                                    eval_duration: None,
                                                });
                                            }
                                        }
                                    }
                                    
                                    // If we didn't find a "done" message, try to use the last valid JSON object
                                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(lines[lines.len() - 1]) {
                                        let model = value.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                        let created_at = value.get("created_at").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        let response_text = value.get("response").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        
                                        // Extract optional numeric fields if available
                                        let prompt_eval_count = value.get("prompt_eval_count").and_then(|v| v.as_u64());
                                        let eval_count = value.get("eval_count").and_then(|v| v.as_u64());
                                        
                                        return Ok(GenerationResponse {
                                            model,
                                            created_at,
                                            response: response_text,
                                            done: true,
                                            context: None,
                                            total_duration: None,
                                            load_duration: None,
                                            prompt_eval_count,
                                            prompt_eval_duration: None,
                                            eval_count,
                                            eval_duration: None,
                                        });
                                    }
                                }
                                
                                // If we still can't parse the response, try our original lenient approach
                                match serde_json::from_str::<serde_json::Value>(&response_text) {
                                    Ok(value) => {
                                        // Try to extract essential fields
                                        let model = value.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                        let response_text = value.get("response").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        let created_at = value.get("created_at").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        let done = value.get("done").and_then(|v| v.as_bool()).unwrap_or(true);
                                        
                                        // Extract optional numeric fields if available
                                        let prompt_eval_count = value.get("prompt_eval_count").and_then(|v| v.as_u64());
                                        let eval_count = value.get("eval_count").and_then(|v| v.as_u64());
                                        
                                        // Create a response with the extracted fields
                                        return Ok(GenerationResponse {
                                            model,
                                            created_at,
                                            response: response_text,
                                            done,
                                            context: None,
                                            total_duration: None,
                                            load_duration: None,
                                            prompt_eval_count,
                                            prompt_eval_duration: None,
                                            eval_count,
                                            eval_duration: None,
                                        });
                                    },
                                    Err(_) => {
                                        // If we can't even parse as a JSON Value, set last_error
                                        last_error = Some(anyhow!("Failed to parse Ollama API response: {}. Response contains invalid JSON.", e));
                                    }
                                }
                            }
                        }
                    } else if status.is_server_error() {
                        // Server error - can retry
                        let error_text = response.text().await
                            .unwrap_or_else(|_| "Failed to get error response text".to_string());
                        last_error = Some(anyhow!("Ollama API error ({}): {}", status, error_text));
                        error!("Ollama API error ({}): {} - attempt {}/{}", status, error_text, attempt + 1, self.max_retries + 1);
                    } else {
                        // Client error - don't retry
                        let error_text = response.text().await
                            .unwrap_or_else(|_| "Failed to get error response text".to_string());
                        error!("Ollama API error ({}): {}", status, error_text);
                        return Err(anyhow!("Ollama API error ({}): {}", status, error_text));
                    }
                },
                Err(e) => {
                    // Network error - can retry
                    last_error = Some(anyhow!("Failed to send request to Ollama API: {}", e));
                    error!("Ollama API network error: {} - attempt {}/{}", last_error.as_ref().unwrap(), attempt + 1, self.max_retries + 1);
                }
            }
            
            attempt += 1;
            
            // If we have more retries left, wait with exponential backoff
            if attempt <= self.max_retries {
                let backoff_ms = self.backoff_base_ms * (1u64 << (attempt - 1));
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            }
        }
        
        // If we get here, all retries failed
        Err(last_error.unwrap_or_else(|| anyhow!("Ollama API request failed after {} attempts", self.max_retries + 1)))
    }
    
    /// Chat with the Ollama API
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/api/chat", self.base_url);
        
        let response = self.client.post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send chat request to Ollama API: {}", e))?;
        
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to get error response text".to_string());
            error!("Ollama API error ({}): {}", status, error_text);
            return Err(anyhow!("Ollama API error ({}): {}", status, error_text));
        }
        
        // Get the raw response text first
        let response_text = response.text().await
            .map_err(|e| anyhow!("Failed to get response text from Ollama API: {}", e))?;
        
        // Try to parse as single JSON object first
        match serde_json::from_str::<ChatResponse>(&response_text) {
            Ok(chat_response) => {
                Ok(chat_response)
            },
            Err(e) => {
                // Log the raw response for debugging
                error!("Failed to parse Ollama API chat response: {}. Raw response (first 500 chars): {}", 
                      e, if response_text.chars().count() > 500 { 
                          response_text.chars().take(500).collect::<String>() 
                      } else { 
                          response_text.clone() 
                      });
                
                // The response might be in JSONL format (streaming response)
                // Split by lines and try to parse each as a JSON object
                let lines: Vec<&str> = response_text.lines().collect();
                
                if !lines.is_empty() {
                    // Try to parse the last line, which should contain the final state
                    for line in lines.iter().rev() {
                        if line.is_empty() {
                            continue;
                        }
                        
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                            // Check if it's a "done" message
                            if value.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                                // Found the final response
                                let model = value.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                let created_at = value.get("created_at").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                
                                // For streaming responses, we need to concatenate all the message content pieces
                                let mut full_content = String::new();
                                for line in lines.iter() {
                                    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
                                        if let Some(message) = obj.get("message") {
                                            if let Some(part) = message.get("content").and_then(|v| v.as_str()) {
                                                full_content.push_str(part);
                                            }
                                        }
                                    }
                                }
                                
                                // Extract optional numeric fields if available
                                let prompt_eval_count = value.get("prompt_eval_count").and_then(|v| v.as_u64());
                                let eval_count = value.get("eval_count").and_then(|v| v.as_u64());
                                
                                return Ok(ChatResponse {
                                    model,
                                    created_at,
                                    message: ChatMessage {
                                        role: Role::Assistant,
                                        content: full_content,
                                    },
                                    done: true,
                                    total_duration: None,
                                    load_duration: None,
                                    prompt_eval_count,
                                    prompt_eval_duration: None,
                                    eval_count,
                                    eval_duration: None,
                                });
                            }
                        }
                    }
                    
                    // If we didn't find a "done" message, try to use the last valid JSON object
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(lines[lines.len() - 1]) {
                        let model = value.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                        let created_at = value.get("created_at").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        
                        // Extract message content
                        let content = if let Some(message) = value.get("message") {
                            message.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string()
                        } else {
                            "".to_string()
                        };
                        
                        // Extract optional numeric fields if available
                        let prompt_eval_count = value.get("prompt_eval_count").and_then(|v| v.as_u64());
                        let eval_count = value.get("eval_count").and_then(|v| v.as_u64());
                        
                        return Ok(ChatResponse {
                            model,
                            created_at,
                            message: ChatMessage {
                                role: Role::Assistant,
                                content,
                            },
                            done: true,
                            total_duration: None,
                            load_duration: None,
                            prompt_eval_count,
                            prompt_eval_duration: None,
                            eval_count,
                            eval_duration: None,
                        });
                    }
                }
                
                // If we still can't parse the response, try our original lenient approach
                match serde_json::from_str::<serde_json::Value>(&response_text) {
                    Ok(value) => {
                        // Try to extract essential fields
                        let model = value.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                        let created_at = value.get("created_at").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let done = value.get("done").and_then(|v| v.as_bool()).unwrap_or(true);
                        
                        // Extract message content
                        let content = if let Some(message) = value.get("message") {
                            message.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string()
                        } else {
                            "".to_string()
                        };
                        
                        // Extract optional numeric fields if available
                        let prompt_eval_count = value.get("prompt_eval_count").and_then(|v| v.as_u64());
                        let eval_count = value.get("eval_count").and_then(|v| v.as_u64());
                        
                        // Create a response with the extracted fields
                        Ok(ChatResponse {
                            model,
                            created_at,
                            message: ChatMessage {
                                role: Role::Assistant,
                                content,
                            },
                            done,
                            total_duration: None,
                            load_duration: None,
                            prompt_eval_count,
                            prompt_eval_duration: None,
                            eval_count,
                            eval_duration: None,
                        })
                    },
                    Err(_) => {
                        // If we can't even parse as a JSON Value, return the original error
                        Err(anyhow!("Failed to parse Ollama API chat response: {}. Response contains invalid JSON.", e))
                    }
                }
            }
        }
    }
    
    /// Generate embeddings from the Ollama API
    pub async fn embed(&self, model: impl Into<String>, prompt: impl Into<String>) -> Result<EmbeddingResponse> {
        let url = format!("{}/api/embeddings", self.base_url);
        
        let request = EmbeddingRequest {
            model: model.into(),
            prompt: prompt.into(),
            options: None,
            keep_alive: None,
        };
        
        let response = self.client.post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send embeddings request to Ollama API: {}", e))?;
        
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await
                .unwrap_or_else(|_| "Failed to get error response text".to_string());
            error!("Ollama API error ({}): {}", status, error_text);
            return Err(anyhow!("Ollama API error ({}): {}", status, error_text));
        }
        
        // Get the raw response text first
        let response_text = response.text().await
            .map_err(|e| anyhow!("Failed to get response text from Ollama API: {}", e))?;
        
        // Try to parse the response
        match serde_json::from_str::<EmbeddingResponse>(&response_text) {
            Ok(embedding_response) => {
                Ok(embedding_response)
            },
            Err(e) => {
                // Log the raw response for debugging
                error!("Failed to parse Ollama API embeddings response: {}. Raw response (first 500 chars): {}", 
                      e, if response_text.chars().count() > 500 { 
                          response_text.chars().take(500).collect::<String>() 
                      } else { 
                          response_text.clone() 
                      });
                
                // Try a more lenient approach - parse as Value first
                match serde_json::from_str::<serde_json::Value>(&response_text) {
                    Ok(value) => {
                        // Try to extract embedding array
                        if let Some(embedding) = value.get("embedding").and_then(|v| v.as_array()) {
                            // Convert to vector of f32
                            let embedding_vec: Vec<f32> = embedding.iter()
                                .filter_map(|v| v.as_f64().map(|f| f as f32))
                                .collect();
                            
                            if !embedding_vec.is_empty() {
                                return Ok(EmbeddingResponse { embedding: embedding_vec });
                            }
                        }
                        
                        // If we couldn't extract the embedding, return an error
                        Err(anyhow!("Failed to extract embedding from Ollama API response"))
                    },
                    Err(_) => {
                        // If we can't even parse as a JSON Value, return the original error
                        Err(anyhow!("Failed to parse Ollama API embeddings response: {}. Response contains invalid JSON.", e))
                    }
                }
            }
        }
    }
    
    /// Get the Ollama API version
    pub async fn version(&self) -> Result<String> {
        let url = format!("{}/api/version", self.base_url);
        let response: serde_json::Value = self.client.get(&url)
            .send()
            .await
            .context("Failed to connect to Ollama")?
            .json()
            .await
            .context("Failed to parse Ollama version response")?;
        
        let version = response["version"].as_str()
            .ok_or_else(|| anyhow!("Invalid version format in response"))?
            .to_string();
        
        Ok(version)
    }

    /// Generate a response in JSON format and parse it.
    ///
    /// This method sets the format to "json" which instructs Ollama to
    /// return valid JSON output. The system prompt should describe the
    /// expected JSON schema.
    ///
    /// # Type Parameters
    /// * `T` - The type to deserialize the JSON response into
    ///
    /// # Arguments
    /// * `model` - The model to use
    /// * `system_prompt` - System message describing the JSON schema
    /// * `user_prompt` - The user message (data to process)
    /// * `temperature` - Temperature for generation
    ///
    /// # Returns
    /// The parsed response of type T, or an error if parsing fails
    pub async fn generate_json<T: DeserializeOwned>(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        temperature: f32,
    ) -> Result<T> {
        let request = GenerationRequest::new(model, user_prompt)
            .system(system_prompt)
            .temperature(temperature)
            .format("json");

        let response = self.generate(request).await?;

        // Parse the JSON response
        serde_json::from_str(&response.response)
            .with_context(|| format!("Failed to parse JSON response: {}", response.response))
    }

    /// Chat with JSON format output and parse the response.
    ///
    /// Similar to generate_json but uses the chat endpoint for multi-turn
    /// conversations.
    ///
    /// # Type Parameters
    /// * `T` - The type to deserialize the JSON response into
    ///
    /// # Arguments
    /// * `model` - The model to use
    /// * `system_prompt` - System message describing the JSON schema
    /// * `messages` - The conversation messages
    /// * `temperature` - Temperature for generation
    ///
    /// # Returns
    /// The parsed response of type T, or an error if parsing fails
    pub async fn chat_json<T: DeserializeOwned>(
        &self,
        model: &str,
        system_prompt: &str,
        messages: Vec<ChatMessage>,
        temperature: f32,
    ) -> Result<T> {
        let request = ChatRequest::new(model, messages)
            .system(system_prompt)
            .temperature(temperature)
            .format("json");

        let response = self.chat(request).await?;

        // Parse the JSON response
        serde_json::from_str(&response.message.content)
            .with_context(|| format!("Failed to parse JSON response: {}", response.message.content))
    }
}

impl std::fmt::Debug for Ollama {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ollama")
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl Provider for Ollama {
    fn translate<'a>(
        &'a self,
        request: &'a TranslationRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TranslationResponse, ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            let gen_request = GenerationRequest::new(&request.model, &request.user_prompt)
                .system(&request.system_prompt)
                .temperature(request.temperature);

            let response = self.generate(gen_request).await
                .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

            Ok(TranslationResponse {
                text: response.response,
                input_tokens: response.prompt_eval_count,
                output_tokens: response.eval_count,
            })
        })
    }

    fn test_connection<'a>(
        &'a self,
        _model: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            self.version().await
                .map(|_| ())
                .map_err(|e| ProviderError::ConnectionError(e.to_string()))
        })
    }

    fn provider_name(&self) -> &'static str {
        "Ollama"
    }
}