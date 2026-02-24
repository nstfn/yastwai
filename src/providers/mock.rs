/*!
 * Mock provider implementations for testing.
 *
 * This module provides mock providers that simulate different behaviors:
 * - `MockProvider::working()` - Always succeeds with translated text
 * - `MockProvider::partial()` - Sometimes fails, sometimes succeeds
 * - `MockProvider::failing()` - Always fails with an error
 */

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::errors::ProviderError;
use crate::providers::{Provider, TranslationRequest, TranslationResponse};

/// Mock request for testing (used by internal mock tests)
#[derive(Debug, Clone)]
pub struct MockRequest {
    /// The text to translate
    pub text: String,
    /// Source language
    pub source_language: String,
    /// Target language
    pub target_language: String,
}

/// Mock response for testing (used by internal mock tests)
#[derive(Debug, Clone)]
pub struct MockResponse {
    /// The translated text
    pub text: String,
    /// Simulated prompt tokens
    pub prompt_tokens: Option<u64>,
    /// Simulated completion tokens
    pub completion_tokens: Option<u64>,
}

/// Behavior mode for the mock provider
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MockBehavior {
    /// Always succeeds with a proper translation
    Working,
    /// Succeeds but returns malformed responses (missing markers)
    PartialMarkers,
    /// Fails intermittently (every Nth request)
    Intermittent { fail_every: usize },
    /// Always fails with an error
    Failing,
    /// Returns truncated responses (missing END marker)
    Truncated,
    /// Returns empty response
    Empty,
    /// Simulates slow response (for timeout testing)
    Slow { delay_ms: u64 },
}

/// Mock provider for testing translation behavior
#[derive(Debug)]
pub struct MockProvider {
    /// Behavior mode
    behavior: MockBehavior,
    /// Request counter for intermittent failures
    request_count: Arc<AtomicUsize>,
    /// Custom response generator (optional)
    custom_response: Option<fn(&MockRequest) -> String>,
}

impl MockProvider {
    /// Create a new mock provider with the specified behavior
    pub fn new(behavior: MockBehavior) -> Self {
        Self {
            behavior,
            request_count: Arc::new(AtomicUsize::new(0)),
            custom_response: None,
        }
    }

    /// Create a working mock provider that always succeeds
    pub fn working() -> Self {
        Self::new(MockBehavior::Working)
    }

    /// Create a partially working mock provider (missing markers)
    pub fn partial_markers() -> Self {
        Self::new(MockBehavior::PartialMarkers)
    }

    /// Create an intermittently failing mock provider
    pub fn intermittent(fail_every: usize) -> Self {
        Self::new(MockBehavior::Intermittent { fail_every })
    }

    /// Create a failing mock provider that always errors
    pub fn failing() -> Self {
        Self::new(MockBehavior::Failing)
    }

    /// Create a mock that returns truncated responses
    pub fn truncated() -> Self {
        Self::new(MockBehavior::Truncated)
    }

    /// Create a mock that returns empty responses
    pub fn empty() -> Self {
        Self::new(MockBehavior::Empty)
    }

    /// Set a custom response generator
    pub fn with_custom_response(mut self, generator: fn(&MockRequest) -> String) -> Self {
        self.custom_response = Some(generator);
        self
    }

    /// Generate a properly formatted batch response with markers
    pub fn generate_batch_response(entries: &[&str]) -> String {
        let mut response = String::new();
        for (i, entry) in entries.iter().enumerate() {
            response.push_str(&format!("<<ENTRY_{}>>\n", i));
            response.push_str(&format!("[TRANSLATED] {}\n", entry));
        }
        response.push_str("<<END>>");
        response
    }

    /// Generate a response with some missing markers
    pub fn generate_partial_response(entries: &[&str]) -> String {
        let mut response = String::new();
        for (i, entry) in entries.iter().enumerate() {
            if i == 0 || i == entries.len() - 1 {
                response.push_str(&format!("<<ENTRY_{}>>\n", i));
            }
            response.push_str(&format!("[TRANSLATED] {}\n", entry));
        }
        response.push_str("<<END>>");
        response
    }

    /// Generate a truncated response (no END marker)
    pub fn generate_truncated_response(entries: &[&str]) -> String {
        let mut response = String::new();
        for (i, entry) in entries.iter().enumerate() {
            response.push_str(&format!("<<ENTRY_{}>>\n", i));
            response.push_str(&format!("[TRANSLATED] {}\n", entry));
        }
        response
    }

