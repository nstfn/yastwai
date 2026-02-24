/*!
 * Consistency checking for translation quality.
 *
 * Ensures consistency across multiple dimensions:
 * - Terminology: Same terms translated the same way
 * - Style: Consistent formality, tone, punctuation
 * - Character names: Names preserved correctly
 * - Technical terms: Domain-specific vocabulary
 */

use std::collections::{HashMap, HashSet};

use crate::translation::document::{DocumentEntry, Glossary, SubtitleDocument};

/// Configuration for consistency checking.
#[derive(Debug, Clone)]
pub struct ConsistencyConfig {
    /// Check terminology consistency
    pub check_terminology: bool,

    /// Check style consistency
    pub check_style: bool,

    /// Check character name preservation
    pub check_names: bool,

    /// Check punctuation consistency
    pub check_punctuation: bool,

    /// Minimum occurrences to flag inconsistency
    pub min_occurrences_for_flag: usize,

    /// Whether to track capitalization differences
    pub case_sensitive: bool,
}

impl Default for ConsistencyConfig {
    fn default() -> Self {
        Self {
            check_terminology: true,
            check_style: true,
            check_names: true,
            check_punctuation: true,
            min_occurrences_for_flag: 2,
            case_sensitive: false,
        }
    }
}

impl ConsistencyConfig {
    /// Create a strict config that checks everything.
    pub fn strict() -> Self {
        Self {
            check_terminology: true,
            check_style: true,
            check_names: true,
            check_punctuation: true,
            min_occurrences_for_flag: 1,
            case_sensitive: true,
        }
    }

    /// Create a lenient config that checks less strictly.
    pub fn lenient() -> Self {
        Self {
            check_terminology: true,
            check_style: false,
            check_names: true,
            check_punctuation: false,
            min_occurrences_for_flag: 3,
            case_sensitive: false,
        }
    }

    /// Create a minimal config for speed.
    pub fn minimal() -> Self {
        Self {
            check_terminology: true,
            check_style: false,
            check_names: true,
            check_punctuation: false,
            min_occurrences_for_flag: 3,
            case_sensitive: false,
        }
    }
}

/// Types of style issues.
#[derive(Debug, Clone, PartialEq)]
pub enum StyleIssue {
    /// Inconsistent term translation
    InconsistentTerm {
        term: String,
        translations: Vec<String>,
        occurrences: Vec<usize>,
    },

    /// Character name not preserved
    NameNotPreserved {
        name: String,
        entry_id: usize,
        found_as: Option<String>,
    },

    /// Inconsistent punctuation style
    InconsistentPunctuation {
        pattern: String,
        entry_ids: Vec<usize>,
    },

    /// Inconsistent formality level
    InconsistentFormality {
        entry_id: usize,
        expected: FormalityLevel,
        found: FormalityLevel,
    },

    /// Mixed quote styles
    MixedQuoteStyles {
        entry_ids: Vec<usize>,
    },

    /// Inconsistent number formatting
    InconsistentNumberFormat {
        entry_ids: Vec<usize>,
    },
}

impl StyleIssue {
    /// Get the severity of this issue (0.0 - 1.0).
    pub fn severity(&self) -> f32 {
        match self {
            StyleIssue::InconsistentTerm { translations, .. } => {
                // More variations = more severe
                (translations.len() as f32 / 5.0).min(1.0)
            }
            StyleIssue::NameNotPreserved { .. } => 0.8,
            StyleIssue::InconsistentPunctuation { .. } => 0.3,
            StyleIssue::InconsistentFormality { .. } => 0.5,
            StyleIssue::MixedQuoteStyles { .. } => 0.2,
            StyleIssue::InconsistentNumberFormat { .. } => 0.2,
        }
    }

    /// Get a description of this issue.
    pub fn description(&self) -> String {
        match self {
            StyleIssue::InconsistentTerm { term, translations, .. } => {
                format!(
                    "Term '{}' translated inconsistently: {}",
                    term,
                    translations.join(", ")
                )
            }
            StyleIssue::NameNotPreserved { name, entry_id, found_as } => {
                match found_as {
                    Some(found) => format!(
                        "Name '{}' changed to '{}' in entry {}",
                        name, found, entry_id
                    ),
                    None => format!("Name '{}' missing in entry {}", name, entry_id),
                }
            }
            StyleIssue::InconsistentPunctuation { pattern, entry_ids } => {
                format!(
                    "Inconsistent punctuation '{}' in {} entries",
                    pattern,
                    entry_ids.len()
                )
            }
            StyleIssue::InconsistentFormality { entry_id, expected, found } => {
                format!(
                    "Entry {}: expected {:?} formality, found {:?}",
                    entry_id, expected, found
                )
            }
            StyleIssue::MixedQuoteStyles { entry_ids } => {
                format!("Mixed quote styles in {} entries", entry_ids.len())
            }
            StyleIssue::InconsistentNumberFormat { entry_ids } => {
                format!("Inconsistent number formats in {} entries", entry_ids.len())
            }
        }
    }
}

