/*!
 * History summarization for long documents.
 *
 * When translating long documents, earlier content is compressed into
 * a summary to maintain context without overwhelming the LLM's context window.
 * This module provides summarization strategies.
 */

use crate::translation::document::{DocumentEntry, SubtitleDocument};

/// Configuration for history summarization.
#[derive(Debug, Clone)]
pub struct SummarizationConfig {
    /// Maximum characters in the summary
    pub max_summary_chars: usize,

    /// How many entries to summarize at a time
    pub entries_per_summary: usize,

    /// Whether to include character names in summaries
    pub include_character_names: bool,

    /// Whether to include key dialogue snippets
    pub include_dialogue_snippets: bool,
}

impl Default for SummarizationConfig {
    fn default() -> Self {
        Self {
            max_summary_chars: 500,
            entries_per_summary: 50,
            include_character_names: true,
            include_dialogue_snippets: true,
        }
    }
}

/// Summary of a portion of the document.
#[derive(Debug, Clone)]
pub struct HistorySummary {
    /// The summary text
    pub text: String,

    /// Start entry ID covered by this summary
    pub start_entry_id: usize,

    /// End entry ID covered by this summary
    pub end_entry_id: usize,

    /// Number of entries summarized
    pub entry_count: usize,
}

impl HistorySummary {
    /// Create a new summary.
    pub fn new(text: String, start_entry_id: usize, end_entry_id: usize, entry_count: usize) -> Self {
        Self {
            text,
            start_entry_id,
            end_entry_id,
            entry_count,
        }
    }
}

/// History summarizer for compressing earlier content.
pub struct HistorySummarizer {
    config: SummarizationConfig,
}

impl HistorySummarizer {
    /// Create a new summarizer with the given configuration.
    pub fn new(config: SummarizationConfig) -> Self {
        Self { config }
    }

