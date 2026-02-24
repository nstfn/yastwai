/*!
 * Format validation for translated subtitles.
 *
 * This module validates that formatting elements are preserved during translation:
 * - Position tags ({\an8}, etc.)
 * - Style tags (<i>, <b>, <u>)
 * - Line break patterns
 * - Language indicators
 */

use log::debug;
use std::sync::LazyLock;
use regex::Regex;

/// Regex for position tags
static POSITION_TAG_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{\\an\d\}").expect("Invalid position tag regex")
});

/// Regex for language indicators like [IN SPANISH]
static LANGUAGE_INDICATOR_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[[^\]]*(?:IN|EN|À|AU|DE)\s+[^\]]*\]").expect("Invalid language indicator regex")
});

/// Result of format validation for a single entry
#[derive(Debug, Clone)]
pub struct FormatEntryResult {
    /// Sequence number of the entry
    pub seq_num: usize,
    /// Whether validation passed
    pub passed: bool,
    /// Issues found
    pub issues: Vec<FormatIssue>,
}

impl FormatEntryResult {
    /// Create a passing result
    pub fn passed(seq_num: usize) -> Self {
        Self {
            seq_num,
            passed: true,
            issues: vec![],
        }
    }

    /// Create a failing result
    pub fn failed(seq_num: usize, issues: Vec<FormatIssue>) -> Self {
        Self {
            seq_num,
            passed: false,
            issues,
        }
    }
}

/// Types of format issues
#[derive(Debug, Clone, PartialEq)]
pub enum FormatIssue {
    /// Position tag missing in translation
    MissingPositionTag {
        tag: String,
    },
    /// Unbalanced italic tags
    UnbalancedItalicTags {
        open_count: usize,
        close_count: usize,
    },
    /// Unbalanced bold tags
    UnbalancedBoldTags {
        open_count: usize,
        close_count: usize,
    },
    /// Unbalanced underline tags
    UnbalancedUnderlineTags {
        open_count: usize,
        close_count: usize,
    },
    /// Line count mismatch
    LineCountMismatch {
        source_lines: usize,
        translated_lines: usize,
    },
    /// Language indicator modified or removed
    LanguageIndicatorChanged {
        source_indicator: String,
    },
}

impl std::fmt::Display for FormatIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatIssue::MissingPositionTag { tag } => {
                write!(f, "Missing position tag: {}", tag)
            }
            FormatIssue::UnbalancedItalicTags { open_count, close_count } => {
                write!(
                    f,
                    "Unbalanced italic tags: {} open, {} close",
                    open_count, close_count
                )
            }
            FormatIssue::UnbalancedBoldTags { open_count, close_count } => {
                write!(
                    f,
                    "Unbalanced bold tags: {} open, {} close",
                    open_count, close_count
                )
            }
            FormatIssue::UnbalancedUnderlineTags { open_count, close_count } => {
                write!(
                    f,
                    "Unbalanced underline tags: {} open, {} close",
                    open_count, close_count
                )
            }
            FormatIssue::LineCountMismatch {
                source_lines,
                translated_lines,
            } => {
                write!(
                    f,
                    "Line count mismatch: source has {} lines, translation has {}",
                    source_lines, translated_lines
                )
            }
            FormatIssue::LanguageIndicatorChanged { source_indicator } => {
                write!(f, "Language indicator changed: {}", source_indicator)
            }
        }
    }
}

/// Result of validating format for a collection
#[derive(Debug, Clone)]
pub struct FormatValidationResult {
    /// Overall pass/fail status
    pub passed: bool,
    /// Results for each entry
    pub entry_results: Vec<FormatEntryResult>,
    /// Total number of issues
    pub total_issues: usize,
}

impl FormatValidationResult {
    /// Get all failed entries
    pub fn failed_entries(&self) -> Vec<&FormatEntryResult> {
        self.entry_results.iter().filter(|r| !r.passed).collect()
    }
}

/// Configuration for format validation
#[derive(Debug, Clone)]
pub struct FormatValidatorConfig {
    /// Check position tags
    pub check_position_tags: bool,
    /// Check style tags (italic, bold, underline)
    pub check_style_tags: bool,
    /// Check line count matches
    pub check_line_count: bool,
    /// Check language indicators
    pub check_language_indicators: bool,
}

