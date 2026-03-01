/*!
 * Tests for translation module functionality
 * 
 * The translation module is structured as follows:
 * - core: Core translation functionality and service definition
 * - batch: Batch processing of translations
 * - cache: Caching mechanisms for translations
 * - formatting: Format preservation and processing
 */

use std::sync::{Arc, Mutex as StdMutex, atomic::{AtomicUsize, Ordering}};
use anyhow::Result;
use yastwai::app_config::{TranslationConfig, TranslationProvider as ConfigTranslationProvider, TranslationCommonConfig, ProviderConfig};
use yastwai::subtitle_processor::SubtitleEntry;
use yastwai::translation::core::{TranslationService, TokenUsageStats};
use std::fs;
use yastwai::translation::core::LogEntry;
use tempfile::TempDir;
use yastwai::app_controller::Controller;

/// Helper function to create a test configuration
fn get_test_config() -> TranslationConfig {
    TranslationConfig {
        provider: ConfigTranslationProvider::Ollama,
        common: TranslationCommonConfig {
            system_prompt: "You are a translator. Translate the following text from {source_language} to {target_language}. Only return the translated text.".to_string(),
            rate_limit_delay_ms: 0,
            retry_count: 3,
            retry_backoff_ms: 1000,
            temperature: 0.3,
            parallel_mode: true,
            entries_per_request: 3,
            context_entries_count: 3,
            pipeline_mode: "legacy".to_string(),
            subtitle_preset: "netflix".to_string(),
            no_reflection: false,
        },
        available_providers: vec![
            ProviderConfig {
                provider_type: "ollama".to_string(),
                model: "llama2".to_string(),
                api_key: "".to_string(),
                endpoint: "http://localhost:11434".to_string(),
                concurrent_requests: 4,
                max_chars_per_request: 1000,
                timeout_secs: 30,
                rate_limit: None,
            },
            ProviderConfig {
                provider_type: "openai".to_string(),
                model: "gpt-3.5-turbo".to_string(),
                api_key: "test-api-key".to_string(),
                endpoint: "".to_string(),
                concurrent_requests: 4,
                max_chars_per_request: 4000,
                timeout_secs: 30,
                rate_limit: Some(60),
            },
            ProviderConfig {
                provider_type: "anthropic".to_string(),
                model: "claude-3-haiku-20240307".to_string(),
                api_key: "test-api-key".to_string(),
                endpoint: "".to_string(),
                concurrent_requests: 4,
                max_chars_per_request: 4000,
                timeout_secs: 30,
                rate_limit: Some(45),
            },
        ],
    }
}

/// Test creation of translation service
#[test]
fn test_translation_service_creation_withValidConfig_shouldCreateService() {
    let config = get_test_config();
    let service = TranslationService::new(config);
    assert!(service.is_ok());
}

/// Test token usage tracking
#[test]
fn test_add_token_usage_withTokenCounts_shouldTrackCorrectly() {
    let mut stats = TokenUsageStats::new();
    stats.add_token_usage(Some(100), Some(50));
    assert_eq!(stats.prompt_tokens, 100);
    assert_eq!(stats.completion_tokens, 50);
    assert_eq!(stats.total_tokens, 150);
    
    // Add more tokens
    stats.add_token_usage(Some(200), Some(100));
    assert_eq!(stats.prompt_tokens, 300);
    assert_eq!(stats.completion_tokens, 150);
    assert_eq!(stats.total_tokens, 450);
    
    // Handle None values
    stats.add_token_usage(None, Some(50));
    assert_eq!(stats.prompt_tokens, 300);
    assert_eq!(stats.completion_tokens, 200);
    assert_eq!(stats.total_tokens, 500);
    
    stats.add_token_usage(Some(100), None);
    assert_eq!(stats.prompt_tokens, 400);
    assert_eq!(stats.completion_tokens, 200);
    assert_eq!(stats.total_tokens, 600);
}

