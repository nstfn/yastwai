/*!
 * Core translation service implementation.
 *
 * This module contains the main TranslationService struct and its implementation,
 * which is responsible for translating text using various AI providers.
 */

use anyhow::{Result, anyhow};
use std::time::{Duration, Instant};
use url::Url;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::app_config::{TranslationConfig, TranslationProvider as ConfigTranslationProvider, ExperimentalFeatures};
use crate::providers::ollama::Ollama;
use crate::providers::openai::OpenAICompatible;
use crate::providers::anthropic::Anthropic;
use crate::providers::{Provider, TranslationRequest};
use super::cache::TranslationCache;
use super::concurrency::ProviderProfile;


/// Token usage statistics for tracking API consumption
#[derive(Clone)]
pub struct TokenUsageStats {
    /// Number of prompt tokens
    pub prompt_tokens: u64,

    /// Number of completion tokens
    pub completion_tokens: u64,

    /// Total number of tokens
    pub total_tokens: u64,

    /// Start time of token tracking
    pub start_time: Instant,

    /// Total time spent on API requests
    pub api_duration: Duration,

    /// Provider name
    pub provider: String,

    /// Model name
    pub model: String,
}

impl Default for TokenUsageStats {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenUsageStats {
    /// Create a new empty token usage stats instance
    pub fn new() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            start_time: Instant::now(),
            api_duration: Duration::from_secs(0),
            provider: String::new(),
            model: String::new(),
        }
    }

    /// Add token usage numbers for testing
    pub fn add_token_usage(&mut self, prompt_tokens: Option<u64>, completion_tokens: Option<u64>) {
        if let Some(pt) = prompt_tokens {
            self.prompt_tokens += pt;
            self.total_tokens += pt;
        }

        if let Some(ct) = completion_tokens {
            self.completion_tokens += ct;
            self.total_tokens += ct;
        }
    }

    /// Create new token usage stats with provider info
    pub fn with_provider_info(provider: String, model: String) -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            start_time: Instant::now(),
            api_duration: Duration::from_secs(0),
            provider,
            model,
        }
    }

    /// Calculate tokens per minute rate
    pub fn tokens_per_minute(&self) -> f64 {
        let duration_minutes = if self.api_duration.as_secs_f64() > 0.0 {
            self.api_duration.as_secs_f64() / 60.0
        } else {
            self.start_time.elapsed().as_secs_f64() / 60.0
        };

        if duration_minutes > 0.0 {
            self.total_tokens as f64 / duration_minutes
        } else {
            0.0
        }
    }

    /// Generate a summary of token usage
    pub fn summary(&self) -> String {
        let elapsed = self.start_time.elapsed();
        let elapsed_minutes = elapsed.as_secs_f64() / 60.0;
        let api_minutes = self.api_duration.as_secs_f64() / 60.0;

        format!(
            "Token Usage Summary:\n\
             Provider: {}\n\
             Model: {}\n\
             Prompt tokens: {}\n\
             Completion tokens: {}\n\
             Total tokens: {}\n\
             Elapsed time: {:.2} minutes\n\
             API request time: {:.2} minutes\n\
             Tokens per minute: {:.2}",
            self.provider,
            self.model,
            self.prompt_tokens,
            self.completion_tokens,
            self.total_tokens,
            elapsed_minutes,
            api_minutes,
            self.tokens_per_minute()
        )
    }
}

/// Parse an endpoint string into host and port
fn parse_endpoint(endpoint: &str) -> Result<(String, u16)> {
    if endpoint.is_empty() {
        return Err(anyhow!("Endpoint cannot be empty"));
    }

    let url = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        Url::parse(endpoint)?
    } else {
        Url::parse(&format!("http://{}", endpoint))?
    };

    let host = url.host_str()
        .ok_or_else(|| anyhow!("Invalid host in endpoint: {}", endpoint))?
        .to_string();

    let port = url.port().unwrap_or(if url.scheme() == "https" { 443 } else { 80 });

    Ok((host, port))
}

/// Translation options for customizing the translation process
pub struct TranslationOptions {
    /// Whether to preserve formatting in the translated text
    pub preserve_formatting: bool,

    /// Maximum number of concurrent requests
    pub max_concurrent_requests: usize,

    /// Whether to retry individual entries on batch failure
    pub retry_individual_entries: bool,
}

