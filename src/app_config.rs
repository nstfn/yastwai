use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::default::Default;

/// Application configuration module
/// This module handles the application configuration including loading,
/// validating and saving configuration settings.
/// Represents the application configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    /// Source language code (ISO)
    pub source_language: String,
    
    /// Target language code (ISO)
    pub target_language: String,
    
    /// Translation config
    pub translation: TranslationConfig,
    
    /// Session and persistence configuration
    #[serde(default)]
    pub session: SessionConfig,
    
    /// Cache configuration
    #[serde(default)]
    pub cache: CacheConfig,
    
    /// Validation configuration
    #[serde(default)]
    pub validation: ValidationConfig,

    /// Experimental features (all disabled by default)
    #[serde(default)]
    pub experimental: ExperimentalFeatures,

    /// Log level
    #[serde(default)]
    pub log_level: LogLevel,
}

/// Translation provider type
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TranslationProvider {
    // @provider: Ollama
    #[default]
    Ollama,
    // @provider: OpenAI
    OpenAI,
    // @provider: Anthropic
    Anthropic,
    // @provider: LM Studio (OpenAI-compatible local server)
    LMStudio,
    // @provider: vLLM (high-throughput local inference server)
    VLLM,
}

impl TranslationProvider {
    // @returns: Capitalized provider name
    pub fn display_name(&self) -> &str {
        match self {
            Self::Ollama => "Ollama",
            Self::OpenAI => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::LMStudio => "LM Studio",
            Self::VLLM => "vLLM",
        }
    }

    // @returns: Lowercase provider identifier
    pub fn to_lowercase_string(&self) -> String {
        match self {
            Self::Ollama => "ollama".to_string(),
            Self::OpenAI => "openai".to_string(),
            Self::Anthropic => "anthropic".to_string(),
            Self::LMStudio => "lmstudio".to_string(),
            Self::VLLM => "vllm".to_string(),
        }
    }
}

// Implement Display trait for TranslationProvider
impl std::fmt::Display for TranslationProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_lowercase_string())
    }
}

// Implement FromStr trait for TranslationProvider
impl std::str::FromStr for TranslationProvider {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "ollama" => Ok(Self::Ollama),
            "openai" => Ok(Self::OpenAI),
            "anthropic" => Ok(Self::Anthropic),
            "lmstudio" => Ok(Self::LMStudio),
            "vllm" => Ok(Self::VLLM),
            _ => Err(anyhow!("Invalid provider type: {}", s)),
        }
    }
}

/// Provider configuration wrapper
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderConfig {
    // @field: Provider type identifier
    #[serde(rename = "type")]
    pub provider_type: String,
    
    // @field: Model name
    #[serde(default = "String::new")]
    pub model: String,
    
    // @field: API key
    #[serde(default = "String::new")]
    pub api_key: String,
    
    // @field: Service URL
    #[serde(default = "String::new")]
    pub endpoint: String,
    
    // @field: Max concurrent requests
    #[serde(default = "default_concurrent_requests")]
    pub concurrent_requests: usize,
    
    // @field: Max chars per request
    #[serde(default = "default_max_chars_per_request")]
    pub max_chars_per_request: usize,
    
    // @field: Timeout seconds
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    
    // @field: Rate limit (requests per minute)
    #[serde(default)]
    pub rate_limit: Option<u32>,
}

impl ProviderConfig {
    // @param provider_type: Provider enum
    // @returns: Provider config with defaults
    pub fn new(provider_type: TranslationProvider) -> Self {
        match provider_type {
            TranslationProvider::Ollama => Self {
                provider_type: "ollama".to_string(),
                model: default_ollama_model(),
                api_key: String::new(),
                endpoint: default_ollama_endpoint(),
                concurrent_requests: default_concurrent_requests(),
                max_chars_per_request: default_max_chars_per_request(),
                timeout_secs: default_timeout_secs(),
                rate_limit: default_ollama_rate_limit(),
            },
            TranslationProvider::OpenAI => Self {
                provider_type: "openai".to_string(),
                model: default_openai_model(),
                api_key: String::new(),
                endpoint: default_openai_endpoint(),
                concurrent_requests: default_concurrent_requests(),
                max_chars_per_request: default_max_chars_per_request(),
                timeout_secs: default_timeout_secs(),
                rate_limit: default_openai_rate_limit(),
            },
            TranslationProvider::Anthropic => Self {
                provider_type: "anthropic".to_string(),
                model: default_anthropic_model(),
                api_key: String::new(),
                endpoint: default_anthropic_endpoint(),
                concurrent_requests: default_concurrent_requests(),
                max_chars_per_request: default_anthropic_max_chars_per_request(),
                timeout_secs: default_anthropic_timeout_secs(),
                rate_limit: default_anthropic_rate_limit(),
            },
            TranslationProvider::LMStudio => Self {
                provider_type: "lmstudio".to_string(),
                model: default_lmstudio_model(),
                api_key: String::new(),
                endpoint: default_lmstudio_endpoint(),
                concurrent_requests: default_concurrent_requests(),
                max_chars_per_request: default_max_chars_per_request(),
                timeout_secs: default_timeout_secs(),
                rate_limit: default_lmstudio_rate_limit(),
            },
            TranslationProvider::VLLM => Self {
                provider_type: "vllm".to_string(),
                model: default_vllm_model(),
                api_key: String::new(),
                endpoint: default_vllm_endpoint(),
                concurrent_requests: default_vllm_concurrent_requests(),
                max_chars_per_request: default_vllm_max_chars_per_request(),
                timeout_secs: default_vllm_timeout_secs(),
                rate_limit: default_vllm_rate_limit(),
            },
        }
    }

}

