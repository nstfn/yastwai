/*!
 * Tests to verify no external network calls are made in tests
 */

use anyhow::Result;
use yastwai::providers::{Provider, TranslationRequest};

// Testing that the mock provider correctly implements the Provider trait
#[tokio::test]
async fn test_mock_openai_provider_correctly_implements_provider_trait() -> Result<()> {
    use crate::common::mock_providers::MockTestProvider;

    let provider = MockTestProvider::openai();

    let request = TranslationRequest {
        model: "gpt-3.5-turbo".to_string(),
        system_prompt: "You are a helpful assistant.".to_string(),
        user_prompt: "Say hello!".to_string(),
        temperature: 0.7,
        max_tokens: 100,
    };

    let response = provider.translate(&request).await?;
    assert!(!response.text.is_empty());

    Ok(())
}

// Testing that the mock Anthropic provider correctly implements the Provider trait
#[tokio::test]
async fn test_mock_anthropic_provider_correctly_implements_provider_trait() -> Result<()> {
    use crate::common::mock_providers::MockTestProvider;

    let provider = MockTestProvider::anthropic();

    let request = TranslationRequest {
        model: "claude-3-haiku-20240307".to_string(),
        system_prompt: "You are a helpful assistant.".to_string(),
        user_prompt: "Hello".to_string(),
        temperature: 0.0,
        max_tokens: 10,
    };

    let response = provider.translate(&request).await?;
    assert!(!response.text.is_empty());

    Ok(())
}

// Testing that we can create and use the mock provider factory
#[test]
fn test_mock_provider_factory() {
    use crate::common::mock_providers::MockProviderFactory;

    let factory = MockProviderFactory::new();

    let openai = factory.create_openai();
    let anthropic = factory.create_anthropic();
    let ollama = factory.create_ollama();

    openai.fail_next_call(crate::common::mock_providers::MockErrorType::Auth);
    anthropic.fail_next_call(crate::common::mock_providers::MockErrorType::RateLimit);
    ollama.fail_next_call(crate::common::mock_providers::MockErrorType::Connection);
}

// Creating a mock translation service
#[test]
fn test_create_mock_translation_service() {
    use crate::common::mock_providers::create_mock_translation_service;

    let service_result = create_mock_translation_service();
    assert!(service_result.is_ok(), "Failed to create mock translation service");
}

// Test that API call monitoring works
#[test]
fn test_api_call_monitoring() {
    use crate::common::mock_providers::{setup_api_call_monitor, assert_no_api_calls};

    let monitor = setup_api_call_monitor();

    // No calls should have been made
    assert_no_api_calls(monitor.clone());

    // Add a call to the monitor for testing
    {
        let mut calls = monitor.lock().unwrap();
        calls.push("test call".to_string());
    }

    // This should fail because there's a call
    let result = std::panic::catch_unwind(|| {
        assert_no_api_calls(monitor.clone());
    });

    assert!(result.is_err(), "Expected assert_no_api_calls to panic");
}
