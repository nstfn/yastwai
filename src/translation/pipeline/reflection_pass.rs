/*!
 * Reflection pass for translation quality improvement.
 *
 * Implements a two-step reflect-then-improve workflow inspired by
 * Andrew Ng's translation-agent and the MQM error typology.
 * After the initial translation, this pass:
 * 1. Asks the LLM to critique the draft across 5 dimensions
 * 2. If issues are found, asks the LLM to apply the suggestions
 */

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::translation::core::TranslationService;
use crate::translation::document::Glossary;
use crate::translation::prompts::{GlossaryContext, TranslatedEntry};
use crate::translation::subtitle_standards::SubtitleStandards;

/// Configuration for the reflection pass.
#[derive(Debug, Clone)]
pub struct ReflectionConfig {
    /// Minimum severity level to apply suggestions. Default: Minor (apply all).
    pub min_severity_to_apply: Severity,

    /// Maximum reflection iterations. Default: 1 (reflect once).
    pub max_reflection_retries: usize,

    /// Skip reflection if all entries have confidence above threshold.
    pub skip_if_high_confidence: bool,

    /// Confidence threshold for skipping. Default: 0.95.
    pub confidence_skip_threshold: f32,
}

impl Default for ReflectionConfig {
    fn default() -> Self {
        Self {
            min_severity_to_apply: Severity::Minor,
            max_reflection_retries: 1,
            skip_if_high_confidence: false,
            confidence_skip_threshold: 0.95,
        }
    }
}

/// Severity levels for reflection suggestions, mapped to MQM framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Minor = 0,
    Major = 1,
    Critical = 2,
}

/// A single suggestion from the reflection step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionSuggestion {
    pub entry_id: usize,
    pub dimension: String,
    pub severity: Severity,
    pub current: String,
    pub suggested: String,
    pub reason: String,
}

/// Response from the reflection LLM call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionResponse {
    pub suggestions: Vec<ReflectionSuggestion>,
    #[serde(default)]
    pub entries_approved: Vec<usize>,
}

impl ReflectionResponse {
    pub fn has_suggestions(&self) -> bool {
        !self.suggestions.is_empty()
    }
}

/// A source-draft pair for the reflection prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDraftPair {
    pub id: usize,
    pub source: String,
    pub draft: String,
    pub duration_seconds: f32,
}

/// Request structure sent to the reflection LLM call.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReflectionRequest {
    entries: Vec<SourceDraftPair>,
    #[serde(skip_serializing_if = "Option::is_none")]
    glossary: Option<GlossaryContext>,
}

/// Request structure sent to the improvement LLM call.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ImprovementRequest {
    entries: Vec<SourceDraftPair>,
    suggestions: Vec<ReflectionSuggestion>,
}

/// The reflection pass.
pub struct ReflectionPass {
    config: ReflectionConfig,
}

impl ReflectionPass {
    pub fn new(config: ReflectionConfig) -> Self {
        Self { config }
    }

    /// Filter suggestions by the configured minimum severity threshold.
    pub fn filter_by_severity(&self, suggestions: Vec<ReflectionSuggestion>) -> Vec<ReflectionSuggestion> {
        suggestions
            .into_iter()
            .filter(|s| s.severity >= self.config.min_severity_to_apply)
            .collect()
    }

    /// Maximum number of reflection retries from config.
    pub fn max_retries(&self) -> usize {
        self.config.max_reflection_retries
    }

    /// Check if reflection should be skipped for this batch based on confidence.
    pub fn should_skip(&self, translations: &[TranslatedEntry]) -> bool {
        if !self.config.skip_if_high_confidence {
            return false;
        }
        translations.iter().all(|t| {
            t.confidence
                .map(|c| c >= self.config.confidence_skip_threshold)
                .unwrap_or(false)
        })
    }