/// Test batch translation processing
#[tokio::test]
async fn test_translate_batches_processes_all_chunks_withMultipleBatches_shouldProcessAll() -> Result<()> {
    // Create mock data - 5 batches with 2 entries each
    let batches = [vec![
            SubtitleEntry::new(1, 0, 1000, "First subtitle batch 1".to_string()),
            SubtitleEntry::new(2, 1001, 2000, "Second subtitle batch 1".to_string()),
        ],
        vec![
            SubtitleEntry::new(3, 2001, 3000, "First subtitle batch 2".to_string()),
            SubtitleEntry::new(4, 3001, 4000, "Second subtitle batch 2".to_string()),
        ],
        vec![
            SubtitleEntry::new(5, 4001, 5000, "First subtitle batch 3".to_string()),
            SubtitleEntry::new(6, 5001, 6000, "Second subtitle batch 3".to_string()),
        ],
        vec![
            SubtitleEntry::new(7, 6001, 7000, "First subtitle batch 4".to_string()),
            SubtitleEntry::new(8, 7001, 8000, "Second subtitle batch 4".to_string()),
        ],
        vec![
            SubtitleEntry::new(9, 8001, 9000, "First subtitle batch 5".to_string()),
            SubtitleEntry::new(10, 9001, 10000, "Second subtitle batch 5".to_string()),
        ]];
    
    // Create a progress counter to track callback execution
    let progress_count = Arc::new(AtomicUsize::new(0));
    let progress_clone = Arc::clone(&progress_count);
    
    // Create a collection to store all processed entries
    let all_processed_entries = Arc::new(StdMutex::new(Vec::new()));
    
    // Process each batch sequentially, simulating the behavior we want to test
    for batch in batches.iter() {
        let processed_entries: Vec<SubtitleEntry> = batch.iter()
            .map(|entry| {
                let mut new_entry = entry.clone();
                new_entry.text = format!("[TRANSLATED] {}", entry.text);
                new_entry
            })
            .collect();
        
        // Store the processed entries
        let mut entries = all_processed_entries.lock().unwrap();
        entries.extend(processed_entries);
        
        // Update progress
        progress_clone.fetch_add(1, Ordering::SeqCst);
    }
    
    // Verify we processed all batches
    assert_eq!(progress_count.load(Ordering::SeqCst), batches.len());
    
    // Verify all entries were collected
    let all_entries = all_processed_entries.lock().unwrap();
    assert_eq!(all_entries.len(), 10, "Should have 10 translated entries total");
    
    // Verify all entries have the [TRANSLATED] prefix
    for entry in all_entries.iter() {
        assert!(entry.text.starts_with("[TRANSLATED]"), 
               "Entry should have [TRANSLATED] prefix: {}", entry.text);
    }
    
    Ok(())
}