/// Translation service configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranslationConfig {
    /// Translation provider to use
    #[serde(default)]
    pub provider: TranslationProvider,
    
    /// Available translation providers
    #[serde(default)]
    pub available_providers: Vec<ProviderConfig>,
    
    /// Common translation settings
    #[serde(default)]
    pub common: TranslationCommonConfig,
}

/// Common translation settings applicable to all providers
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranslationCommonConfig {
    /// System prompt template for translation
    /// Placeholders: {source_language}, {target_language}
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    
    /// Rate limit delay in milliseconds between consecutive requests
    #[serde(default = "default_rate_limit_delay_ms")]
    pub rate_limit_delay_ms: u64,
    
    /// Retry count for failed requests
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,
    
    /// Backoff multiplier for retries (in milliseconds)
    #[serde(default = "default_retry_backoff_ms")]
    pub retry_backoff_ms: u64,
    
    /// Temperature parameter for text generation (0.0 to 1.0)
    /// Lower values make output more deterministic, higher values more creative
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    
    /// Enable parallel translation mode (default: true)
    /// When enabled, uses multiple concurrent API requests for faster translation
    #[serde(default = "default_true")]
    pub parallel_mode: bool,
    
    /// Number of entries to include in each parallel request (1-10 recommended)
    /// Lower values = more requests but better parallelism
    /// Higher values = fewer requests but less parallelism
    #[serde(default = "default_entries_per_request")]
    pub entries_per_request: usize,
    
    /// Number of previous entries to include as context for translation consistency
    /// This helps maintain consistency in formal/informal address, character genders, etc.
    /// Set to 0 to disable context. Default: 3
    #[serde(default = "default_context_entries_count")]
    pub context_entries_count: usize,

    /// Pipeline mode for translation processing
    /// Options: "legacy" (default), "fast", "standard", "quality"
    /// - legacy: Use traditional batch translator (backwards compatible)
    /// - fast: Use new pipeline with minimal analysis, no validation
    /// - standard: Use new pipeline with default settings
    /// - quality: Use new pipeline with thorough analysis and validation
    #[serde(default = "default_pipeline_mode")]
    pub pipeline_mode: String,

    /// Subtitle standard preset: "netflix" (default), "children", "relaxed"
    /// Controls reading speed (CPS) and line length (CPL) targets
    #[serde(default = "default_subtitle_preset")]
    pub subtitle_preset: String,

    /// Disable the reflection pass (saves API calls at cost of quality)
    /// The reflection pass reviews and improves translations before validation
    #[serde(default)]
    pub no_reflection: bool,
}

fn default_context_entries_count() -> usize {
    3 // Include 3 previous entries as context by default
}

fn default_pipeline_mode() -> String {
    "legacy".to_string()
}

fn default_subtitle_preset() -> String {
    "netflix".to_string()
}

impl Default for TranslationCommonConfig {
    fn default() -> Self {
        Self {
            system_prompt: default_system_prompt(),
            rate_limit_delay_ms: default_rate_limit_delay_ms(),
            retry_count: default_retry_count(),
            retry_backoff_ms: default_retry_backoff_ms(),
            temperature: default_temperature(),
            parallel_mode: true,
            entries_per_request: default_entries_per_request(),
            context_entries_count: default_context_entries_count(),
            pipeline_mode: default_pipeline_mode(),
            subtitle_preset: default_subtitle_preset(),
            no_reflection: false,
        }
    }
}

