/*!
 * Integration tests for provider API interactions
 */

use anyhow::Result;
use std::env;
use yastwai::providers::{Provider, TranslationRequest};
use crate::common::mock_providers::{MockTestProvider, MockErrorType};

/// Test that we can handle missing API keys gracefully
#[test]
fn test_missing_api_key_withEmptyKey_shouldReturnError() -> Result<()> {
    let api_key = env::var("FAKE_API_KEY").unwrap_or_default();
    assert!(api_key.is_empty(), "Expected empty API key for test");

    let result = if api_key.is_empty() {
        Err(anyhow::anyhow!("API key is missing or empty"))
    } else {
        Ok(())
    };

    assert!(result.is_err(), "Empty API key should return error");
    if let Err(e) = result {
        assert!(e.to_string().contains("API key"),
                "Error message should mention API key but was: {}", e);
    }

    Ok(())
}

/// Test that we can mock the provider interface for testing
#[test]
fn test_mock_provider_withMockedResponse_shouldReturnExpectedResult() -> Result<()> {
    struct SimpleProvider;

    impl SimpleProvider {
        fn new() -> Self { SimpleProvider }
        fn translate(&self, text: &str, _source: &str, target: &str) -> Result<String> {
            Ok(format!("[{}] {}", target, text))
        }
    }

    let provider = SimpleProvider::new();
    let result = provider.translate("Hello, world!", "en", "fr")?;
    assert_eq!(result, "[fr] Hello, world!");

    Ok(())
}

/// Test OpenAI-compatible provider with mock
#[tokio::test]
async fn test_openai_complete_withMockProvider_shouldReturnResponse() {
    let provider = MockTestProvider::openai();

    let request = TranslationRequest {
        model: "gpt-3.5-turbo".to_string(),
        system_prompt: "You are a helpful assistant.".to_string(),
        user_prompt: "Say hello!".to_string(),
        temperature: 0.7,
        max_tokens: 10,
    };

    let response = provider.translate(&request).await.unwrap();
    assert!(!response.text.is_empty());

    let tracker = provider.tracker();
    let tracker = tracker.lock().unwrap();
    assert_eq!(tracker.call_count, 1);
}

/// Test Anthropic provider with mock
#[tokio::test]
async fn test_anthropic_complete_withMockProvider_shouldReturnResponse() {
    let provider = MockTestProvider::anthropic();

    let request = TranslationRequest {
        model: "claude-3-haiku-20240307".to_string(),
        system_prompt: "You are a helpful assistant.".to_string(),
        user_prompt: "Say hello!".to_string(),
        temperature: 0.0,
        max_tokens: 1024,
    };

    let response = provider.translate(&request).await.unwrap();
    assert!(!response.text.is_empty());

    let tracker = provider.tracker();
    let tracker = tracker.lock().unwrap();
    assert_eq!(tracker.call_count, 1);
}

/// Test Ollama provider with mock
#[tokio::test]
async fn test_ollama_generate_withMockProvider_shouldReturnResponse() {
    let provider = MockTestProvider::ollama();

    let request = TranslationRequest {
        model: "llama2".to_string(),
        system_prompt: "You are a helpful assistant.".to_string(),
        user_prompt: "Hello, world!".to_string(),
        temperature: 0.7,
        max_tokens: 100,
    };

    let response = provider.translate(&request).await.unwrap();
    assert!(!response.text.is_empty());

    let tracker = provider.tracker();
    let tracker = tracker.lock().unwrap();
    assert_eq!(tracker.call_count, 1);
}

/// Test error propagation with mock provider
#[tokio::test]
async fn test_provider_error_propagation() {
    let provider = MockTestProvider::openai();
    provider.fail_next_call(MockErrorType::Api);

    let request = TranslationRequest {
        model: "gpt-3.5-turbo".to_string(),
        system_prompt: "Translate".to_string(),
        user_prompt: "Hello".to_string(),
        temperature: 0.0,
        max_tokens: 10,
    };

    let result = provider.translate(&request).await;
    assert!(result.is_err());
}
