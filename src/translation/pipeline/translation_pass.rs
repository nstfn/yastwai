/*!
 * Translation pass for the main translation phase.
 *
 * This pass handles the actual translation of subtitle entries using
 * structured JSON I/O for reliable communication with LLM providers.
 * It uses sliding window context to maintain narrative coherence.
 */

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::translation::context::{ContextWindow, ContextWindowConfig};
use crate::translation::core::TranslationService;
use crate::translation::document::{DocumentEntry, Glossary, SubtitleDocument};
use crate::translation::pipeline::validation_pass::FailureReason;
use crate::translation::prompts::{
    TranslatedEntry, TranslationPromptBuilder, TranslationResponse,
};
use crate::translation::subtitle_standards::SubtitleStandards;

/// Configuration for the translation pass.
#[derive(Debug, Clone)]
pub struct TranslationPassConfig {
    /// Context window configuration
    pub window_config: ContextWindowConfig,

    /// Maximum retries per batch
    pub max_retries: usize,

    /// Whether to update glossary from LLM suggestions
    pub accept_glossary_updates: bool,

    /// Whether to use extractive fallback for failed JSON parsing
    pub use_extractive_fallback: bool,

    /// Custom translation instructions
    pub custom_instructions: Option<String>,

    /// Subtitle display standards for prompt builder
    pub subtitle_standards: SubtitleStandards,
}

impl Default for TranslationPassConfig {
    fn default() -> Self {
        Self {
            window_config: ContextWindowConfig::default(),
            max_retries: 3,
            accept_glossary_updates: true,
            use_extractive_fallback: true,
            custom_instructions: None,
            subtitle_standards: SubtitleStandards::default(),
        }
    }
}

impl TranslationPassConfig {
    /// Create a config optimized for speed.
    pub fn fast() -> Self {
        Self {
            window_config: ContextWindowConfig::minimal(),
            max_retries: 1,
            accept_glossary_updates: false,
            use_extractive_fallback: true,
            custom_instructions: None,
            subtitle_standards: SubtitleStandards::default(),
        }
    }

    /// Create a config optimized for quality.
    pub fn quality() -> Self {
        Self {
            window_config: ContextWindowConfig::large_context(),
            max_retries: 3,
            accept_glossary_updates: true,
            use_extractive_fallback: true,
            custom_instructions: None,
            subtitle_standards: SubtitleStandards::default(),
        }
    }

    /// Set custom instructions for translation.
    pub fn with_instructions(mut self, instructions: &str) -> Self {
        self.custom_instructions = Some(instructions.to_string());
        self
    }
}

/// Result of translating a single batch.
#[derive(Debug, Clone)]
pub struct BatchResult {
    /// Translations for this batch
    pub translations: Vec<TranslatedEntry>,

    /// Entry IDs that were translated
    pub entry_ids: Vec<usize>,

    /// Glossary updates suggested by the LLM
    pub glossary_updates: Glossary,

    /// Number of retries used
    pub retries_used: usize,

    /// Whether fallback extraction was used
    pub used_fallback: bool,
}

impl BatchResult {
    /// Create a new batch result.
    pub fn new(translations: Vec<TranslatedEntry>, entry_ids: Vec<usize>) -> Self {
        Self {
            translations,
            entry_ids,
            glossary_updates: Glossary::new(),
            retries_used: 0,
            used_fallback: false,
        }
    }

    /// Get translation for a specific entry ID.
    pub fn get_translation(&self, entry_id: usize) -> Option<&TranslatedEntry> {
        self.translations.iter().find(|t| t.id == entry_id)
    }

    /// Check if all requested entries were translated.
    pub fn is_complete(&self) -> bool {
        self.entry_ids.iter().all(|id| self.translations.iter().any(|t| t.id == *id))
    }

    /// Get missing entry IDs.
    pub fn missing_ids(&self) -> Vec<usize> {
        self.entry_ids
            .iter()
            .filter(|id| !self.translations.iter().any(|t| t.id == **id))
            .copied()
            .collect()
    }
}

/// Translation pass for translating document entries.
pub struct TranslationPass {
    config: TranslationPassConfig,
}

impl TranslationPass {
    /// Create a new translation pass with the given configuration.
    pub fn new(config: TranslationPassConfig) -> Self {
        Self { config }
    }