/// Information about a subtitle track
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleInfo {
    /// The index/id of the subtitle track
    pub index: usize,
    /// The codec name of the subtitle track
    pub codec_name: String,
    /// The language code (ISO 639-1 or ISO 639-2)
    pub language: Option<String>,
    /// The title of the subtitle track if available
    pub title: Option<String>,
}

/// Configuration for subtitle processing
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubtitleConfig {
    /// Whether to preserve formatting tags
    #[serde(default = "default_true")]
    pub preserve_formatting: bool,
    
    /// Whether to adjust timing for better readability
    #[serde(default)]
    pub adjust_timing: bool,
}

impl Default for SubtitleConfig {
    fn default() -> Self {
        Self {
            preserve_formatting: true,
            adjust_timing: false,
        }
    }
}

/// Session and persistence configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionConfig {
    /// Path to the database file (empty = default location)
    #[serde(default)]
    pub database_path: String,
    
    /// Whether to automatically resume interrupted sessions
    #[serde(default = "default_true")]
    pub auto_resume: bool,
    
    /// Number of days to keep session data (0 = forever)
    #[serde(default = "default_session_retention_days")]
    pub keep_sessions_days: u32,
    
    /// Whether session persistence is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_session_retention_days() -> u32 {
    30
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            database_path: String::new(),
            auto_resume: true,
            keep_sessions_days: default_session_retention_days(),
            enabled: true,
        }
    }
}

/// Cache configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CacheConfig {
    /// Whether caching is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Maximum in-memory cache size in MB (0 = unlimited)
    #[serde(default = "default_cache_size_mb")]
    pub in_memory_size_mb: u32,
    
    /// Whether to use cross-session database caching
    #[serde(default = "default_true")]
    pub cross_session: bool,
}

fn default_cache_size_mb() -> u32 {
    50
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            in_memory_size_mb: default_cache_size_mb(),
            cross_session: true,
        }
    }
}

/// Validation configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ValidationConfig {
    /// Whether validation is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Whether to validate markers in batch responses
    #[serde(default = "default_true")]
    pub marker_validation: bool,
    
    /// Whether to validate timecodes
    #[serde(default = "default_true")]
    pub timecode_validation: bool,
    
    /// Whether to validate format preservation
    #[serde(default = "default_true")]
    pub format_validation: bool,
    
    /// Minimum acceptable length ratio (translated / source)
    #[serde(default = "default_length_ratio_min")]
    pub length_ratio_min: f64,
    
    /// Maximum acceptable length ratio (translated / source)
    #[serde(default = "default_length_ratio_max")]
    pub length_ratio_max: f64,
    
    /// Maximum characters per second for readability
    #[serde(default = "default_max_chars_per_second")]
    pub max_chars_per_second: f64,
    
    /// Whether to retry translations that fail validation
    #[serde(default = "default_true")]
    pub retry_on_validation_failure: bool,
    
    /// Maximum number of retries for failed validations
    #[serde(default = "default_validation_max_retries")]
    pub max_retries: u32,
}

fn default_length_ratio_min() -> f64 {
    0.3
}

fn default_length_ratio_max() -> f64 {
    3.0
}

fn default_max_chars_per_second() -> f64 {
    25.0
}

fn default_validation_max_retries() -> u32 {
    3
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            marker_validation: true,
            timecode_validation: true,
            format_validation: true,
            length_ratio_min: default_length_ratio_min(),
            length_ratio_max: default_length_ratio_max(),
            max_chars_per_second: default_max_chars_per_second(),
            retry_on_validation_failure: true,
            max_retries: default_validation_max_retries(),
        }
    }
}

/// Experimental features configuration
/// All flags default to false for safe rollout
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ExperimentalFeatures {
    /// Auto-tune concurrency based on provider characteristics
    #[serde(default)]
    pub enable_auto_tune_concurrency: bool,

    /// Adaptive batch sizing based on token limits
    #[serde(default)]
    pub enable_adaptive_batch_sizing: bool,

    /// Warm L1 cache from L2 on startup
    #[serde(default)]
    pub enable_cache_warming: bool,

    /// Keep N batches in-flight for better throughput
    #[serde(default)]
    pub enable_speculative_batching: bool,

    /// Use language-pair specific length ratio thresholds
    #[serde(default)]
    pub enable_language_pair_thresholds: bool,

    /// Run glossary preflight checks before translation
    #[serde(default)]
    pub enable_glossary_preflight: bool,

    /// Use fuzzy matching for glossary terms
    #[serde(default)]
    pub enable_fuzzy_glossary_matching: bool,

    /// Retry with structured feedback on validation failure
    #[serde(default)]
    pub enable_feedback_retry: bool,

    /// Use AI to validate semantic accuracy
    #[serde(default)]
    pub enable_semantic_validation: bool,

    /// Dynamic context window sizing based on content
    #[serde(default)]
    pub enable_dynamic_context_window: bool,

    /// Scene-aware batching for better context
    #[serde(default)]
    pub enable_scene_aware_batching: bool,

    /// Track and maintain speaker consistency
    #[serde(default)]
    pub enable_speaker_tracking: bool,
}

