/*!
 * Provider implementations for different translation services.
 *
 * This module contains client implementations for various LLM providers:
 * - Ollama: Local LLM server
 * - OpenAI-compatible: OpenAI, LM Studio, vLLM
 * - Anthropic: Anthropic API integration
 */

use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;

use serde::{Serialize, Deserialize};

use crate::errors::ProviderError;

/// Message role for chat-based LLM APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// Unified translation request for all providers.
#[derive(Debug, Clone)]
pub struct TranslationRequest {
    /// Model to use for translation
    pub model: String,
    /// System prompt (instructions for the LLM)
    pub system_prompt: String,
    /// User prompt (text to translate)
    pub user_prompt: String,
    /// Temperature for generation (0.0 - 2.0)
    pub temperature: f32,
    /// Maximum tokens to generate
    pub max_tokens: u32,
}

/// Unified translation response from all providers.
#[derive(Debug, Clone)]
pub struct TranslationResponse {
    /// The translated text
    pub text: String,
    /// Number of input/prompt tokens (if reported by provider)
    pub input_tokens: Option<u64>,
    /// Number of output/completion tokens (if reported by provider)
    pub output_tokens: Option<u64>,
}

/// Common trait for all LLM providers.
///
/// Uses boxed futures for object safety, allowing `Arc<dyn Provider>`.
pub trait Provider: Send + Sync + Debug {
    /// Send a translation request and get a response.
    fn translate<'a>(
        &'a self,
        request: &'a TranslationRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TranslationResponse, ProviderError>> + Send + 'a>>;

    /// Test that the provider is reachable and the credentials work.
    fn test_connection<'a>(
        &'a self,
        model: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), ProviderError>> + Send + 'a>>;

    /// Human-readable provider name (e.g. "OpenAI", "Ollama").
    fn provider_name(&self) -> &'static str;
}

pub mod http_client;
pub mod retry;
pub mod ollama;
pub mod openai;
pub mod anthropic;

#[cfg(test)]
pub mod mock;