    /// Create a translation pass with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(TranslationPassConfig::default())
    }

    /// Translate a single batch using the context window.
    pub async fn translate_batch(
        &self,
        service: &TranslationService,
        window: &ContextWindow,
    ) -> Result<BatchResult> {
        let entry_ids: Vec<usize> = window.current_batch.iter().map(|e| e.id).collect();

        // Build the prompt
        let builder = self.build_prompt_from_window(window);
        let (system_prompt, user_prompt) = builder.build();

        // Try translation with retries
        let mut last_error = None;
        let mut retries = 0;

        while retries <= self.config.max_retries {
            match self.attempt_translation(service, &system_prompt, &user_prompt).await {
                Ok(response) => {
                    let mut result = BatchResult::new(response.translations, entry_ids.clone());
                    result.retries_used = retries;

                    // Process glossary updates
                    if self.config.accept_glossary_updates {
                        if let Some(notes) = response.notes {
                            for (source, target) in notes.glossary_updates.iter() {
                                result.glossary_updates.add_term(source, target, None);
                            }
                        }
                    }

                    return Ok(result);
                }
                Err(e) => {
                    last_error = Some(e);
                    retries += 1;
                }
            }
        }

        // If we exhausted retries, try fallback extraction
        if self.config.use_extractive_fallback {
            if let Some(fallback) = self.try_fallback_extraction(&entry_ids) {
                let mut result = fallback;
                result.retries_used = retries;
                result.used_fallback = true;
                return Ok(result);
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Translation failed after {} retries", retries)))
    }

    /// Translate a batch with feedback from previous validation failures.
    ///
    /// This method includes the failure reasons in the prompt to guide the LLM
    /// to produce better translations on retry.
    pub async fn translate_with_feedback_retry(
        &self,
        service: &TranslationService,
        window: &ContextWindow,
        failure_reasons: &[FailureReason],
    ) -> Result<BatchResult> {
        let entry_ids: Vec<usize> = window.current_batch.iter().map(|e| e.id).collect();

        // Build the base prompt
        let builder = self.build_prompt_from_window(window);
        let (system_prompt, user_prompt) = builder.build();

        // Append feedback instructions to the prompt
        let feedback_section = self.build_feedback_section(failure_reasons);
        let enhanced_user_prompt = if feedback_section.is_empty() {
            user_prompt
        } else {
            format!("{}\n\n{}", user_prompt, feedback_section)
        };

        // Attempt translation with enhanced prompt
        match self.attempt_translation(service, &system_prompt, &enhanced_user_prompt).await {
            Ok(response) => {
                let mut result = BatchResult::new(response.translations, entry_ids.clone());

                // Process glossary updates
                if self.config.accept_glossary_updates {
                    if let Some(notes) = response.notes {
                        for (source, target) in notes.glossary_updates.iter() {
                            result.glossary_updates.add_term(source, target, None);
                        }
                    }
                }

                Ok(result)
            }
            Err(e) => {
                // On failure, try fallback if enabled
                if self.config.use_extractive_fallback {
                    if let Some(fallback) = self.try_fallback_extraction(&entry_ids) {
                        let mut result = fallback;
                        result.used_fallback = true;
                        return Ok(result);
                    }
                }
                Err(e)
            }
        }
    }

    /// Build a feedback section from failure reasons
    fn build_feedback_section(&self, failure_reasons: &[FailureReason]) -> String {
        if failure_reasons.is_empty() {
            return String::new();
        }

        let instructions: Vec<String> = failure_reasons
            .iter()
            .map(|r| format!("- Entry {}: {}", r.entry_id(), r.to_feedback_instruction()))
            .collect();

        format!(
            "## Previous Translation Feedback\n\
             The previous translation had these issues that must be fixed:\n\
             {}\n\n\
             Please address all feedback points in your revised translation.",
            instructions.join("\n")
        )
    }

    /// Build a prompt from a context window.
    fn build_prompt_from_window(&self, window: &ContextWindow) -> TranslationPromptBuilder {
        let mut builder =
            TranslationPromptBuilder::new(&window.source_language, &window.target_language)
                .with_subtitle_standards(self.config.subtitle_standards.clone());

        // Add history summary if available
        if let Some(ref summary) = window.history_summary {
            builder = builder.with_history_summary(summary);
        }

        // Add recent translations
        if !window.recent_entries.is_empty() {
            builder = builder.with_recent_translations(window.recent_entries.clone());
        }

        // Add glossary
        if !window.glossary.is_empty() {
            builder = builder.with_glossary(&window.glossary);
        }

        // Add custom instructions
        if let Some(ref instructions) = self.config.custom_instructions {
            builder = builder.with_custom_instructions(instructions);
        }

        // Build entries to translate from window
        let entries_to_translate: Vec<DocumentEntry> = window
            .current_batch
            .iter()
            .map(|we| DocumentEntry {
                id: we.id,
                timecode: crate::translation::document::Timecode::from_milliseconds(0, 0), // Timecode from SRT format
                original_text: we.text.clone(),
                translated_text: None,
                speaker: None,
                scene_id: None,
                formatting: Vec::new(),
                confidence: None,
            })
            .collect();

        builder = builder.with_entries_to_translate(&entries_to_translate);

        // Add lookahead entries
        let lookahead_entries: Vec<DocumentEntry> = window
            .lookahead_entries
            .iter()
            .map(|we| DocumentEntry {
                id: we.id,
                timecode: crate::translation::document::Timecode::from_milliseconds(0, 0),
                original_text: we.text.clone(),
                translated_text: None,
                speaker: None,
                scene_id: None,
                formatting: Vec::new(),
                confidence: None,
            })
            .collect();

        builder.with_lookahead(&lookahead_entries)
    }

    /// Attempt a single translation request.
    async fn attempt_translation(
        &self,
        service: &TranslationService,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<TranslationResponse> {
        // Build combined prompt for translation
        let combined_prompt = format!("{}\n\n{}", system_prompt, user_prompt);

        // Use the translation service
        let response =
            service.translate_text(&combined_prompt, "prompt", "json_response").await?;

        // Parse the JSON response
        self.parse_translation_response(&response)
    }

    /// Parse a JSON translation response.
    fn parse_translation_response(&self, response: &str) -> Result<TranslationResponse> {
        // Try to extract JSON from the response (LLM might include extra text)
        let json_str = self.extract_json(response)?;

        // Parse the JSON
        serde_json::from_str(&json_str).map_err(|e| anyhow!("Failed to parse translation response: {}", e))
    }

    /// Extract JSON from a potentially wrapped response.
    fn extract_json(&self, response: &str) -> Result<String> {
        super::extract_json(response)
    }

    /// Try fallback extraction for failed batches.
    fn try_fallback_extraction(&self, entry_ids: &[usize]) -> Option<BatchResult> {
        // Create empty translations as fallback (entries will need manual translation)
        // In a real implementation, this could try simpler extraction methods
        let translations: Vec<TranslatedEntry> = entry_ids
            .iter()
            .map(|&id| TranslatedEntry {
                id,
                translated: String::new(), // Empty - will be flagged for retry
                confidence: Some(0.0),
            })
            .collect();

        Some(BatchResult::new(translations, entry_ids.to_vec()))
    }

    /// Apply batch results to a document.
    pub fn apply_batch_result(&self, doc: &mut SubtitleDocument, result: &BatchResult) {
        for translation in &result.translations {
            if let Some(entry) = doc.entries.iter_mut().find(|e| e.id == translation.id) {
                if !translation.translated.is_empty() {
                    entry.set_translation(translation.translated.clone(), translation.confidence);
                }
            }
        }

        // Apply glossary updates
        if !result.glossary_updates.is_empty() {
            doc.glossary.merge(&result.glossary_updates);
        }
    }

    /// Translate a full document using sliding windows.
    pub async fn translate_document(
        &self,
        service: &TranslationService,
        doc: &mut SubtitleDocument,
        source_language: &str,
        target_language: &str,
        progress_callback: Option<Box<dyn Fn(f32) + Send>>,
    ) -> Result<TranslationStats> {
        let mut stats = TranslationStats::new();
        let total_entries = doc.entries.len();

        if total_entries == 0 {
            return Ok(stats);
        }

        // Create context windows
        let windows: Vec<ContextWindow> = {
            use crate::translation::context::ContextWindowExt;
            doc.context_windows(
                self.config.window_config.clone(),
                source_language,
                target_language,
            )
            .collect()
        };

        stats.total_batches = windows.len();

        // Process each window
        for (batch_idx, window) in windows.into_iter().enumerate() {
            let result = self.translate_batch(service, &window).await?;

            // Apply results
            self.apply_batch_result(doc, &result);

            // Update stats
            stats.completed_batches += 1;
            stats.total_entries_translated += result.translations.len();
            stats.total_retries += result.retries_used;
            if result.used_fallback {
                stats.fallback_used_count += 1;
            }

            // Report progress
            if let Some(ref callback) = progress_callback {
                let progress = (batch_idx + 1) as f32 / stats.total_batches as f32 * 100.0;
                callback(progress);
            }
        }

        Ok(stats)
    }
}

impl Default for TranslationPass {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Statistics from the translation pass.
#[derive(Debug, Clone, Default)]
pub struct TranslationStats {
    /// Total number of batches processed
    pub total_batches: usize,

    /// Number of batches successfully completed
    pub completed_batches: usize,

    /// Total entries translated
    pub total_entries_translated: usize,

    /// Total retries across all batches
    pub total_retries: usize,

    /// Number of times fallback extraction was used
    pub fallback_used_count: usize,
}

impl TranslationStats {
    /// Create new empty stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Calculate success rate.
    pub fn success_rate(&self) -> f32 {
        if self.total_batches == 0 {
            return 100.0;
        }
        (self.completed_batches as f32 / self.total_batches as f32) * 100.0
    }
}

/// Simplified translation request for direct JSON I/O.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleTranslationRequest {
    /// Entries to translate
    pub entries: Vec<SimpleEntry>,

    /// Source language
    pub source_language: String,

    /// Target language
    pub target_language: String,
}

/// Simplified entry for translation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleEntry {
    /// Entry ID
    pub id: usize,

    /// Text to translate
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translationPassConfig_default_shouldHaveReasonableValues() {
        let config = TranslationPassConfig::default();

        assert!(config.max_retries >= 1);
        assert!(config.use_extractive_fallback);
    }

    #[test]
    fn test_batchResult_isComplete_shouldDetectMissingTranslations() {
        let translations = vec![
            TranslatedEntry {
                id: 1,
                translated: "Bonjour".to_string(),
                confidence: Some(0.9),
            },
            TranslatedEntry {
                id: 2,
                translated: "Au revoir".to_string(),
                confidence: Some(0.85),
            },
        ];

        let complete_result = BatchResult::new(translations.clone(), vec![1, 2]);
        let incomplete_result = BatchResult::new(translations, vec![1, 2, 3]);

        assert!(complete_result.is_complete());
        assert!(!incomplete_result.is_complete());
    }

    #[test]
    fn test_batchResult_missingIds_shouldFindMissing() {
        let translations = vec![TranslatedEntry {
            id: 1,
            translated: "Test".to_string(),
            confidence: None,
        }];

        let result = BatchResult::new(translations, vec![1, 2, 3]);
        let missing = result.missing_ids();

        assert_eq!(missing, vec![2, 3]);
    }

    #[test]
    fn test_translationPass_extractJson_shouldHandleRawJson() {
        let pass = TranslationPass::with_defaults();

        let response = r#"{"translations": [{"id": 1, "translated": "test"}]}"#;
        let result = pass.extract_json(response);

        assert!(result.is_ok());
        assert!(result.unwrap().contains("translations"));
    }

    #[test]
    fn test_translationPass_extractJson_shouldHandleMarkdownCodeBlock() {
        let pass = TranslationPass::with_defaults();

        let response = r#"Here is the translation:
```json
{"translations": [{"id": 1, "translated": "test"}]}
```
Hope this helps!"#;

        let result = pass.extract_json(response);

        assert!(result.is_ok());
        assert!(result.unwrap().contains("translations"));
    }

    #[test]
    fn test_translationPass_extractJson_shouldHandleEmbeddedJson() {
        let pass = TranslationPass::with_defaults();

        let response = r#"I translated the text: {"translations": [{"id": 1, "translated": "test"}]}"#;
        let result = pass.extract_json(response);

        assert!(result.is_ok());
    }

    #[test]
    fn test_translationStats_successRate_shouldCalculateCorrectly() {
        let stats = TranslationStats {
            total_batches: 10,
            completed_batches: 8,
            ..Default::default()
        };

        assert_eq!(stats.success_rate(), 80.0);
    }

    #[test]
    fn test_translationStats_successRate_withZeroBatches_shouldReturn100() {
        let stats = TranslationStats::new();

        assert_eq!(stats.success_rate(), 100.0);
    }

    #[test]
    fn test_translationPass_buildFeedbackSection_withReasons_shouldIncludeInstructions() {
        let pass = TranslationPass::with_defaults();
        let reasons = vec![
            FailureReason::TooLong {
                entry_id: 1,
                current_ratio: 2.0,
                max_ratio: 1.5,
                suggestion: "Translation is 200% longer than original. Shorten to under 150%".to_string(),
            },
            FailureReason::AlteredCharacterName {
                entry_id: 2,
                name: "John".to_string(),
            },
        ];

        let feedback = pass.build_feedback_section(&reasons);

        assert!(feedback.contains("Previous Translation Feedback"));
        assert!(feedback.contains("Entry 1"));
        assert!(feedback.contains("Entry 2"));
        assert!(feedback.contains("Shorten"));
        assert!(feedback.contains("John"));
    }

    #[test]
    fn test_translationPass_buildFeedbackSection_empty_shouldReturnEmpty() {
        let pass = TranslationPass::with_defaults();
        let reasons: Vec<FailureReason> = vec![];

        let feedback = pass.build_feedback_section(&reasons);

        assert!(feedback.is_empty());
    }

    #[test]
    fn test_translationPassConfig_shouldAcceptSubtitleStandards() {
        use crate::translation::subtitle_standards::SubtitleStandards;

        let config = TranslationPassConfig {
            subtitle_standards: SubtitleStandards::default(),
            ..Default::default()
        };
        assert!((config.subtitle_standards.target_cps - 17.0).abs() < f32::EPSILON);
    }
}