impl Default for FormatValidatorConfig {
    fn default() -> Self {
        Self {
            check_position_tags: true,
            check_style_tags: true,
            check_line_count: false, // Off by default as AI may reasonably change line breaks
            check_language_indicators: true,
        }
    }
}

/// Format validator for subtitle translations
pub struct FormatValidator {
    config: FormatValidatorConfig,
}

impl FormatValidator {
    /// Create a new validator with default configuration
    pub fn new() -> Self {
        Self {
            config: FormatValidatorConfig::default(),
        }
    }

    /// Create a new validator with custom configuration
    pub fn with_config(config: FormatValidatorConfig) -> Self {
        Self { config }
    }

    /// Validate a single translation pair (source and translated)
    pub fn validate_entry(
        &self,
        seq_num: usize,
        source_text: &str,
        translated_text: &str,
    ) -> FormatEntryResult {
        let mut issues = Vec::new();

        // Check position tags
        if self.config.check_position_tags {
            issues.extend(self.check_position_tags(source_text, translated_text));
        }

        // Check style tags
        if self.config.check_style_tags {
            issues.extend(self.check_style_tag_balance(translated_text));
        }

        // Check line count
        if self.config.check_line_count {
            issues.extend(self.check_line_count(source_text, translated_text));
        }

        // Check language indicators
        if self.config.check_language_indicators {
            issues.extend(self.check_language_indicators(source_text, translated_text));
        }

        if issues.is_empty() {
            FormatEntryResult::passed(seq_num)
        } else {
            FormatEntryResult::failed(seq_num, issues)
        }
    }

    /// Check that position tags are preserved
    fn check_position_tags(&self, source: &str, translated: &str) -> Vec<FormatIssue> {
        let mut issues = Vec::new();

        // Find all position tags in source
        for cap in POSITION_TAG_REGEX.find_iter(source) {
            let tag = cap.as_str();
            if !translated.contains(tag) {
                issues.push(FormatIssue::MissingPositionTag {
                    tag: tag.to_string(),
                });
            }
        }

        issues
    }

    /// Check that style tags are balanced
    fn check_style_tag_balance(&self, text: &str) -> Vec<FormatIssue> {
        let mut issues = Vec::new();

        // Check italic tags
        let italic_open = text.matches("<i>").count();
        let italic_close = text.matches("</i>").count();
        if italic_open != italic_close {
            issues.push(FormatIssue::UnbalancedItalicTags {
                open_count: italic_open,
                close_count: italic_close,
            });
        }

        // Check bold tags
        let bold_open = text.matches("<b>").count();
        let bold_close = text.matches("</b>").count();
        if bold_open != bold_close {
            issues.push(FormatIssue::UnbalancedBoldTags {
                open_count: bold_open,
                close_count: bold_close,
            });
        }

        // Check underline tags
        let underline_open = text.matches("<u>").count();
        let underline_close = text.matches("</u>").count();
        if underline_open != underline_close {
            issues.push(FormatIssue::UnbalancedUnderlineTags {
                open_count: underline_open,
                close_count: underline_close,
            });
        }

        issues
    }

    /// Check that line count matches
    fn check_line_count(&self, source: &str, translated: &str) -> Vec<FormatIssue> {
        let source_lines = source.lines().count();
        let translated_lines = translated.lines().count();

        if source_lines != translated_lines {
            vec![FormatIssue::LineCountMismatch {
                source_lines,
                translated_lines,
            }]
        } else {
            vec![]
        }
    }

    /// Check that language indicators are preserved
    fn check_language_indicators(&self, source: &str, translated: &str) -> Vec<FormatIssue> {
        let mut issues = Vec::new();

        for cap in LANGUAGE_INDICATOR_REGEX.find_iter(source) {
            let indicator = cap.as_str();
            // Check if the indicator (or a reasonable translation of it) is present
            // We're lenient here - just check if square brackets with content exist
            if !translated.contains(indicator) && !translated.contains('[') {
                issues.push(FormatIssue::LanguageIndicatorChanged {
                    source_indicator: indicator.to_string(),
                });
            }
        }

        issues
    }