impl Default for TranslationOptions {
    fn default() -> Self {
        Self {
            preserve_formatting: true,
            max_concurrent_requests: 3,
            retry_individual_entries: true,
        }
    }
}

/// Log entry for capturing translation process logs
#[derive(Clone)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
}

/// Main translation service for subtitle translation
pub struct TranslationService {
    /// Provider implementation (shared via Arc for cheap cloning)
    provider: Arc<dyn Provider>,

    /// Configuration for the translation service
    pub config: TranslationConfig,

    /// Translation options
    pub options: TranslationOptions,

    /// Translation cache for storing and retrieving translations
    pub cache: TranslationCache,
}

impl TranslationService {
    /// Create a new translation service with the given configuration
    pub fn new(config: TranslationConfig) -> Result<Self> {
        let retry_count = config.common.retry_count;
        let retry_backoff_ms = config.common.retry_backoff_ms;

        let provider: Arc<dyn Provider> = match config.provider {
            ConfigTranslationProvider::Ollama => {
                let (host, port) = parse_endpoint(&config.get_endpoint())?;
                let rate_limit = config.get_rate_limit();
                Arc::new(Ollama::new_with_config(&host, port, retry_count, retry_backoff_ms, rate_limit))
            },
            ConfigTranslationProvider::OpenAI => {
                Arc::new(OpenAICompatible::new(
                    config.get_endpoint(),
                    config.get_api_key(),
                    retry_count,
                    retry_backoff_ms,
                    "OpenAI",
                    120,
                    20,
                ))
            },
            ConfigTranslationProvider::LMStudio => {
                let api_key = {
                    let k = config.get_api_key();
                    if k.is_empty() { "lm-studio".to_string() } else { k }
                };
                Arc::new(OpenAICompatible::new(
                    config.get_endpoint(),
                    api_key,
                    retry_count,
                    retry_backoff_ms,
                    "LM Studio",
                    120,
                    20,
                ))
            },
            ConfigTranslationProvider::Anthropic => {
                let rate_limit = config.get_rate_limit();
                Arc::new(Anthropic::new_with_config(
                    config.get_api_key(),
                    config.get_endpoint(),
                    retry_count,
                    retry_backoff_ms,
                    rate_limit,
                ))
            },
            ConfigTranslationProvider::VLLM => {
                Arc::new(OpenAICompatible::new(
                    config.get_endpoint(),
                    config.get_api_key(),
                    retry_count,
                    retry_backoff_ms,
                    "vLLM",
                    180,
                    32,
                ))
            },
        };

        let options = TranslationOptions {
            preserve_formatting: true,
            max_concurrent_requests: config.optimal_concurrent_requests(),
            retry_individual_entries: true,
        };

        Ok(Self {
            provider,
            config,
            options,
            cache: TranslationCache::new(true),
        })
    }

    /// Apply experimental features settings to the service
    pub fn with_experimental_features(mut self, features: &ExperimentalFeatures) -> Self {
        if features.enable_auto_tune_concurrency {
            let profile = ProviderProfile::for_provider(self.config.provider.clone());
            self.options.max_concurrent_requests = profile.max_concurrent_requests;
        }
        self
    }

    /// Test the connection to the translation provider
    pub async fn test_connection(
        &self,
        _source_language: &str,
        _target_language: &str,
        log_capture: Option<Arc<Mutex<Vec<LogEntry>>>>
    ) -> Result<()> {
        if let Some(log) = &log_capture {
            log.lock().await.push(LogEntry {
                level: "INFO".to_string(),
                message: format!("Testing connection to {} with model {}",
                                self.provider.provider_name(), self.config.get_model()),
            });
        }

        let result = self.provider.test_connection(&self.config.get_model()).await;

        match result {
            Ok(()) => {
                if let Some(log) = &log_capture {
                    log.lock().await.push(LogEntry {
                        level: "INFO".to_string(),
                        message: format!("Successfully connected to {}", self.provider.provider_name()),
                    });
                }
                Ok(())
            },
            Err(e) => {
                let msg = format!("Failed to connect to {}: {}", self.provider.provider_name(), e);
                if let Some(log) = &log_capture {
                    log.lock().await.push(LogEntry {
                        level: "ERROR".to_string(),
                        message: msg.clone(),
                    });
                }
                Err(anyhow!(msg))
            }
        }
    }