    /// Run the reflection step: critique draft translations.
    pub async fn reflect(
        &self,
        service: &TranslationService,
        source_draft_pairs: &[SourceDraftPair],
        glossary: &Option<Glossary>,
        source_language: &str,
        target_language: &str,
        subtitle_standards: &SubtitleStandards,
    ) -> Result<ReflectionResponse> {
        let system_prompt = self.build_reflection_system_prompt(
            source_language,
            target_language,
            subtitle_standards.target_cps,
        );
        let user_prompt = self.build_reflection_user_prompt(source_draft_pairs, glossary);

        let combined = format!("{}\n\n{}", system_prompt, user_prompt);
        let response = service
            .translate_text(&combined, "reflection", "json_response")
            .await?;

        self.parse_reflection_response(&response)
    }

    /// Run the improvement step: apply suggestions to produce final translations.
    pub async fn improve(
        &self,
        service: &TranslationService,
        source_draft_pairs: &[SourceDraftPair],
        suggestions: &[ReflectionSuggestion],
        source_language: &str,
        target_language: &str,
        subtitle_standards: &SubtitleStandards,
    ) -> Result<Vec<TranslatedEntry>> {
        let system_prompt =
            self.build_improvement_system_prompt(source_language, target_language);
        let user_prompt =
            self.build_improvement_user_prompt(source_draft_pairs, suggestions, subtitle_standards);

        let combined = format!("{}\n\n{}", system_prompt, user_prompt);
        let response = service
            .translate_text(&combined, "improvement", "json_response")
            .await?;

        self.parse_improvement_response(&response)
    }

    pub fn build_reflection_system_prompt(
        &self,
        source_language: &str,
        target_language: &str,
        target_cps: f32,
    ) -> String {
        format!(
            r#"You are a senior subtitle QA reviewer specializing in {source} to {target}. You are reviewing draft translations produced by another translator.

Review each translation against the source text and provide specific, actionable suggestions. Only flag genuine issues — do not suggest changes for translations that are already good.

## Evaluation Dimensions

### 1. Accuracy
- Mistranslation: meaning changed or wrong
- Omission: important meaning lost (deliberate condensation is acceptable)
- Addition: meaning added that is not in the source
- False friends: words that look similar but mean different things

### 2. Fluency
- Does it sound natural in spoken {target}?
- Grammar and spelling errors
- Awkward phrasing that a native speaker would never say

### 3. Style & Register
- Does the formality level match the original?
- Is character voice preserved? (a teenager should not sound formal)
- Is sarcasm/irony preserved or lost?

### 4. Terminology
- Are glossary terms used correctly and consistently?
- Are character names left untranslated?

### 5. Subtitle Fitness
- Reading speed: will viewers have time to read this? (target: {cps:.0} CPS)
- Can the text be condensed without losing meaning?
- Are sound effects and formatting tags preserved?

## Severity Levels
- critical: meaning is wrong or offensive
- major: noticeable quality issue that affects comprehension
- minor: stylistic improvement, optional but recommended

## Output Format
Return ONLY valid JSON:
{{"suggestions": [{{"entry_id": 1, "dimension": "accuracy|fluency|style|terminology|subtitle_fitness", "severity": "critical|major|minor", "current": "the current translation", "suggested": "your improved version", "reason": "brief explanation"}}], "entries_approved": [2, 3]}}

If all translations are good, return {{"suggestions": [], "entries_approved": [all IDs]}}"#,
            source = source_language,
            target = target_language,
            cps = target_cps,
        )
    }