    /// Validate a collection of translation pairs
    pub fn validate_collection(
        &self,
        pairs: &[(usize, &str, &str)], // (seq_num, source, translated)
    ) -> FormatValidationResult {
        let entry_results: Vec<FormatEntryResult> = pairs
            .iter()
            .map(|(seq_num, source, translated)| {
                self.validate_entry(*seq_num, source, translated)
            })
            .collect();

        let total_issues: usize = entry_results.iter().map(|r| r.issues.len()).sum();
        let passed = entry_results.iter().all(|r| r.passed);

        debug!(
            "Format validation: {} entries, {} issues",
            pairs.len(),
            total_issues
        );

        FormatValidationResult {
            passed,
            entry_results,
            total_issues,
        }
    }

    /// Count position tags in text
    pub fn count_position_tags(text: &str) -> usize {
        POSITION_TAG_REGEX.find_iter(text).count()
    }

    /// Extract all position tags from text
    pub fn extract_position_tags(text: &str) -> Vec<String> {
        POSITION_TAG_REGEX
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .collect()
    }
}

impl Default for FormatValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validateEntry_withMatchingFormat_shouldPass() {
        let validator = FormatValidator::new();

        let result = validator.validate_entry(
            1,
            r"{\an8}Hello World",
            r"{\an8}Bonjour le Monde",
        );

        assert!(result.passed);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_validateEntry_withMissingPositionTag_shouldFail() {
        let validator = FormatValidator::new();

        let result = validator.validate_entry(
            1,
            r"{\an8}Hello World",
            "Bonjour le Monde", // Missing position tag
        );

        assert!(!result.passed);
        assert!(matches!(
            result.issues[0],
            FormatIssue::MissingPositionTag { .. }
        ));
    }

    #[test]
    fn test_validateEntry_withUnbalancedItalics_shouldFail() {
        let validator = FormatValidator::new();

        let result = validator.validate_entry(
            1,
            "<i>Hello</i>",
            "<i>Bonjour", // Missing closing tag
        );

        assert!(!result.passed);
        assert!(matches!(
            result.issues[0],
            FormatIssue::UnbalancedItalicTags { .. }
        ));
    }

    #[test]
    fn test_validateEntry_withBalancedItalics_shouldPass() {
        let validator = FormatValidator::new();

        let result = validator.validate_entry(1, "<i>Hello</i>", "<i>Bonjour</i>");

        assert!(result.passed);
    }

    #[test]
    fn test_validateEntry_withUnbalancedBold_shouldFail() {
        let validator = FormatValidator::new();

        let result = validator.validate_entry(1, "<b>Hello</b>", "<b>Bonjour");

        assert!(!result.passed);
        assert!(matches!(
            result.issues[0],
            FormatIssue::UnbalancedBoldTags { .. }
        ));
    }

    #[test]
    fn test_checkLineCount_withMismatch_shouldFail() {
        let config = FormatValidatorConfig {
            check_line_count: true,
            ..Default::default()
        };
        let validator = FormatValidator::with_config(config);

        let result = validator.validate_entry(
            1,
            "Line 1\nLine 2",
            "Only one line",
        );

        assert!(!result.passed);
        assert!(matches!(
            result.issues[0],
            FormatIssue::LineCountMismatch { .. }
        ));
    }

    #[test]
    fn test_languageIndicator_preserved_shouldPass() {
        let validator = FormatValidator::new();

        let result = validator.validate_entry(
            1,
            "[IN SPANISH] Hola",
            "[IN SPANISH] Salut",
        );

        assert!(result.passed);
    }

    #[test]
    fn test_validateCollection_shouldValidateAll() {
        let validator = FormatValidator::new();

        let pairs = vec![
            (1, r"{\an8}Hello", r"{\an8}Bonjour"),
            (2, "World", "Monde"),
            (3, "<i>Test</i>", "<i>Essai</i>"),
        ];

        let result = validator.validate_collection(&pairs);

        assert!(result.passed);
        assert_eq!(result.entry_results.len(), 3);
    }

    #[test]
    fn test_countPositionTags_shouldCountCorrectly() {
        let text = r"{\an8}Line 1{\an2}Line 2";
        assert_eq!(FormatValidator::count_position_tags(text), 2);
    }

    #[test]
    fn test_extractPositionTags_shouldExtractAll() {
        let text = r"{\an8}Hello {\an2}World";
        let tags = FormatValidator::extract_position_tags(text);

        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], r"{\an8}");
        assert_eq!(tags[1], r"{\an2}");
    }
}