    /// Test translation by translating a simple test phrase
    pub async fn test_translation(&self, source_language: &str, target_language: &str) -> Result<String> {
        let test_text = format!("This is a test message from English to {}.", target_language);
        self.translate_text(&test_text, source_language, target_language).await
    }

    /// Translate a single text string
    pub async fn translate_text(&self, text: &str, source_language: &str, target_language: &str) -> Result<String> {
        let (translated, _) = self.translate_text_with_usage(text, source_language, target_language, None).await?;
        Ok(translated)
    }

    /// Translate text with token usage tracking
    pub async fn translate_text_with_usage(
        &self,
        text: &str,
        source_language: &str,
        target_language: &str,
        log_capture: Option<Arc<Mutex<Vec<LogEntry>>>>
    ) -> Result<(String, Option<(Option<u64>, Option<u64>, Option<Duration>)>)> {
        let start_time = Instant::now();

        // Skip empty text
        if text.trim().is_empty() {
            return Ok((String::new(), None));
        }

        // Check cache first
        if let Some(cached_translation) = self.cache.get(text, source_language, target_language).await {
            if let Some(log) = &log_capture {
                log.lock().await.push(LogEntry {
                    level: "INFO".to_string(),
                    message: format!("Cache hit for translation ({} -> {})", source_language, target_language),
                });
            }
            return Ok((cached_translation, None));
        }

        // Prepare system prompt
        let system_prompt = format!(
            "You are a professional translator. Translate the following text from {} to {}. \
             Preserve all formatting, line breaks, and special characters. \
             Only respond with the translated text, without any explanations or notes.",
            source_language, target_language
        );

        let request = TranslationRequest {
            model: self.config.get_model(),
            system_prompt,
            user_prompt: text.to_string(),
            temperature: self.config.common.temperature,
            max_tokens: self.max_tokens_for_model(&self.config.get_model()),
        };

        let provider_name = self.provider.provider_name();

        match self.provider.translate(&request).await {
            Ok(response) => {
                let duration = start_time.elapsed();

                if let Some(log) = &log_capture {
                    log.lock().await.push(LogEntry {
                        level: "INFO".to_string(),
                        message: format!("{} response received in {:?}", provider_name, duration),
                    });
                }

                // Store in cache
                self.cache.store(text, source_language, target_language, &response.text).await;

                Ok((response.text, Some((response.input_tokens, response.output_tokens, Some(duration)))))
            },
            Err(e) => {
                if let Some(log) = &log_capture {
                    log.lock().await.push(LogEntry {
                        level: "ERROR".to_string(),
                        message: format!("{} translation error: {}", provider_name, e),
                    });
                }

                Err(anyhow!("{} translation error: {}", provider_name, e))
            }
        }
    }

    /// Get the maximum number of tokens for a given model
    fn max_tokens_for_model(&self, model: &str) -> u32 {
        match model {
            // OpenAI models
            "gpt-4" | "gpt-4-0613" => 8192,
            "gpt-4-32k" | "gpt-4-32k-0613" => 32768,
            "gpt-4-turbo" | "gpt-4-turbo-preview" | "gpt-4-0125-preview" => 4096,
            "gpt-3.5-turbo" | "gpt-3.5-turbo-0613" => 4096,
            "gpt-3.5-turbo-16k" | "gpt-3.5-turbo-16k-0613" => 16384,

            // Anthropic models — current generation
            "claude-opus-4-6" | "claude-sonnet-4-6" | "claude-sonnet-4-5-20250514" => 8192,
            "claude-haiku-4-5" | "claude-haiku-4-5-20251001" => 8192,
            "claude-3-7-sonnet-latest" | "claude-3-5-haiku-latest" => 8192,
            // Anthropic models — previous generation
            "claude-3-opus-20240229" => 4096,
            "claude-3-sonnet-20240229" => 4096,
            "claude-3-haiku-20240307" => 4096,
            "claude-2.1" => 4096,
            "claude-2.0" => 4096,
            "claude-instant-1.2" => 4096,

            // Default for unknown models
            _ => 4096,
        }
    }
}

impl Clone for TranslationService {
    fn clone(&self) -> Self {
        Self {
            provider: Arc::clone(&self.provider),
            config: self.config.clone(),
            options: TranslationOptions {
                preserve_formatting: self.options.preserve_formatting,
                max_concurrent_requests: self.options.max_concurrent_requests,
                retry_individual_entries: self.options.retry_individual_entries,
            },
            cache: self.cache.clone(),
        }
    }
}