/// Formality levels for style checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormalityLevel {
    /// Casual/informal language
    Informal,
    /// Neutral tone
    Neutral,
    /// Formal language
    Formal,
}

/// Report of consistency check results.
#[derive(Debug, Clone)]
pub struct ConsistencyReport {
    /// All issues found
    pub issues: Vec<StyleIssue>,

    /// Number of terms checked
    pub terms_checked: usize,

    /// Number of names checked
    pub names_checked: usize,

    /// Number of entries analyzed
    pub entries_analyzed: usize,

    /// Overall consistency score (0.0 - 1.0)
    pub score: f32,
}

impl ConsistencyReport {
    /// Create a new empty report.
    pub fn new() -> Self {
        Self {
            issues: Vec::new(),
            terms_checked: 0,
            names_checked: 0,
            entries_analyzed: 0,
            score: 1.0,
        }
    }

    /// Add an issue.
    pub fn add_issue(&mut self, issue: StyleIssue) {
        self.issues.push(issue);
    }

    /// Calculate the consistency score.
    pub fn calculate_score(&mut self) {
        if self.issues.is_empty() {
            self.score = 1.0;
            return;
        }

        let total_severity: f32 = self.issues.iter().map(|i| i.severity()).sum();
        let max_severity = self.entries_analyzed.max(1) as f32;

        self.score = (1.0 - total_severity / max_severity).max(0.0);
    }

    /// Check if consistency is acceptable.
    pub fn is_acceptable(&self, threshold: f32) -> bool {
        self.score >= threshold
    }

    /// Get issues by severity (highest first).
    pub fn issues_by_severity(&self) -> Vec<&StyleIssue> {
        let mut sorted: Vec<_> = self.issues.iter().collect();
        sorted.sort_by(|a, b| b.severity().partial_cmp(&a.severity()).unwrap_or(std::cmp::Ordering::Equal));
        sorted
    }

    /// Get summary string.
    pub fn summary(&self) -> String {
        format!(
            "Consistency: {:.1}% ({} issues, {} terms, {} names checked)",
            self.score * 100.0,
            self.issues.len(),
            self.terms_checked,
            self.names_checked
        )
    }
}

impl Default for ConsistencyReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Consistency checker for translations.
pub struct ConsistencyChecker {
    config: ConsistencyConfig,
}

impl ConsistencyChecker {
    /// Create a new checker with default config.
    pub fn new() -> Self {
        Self {
            config: ConsistencyConfig::default(),
        }
    }

    /// Create with custom config.
    pub fn with_config(config: ConsistencyConfig) -> Self {
        Self { config }
    }

    /// Check consistency of a document.
    pub fn check(&self, doc: &SubtitleDocument) -> ConsistencyReport {
        let mut report = ConsistencyReport::new();
        report.entries_analyzed = doc.entries.len();

        if self.config.check_terminology {
            self.check_terminology(doc, &mut report);
        }

        if self.config.check_names {
            self.check_names(doc, &mut report);
        }

        if self.config.check_style {
            self.check_style(doc, &mut report);
        }

        if self.config.check_punctuation {
            self.check_punctuation(doc, &mut report);
        }

        report.calculate_score();
        report
    }

    /// Check terminology consistency.
    fn check_terminology(&self, doc: &SubtitleDocument, report: &mut ConsistencyReport) {
        // Track how each term is translated
        let mut term_translations: HashMap<String, Vec<(String, usize)>> = HashMap::new();

        for term in doc.glossary.terms.keys() {
            let term_key = if self.config.case_sensitive {
                term.clone()
            } else {
                term.to_lowercase()
            };

            for entry in &doc.entries {
                let source_check = if self.config.case_sensitive {
                    &entry.original_text
                } else {
                    &entry.original_text.to_lowercase()
                };

                if source_check.contains(&term_key) {
                    if let Some(ref translated) = entry.translated_text {
                        term_translations
                            .entry(term.clone())
                            .or_default()
                            .push((translated.clone(), entry.id));
                    }
                }
            }
        }

        report.terms_checked = term_translations.len();

        // Find inconsistent translations
        for (term, translations) in term_translations {
            if translations.len() < self.config.min_occurrences_for_flag {
                continue;
            }

            // Group by unique translation
            let mut unique: HashMap<String, Vec<usize>> = HashMap::new();
            for (trans, entry_id) in translations {
                let key = if self.config.case_sensitive {
                    trans
                } else {
                    trans.to_lowercase()
                };
                unique.entry(key).or_default().push(entry_id);
            }

            if unique.len() > 1 {
                let translations: Vec<String> = unique.keys().cloned().collect();
                let occurrences: Vec<usize> = unique.values().flatten().copied().collect();

                report.add_issue(StyleIssue::InconsistentTerm {
                    term,
                    translations,
                    occurrences,
                });
            }
        }
    }