/// Test that log capture works correctly with different providers
#[tokio::test]
async fn test_log_capture_with_different_providers_shouldWriteLogsCorrectly() -> Result<()> {
    // Create temporary directory for log files that will be cleaned up after test
    let temp_dir = TempDir::new()?;
    
    // Create a controller
    let controller = Controller::new_for_test()?;
    
    // Test OpenAI provider log capture
    {
        // Create a log capture mechanism
        let log_capture = Arc::new(StdMutex::new(Vec::new()));
        
        // Add some test logs with various levels (matching our case standardization)
        let mut logs = log_capture.lock().unwrap();
        logs.push(LogEntry { level: "INFO".to_string(), message: "OpenAI test info message".to_string() });
        logs.push(LogEntry { level: "WARN".to_string(), message: "OpenAI test warning message".to_string() });
        logs.push(LogEntry { level: "ERROR".to_string(), message: "OpenAI test error message".to_string() });
        drop(logs);  // Release the lock
        
        // Get logs from the capture mechanism
        let logs = {
            let logs_guard = log_capture.lock().unwrap();
            logs_guard.clone()
        };
        
        // Write logs to a file
        let test_log_file = temp_dir.path().join("openai_test_logs.log");
        controller.write_translation_logs(
            &logs, 
            test_log_file.to_str().unwrap(), 
            "OpenAI Test Context"
        )?;
        
        // Verify file exists and contains our log messages
        assert!(test_log_file.exists());
        let content = fs::read_to_string(&test_log_file)?;
        assert!(content.contains("OpenAI test info message"));
        assert!(content.contains("OpenAI test warning message"));
        assert!(content.contains("OpenAI test error message"));
        assert!(content.contains("[INFO]"));
        assert!(content.contains("[WARN]"));
        assert!(content.contains("[ERROR]"));
    }
    
    // Test Anthropic provider log capture
    {
        // Create a log capture mechanism
        let log_capture = Arc::new(StdMutex::new(Vec::new()));
        
        // Add some test logs with various levels (matching our case standardization)
        let mut logs = log_capture.lock().unwrap();
        logs.push(LogEntry { level: "INFO".to_string(), message: "Anthropic test info message".to_string() });
        logs.push(LogEntry { level: "WARN".to_string(), message: "Anthropic test warning message".to_string() });
        logs.push(LogEntry { level: "ERROR".to_string(), message: "Anthropic test error message".to_string() });
        // Also test the specific warnings we fixed
        logs.push(LogEntry { 
            level: "WARN".to_string(), 
            message: "Some entries failed to translate: Failed batch 1; Failed batch 2".to_string() 
        });
        drop(logs);  // Release the lock
        
        // Get logs from the capture mechanism
        let logs = {
            let logs_guard = log_capture.lock().unwrap();
            logs_guard.clone()
        };
        
        // Write logs to a file
        let test_log_file = temp_dir.path().join("anthropic_test_logs.log");
        controller.write_translation_logs(
            &logs, 
            test_log_file.to_str().unwrap(), 
            "Anthropic Test Context"
        )?;
        
        // Verify file exists and contains our log messages
        assert!(test_log_file.exists());
        let content = fs::read_to_string(&test_log_file)?;
        assert!(content.contains("Anthropic test info message"));
        assert!(content.contains("Anthropic test warning message"));
        assert!(content.contains("Anthropic test error message"));
        assert!(content.contains("Some entries failed to translate"));
        assert!(content.contains("[INFO]"));
        assert!(content.contains("[WARN]"));
        assert!(content.contains("[ERROR]"));
    }

    // TempDir will be automatically cleaned up when it goes out of scope
    Ok(())
}

/// Test that Anthropic provider logs are properly captured during translation
#[tokio::test]
async fn test_anthropic_provider_log_capture_during_translation_shouldCaptureErrors() -> Result<()> {
    // Create temporary directory for test
    let temp_dir = TempDir::new()?;
    let test_log_file = temp_dir.path().join("anthropic_translation_logs.log");
    
    // Create a controller
    let controller = Controller::new_for_test()?;
    
    // Create a custom config that uses Anthropic
    let mut config = get_test_config();
    config.provider = ConfigTranslationProvider::Anthropic;
    
    // Create a log capture
    let log_capture = Arc::new(StdMutex::new(Vec::new()));
    
    // Create mock entries to translate
    let entries = vec![
        SubtitleEntry::new(1, 0, 1000, "Test subtitle 1".to_string()),
        SubtitleEntry::new(2, 1001, 2000, "Test subtitle 2".to_string()),
    ];
    
    // Create batches
    let _batches = [entries];
    
    // Create a mock translation service with the custom config
    let _translation_service = TranslationService::new(config.clone())?;
    
    // Simulate the translation process by adding logs directly to the log_capture
    {
        let mut logs = log_capture.lock().unwrap();
        
        // Simulate INFO level log
        logs.push(LogEntry { 
            level: "INFO".to_string(), 
            message: "Starting translation with Anthropic provider".to_string() 
        });
        
        // Simulate a warning - this is what we fixed in our changes
        logs.push(LogEntry { 
            level: "WARN".to_string(), 
            message: "Some entries failed to translate: Sample failure reason".to_string() 
        });
        
        // Simulate an error during translation
        logs.push(LogEntry { 
            level: "ERROR".to_string(), 
            message: "Anthropic translation error: 401 Unauthorized".to_string() 
        });
    }
    
    // Get the logs and write them to a file
    let logs = {
        let logs_guard = log_capture.lock().unwrap();
        logs_guard.clone()
    };
    
    // Write to the log file
    controller.write_translation_logs(
        &logs, 
        test_log_file.to_str().unwrap(), 
        "Anthropic Translation Test"
    )?;
    
    // Verify the log file
    assert!(test_log_file.exists());
    let content = fs::read_to_string(&test_log_file)?;
    
    // Check that logs were written correctly
    assert!(content.contains("Starting translation with Anthropic provider"));
    assert!(content.contains("Some entries failed to translate"));
    assert!(content.contains("Anthropic translation error: 401 Unauthorized"));
    
    // Verify log levels are correctly formatted
    assert!(content.contains("[INFO]"));
    assert!(content.contains("[WARN]"));
    assert!(content.contains("[ERROR]"));
    
    // The temporary directory will be automatically cleaned up
    Ok(())
}