/// Log verbosity level
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

fn default_concurrent_requests() -> usize {
    4
}

fn default_max_chars_per_request() -> usize {
    1000
}

fn default_anthropic_max_chars_per_request() -> usize {
    8000
}

fn default_timeout_secs() -> u64 {
    30
}

fn default_anthropic_timeout_secs() -> u64 {
    60
}

fn default_rate_limit_delay_ms() -> u64 {
    500 // 500ms default delay between requests
}

fn default_retry_count() -> u32 {
    3 // Default to 3 retries
}

fn default_retry_backoff_ms() -> u64 {
    1000 // 1 second base backoff time, doubled on each retry
}

fn default_temperature() -> f32 {
    0.3
}

fn default_entries_per_request() -> usize {
    3 // Sweet spot for most LLMs - balances request overhead with parallelism
}

fn default_true() -> bool {
    true
}

fn default_ollama_endpoint() -> String {
    "http://localhost:11434".to_string()
}

fn default_openai_endpoint() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_anthropic_endpoint() -> String {
    "https://api.anthropic.com".to_string()
}

fn default_lmstudio_endpoint() -> String {
    // LM Studio default server (OpenAI compatible) runs on port 1234 under /v1
    "http://localhost:1234/v1".to_string()
}

fn default_ollama_model() -> String {
    "llama2".to_string()
}

fn default_openai_model() -> String {
    "gpt-3.5-turbo".to_string()
}

fn default_anthropic_model() -> String {
    "claude-haiku-4-5".to_string()
}

fn default_lmstudio_model() -> String {
    // Placeholder; users should set to the loaded model name in LM Studio
    "local-model".to_string()
}

fn default_system_prompt() -> String {
    "You are a professional translator. Translate the following text from {source_language} to {target_language}. Preserve formatting and maintain the original meaning and tone.".to_string()
}

fn default_anthropic_rate_limit() -> Option<u32> {
    // Default to 45 requests per minute (slightly below the 50 limit for safety)
    // Anthropic's standard rate limit is 50 requests per minute
    // We use a slightly lower limit to prevent edge cases where our
    // timer might not be perfectly synced with Anthropic's
    Some(45)
}

// Default rate limits for providers
fn default_ollama_rate_limit() -> Option<u32> {
    None // No rate limit by default for local provider
}

fn default_openai_rate_limit() -> Option<u32> {
    Some(60) // 60 requests per minute by default
}

// LM Studio is local; do not enforce rate limiting by default
fn default_lmstudio_rate_limit() -> Option<u32> {
    None
}

// vLLM defaults - optimized for high-throughput local inference
fn default_vllm_endpoint() -> String {
    "http://localhost:8000/v1".to_string()
}

fn default_vllm_model() -> String {
    // Common default; users should set to their loaded model
    "meta-llama/Llama-3.1-8B-Instruct".to_string()
}

fn default_vllm_concurrent_requests() -> usize {
    16 // vLLM handles high concurrency well with continuous batching
}

fn default_vllm_max_chars_per_request() -> usize {
    4000 // vLLM can handle larger context efficiently
}

fn default_vllm_timeout_secs() -> u64 {
    180 // Longer timeout for batch processing
}

fn default_vllm_rate_limit() -> Option<u32> {
    None // No rate limit for local vLLM server
}

impl Config {
    
    /// Validate the configuration for consistency and required values
    pub fn validate(&self) -> Result<()> {
        // Validate languages
        let _source_name = crate::language_utils::get_language_name(&self.source_language)?;
        let _target_name = crate::language_utils::get_language_name(&self.target_language)?;
        
        // Validate API key for all providers except Ollama
        match self.translation.provider {
            TranslationProvider::OpenAI => {
                let api_key = self.translation.get_api_key();
                if api_key.is_empty() {
                    return Err(anyhow!("Translation API key is required for OpenAI provider"));
                }
            },
            TranslationProvider::Anthropic => {
                let api_key = self.translation.get_api_key();
                if api_key.is_empty() {
                    return Err(anyhow!("Translation API key is required for Anthropic provider"));
                }
            },
            _ => {}
        }
        
        Ok(())
    }
    
    
}

