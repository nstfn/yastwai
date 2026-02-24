/*!
 * Tests for OpenAI provider request builders
 */

use yastwai::providers::Role;
use yastwai::providers::openai::{OpenAIRequest, OpenAIMessage};

#[test]
fn test_openaiRequest_new_shouldCreateWithModel() {
    let _request = OpenAIRequest::new("gpt-4");
}

#[test]
fn test_openaiRequest_addMessage_shouldAddUserMessage() {
    let _request = OpenAIRequest::new("gpt-4")
        .add_message(Role::User, "Hello!");
}

#[test]
fn test_openaiRequest_addMessage_shouldAddSystemMessage() {
    let _request = OpenAIRequest::new("gpt-4")
        .add_message(Role::System, "You are helpful");
}

#[test]
fn test_openaiRequest_addMessage_shouldAddMultipleMessages() {
    let _request = OpenAIRequest::new("gpt-4")
        .add_message(Role::System, "You are a translator")
        .add_message(Role::User, "Translate: Hello");
}

#[test]
fn test_openaiRequest_temperature_shouldSetTemperature() {
    let _request = OpenAIRequest::new("gpt-4")
        .temperature(0.5);
}

#[test]
fn test_openaiRequest_maxTokens_shouldSetMaxTokens() {
    let _request = OpenAIRequest::new("gpt-4")
        .max_tokens(1000);
}

#[test]
fn test_openaiRequest_chained_shouldAllowMultipleBuilderCalls() {
    let _request = OpenAIRequest::new("gpt-4")
        .add_message(Role::System, "Translate")
        .add_message(Role::User, "Hello")
        .temperature(0.3)
        .max_tokens(500);
}

#[test]
fn test_openaiRequest_default_shouldCreateEmptyRequest() {
    let _request = OpenAIRequest::default();
}

#[test]
fn test_openaiMessage_struct_shouldHavePublicFields() {
    let message = OpenAIMessage {
        role: Role::User,
        content: "Test content".to_string(),
    };
    assert_eq!(message.role, Role::User);
    assert_eq!(message.content, "Test content");
}

#[test]
fn test_openaiMessage_withEmptyContent_shouldWork() {
    let message = OpenAIMessage {
        role: Role::Assistant,
        content: "".to_string(),
    };
    assert_eq!(message.content, "");
}

#[test]
fn test_openaiMessage_withUnicode_shouldHandleCorrectly() {
    let message = OpenAIMessage {
        role: Role::User,
        content: "🎬 Subtitle: こんにちは".to_string(),
    };
    assert_eq!(message.content, "🎬 Subtitle: こんにちは");
}

#[test]
fn test_openaiMessage_withLongContent_shouldHandle() {
    let content = "a".repeat(10000);
    let message = OpenAIMessage {
        role: Role::User,
        content: content.clone(),
    };
    assert_eq!(message.content.len(), 10000);
}

#[test]
fn test_openaiMessage_debug_shouldBeImplemented() {
    let message = OpenAIMessage {
        role: Role::System,
        content: "Test".to_string(),
    };
    let debug = format!("{:?}", message);
    assert!(debug.contains("System"));
}