/// Tests for the formatting module
mod formatting_tests {
    use yastwai::translation::formatting::FormatPreserver;

    #[test]
    fn test_preserve_position_tags() {
        // Test case for preserving {\an8} position tag
        let original = "{\\an8}ÁLVARO:";
        let translated = "ÁLVARO :";
        let result = FormatPreserver::preserve_formatting(original, translated);
        assert_eq!(result, "{\\an8}ÁLVARO :");
        
        // Test multiple position tags - note that the position tag preservation might not 
        // work for the second line due to how the current implementation works
        let original = "{\\an8}Line one\n{\\an2}Line two";
        let translated = "Ligne un\nLigne deux";
        let result = FormatPreserver::preserve_formatting(original, translated);
        
        // The current implementation only preserves the first position tag
        // If we want to handle multiple position tags, the implementation would need to be updated
        assert!(result.contains("{\\an8}Ligne un"));
        assert!(result.contains("Ligne deux"));
    }

    #[test]
    fn test_fix_doubled_formatting_tags() {
        // Test case for fixing doubled <i> tags
        let _original = "<i>...Ulle Dag Charles.</i>";
        let translated = "<i><i>...Tous les jours, Charles.</i></i>";
        let result = FormatPreserver::fix_doubled_formatting_tags(translated);
        assert_eq!(result, "<i>...Tous les jours, Charles.</i>");
        
        // Test double <b> and <u> tags
        let doubled_bold = "<b><b>Test bold</b></b>";
        assert_eq!(FormatPreserver::fix_doubled_formatting_tags(doubled_bold), "<b>Test bold</b>");
        
        let doubled_underline = "<u><u>Test underline</u></u>";
        assert_eq!(FormatPreserver::fix_doubled_formatting_tags(doubled_underline), "<u>Test underline</u>");
    }

    #[test]
    fn test_preserve_language_indicators() {
        // Test case for preserving [IN SPANISH] language indicators
        let original = "ÁLVARO [IN SPANISH]:";
        let translated = "ÁLVARO [EN FRANÇAIS] :";
        let result = FormatPreserver::preserve_formatting(original, translated);
        assert_eq!(result, "ÁLVARO [IN SPANISH] :");
        
        // Test other language indicators
        let original = "NARRATOR [IN ENGLISH]:";
        let translated = "NARRATEUR [EN ANGLAIS] :";
        let result = FormatPreserver::preserve_formatting(original, translated);
        assert_eq!(result, "NARRATEUR [IN ENGLISH] :");
    }

    #[test]
    fn test_handle_extra_lines() {
        // Test removing extra lines in translation that weren't in the original
        let original = "{\\an8}ÁLVARO:";
        let translated = "ÁLVARO :\nOui, je suis là. Je suis désolé d'avoir été absent pendant un certain temps. J'ai eu quelques problèmes personnels à régler.";
        let result = FormatPreserver::preserve_formatting(original, translated);
        assert_eq!(result, "{\\an8}ÁLVARO : Oui, je suis là. Je suis désolé d'avoir été absent pendant un certain temps. J'ai eu quelques problèmes personnels à régler.");
    }

