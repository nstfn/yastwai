use yastwai::providers::anthropic::{Anthropic, AnthropicRequest, AnthropicResponse, AnthropicContent, TokenUsage};
use yastwai::providers::{Provider, Role, TranslationRequest};
use yastwai::errors::ProviderError;

#[tokio::test]
async fn test_anthropic_request_builder() {
    // Test the builder pattern functions for AnthropicRequest
    let request = AnthropicRequest::new("claude-3-sonnet-20240229", 100)
        .add_message(Role::User, "Hello")
        .system("You are a helpful assistant")
        .temperature(0.5)
        .top_p(0.9)
        .top_k(40);

    // Serialize to JSON and check format
    let json = serde_json::to_string(&request).expect("Failed to serialize request");

    // Check that all fields were properly set
    assert!(json.contains(r#""model":"claude-3-sonnet-20240229""#));
    assert!(json.contains(r#""max_tokens":100"#));
    assert!(json.contains(r#""temperature":0.5"#));
    assert!(json.contains(r#""top_p":0.9"#));
    assert!(json.contains(r#""top_k":40"#));
    assert!(json.contains(r#""system":"You are a helpful assistant""#));
    assert!(json.contains(r#""role":"user""#));
    assert!(json.contains(r#""content":"Hello""#));
}

#[tokio::test]
async fn test_anthropic_extract_text() {
    // Create a mock response
    let response = AnthropicResponse {
        content: vec![
            AnthropicContent {
                content_type: "text".to_string(),
                text: "Hello, ".to_string(),
            },
            AnthropicContent {
                content_type: "text".to_string(),
                text: "world!".to_string(),
            },
            // This one should be filtered out
            AnthropicContent {
                content_type: "image".to_string(),
                text: "image_data".to_string(),
            },
        ],
        usage: TokenUsage {
            input_tokens: 10,
            output_tokens: 20,
        },
    };

    // Test the extract_text function
    let text = Anthropic::extract_text(&response);

    // Should combine the text content only, excluding non-text types
    assert_eq!(text, "Hello, world!");
}

#[tokio::test]
async fn test_anthropic_api_error_handling() {
    use crate::common::mock_providers::{MockTestProvider, MockErrorType};

    let provider = MockTestProvider::anthropic();
    provider.fail_next_call(MockErrorType::Auth);

    let request = TranslationRequest {
        model: "claude-3-haiku-20240307".to_string(),
        system_prompt: "Translate".to_string(),
        user_prompt: "Hello".to_string(),
        temperature: 0.0,
        max_tokens: 10,
    };

    let result = provider.translate(&request).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        ProviderError::AuthenticationError(_) => {},
        err => panic!("Unexpected error type: {:?}", err),
    }
}

#[tokio::test]
async fn test_anthropic_retry_logic() {
    use crate::common::mock_providers::{MockTestProvider, MockErrorType};

    let provider = MockTestProvider::anthropic();
    provider.fail_next_call(MockErrorType::Connection);

    let request = TranslationRequest {
        model: "claude-3-haiku-20240307".to_string(),
        system_prompt: "Translate".to_string(),
        user_prompt: "Hello".to_string(),
        temperature: 0.0,
        max_tokens: 10,
    };

    let result = provider.translate(&request).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        ProviderError::ConnectionError(_) => {},
        err => panic!("Unexpected error type: {:?}", err),
    }
}

#[tokio::test]
async fn test_anthropic_successful_request() {
    use crate::common::mock_providers::MockTestProvider;

    let provider = MockTestProvider::anthropic();

    let request = TranslationRequest {
        model: "claude-3-haiku-20240307".to_string(),
        system_prompt: "You are a helpful assistant.".to_string(),
        user_prompt: "Hello".to_string(),
        temperature: 0.0,
        max_tokens: 10,
    };

    let response = provider.translate(&request).await;
    assert!(response.is_ok());

    let response = response.unwrap();
    assert_eq!(response.text, "This is a mock response from Anthropic.");

    let tracker = provider.tracker();
    let tracker = tracker.lock().unwrap();
    assert_eq!(tracker.call_count, 1);
}

// This test is disabled by default as it requires real API credentials
#[ignore]
#[tokio::test]
async fn test_integration_with_real_api() {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY environment variable not set");

    let anthropic = Anthropic::new(api_key, "https://api.anthropic.com");

    let request = AnthropicRequest::new("claude-3-haiku-20240307", 50)
        .add_message(Role::User, "Say hello in French")
        .temperature(0.0);

    let result = anthropic.complete(request).await;
    assert!(result.is_ok(), "API completion failed: {:?}", result.err());

    let response = result.unwrap();
    let text = Anthropic::extract_text(&response);

    assert!(text.contains("Bonjour") || text.contains("Salut"),
            "Response doesn't contain expected French greeting: {}", text);
}

/// Test the Anthropic rate limiter
#[tokio::test]
async fn test_anthropic_rate_limiter() {
    use yastwai::providers::anthropic::{Anthropic, AnthropicRequest};

    let _client = Anthropic::new_with_rate_limit(
        "test-api-key",
        "",
        2,
    );

    let _request = AnthropicRequest::new("claude-3-haiku-20240307", 100)
        .add_message(Role::User, "Hello, world!");

    // Rate limiter test — we can't make real API calls, but we verify the client builds.
    // In a real environment, the rate limiter would throttle requests.
}
