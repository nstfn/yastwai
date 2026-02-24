/*!
 * Provider-specific concurrency tuning.
 *
 * This module provides optimal concurrency settings based on provider characteristics
 * such as rate limits, response times, and API constraints.
 */

use crate::app_config::TranslationProvider;

/// Provider-specific concurrency profile with tuned defaults
#[derive(Debug, Clone)]
pub struct ProviderProfile {
    /// Maximum concurrent requests
    pub max_concurrent_requests: usize,
    /// Target requests per minute (for rate limiting)
    pub target_rpm: Option<u32>,
    /// Recommended batch size for this provider
    pub recommended_batch_size: usize,
    /// Whether provider supports streaming
    pub supports_streaming: bool,
}

impl ProviderProfile {
    /// Get the optimal profile for a given provider
    pub fn for_provider(provider: TranslationProvider) -> Self {
        match provider {
            TranslationProvider::Ollama => Self {
                // Ollama is local, can handle high concurrency
                max_concurrent_requests: 8,
                target_rpm: None, // No rate limit for local
                recommended_batch_size: 5,
                supports_streaming: true,
            },
            TranslationProvider::OpenAI => Self {
                // OpenAI has good throughput but rate limits
                max_concurrent_requests: 10,
                target_rpm: Some(60),
                recommended_batch_size: 5,
                supports_streaming: true,
            },
            TranslationProvider::Anthropic => Self {
                // Anthropic has lower rate limits but larger context
                max_concurrent_requests: 5,
                target_rpm: Some(45),
                recommended_batch_size: 8,
                supports_streaming: true,
            },
            TranslationProvider::LMStudio => Self {
                // LM Studio is local like Ollama
                max_concurrent_requests: 6,
                target_rpm: None,
                recommended_batch_size: 4,
                supports_streaming: true,
            },
            TranslationProvider::VLLM => Self {
                // vLLM excels at high-throughput with continuous batching
                max_concurrent_requests: 16,
                target_rpm: None, // No rate limit for local vLLM
                recommended_batch_size: 10,
                supports_streaming: true,
            },
        }
    }

    /// Get effective concurrent requests, respecting any user override
    pub fn effective_concurrent_requests(&self, user_override: Option<usize>) -> usize {
        user_override.unwrap_or(self.max_concurrent_requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_profile_for_ollama_should_have_high_concurrency() {
        let profile = ProviderProfile::for_provider(TranslationProvider::Ollama);
        assert_eq!(profile.max_concurrent_requests, 8);
        assert!(profile.target_rpm.is_none());
    }

    #[test]
    fn test_provider_profile_for_openai_should_have_rate_limit() {
        let profile = ProviderProfile::for_provider(TranslationProvider::OpenAI);
        assert_eq!(profile.max_concurrent_requests, 10);
        assert_eq!(profile.target_rpm, Some(60));
    }

    #[test]
    fn test_provider_profile_for_anthropic_should_have_conservative_settings() {
        let profile = ProviderProfile::for_provider(TranslationProvider::Anthropic);
        assert_eq!(profile.max_concurrent_requests, 5);
        assert_eq!(profile.target_rpm, Some(45));
        assert_eq!(profile.recommended_batch_size, 8);
    }

    #[test]
    fn test_provider_profile_for_lmstudio_should_be_local_like() {
        let profile = ProviderProfile::for_provider(TranslationProvider::LMStudio);
        assert!(profile.target_rpm.is_none());
    }

    #[test]
    fn test_effective_concurrent_requests_with_override_should_use_override() {
        let profile = ProviderProfile::for_provider(TranslationProvider::Ollama);
        assert_eq!(profile.effective_concurrent_requests(Some(3)), 3);
        assert_eq!(profile.effective_concurrent_requests(None), 8);
    }
}