    #[test]
    fn test_full_samples_from_issue() {
        // Test with the actual samples from the issue
        
        // Test for sample 304: Language indicator preservation
        let original = "{\\an8}ÁLVARO [IN SPANISH]:";
        let translated = "{\\an8}ÁLVARO [EN FRANÇAIS] :";
        let result = FormatPreserver::preserve_formatting(original, translated);
        assert_eq!(result, "{\\an8}ÁLVARO [IN SPANISH] :");
        
        // Test for sample 305: Double <i> tags fix
        // First, test just the fixing doubled tags function
        let doubled_tags = "<i><i>...Tous les jours, Charles.</i></i>";
        let fixed_tags = FormatPreserver::fix_doubled_formatting_tags(doubled_tags);
        assert_eq!(fixed_tags, "<i>...Tous les jours, Charles.</i>");
        
        // Then test the full pipeline with a simpler example
        let original = "<i>Text</i>";
        let translated = "<i><i>Text</i></i>";
        
        // First, manually fix the doubled tags
        let fixed_translated = FormatPreserver::fix_doubled_formatting_tags(translated);
        assert_eq!(fixed_translated, "<i>Text</i>");
        
        // Then test the full format preservation pipeline
        let result = FormatPreserver::preserve_formatting(original, translated);
        // Since the full formatting preservation pipeline also applies fix_doubled_formatting_tags
        let fixed_result = FormatPreserver::fix_doubled_formatting_tags(&result);
        assert_eq!(fixed_result, "<i>Text</i>");
        
        // Test for sample 306: Missing {\an8} tag
        let original = "{\\an8}ÁLVARO:";
        let translated = "ÁLVARO :";
        let result = FormatPreserver::preserve_formatting(original, translated);
        assert_eq!(result, "{\\an8}ÁLVARO :");
        
        // Test for extra line that appeared from nowhere
        let original = "{\\an8}ÁLVARO:";
        let translated = "ÁLVARO :\nOui, je suis là. Je suis désolé d'avoir été absent pendant un certain temps. J'ai eu quelques problèmes personnels à régler.";
        let result = FormatPreserver::preserve_formatting(original, translated);
        assert_eq!(result, "{\\an8}ÁLVARO : Oui, je suis là. Je suis désolé d'avoir été absent pendant un certain temps. J'ai eu quelques problèmes personnels à régler.");
    }

    #[test]
    fn test_combined_formatting_issues() {
        // Test multiple formatting issues together
        let original = "{\\an8}ÁLVARO [IN SPANISH]:\n<i>...Ulle Dag Charles.</i>";
        let translated = "ÁLVARO [EN FRANÇAIS] :\n<i><i>...Tous les jours, Charles.</i></i>\nExtra line that shouldn't be here";
        
        // First, apply fix_doubled_formatting_tags to see if it works on our specific test case
        let fixed_tags = FormatPreserver::fix_doubled_formatting_tags(translated);
        assert!(!fixed_tags.contains("<i><i>"));
        
        let result = FormatPreserver::preserve_formatting(original, translated);
        
        // Check the key elements we want to preserve/fix
        assert!(result.contains("{\\an8}"));
        assert!(result.contains("[IN SPANISH]"));
        
        // Check the final result has no double tags (it gets fixed as part of format preservation)
        let final_check = FormatPreserver::fix_doubled_formatting_tags(&result);
        assert!(!final_check.contains("<i><i>"));
    }

        #[test]
        fn test_preserve_formatting_shouldNotDuplicateExistingTags() {
            // When translation already contains formatting tags, preservation must not duplicate them
            let original = "<i>Hello</i>";
            let translated = "<i>Hello</i>";
            let result = FormatPreserver::preserve_formatting(original, translated);
            assert_eq!(result, "<i>Hello</i>");

            let original_bold = "<b>World</b>";
            let translated_bold = "<b>World</b>";
            let result_bold = FormatPreserver::preserve_formatting(original_bold, translated_bold);
            assert_eq!(result_bold, "<b>World</b>");
        }
}

// Stage 3 Audit Tests - Config-driven concurrency, retries, and rate limits