    /// Check character name preservation.
    fn check_names(&self, doc: &SubtitleDocument, report: &mut ConsistencyReport) {
        report.names_checked = doc.glossary.character_names.len();

        for name in &doc.glossary.character_names {
            for entry in &doc.entries {
                // Check if name appears in original
                if entry.original_text.contains(name) {
                    if let Some(ref translated) = entry.translated_text {
                        // Check if name is preserved
                        if !translated.contains(name) {
                            // Try to find similar name (possible mistranslation)
                            let found_as = self.find_similar_name(name, translated);

                            report.add_issue(StyleIssue::NameNotPreserved {
                                name: name.clone(),
                                entry_id: entry.id,
                                found_as,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Find a similar name in text (possible mistranslation).
    fn find_similar_name(&self, name: &str, text: &str) -> Option<String> {
        // Simple heuristic: look for capitalized words of similar length
        let name_len = name.len();
        let tolerance = (name_len / 3).max(1);

        for word in text.split_whitespace() {
            let clean_word = word.trim_matches(|c: char| !c.is_alphabetic());
            if clean_word.is_empty() {
                continue;
            }

            // Check if it's capitalized
            if clean_word.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                let len_diff = (clean_word.len() as i32 - name_len as i32).unsigned_abs() as usize;
                if len_diff <= tolerance {
                    return Some(clean_word.to_string());
                }
            }
        }

        None
    }

    /// Check style consistency (formality, tone).
    fn check_style(&self, doc: &SubtitleDocument, report: &mut ConsistencyReport) {
        // Detect dominant formality level
        let mut formality_counts: HashMap<FormalityLevel, usize> = HashMap::new();

        for entry in &doc.entries {
            if let Some(ref translated) = entry.translated_text {
                let level = self.detect_formality(translated);
                *formality_counts.entry(level).or_insert(0) += 1;
            }
        }

        // Find dominant level
        let dominant = formality_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(level, _)| *level)
            .unwrap_or(FormalityLevel::Neutral);

        // Flag entries that don't match
        for entry in &doc.entries {
            if let Some(ref translated) = entry.translated_text {
                let level = self.detect_formality(translated);
                if level != dominant && level != FormalityLevel::Neutral {
                    report.add_issue(StyleIssue::InconsistentFormality {
                        entry_id: entry.id,
                        expected: dominant,
                        found: level,
                    });
                }
            }
        }
    }

    /// Detect formality level of text.
    fn detect_formality(&self, text: &str) -> FormalityLevel {
        let lower = text.to_lowercase();

        // Informal indicators
        let informal_patterns = ["gonna", "wanna", "gotta", "ain't", "y'all", "kinda", "sorta"];
        let informal_count = informal_patterns.iter().filter(|p| lower.contains(*p)).count();

        // Formal indicators
        let formal_patterns = [
            "therefore",
            "furthermore",
            "nevertheless",
            "consequently",
            "accordingly",
        ];
        let formal_count = formal_patterns.iter().filter(|p| lower.contains(*p)).count();

        if informal_count > formal_count {
            FormalityLevel::Informal
        } else if formal_count > informal_count {
            FormalityLevel::Formal
        } else {
            FormalityLevel::Neutral
        }
    }

    /// Check punctuation consistency.
    fn check_punctuation(&self, doc: &SubtitleDocument, report: &mut ConsistencyReport) {
        let mut quote_styles: HashMap<char, Vec<usize>> = HashMap::new();

        for entry in &doc.entries {
            if let Some(ref translated) = entry.translated_text {
                // Check quote styles
                for c in translated.chars() {
                    if matches!(c, '"' | '\u{201C}' | '\u{201D}' | '«' | '»' | '\'' | '\u{2019}' | '\u{2018}') {
                        quote_styles.entry(c).or_default().push(entry.id);
                    }
                }
            }
        }

        // Check for mixed quote styles
        let quote_types: HashSet<_> = quote_styles.keys().collect();
        if quote_types.len() > 2 {
            let all_entries: Vec<usize> = quote_styles.values().flatten().copied().collect();
            report.add_issue(StyleIssue::MixedQuoteStyles {
                entry_ids: all_entries,
            });
        }
    }

    /// Check consistency between two specific entries.
    pub fn check_pair(&self, entry1: &DocumentEntry, entry2: &DocumentEntry, glossary: &Glossary) -> Vec<StyleIssue> {
        let mut issues = Vec::new();

        // Check if same terms are translated consistently
        for term in glossary.terms.keys() {
            let in_e1 = entry1.original_text.contains(term);
            let in_e2 = entry2.original_text.contains(term);

            if in_e1 && in_e2 {
                if let (Some(t1), Some(t2)) = (&entry1.translated_text, &entry2.translated_text) {
                    // Extract how the term was translated in each
                    // This is a simplified check - just verifying both contain similar content
                    let _t1_lower = t1.to_lowercase();
                    let _t2_lower = t2.to_lowercase();

                    // Very different lengths might indicate inconsistent translation
                    let len_ratio = t1.len() as f32 / t2.len().max(1) as f32;
                    if !(0.3..=3.0).contains(&len_ratio) {
                        issues.push(StyleIssue::InconsistentTerm {
                            term: term.clone(),
                            translations: vec![t1.clone(), t2.clone()],
                            occurrences: vec![entry1.id, entry2.id],
                        });
                    }
                }
            }
        }

        issues
    }
}

impl Default for ConsistencyChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitle_processor::SubtitleEntry;

    fn create_test_document() -> SubtitleDocument {
        let entries = vec![
            SubtitleEntry::new(1, 0, 2000, "John said hello.".to_string()),
            SubtitleEntry::new(2, 2000, 4000, "John waved goodbye.".to_string()),
            SubtitleEntry::new(3, 4000, 6000, "The end.".to_string()),
        ];

        let mut doc = SubtitleDocument::from_entries(entries, "en");
        doc.glossary.add_character("John");
        doc
    }

    #[test]
    fn test_consistencyChecker_checkNames_shouldDetectMissing() {
        let mut doc = create_test_document();

        // Set translations - second one doesn't preserve the name
        doc.entries[0].set_translation("John a dit bonjour.".to_string(), None);
        doc.entries[1].set_translation("Jean a fait au revoir.".to_string(), None); // Name changed!
        doc.entries[2].set_translation("La fin.".to_string(), None);

        let checker = ConsistencyChecker::new();
        let report = checker.check(&doc);

        assert!(report.issues.iter().any(|i| matches!(
            i,
            StyleIssue::NameNotPreserved { name, .. } if name == "John"
        )));
    }

    #[test]
    fn test_consistencyChecker_checkNames_shouldPassWhenPreserved() {
        let mut doc = create_test_document();

        doc.entries[0].set_translation("John a dit bonjour.".to_string(), None);
        doc.entries[1].set_translation("John a fait au revoir.".to_string(), None);
        doc.entries[2].set_translation("La fin.".to_string(), None);

        let checker = ConsistencyChecker::new();
        let report = checker.check(&doc);

        let name_issues: Vec<_> = report
            .issues
            .iter()
            .filter(|i| matches!(i, StyleIssue::NameNotPreserved { .. }))
            .collect();

        assert!(name_issues.is_empty());
    }

    #[test]
    fn test_consistencyReport_calculateScore_shouldReflectIssues() {
        let mut report = ConsistencyReport::new();
        report.entries_analyzed = 10;

        // No issues = perfect score
        report.calculate_score();
        assert_eq!(report.score, 1.0);

        // Add issues
        report.add_issue(StyleIssue::NameNotPreserved {
            name: "Test".to_string(),
            entry_id: 1,
            found_as: None,
        });
        report.calculate_score();

        assert!(report.score < 1.0);
    }

    #[test]
    fn test_styleIssue_severity_shouldVaryByType() {
        let name_issue = StyleIssue::NameNotPreserved {
            name: "Test".to_string(),
            entry_id: 1,
            found_as: None,
        };

        let punctuation_issue = StyleIssue::InconsistentPunctuation {
            pattern: "...".to_string(),
            entry_ids: vec![1, 2],
        };

        assert!(name_issue.severity() > punctuation_issue.severity());
    }

    #[test]
    fn test_detectFormality_shouldIdentifyLevels() {
        let checker = ConsistencyChecker::new();

        assert_eq!(
            checker.detect_formality("I'm gonna go now"),
            FormalityLevel::Informal
        );

        assert_eq!(
            checker.detect_formality("Therefore, we must proceed"),
            FormalityLevel::Formal
        );

        assert_eq!(
            checker.detect_formality("I will go now"),
            FormalityLevel::Neutral
        );
    }
}
