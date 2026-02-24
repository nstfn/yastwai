/*!
 * Mock provider implementations for testing
 *
 * This module provides mock implementations of all providers to avoid
 * external API calls in tests. Each provider implements the Provider trait
 * and returns predetermined responses.
 */

use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use anyhow::Result;

use yastwai::errors::ProviderError;
use yastwai::providers::{Provider, TranslationRequest, TranslationResponse};

/// Tracks API calls to ensure no actual external requests are made
#[derive(Debug, Default)]
pub struct ApiCallTracker {
    /// Count of mock API calls made
    pub call_count: usize,
    /// Last request received
    pub last_request: Option<String>,
    /// Should the next call fail
    pub should_fail: bool,
    /// Error to return if failing
    pub error_type: MockErrorType,
}

/// Type of error to simulate
#[derive(Debug, Clone, Copy)]
#[derive(Default)]
pub enum MockErrorType {
    /// Authentication error (invalid API key)
    #[default]
    Auth,
    /// Connection error
    Connection,
    /// Rate limit error
    RateLimit,
    /// API error
    Api,
}

/// Mock provider that implements the unified Provider trait
#[derive(Debug)]
pub struct MockTestProvider {
    tracker: Arc<Mutex<ApiCallTracker>>,
    name: &'static str,
    response_text: String,
}

impl MockTestProvider {
    /// Create a new mock provider with a given name and default response
    pub fn new(name: &'static str, response_text: impl Into<String>) -> Self {
        Self {
            tracker: Arc::new(Mutex::new(ApiCallTracker::default())),
            name,
            response_text: response_text.into(),
        }
    }

    /// Create a mock OpenAI provider
    pub fn openai() -> Self {
        Self::new("OpenAI", "This is a mock response from OpenAI.")
    }

    /// Create a mock Anthropic provider
    pub fn anthropic() -> Self {
        Self::new("Anthropic", "This is a mock response from Anthropic.")
    }

    /// Create a mock Ollama provider
    pub fn ollama() -> Self {
        Self::new("Ollama", "This is a mock response from Ollama.")
    }

    /// Get the API call tracker
    pub fn tracker(&self) -> Arc<Mutex<ApiCallTracker>> {
        self.tracker.clone()
    }

    /// Configure the mock to fail on the next call
    pub fn fail_next_call(&self, error_type: MockErrorType) {
        let mut tracker = self.tracker.lock().unwrap();
        tracker.should_fail = true;
        tracker.error_type = error_type;
    }
}

impl Provider for MockTestProvider {
    fn translate<'a>(
        &'a self,
        request: &'a TranslationRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TranslationResponse, ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            let mut tracker = self.tracker.lock().unwrap();
            tracker.call_count += 1;
            tracker.last_request = Some(format!("{:?}", request));

            if tracker.should_fail {
                tracker.should_fail = false;
                return match tracker.error_type {
                    MockErrorType::Auth => Err(ProviderError::AuthenticationError("Invalid API key".into())),
                    MockErrorType::Connection => Err(ProviderError::ConnectionError("Connection failed".into())),
                    MockErrorType::RateLimit => Err(ProviderError::RateLimitExceeded { message: "Rate limit exceeded".into(), retry_after_secs: None }),
                    MockErrorType::Api => Err(ProviderError::ApiError {
                        status_code: 400,
                        message: "Bad request".into()
                    }),
                };
            }

            Ok(TranslationResponse {
                text: self.response_text.clone(),
                input_tokens: Some(10),
                output_tokens: Some(20),
            })
        })
    }

    fn test_connection<'a>(
        &'a self,
        _model: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            let tracker = self.tracker.lock().unwrap();
            if tracker.should_fail {
                match tracker.error_type {
                    MockErrorType::Connection => Err(ProviderError::ConnectionError("Connection failed".into())),
                    _ => Ok(()),
                }
            } else {
                Ok(())
            }
        })
    }

    fn provider_name(&self) -> &'static str {
        self.name
    }
}

/// Factory for creating mock providers
#[derive(Debug, Default)]
pub struct MockProviderFactory;

impl MockProviderFactory {
    /// Create a new mock provider factory
    pub fn new() -> Self {
        MockProviderFactory
    }

    /// Create a mock OpenAI provider
    pub fn create_openai(&self) -> MockTestProvider {
        MockTestProvider::openai()
    }

    /// Create a mock Anthropic provider
    pub fn create_anthropic(&self) -> MockTestProvider {
        MockTestProvider::anthropic()
    }

    /// Create a mock Ollama provider
    pub fn create_ollama(&self) -> MockTestProvider {
        MockTestProvider::ollama()
    }
}

/// Helper function to create a translation service with mock providers
pub fn create_mock_translation_service() -> Result<yastwai::translation::core::TranslationService> {
    use yastwai::app_config::{TranslationConfig, TranslationProvider, TranslationCommonConfig, ProviderConfig};

    let config = TranslationConfig {
        provider: TranslationProvider::OpenAI,
        common: TranslationCommonConfig {
            system_prompt: "You are a translator. Translate the following text from {source_language} to {target_language}. Only return the translated text.".into(),
            rate_limit_delay_ms: 0,
            retry_count: 1,
            retry_backoff_ms: 1,
            temperature: 0.3,
            parallel_mode: true,
            entries_per_request: 3,
            context_entries_count: 3,
            pipeline_mode: "legacy".to_string(),
        },
        available_providers: vec![
            ProviderConfig {
                provider_type: "openai".to_string(),
                model: "gpt-3.5-turbo".to_string(),
                api_key: "mock-api-key".to_string(),
                endpoint: "".to_string(),
                concurrent_requests: 1,
                max_chars_per_request: 1000,
                timeout_secs: 1,
                rate_limit: Some(60),
            },
        ],
    };

    yastwai::translation::core::TranslationService::new(config)
}

/// Helper function to set up a test environment that captures any API calls
pub fn setup_api_call_monitor() -> Arc<Mutex<Vec<String>>> {
    Arc::new(Mutex::new(Vec::new()))
}

/// Helper to check if any API calls were made during a test
pub fn assert_no_api_calls(monitor: Arc<Mutex<Vec<String>>>) {
    let calls = monitor.lock().unwrap();
    assert!(
        calls.is_empty(),
        "Expected no API calls, but found: {:?}",
        *calls
    );
}