    /// Internal complete method using MockRequest (for unit tests)
    pub async fn complete(&self, request: MockRequest) -> Result<MockResponse, ProviderError> {
        let count = self.request_count.fetch_add(1, Ordering::SeqCst);

        match self.behavior {
            MockBehavior::Working => {
                let text = if let Some(generator) = self.custom_response {
                    generator(&request)
                } else {
                    format!("[TRANSLATED to {}] {}", request.target_language, request.text)
                };

                Ok(MockResponse {
                    text,
                    prompt_tokens: Some(request.text.len() as u64),
                    completion_tokens: Some((request.text.len() / 2) as u64),
                })
            }

            MockBehavior::PartialMarkers => {
                let text = "<<ENTRY_0>>\n[TRANSLATED] First part\nMissing markers in middle\n<<END>>".to_string();
                Ok(MockResponse {
                    text,
                    prompt_tokens: Some(10),
                    completion_tokens: Some(10),
                })
            }

            MockBehavior::Intermittent { fail_every } => {
                if count % fail_every == fail_every - 1 {
                    Err(ProviderError::ApiError {
                        message: format!("Simulated intermittent failure (request #{})", count + 1),
                        status_code: 503,
                    })
                } else {
                    Ok(MockResponse {
                        text: format!("[TRANSLATED] {}", request.text),
                        prompt_tokens: Some(10),
                        completion_tokens: Some(10),
                    })
                }
            }

            MockBehavior::Failing => Err(ProviderError::ApiError {
                message: "Simulated provider failure".to_string(),
                status_code: 500,
            }),

            MockBehavior::Truncated => {
                let text = format!("<<ENTRY_0>>\n[TRANSLATED] {}", request.text);
                Ok(MockResponse {
                    text,
                    prompt_tokens: Some(10),
                    completion_tokens: Some(5),
                })
            }

            MockBehavior::Empty => Ok(MockResponse {
                text: String::new(),
                prompt_tokens: Some(0),
                completion_tokens: Some(0),
            }),

            MockBehavior::Slow { delay_ms } => {
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                Ok(MockResponse {
                    text: format!("[TRANSLATED] {}", request.text),
                    prompt_tokens: Some(10),
                    completion_tokens: Some(10),
                })
            }
        }
    }
}

impl Clone for MockProvider {
    fn clone(&self) -> Self {
        Self {
            behavior: self.behavior,
            request_count: Arc::clone(&self.request_count),
            custom_response: self.custom_response,
        }
    }
}