    /// Create a summarizer with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(SummarizationConfig::default())
    }

    /// Generate a simple extractive summary from entries.
    ///
    /// This is a fallback method that doesn't require an LLM.
    /// It extracts key information from the entries to create a summary.
    pub fn summarize_extractive(&self, entries: &[DocumentEntry]) -> HistorySummary {
        if entries.is_empty() {
            return HistorySummary::new(String::new(), 0, 0, 0);
        }

        let mut summary_parts = Vec::new();
        let entry_count = entries.len();
        let start_id = entries.first().map(|e| e.id).unwrap_or(0);
        let end_id = entries.last().map(|e| e.id).unwrap_or(0);

        // Extract character names if enabled
        if self.config.include_character_names {
            let names = self.extract_likely_names(entries);
            if !names.is_empty() {
                summary_parts.push(format!("Characters: {}", names.join(", ")));
            }
        }

        // Include key dialogue snippets
        if self.config.include_dialogue_snippets {
            let snippets = self.extract_key_snippets(entries, 3);
            if !snippets.is_empty() {
                summary_parts.push(format!("Key dialogue: {}", snippets.join(" ... ")));
            }
        }

        // Add entry count context
        summary_parts.push(format!("[{} lines of dialogue]", entry_count));

        let text = summary_parts.join(". ");
        let truncated = self.truncate_to_limit(&text);

        HistorySummary::new(truncated, start_id, end_id, entry_count)
    }

    /// Build a prompt for LLM-based summarization.
    ///
    /// Returns a prompt that can be sent to an LLM to generate a summary.
    pub fn build_summarization_prompt(&self, entries: &[DocumentEntry]) -> String {
        let mut content = String::new();

        for entry in entries {
            content.push_str(&entry.original_text);
            content.push('\n');
        }

        format!(
            r#"Summarize the following dialogue in 2-3 sentences, focusing on:
- Main characters and their relationships
- Key plot points or events
- Overall tone and setting

Dialogue:
{}

Summary:"#,
            content
        )
    }

    /// Extract likely character names from entries.
    fn extract_likely_names(&self, entries: &[DocumentEntry]) -> Vec<String> {
        use regex::Regex;
        use std::collections::HashMap;
        use std::sync::LazyLock;

        static NAME_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"\b([A-Z][a-z]+)\b").unwrap()
        });
        let mut name_counts: HashMap<String, usize> = HashMap::new();

        // Common words to exclude
        let exclude = [
            "The", "This", "That", "What", "Where", "When", "Why", "How", "Yes", "No", "Oh", "Hey",
            "Well", "Now", "Here", "Please", "Thank", "Hello", "Sorry", "Just", "Really",
        ];

        for entry in entries {
            for cap in NAME_PATTERN.captures_iter(&entry.original_text) {
                if let Some(name) = cap.get(1) {
                    let name_str = name.as_str();
                    if !exclude.contains(&name_str) {
                        *name_counts.entry(name_str.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }

        // Return names that appear at least twice
        let mut names: Vec<_> = name_counts
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        names.sort_by(|a, b| b.1.cmp(&a.1));
        names.into_iter().take(5).map(|(name, _)| name).collect()
    }

    /// Extract key dialogue snippets.
    fn extract_key_snippets(&self, entries: &[DocumentEntry], count: usize) -> Vec<String> {
        // Simple heuristic: take entries from beginning, middle, and end
        if entries.is_empty() {
            return Vec::new();
        }

        let mut snippets = Vec::new();
        let len = entries.len();

        // Get evenly spaced entries
        let indices = if len <= count {
            (0..len).collect::<Vec<_>>()
        } else {
            let step = len / count;
            (0..count).map(|i| i * step).collect()
        };

        for idx in indices {
            if idx < len {
                let text = &entries[idx].original_text;
                // Truncate long snippets
                let snippet = if text.len() > 50 {
                    format!("{}...", crate::utils::truncate_utf8(text, 47))
                } else {
                    text.clone()
                };
                snippets.push(snippet);
            }
        }

        snippets
    }

    /// Truncate text to the configured maximum length.
    fn truncate_to_limit(&self, text: &str) -> String {
        if text.len() <= self.config.max_summary_chars {
            return text.to_string();
        }

        let truncated = crate::utils::truncate_utf8(text, self.config.max_summary_chars.saturating_sub(3));
        // Find last complete sentence or word
        if let Some(last_period) = truncated.rfind(". ") {
            format!("{}.", &truncated[..last_period])
        } else if let Some(last_space) = truncated.rfind(' ') {
            format!("{}...", &truncated[..last_space])
        } else {
            format!("{truncated}...")
        }
    }

    /// Combine multiple summaries into one.
    pub fn combine_summaries(&self, summaries: &[HistorySummary]) -> HistorySummary {
        if summaries.is_empty() {
            return HistorySummary::new(String::new(), 0, 0, 0);
        }

        let combined_text = summaries
            .iter()
            .map(|s| s.text.clone())
            .collect::<Vec<_>>()
            .join(" ");

        let start_id = summaries.first().map(|s| s.start_entry_id).unwrap_or(0);
        let end_id = summaries.last().map(|s| s.end_entry_id).unwrap_or(0);
        let total_entries: usize = summaries.iter().map(|s| s.entry_count).sum();

        let truncated = self.truncate_to_limit(&combined_text);

        HistorySummary::new(truncated, start_id, end_id, total_entries)
    }
}

/// Extension trait for SubtitleDocument to add summarization.
pub trait SummarizationExt {
    /// Generate an extractive summary of entries up to the given position.
    fn summarize_history(&self, up_to_position: usize) -> HistorySummary;

    /// Generate an extractive summary with custom configuration.
    fn summarize_history_with_config(
        &self,
        up_to_position: usize,
        config: SummarizationConfig,
    ) -> HistorySummary;
}

impl SummarizationExt for SubtitleDocument {
    fn summarize_history(&self, up_to_position: usize) -> HistorySummary {
        let summarizer = HistorySummarizer::with_defaults();
        let entries = &self.entries[..up_to_position.min(self.entries.len())];
        summarizer.summarize_extractive(entries)
    }

    fn summarize_history_with_config(
        &self,
        up_to_position: usize,
        config: SummarizationConfig,
    ) -> HistorySummary {
        let summarizer = HistorySummarizer::new(config);
        let entries = &self.entries[..up_to_position.min(self.entries.len())];
        summarizer.summarize_extractive(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitle_processor::SubtitleEntry;

    fn create_entries(texts: &[&str]) -> Vec<DocumentEntry> {
        texts
            .iter()
            .enumerate()
            .map(|(i, text)| {
                let entry = SubtitleEntry::new(
                    i + 1,
                    (i as u64) * 1000,
                    (i as u64 + 1) * 1000,
                    text.to_string(),
                );
                DocumentEntry::from_subtitle_entry(entry)
            })
            .collect()
    }

    #[test]
    fn test_historySummarizer_summarizeExtractive_shouldCreateSummary() {
        let entries = create_entries(&[
            "John walked into the room.",
            "Mary was already there.",
            "They discussed the plan.",
            "John agreed to help.",
        ]);

        let summarizer = HistorySummarizer::with_defaults();
        let summary = summarizer.summarize_extractive(&entries);

        assert!(!summary.text.is_empty());
        assert_eq!(summary.entry_count, 4);
        assert_eq!(summary.start_entry_id, 1);
        assert_eq!(summary.end_entry_id, 4);
    }

    #[test]
    fn test_historySummarizer_extractLikelyNames_shouldFindNames() {
        let entries = create_entries(&[
            "Sarah called Michael.",
            "Michael answered the phone.",
            "Sarah explained the situation.",
        ]);

        let summarizer = HistorySummarizer::with_defaults();
        let names = summarizer.extract_likely_names(&entries);

        assert!(names.contains(&"Sarah".to_string()));
        assert!(names.contains(&"Michael".to_string()));
    }

    #[test]
    fn test_historySummarizer_truncateToLimit_shouldRespectLimit() {
        let summarizer = HistorySummarizer::new(SummarizationConfig {
            max_summary_chars: 50,
            ..Default::default()
        });

        let long_text = "This is a very long text that should be truncated to fit within the limit.";
        let truncated = summarizer.truncate_to_limit(long_text);

        assert!(truncated.len() <= 50);
        assert!(truncated.ends_with("...") || truncated.ends_with('.'));
    }

    #[test]
    fn test_historySummarizer_buildPrompt_shouldIncludeDialogue() {
        let entries = create_entries(&["Hello there.", "General Kenobi."]);

        let summarizer = HistorySummarizer::with_defaults();
        let prompt = summarizer.build_summarization_prompt(&entries);

        assert!(prompt.contains("Hello there."));
        assert!(prompt.contains("General Kenobi."));
        assert!(prompt.contains("Summarize"));
    }

    #[test]
    fn test_historySummarizer_combineSummaries_shouldMerge() {
        let summary1 = HistorySummary::new("First part of story.".to_string(), 1, 10, 10);
        let summary2 = HistorySummary::new("Second part continues.".to_string(), 11, 20, 10);

        let summarizer = HistorySummarizer::with_defaults();
        let combined = summarizer.combine_summaries(&[summary1, summary2]);

        assert!(combined.text.contains("First part"));
        assert!(combined.text.contains("Second part"));
        assert_eq!(combined.start_entry_id, 1);
        assert_eq!(combined.end_entry_id, 20);
        assert_eq!(combined.entry_count, 20);
    }

    #[test]
    fn test_summarizationExt_summarizeHistory_shouldWork() {
        let entries: Vec<SubtitleEntry> = vec![
            SubtitleEntry::new(1, 0, 1000, "John entered.".to_string()),
            SubtitleEntry::new(2, 1000, 2000, "He looked around.".to_string()),
            SubtitleEntry::new(3, 2000, 3000, "John found the key.".to_string()),
        ];

        let doc = SubtitleDocument::from_entries(entries, "en");
        let summary = doc.summarize_history(2);

        assert_eq!(summary.entry_count, 2);
        assert_eq!(summary.end_entry_id, 2);
    }
}

