/*!
 * Tests for the provider implementations
 */

use yastwai::providers::Provider;
use yastwai::providers::Role;
use yastwai::providers::openai::{OpenAI, OpenAIRequest};
use yastwai::providers::anthropic::{Anthropic, AnthropicRequest};
use yastwai::providers::ollama::{Ollama, GenerationRequest, ChatRequest, ChatMessage};

/// Test the OpenAI provider
#[tokio::test]
#[ignore]
async fn test_openai_provider_withValidApiKey_shouldComplete() {
    // This test should only run if an API key is provided
    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        return;
    }
    
    let client = OpenAI::new(api_key, "");
    let request = OpenAIRequest::new("gpt-3.5-turbo")
        .add_message(Role::System, "You are a helpful assistant.")
        .add_message(Role::User, "Say hello!")
        .max_tokens(10);
    
    let response = client.complete(request).await.unwrap();
    assert!(!response.choices.is_empty());
    assert!(!response.choices[0].message.content.is_empty());
    
    // Output the response
    println!("OpenAI response: {}", response.choices[0].message.content);
}

/// Test the Anthropic provider
#[tokio::test]
#[ignore]
async fn test_anthropic_provider_withValidApiKey_shouldComplete() {
    // This test should only run if an API key is provided
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        return;
    }
    
    let client = Anthropic::new(api_key, "");
    let request = AnthropicRequest::new("claude-3-haiku-20240307", 1024)
        .system("You are a helpful assistant.")
        .add_message(Role::User, "Say hello!");
    
    let response = client.complete(request).await.unwrap();
    
    // Extract text from the response
    let text = if let Some(content) = response.content.first() {
        &content.text
    } else {
        ""
    };
    
    assert!(!text.is_empty());
    
    // Output the response
    println!("Anthropic response: {}", text);
}

/// Test the Ollama generate endpoint
#[tokio::test]
#[ignore]
async fn test_ollama_provider_withLocalServer_shouldGenerate() {
    // This test should only run if Ollama is available locally
    let client = Ollama::new("http://localhost", 11434);
    
    // Try to get the version, if it fails, skip the test
    if client.version().await.is_err() {
        println!("Skipping test because Ollama is not available");
        return;
    }
    
    let request = GenerationRequest::new("gemma3:27b", "Hello, world!")
        .system("You are a helpful assistant.")
        .temperature(0.7);
    
    let response = client.generate(request).await;
    assert!(response.is_ok());
    
    // Output the response
    if let Ok(resp) = response {
        println!("Ollama generate response: {}", resp.response);
    }
}

/// Test the Ollama chat endpoint
#[tokio::test]
#[ignore]
async fn test_ollama_provider_withLocalServer_shouldChat() {
    // This test should only run if Ollama is available locally
    let client = Ollama::new("http://localhost", 11434);
    
    // Try to get the version, if it fails, skip the test
    if client.version().await.is_err() {
        println!("Skipping test because Ollama is not available");
        return;
    }
    
    let messages = vec![
        ChatMessage { role: Role::User, content: "Hello, world!".to_string() }
    ];
    
    let request = ChatRequest::new("gemma3:27b", messages)
        .system("You are a helpful assistant.")
        .temperature(0.7);
    
    let response = client.chat(request).await;
    assert!(response.is_ok());
    
    // Output the response
    if let Ok(resp) = response {
        println!("Ollama chat response: {}", resp.message.content);
    }
} 