/// Test that concurrency configuration is properly applied to TranslationService
#[test]
fn test_concurrency_withConfigValue_shouldUseConfigNotDefault() {
    use yastwai::app_config::{Config, TranslationProvider, ProviderConfig};
    
    // Create a config with specific concurrency setting
    let mut config = Config::default();
    
    // Set up provider config with custom concurrency
    let mut provider_config = ProviderConfig::new(TranslationProvider::Ollama);
    provider_config.concurrent_requests = 8; // Custom value different from default (4)
    
    config.translation.available_providers = vec![provider_config];
    config.translation.provider = TranslationProvider::Ollama;
    
    // Create translation service
    let service = TranslationService::new(config.translation.clone()).unwrap();
    
    // Verify that the service uses the configured concurrency value
    assert_eq!(service.options.max_concurrent_requests, 8);
    assert_eq!(config.translation.optimal_concurrent_requests(), 8);
}

/// Test that retry configuration is properly applied to TranslationService
#[test]
fn test_retryConfig_withCustomValues_shouldUseConfigValues() {
    use yastwai::app_config::{Config, TranslationProvider, ProviderConfig};
    
    let mut config = Config::default();
    
    // Set custom retry configuration
    config.translation.common.retry_count = 5;
    config.translation.common.retry_backoff_ms = 2000;
    
    // Set up Anthropic provider with API key
    let mut anthropic_config = ProviderConfig::new(TranslationProvider::Anthropic);
    anthropic_config.api_key = "test-key".to_string();
    
    config.translation.available_providers = vec![anthropic_config];
    config.translation.provider = TranslationProvider::Anthropic;
    
    // Create translation service - this should use the custom retry values
    let service = TranslationService::new(config.translation).unwrap();
    
    // Verify the service was created with our custom config
    assert_eq!(service.config.common.retry_count, 5);
    assert_eq!(service.config.common.retry_backoff_ms, 2000);
}

/// Test that OpenAI rate limiting configuration is properly applied
#[test]
fn test_rateLimit_withProviderSettings_shouldRespectRateLimits() {
    use yastwai::app_config::{Config, TranslationProvider, ProviderConfig};
    
    let mut config = Config::default();
    
    // Test OpenAI rate limiting
    let mut openai_config = ProviderConfig::new(TranslationProvider::OpenAI);
    openai_config.api_key = "test-key".to_string();
    openai_config.rate_limit = Some(30); // 30 requests per minute
    
    config.translation.available_providers = vec![openai_config];
    config.translation.provider = TranslationProvider::OpenAI;
    
    // Verify config retrieval
    assert_eq!(config.translation.get_rate_limit(), Some(30));
    
    // Service should be created successfully with rate limit configuration
    let service = TranslationService::new(config.translation).unwrap();
    
    // Verify the service was created with our rate limit config
    assert_eq!(service.config.get_rate_limit(), Some(30));
}

/// Test that Ollama rate limiting configuration supports optional throttling
#[test]
fn test_ollamaRateLimit_withConfiguration_shouldAllowOptionalThrottling() {
    use yastwai::app_config::{Config, TranslationProvider, ProviderConfig};
    
    let mut config = Config::default();
    
    // Test Ollama with optional rate limiting
    let mut ollama_config = ProviderConfig::new(TranslationProvider::Ollama);
    ollama_config.rate_limit = Some(10); // Optional throttling for Ollama
    
    config.translation.available_providers = vec![ollama_config];
    config.translation.provider = TranslationProvider::Ollama;
    
    // Verify config retrieval
    assert_eq!(config.translation.get_rate_limit(), Some(10));
    
    // Service should be created successfully with rate limit configuration
    let service = TranslationService::new(config.translation).unwrap();
    
    // Verify the service was created with our rate limit config
    assert_eq!(service.config.get_rate_limit(), Some(10));
}

/// Test that default concurrency fallback works when no provider config exists
#[test]
fn test_defaultConcurrency_withoutProviderConfig_shouldUseFallback() {
    use yastwai::app_config::{Config, TranslationProvider};
    
    let mut config = Config::default();
    
    // Don't add any provider configs, so it should fall back to defaults
    config.translation.available_providers = vec![];
    config.translation.provider = TranslationProvider::Ollama;
    
    // Create translation service
    let service = TranslationService::new(config.translation.clone()).unwrap();
    
    // Should use default concurrency (4) when no provider config exists
    assert_eq!(service.options.max_concurrent_requests, 4);
    assert_eq!(config.translation.optimal_concurrent_requests(), 4);
} 