    pub fn build_reflection_user_prompt(
        &self,
        pairs: &[SourceDraftPair],
        glossary: &Option<Glossary>,
    ) -> String {
        let glossary_context = glossary.as_ref().and_then(|g| {
            if g.is_empty() {
                None
            } else {
                Some(GlossaryContext {
                    character_names: g.character_names.iter().cloned().collect(),
                    terms: g
                        .terms
                        .iter()
                        .map(|(k, v)| (k.clone(), v.target.clone()))
                        .collect(),
                })
            }
        });

        let request = ReflectionRequest {
            entries: pairs.to_vec(),
            glossary: glossary_context,
        };

        serde_json::to_string_pretty(&request).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn build_improvement_system_prompt(
        &self,
        source_language: &str,
        target_language: &str,
    ) -> String {
        format!(
            r#"You are a professional subtitle translator editing {source} to {target} translations. Apply the provided review suggestions to improve the draft translations. Only modify entries that have suggestions. For approved entries, return them unchanged.

Return the complete set of translations as JSON:
{{"translations": [{{"id": 1, "translated": "improved text", "confidence": 0.95}}]}}"#,
            source = source_language,
            target = target_language,
        )
    }

    fn build_improvement_user_prompt(
        &self,
        pairs: &[SourceDraftPair],
        suggestions: &[ReflectionSuggestion],
        subtitle_standards: &SubtitleStandards,
    ) -> String {
        let request = ImprovementRequest {
            entries: pairs.to_vec(),
            suggestions: suggestions.to_vec(),
        };

        let json = serde_json::to_string_pretty(&request).unwrap_or_else(|_| "{}".to_string());
        format!(
            "{}\n\nMaintain subtitle constraints: {:.0} CPS, {} characters per line, maximum 2 lines.",
            json,
            subtitle_standards.target_cps,
            subtitle_standards.max_chars_per_line
        )
    }

    fn parse_reflection_response(&self, response: &str) -> Result<ReflectionResponse> {
        let json_str = extract_json(response)?;
        serde_json::from_str(&json_str)
            .map_err(|e| anyhow!("Failed to parse reflection response: {}", e))
    }

    fn parse_improvement_response(&self, response: &str) -> Result<Vec<TranslatedEntry>> {
        let json_str = extract_json(response)?;

        #[derive(Deserialize)]
        struct ImprovementResponse {
            translations: Vec<TranslatedEntry>,
        }

        let parsed: ImprovementResponse = serde_json::from_str(&json_str)
            .map_err(|e| anyhow!("Failed to parse improvement response: {}", e))?;
        Ok(parsed.translations)
    }
}

use super::extract_json;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reflection_config_default() {
        let config = ReflectionConfig::default();
        assert_eq!(config.min_severity_to_apply, Severity::Minor);
        assert_eq!(config.max_reflection_retries, 1);
        assert!(!config.skip_if_high_confidence);
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Critical > Severity::Major);
        assert!(Severity::Major > Severity::Minor);
    }

    #[test]
    fn test_reflection_suggestion_from_json() {
        let json = r#"{
            "suggestions": [
                {
                    "entry_id": 1,
                    "dimension": "accuracy",
                    "severity": "major",
                    "current": "Bonjour monde",
                    "suggested": "Salut le monde",
                    "reason": "Too formal for casual dialogue"
                }
            ],
            "entries_approved": [2, 3]
        }"#;

        let response: ReflectionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.suggestions.len(), 1);
        assert_eq!(response.entries_approved.len(), 2);
        assert_eq!(response.suggestions[0].dimension, "accuracy");
        assert_eq!(response.suggestions[0].severity, Severity::Major);
    }

    #[test]
    fn test_reflection_response_has_suggestions() {
        let response = ReflectionResponse {
            suggestions: vec![ReflectionSuggestion {
                entry_id: 1,
                dimension: "fluency".to_string(),
                severity: Severity::Minor,
                current: "old".to_string(),
                suggested: "new".to_string(),
                reason: "sounds better".to_string(),
            }],
            entries_approved: vec![],
        };
        assert!(response.has_suggestions());
    }

    #[test]
    fn test_reflection_response_no_suggestions() {
        let response = ReflectionResponse {
            suggestions: vec![],
            entries_approved: vec![1, 2, 3],
        };
        assert!(!response.has_suggestions());
    }

    #[test]
    fn test_build_reflection_system_prompt_contains_dimensions() {
        let pass = ReflectionPass::new(ReflectionConfig::default());
        let prompt = pass.build_reflection_system_prompt("English", "French", 17.0);
        assert!(prompt.contains("Accuracy"));
        assert!(prompt.contains("Fluency"));
        assert!(prompt.contains("Style"));
        assert!(prompt.contains("Terminology"));
        assert!(prompt.contains("Subtitle Fitness"));
        assert!(prompt.contains("17"));
    }

    #[test]
    fn test_build_reflection_user_prompt_is_valid_json() {
        let pass = ReflectionPass::new(ReflectionConfig::default());

        let source_entries = vec![SourceDraftPair {
            id: 1,
            source: "Hello".to_string(),
            draft: "Bonjour".to_string(),
            duration_seconds: 2.0,
        }];

        let prompt = pass.build_reflection_user_prompt(&source_entries, &None);
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert!(parsed.get("entries").is_some());
    }

    #[test]
    fn test_build_improvement_system_prompt() {
        let pass = ReflectionPass::new(ReflectionConfig::default());
        let prompt = pass.build_improvement_system_prompt("English", "French");
        assert!(prompt.contains("Apply"));
        assert!(prompt.contains("suggestions"));
    }

    #[test]
    fn test_should_skip_when_disabled() {
        let pass = ReflectionPass::new(ReflectionConfig::default());
        let entries = vec![TranslatedEntry {
            id: 1,
            translated: "test".to_string(),
            confidence: Some(0.99),
        }];
        // skip_if_high_confidence is false by default
        assert!(!pass.should_skip(&entries));
    }

    #[test]
    fn test_should_skip_when_enabled_and_high_confidence() {
        let config = ReflectionConfig {
            skip_if_high_confidence: true,
            confidence_skip_threshold: 0.95,
            ..Default::default()
        };
        let pass = ReflectionPass::new(config);
        let entries = vec![TranslatedEntry {
            id: 1,
            translated: "test".to_string(),
            confidence: Some(0.99),
        }];
        assert!(pass.should_skip(&entries));
    }

    #[test]
    fn test_should_not_skip_when_low_confidence() {
        let config = ReflectionConfig {
            skip_if_high_confidence: true,
            confidence_skip_threshold: 0.95,
            ..Default::default()
        };
        let pass = ReflectionPass::new(config);
        let entries = vec![TranslatedEntry {
            id: 1,
            translated: "test".to_string(),
            confidence: Some(0.5),
        }];
        assert!(!pass.should_skip(&entries));
    }

    #[test]
    fn test_extract_json_raw() {
        let response = r#"{"suggestions": [], "entries_approved": [1]}"#;
        let result = extract_json(response);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_json_markdown_fence() {
        let response = "Here is the result:\n```json\n{\"suggestions\": []}\n```\nDone.";
        let result = extract_json(response);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("suggestions"));
    }

    #[test]
    fn test_extract_json_embedded() {
        let response = "I reviewed the text: {\"suggestions\": []} and found no issues.";
        let result = extract_json(response);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_reflection_response() {
        let pass = ReflectionPass::new(ReflectionConfig::default());
        let json = r#"{"suggestions": [{"entry_id": 1, "dimension": "fluency", "severity": "minor", "current": "a", "suggested": "b", "reason": "c"}], "entries_approved": [2]}"#;
        let result = pass.parse_reflection_response(json);
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.suggestions.len(), 1);
        assert_eq!(response.entries_approved, vec![2]);
    }

    #[test]
    fn test_parse_improvement_response() {
        let pass = ReflectionPass::new(ReflectionConfig::default());
        let json = r#"{"translations": [{"id": 1, "translated": "Bonjour", "confidence": 0.95}]}"#;
        let result = pass.parse_improvement_response(json);
        assert!(result.is_ok());
        let entries = result.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].translated, "Bonjour");
    }
}