impl Provider for MockProvider {
    fn translate<'a>(
        &'a self,
        request: &'a TranslationRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TranslationResponse, ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            let mock_request = MockRequest {
                text: request.user_prompt.clone(),
                source_language: "en".to_string(),
                target_language: "target".to_string(),
            };

            let response = self.complete(mock_request).await?;

            Ok(TranslationResponse {
                text: response.text,
                input_tokens: response.prompt_tokens,
                output_tokens: response.completion_tokens,
            })
        })
    }

    fn test_connection<'a>(
        &'a self,
        _model: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            if matches!(self.behavior, MockBehavior::Failing) {
                Err(ProviderError::ConnectionError("Mock connection failure".to_string()))
            } else {
                Ok(())
            }
        })
    }

    fn provider_name(&self) -> &'static str {
        "Mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_workingProvider_shouldReturnTranslatedText() {
        let provider = MockProvider::working();
        let request = MockRequest {
            text: "Hello world".to_string(),
            source_language: "en".to_string(),
            target_language: "fr".to_string(),
        };

        let response = provider.complete(request).await.unwrap();
        assert!(response.text.contains("TRANSLATED"));
        assert!(response.text.contains("fr"));
    }

    #[tokio::test]
    async fn test_failingProvider_shouldReturnError() {
        let provider = MockProvider::failing();
        let request = MockRequest {
            text: "Hello".to_string(),
            source_language: "en".to_string(),
            target_language: "fr".to_string(),
        };

        let result = provider.complete(request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_intermittentProvider_shouldFailPeriodically() {
        let provider = MockProvider::intermittent(3);

        let request = MockRequest {
            text: "Test".to_string(),
            source_language: "en".to_string(),
            target_language: "fr".to_string(),
        };

        assert!(provider.complete(request.clone()).await.is_ok());
        assert!(provider.complete(request.clone()).await.is_ok());
        assert!(provider.complete(request.clone()).await.is_err());
        assert!(provider.complete(request.clone()).await.is_ok());
        assert!(provider.complete(request.clone()).await.is_ok());
        assert!(provider.complete(request.clone()).await.is_err());
    }

    #[tokio::test]
    async fn test_truncatedProvider_shouldReturnWithoutEndMarker() {
        let provider = MockProvider::truncated();
        let request = MockRequest {
            text: "Hello".to_string(),
            source_language: "en".to_string(),
            target_language: "fr".to_string(),
        };

        let response = provider.complete(request).await.unwrap();
        assert!(response.text.contains("<<ENTRY_0>>"));
        assert!(!response.text.contains("<<END>>"));
    }

    #[tokio::test]
    async fn test_emptyProvider_shouldReturnEmptyText() {
        let provider = MockProvider::empty();
        let request = MockRequest {
            text: "Hello".to_string(),
            source_language: "en".to_string(),
            target_language: "fr".to_string(),
        };

        let response = provider.complete(request).await.unwrap();
        assert!(response.text.is_empty());
    }

    #[test]
    fn test_generateBatchResponse_shouldIncludeAllMarkers() {
        let entries = vec!["First", "Second", "Third"];
        let response = MockProvider::generate_batch_response(&entries);

        assert!(response.contains("<<ENTRY_0>>"));
        assert!(response.contains("<<ENTRY_1>>"));
        assert!(response.contains("<<ENTRY_2>>"));
        assert!(response.contains("<<END>>"));
        assert!(response.contains("[TRANSLATED] First"));
        assert!(response.contains("[TRANSLATED] Second"));
        assert!(response.contains("[TRANSLATED] Third"));
    }

    #[test]
    fn test_generatePartialResponse_shouldSkipMiddleMarkers() {
        let entries = vec!["First", "Second", "Third"];
        let response = MockProvider::generate_partial_response(&entries);

        assert!(response.contains("<<ENTRY_0>>"));
        assert!(!response.contains("<<ENTRY_1>>"));
        assert!(response.contains("<<ENTRY_2>>"));
        assert!(response.contains("<<END>>"));
    }

    #[test]
    fn test_generateTruncatedResponse_shouldOmitEndMarker() {
        let entries = vec!["First", "Second"];
        let response = MockProvider::generate_truncated_response(&entries);

        assert!(response.contains("<<ENTRY_0>>"));
        assert!(response.contains("<<ENTRY_1>>"));
        assert!(!response.contains("<<END>>"));
    }

    #[tokio::test]
    async fn test_customResponseGenerator_shouldBeUsed() {
        let provider = MockProvider::working().with_custom_response(|req| {
            format!("CUSTOM: {} -> {}", req.source_language, req.target_language)
        });

        let request = MockRequest {
            text: "Test".to_string(),
            source_language: "en".to_string(),
            target_language: "de".to_string(),
        };

        let response = provider.complete(request).await.unwrap();
        assert_eq!(response.text, "CUSTOM: en -> de");
    }

    #[tokio::test]
    async fn test_clonedProvider_shouldShareRequestCount() {
        let provider = MockProvider::intermittent(2);
        let cloned = provider.clone();

        let request = MockRequest {
            text: "Test".to_string(),
            source_language: "en".to_string(),
            target_language: "fr".to_string(),
        };

        assert!(provider.complete(request.clone()).await.is_ok());
        assert!(cloned.complete(request.clone()).await.is_err());
    }

    #[tokio::test]
    async fn test_provider_trait_translate() {
        let provider = MockProvider::working();
        let request = TranslationRequest {
            model: "test-model".to_string(),
            system_prompt: "Translate".to_string(),
            user_prompt: "Hello world".to_string(),
            temperature: 0.3,
            max_tokens: 100,
        };

        let response = provider.translate(&request).await.unwrap();
        assert!(response.text.contains("TRANSLATED"));
    }

    #[tokio::test]
    async fn test_provider_trait_test_connection() {
        let working = MockProvider::working();
        assert!(working.test_connection("test").await.is_ok());

        let failing = MockProvider::failing();
        assert!(failing.test_connection("test").await.is_err());
    }
}