/// Default implementation for Config
impl Default for Config {
    fn default() -> Self {
        Config {
            source_language: "en".to_string(),
            target_language: "fr".to_string(),
            translation: TranslationConfig::default(),
            session: SessionConfig::default(),
            cache: CacheConfig::default(),
            validation: ValidationConfig::default(),
            experimental: ExperimentalFeatures::default(),
            log_level: LogLevel::default(),
        }
    }
}


impl TranslationConfig {
    pub fn optimal_concurrent_requests(&self) -> usize {
        // Check if the provider exists in the available_providers
        if let Some(provider_config) = self.get_active_provider_config() {
            return provider_config.concurrent_requests;
        }
        
        // Default fallback
        default_concurrent_requests()
    }
    
    /// Get the active provider configuration from the available_providers array
    pub fn get_active_provider_config(&self) -> Option<&ProviderConfig> {
        let provider_str = self.provider.to_lowercase_string();
        self.available_providers.iter()
            .find(|p| p.provider_type == provider_str)
    }
    
    /// Get the model for the active provider
    pub fn get_model(&self) -> String {
        if let Some(provider_config) = self.get_active_provider_config() {
            if !provider_config.model.is_empty() {
                return provider_config.model.clone();
            }
        }
        
        // Default fallback based on provider type
        match self.provider {
            TranslationProvider::Ollama => default_ollama_model(),
            TranslationProvider::OpenAI => default_openai_model(),
            TranslationProvider::Anthropic => default_anthropic_model(),
            TranslationProvider::LMStudio => default_lmstudio_model(),
            TranslationProvider::VLLM => default_vllm_model(),
        }
    }
    
    /// Get the API key for the active provider
    pub fn get_api_key(&self) -> String {
        if let Some(provider_config) = self.get_active_provider_config() {
            if !provider_config.api_key.is_empty() {
                return provider_config.api_key.clone();
            }
        }
        
        // Default fallback - Ollama doesn't use API keys
        String::new()
    }
    
    /// Get the endpoint for the active provider
    pub fn get_endpoint(&self) -> String {
        if let Some(provider_config) = self.get_active_provider_config() {
            if !provider_config.endpoint.is_empty() {
                return provider_config.endpoint.clone();
            }
        }
        
        // Default fallback based on provider type
        match self.provider {
            TranslationProvider::Ollama => default_ollama_endpoint(),
            TranslationProvider::OpenAI => default_openai_endpoint(),
            TranslationProvider::Anthropic => default_anthropic_endpoint(),
            TranslationProvider::LMStudio => default_lmstudio_endpoint(),
            TranslationProvider::VLLM => default_vllm_endpoint(),
        }
    }
    
    /// Get the max chars per request for the active provider
    pub fn get_max_chars_per_request(&self) -> usize {
        if let Some(provider_config) = self.get_active_provider_config() {
            if provider_config.max_chars_per_request > 0 {
                return provider_config.max_chars_per_request;
            }
        }
        
        // Default fallback
        default_max_chars_per_request()
    }
    
    
    
    
    /// Get the rate limit for the active provider
    pub fn get_rate_limit(&self) -> Option<u32> {
        if let Some(provider_config) = self.get_active_provider_config() {
            return provider_config.rate_limit;
        }
        
        // Default fallback based on provider type
        match self.provider {
            TranslationProvider::Ollama => default_ollama_rate_limit(),
            TranslationProvider::OpenAI => default_openai_rate_limit(),
            TranslationProvider::Anthropic => default_anthropic_rate_limit(),
            TranslationProvider::LMStudio => default_lmstudio_rate_limit(),
            TranslationProvider::VLLM => default_vllm_rate_limit(),
        }
    }
}

impl Default for TranslationConfig {
    fn default() -> Self {
        let mut config = Self {
            provider: TranslationProvider::default(),
            available_providers: Vec::new(),
            common: TranslationCommonConfig::default(),
        };
        
        // Add default providers
        config.available_providers.push(ProviderConfig::new(TranslationProvider::Ollama));
        config.available_providers.push(ProviderConfig::new(TranslationProvider::OpenAI));
        config.available_providers.push(ProviderConfig::new(TranslationProvider::Anthropic));
        config.available_providers.push(ProviderConfig::new(TranslationProvider::LMStudio));
        config.available_providers.push(ProviderConfig::new(TranslationProvider::VLLM));

        config
    }
} 