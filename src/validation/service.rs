/*!
 * Validation service that orchestrates all validators.
 *
 * This module provides a unified interface for running all validation
 * checks on translated subtitles.
 */

use log::debug;
use serde::{Deserialize, Serialize};

use crate::subtitle_processor::SubtitleEntry;

use super::formatting::{FormatValidator, FormatValidatorConfig, FormatValidationResult};
use super::length::{LengthValidator, LengthValidatorConfig, LengthValidationResult};
use super::markers::{MarkerValidator, MarkerValidationResult};
use super::timecodes::{TimecodeValidator, TimecodeValidatorConfig, TimecodeValidationResult};

/// Configuration for the validation service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// Whether validation is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Whether to validate markers in batch responses
    #[serde(default = "default_true")]
    pub marker_validation: bool,

    /// Whether to validate timecodes
    #[serde(default = "default_true")]
    pub timecode_validation: bool,

    /// Whether to validate format preservation
    #[serde(default = "default_true")]
    pub format_validation: bool,

    /// Whether to validate length ratios
    #[serde(default = "default_true")]
    pub length_validation: bool,

    /// Minimum acceptable length ratio
    #[serde(default = "default_min_ratio")]
    pub length_ratio_min: f64,

    /// Maximum acceptable length ratio
    #[serde(default = "default_max_ratio")]
    pub length_ratio_max: f64,

    /// Maximum characters per second for readable subtitles
    #[serde(default = "default_max_cps")]
    pub max_chars_per_second: f64,

    /// Whether to retry translations that fail validation
    #[serde(default = "default_true")]
    pub retry_on_validation_failure: bool,

    /// Maximum number of retries for failed validation
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_true() -> bool {
    true
}

fn default_min_ratio() -> f64 {
    0.3
}

fn default_max_ratio() -> f64 {
    3.0
}

fn default_max_cps() -> f64 {
    25.0
}

fn default_max_retries() -> u32 {
    3
}

/// Convert from app_config::ValidationConfig to validation::ValidationConfig
impl From<crate::app_config::ValidationConfig> for ValidationConfig {
    fn from(config: crate::app_config::ValidationConfig) -> Self {
        Self {
            enabled: config.enabled,
            marker_validation: config.marker_validation,
            timecode_validation: config.timecode_validation,
            format_validation: config.format_validation,
            length_validation: true, // Not in app_config, default to true
            length_ratio_min: config.length_ratio_min,
            length_ratio_max: config.length_ratio_max,
            max_chars_per_second: config.max_chars_per_second,
            retry_on_validation_failure: config.retry_on_validation_failure,
            max_retries: config.max_retries,
        }
    }
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            marker_validation: true,
            timecode_validation: true,
            format_validation: true,
            length_validation: true,
            length_ratio_min: default_min_ratio(),
            length_ratio_max: default_max_ratio(),
            max_chars_per_second: default_max_cps(),
            retry_on_validation_failure: true,
            max_retries: default_max_retries(),
        }
    }
}

/// Result of a single entry validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Sequence number
    pub seq_num: usize,
    /// Whether all validations passed
    pub passed: bool,
    /// Whether any errors (vs warnings) were found
    pub has_errors: bool,
    /// List of issues found
    pub issues: Vec<ValidationIssue>,
    /// Suggested action
    pub action: ValidationAction,
}

impl ValidationResult {
    /// Create a passing result
    pub fn passed(seq_num: usize) -> Self {
        Self {
            seq_num,
            passed: true,
            has_errors: false,
            issues: vec![],
            action: ValidationAction::Accept,
        }
    }

    /// Create a result with warnings (still passes)
    pub fn with_warnings(seq_num: usize, issues: Vec<ValidationIssue>) -> Self {
        Self {
            seq_num,
            passed: true,
            has_errors: false,
            issues,
            action: ValidationAction::AcceptWithWarnings,
        }
    }

    /// Create a failing result
    pub fn failed(seq_num: usize, issues: Vec<ValidationIssue>) -> Self {
        Self {
            seq_num,
            passed: false,
            has_errors: true,
            issues,
            action: ValidationAction::Retry,
        }
    }
}

/// A validation issue
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Type of validation that found the issue
    pub validation_type: String,
    /// Severity of the issue
    pub severity: IssueSeverity,
    /// Description of the issue
    pub message: String,
}

impl ValidationIssue {
    /// Create a warning issue
    pub fn warning(validation_type: &str, message: String) -> Self {
        Self {
            validation_type: validation_type.to_string(),
            severity: IssueSeverity::Warning,
            message,
        }
    }

    /// Create an error issue
    pub fn error(validation_type: &str, message: String) -> Self {
        Self {
            validation_type: validation_type.to_string(),
            severity: IssueSeverity::Error,
            message,
        }
    }
}

/// Severity of a validation issue
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Warning - translation usable but may have issues
    Warning,
    /// Error - translation should be retried
    Error,
}

/// Suggested action based on validation results
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationAction {
    /// Accept the translation as-is
    Accept,
    /// Accept but log warnings
    AcceptWithWarnings,
    /// Retry the translation
    Retry,
    /// Reject and use fallback (original text)
    Reject,
}

/// Complete validation report for a batch
#[derive(Debug, Clone)]
pub struct ValidationReport {
    /// Whether all entries passed
    pub all_passed: bool,
    /// Total entries validated
    pub total_entries: usize,
    /// Entries that passed
    pub passed_count: usize,
    /// Entries with warnings
    pub warning_count: usize,
    /// Entries with errors
    pub error_count: usize,
    /// Individual results
    pub results: Vec<ValidationResult>,
    /// Marker validation result (if batch validation was performed)
    pub marker_result: Option<MarkerValidationResult>,
}

impl ValidationReport {
    /// Get entries that need retry
    pub fn entries_to_retry(&self) -> Vec<usize> {
        self.results
            .iter()
            .filter(|r| r.action == ValidationAction::Retry)
            .map(|r| r.seq_num)
            .collect()
    }

    /// Get entries that should be rejected
    pub fn entries_to_reject(&self) -> Vec<usize> {
        self.results
            .iter()
            .filter(|r| r.action == ValidationAction::Reject)
            .map(|r| r.seq_num)
            .collect()
    }

    /// Get a summary string
    pub fn summary(&self) -> String {
        format!(
            "Validation: {}/{} passed, {} warnings, {} errors",
            self.passed_count, self.total_entries, self.warning_count, self.error_count
        )
    }
}

/// Validation service for subtitle translations
pub struct ValidationService {
    config: ValidationConfig,
    format_validator: FormatValidator,
    length_validator: LengthValidator,
    timecode_validator: TimecodeValidator,
}

impl ValidationService {
    /// Create a new validation service with default configuration
    pub fn new() -> Self {
        Self::with_config(ValidationConfig::default())
    }

    /// Create a new validation service with custom configuration
    pub fn with_config(config: ValidationConfig) -> Self {
        let format_config = FormatValidatorConfig::default();

        let length_config = LengthValidatorConfig {
            min_ratio: config.length_ratio_min,
            max_ratio: config.length_ratio_max,
            ..Default::default()
        };

        let timecode_config = TimecodeValidatorConfig {
            max_cps: config.max_chars_per_second,
            ..Default::default()
        };

        Self {
            config,
            format_validator: FormatValidator::with_config(format_config),
            length_validator: LengthValidator::with_config(length_config),
            timecode_validator: TimecodeValidator::with_config(timecode_config),
        }
    }

    /// Check if validation is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Validate a batch response (check markers)
    pub fn validate_batch_markers(
        &self,
        response: &str,
        expected_count: usize,
    ) -> MarkerValidationResult {
        if !self.config.enabled || !self.config.marker_validation {
            return MarkerValidationResult::success(expected_count);
        }

        MarkerValidator::validate(response, expected_count)
    }

    /// Validate a single translated entry
    pub fn validate_entry(
        &self,
        source_entry: &SubtitleEntry,
        translated_entry: &SubtitleEntry,
    ) -> ValidationResult {
        if !self.config.enabled {
            return ValidationResult::passed(source_entry.seq_num);
        }

        let mut issues = Vec::new();

        // Format validation
        if self.config.format_validation {
            let format_result = self.format_validator.validate_entry(
                source_entry.seq_num,
                &source_entry.text,
                &translated_entry.text,
            );

            if !format_result.passed {
                for issue in format_result.issues {
                    issues.push(ValidationIssue::warning("format", issue.to_string()));
                }
            }
        }

        // Length validation
        if self.config.length_validation {
            let length_result = self.length_validator.validate_entry(
                source_entry.seq_num,
                &source_entry.text,
                &translated_entry.text,
            );

            if !length_result.passed {
                for issue in length_result.issues {
                    // Empty translations are errors, ratio issues are warnings
                    let severity = match &issue {
                        super::length::LengthIssue::EmptyTranslation => IssueSeverity::Error,
                        _ => IssueSeverity::Warning,
                    };
                    issues.push(ValidationIssue {
                        validation_type: "length".to_string(),
                        severity,
                        message: issue.to_string(),
                    });
                }
            }
        }

        // Timecode validation (on translated entry)
        if self.config.timecode_validation {
            let timecode_result = self.timecode_validator.validate_entry(translated_entry);

            if !timecode_result.passed {
                for issue in timecode_result.issues {
                    issues.push(ValidationIssue::warning("timecode", issue.to_string()));
                }
            }
        }

        // Determine result
        if issues.is_empty() {
            ValidationResult::passed(source_entry.seq_num)
        } else {
            let has_errors = issues.iter().any(|i| i.severity == IssueSeverity::Error);
            if has_errors {
                ValidationResult::failed(source_entry.seq_num, issues)
            } else {
                ValidationResult::with_warnings(source_entry.seq_num, issues)
            }
        }
    }

    /// Validate a collection of translated entries
    pub fn validate_collection(
        &self,
        source_entries: &[SubtitleEntry],
        translated_entries: &[SubtitleEntry],
    ) -> ValidationReport {
        if !self.config.enabled {
            return ValidationReport {
                all_passed: true,
                total_entries: source_entries.len(),
                passed_count: source_entries.len(),
                warning_count: 0,
                error_count: 0,
                results: source_entries
                    .iter()
                    .map(|e| ValidationResult::passed(e.seq_num))
                    .collect(),
                marker_result: None,
            };
        }

        // Match source and translated entries by seq_num
        let mut results = Vec::new();

        for source in source_entries {
            let translated = translated_entries
                .iter()
                .find(|t| t.seq_num == source.seq_num);

            let result = if let Some(translated) = translated {
                self.validate_entry(source, translated)
            } else {
                // Missing translation
                ValidationResult::failed(
                    source.seq_num,
                    vec![ValidationIssue::error(
                        "missing",
                        "No translation found for this entry".to_string(),
                    )],
                )
            };

            results.push(result);
        }

        // Calculate summary statistics
        let passed_count = results.iter().filter(|r| r.passed).count();
        let warning_count = results
            .iter()
            .filter(|r| r.passed && !r.issues.is_empty())
            .count();
        let error_count = results.iter().filter(|r| r.has_errors).count();
        let all_passed = results.iter().all(|r| r.passed);

        debug!(
            "Validation complete: {}/{} passed, {} warnings, {} errors",
            passed_count,
            results.len(),
            warning_count,
            error_count
        );

        ValidationReport {
            all_passed,
            total_entries: source_entries.len(),
            passed_count,
            warning_count,
            error_count,
            results,
            marker_result: None,
        }
    }

    /// Validate timecodes for a collection of entries
    pub fn validate_timecodes(&self, entries: &[SubtitleEntry]) -> TimecodeValidationResult {
        if !self.config.enabled || !self.config.timecode_validation {
            return TimecodeValidationResult {
                passed: true,
                entry_results: vec![],
                total_issues: 0,
                overlap_count: 0,
            };
        }

        self.timecode_validator.validate_collection(entries)
    }

    /// Validate format preservation for a collection
    pub fn validate_formats(
        &self,
        pairs: &[(usize, &str, &str)],
    ) -> FormatValidationResult {
        if !self.config.enabled || !self.config.format_validation {
            return FormatValidationResult {
                passed: true,
                entry_results: vec![],
                total_issues: 0,
            };
        }

        self.format_validator.validate_collection(pairs)
    }

    /// Validate lengths for a collection
    pub fn validate_lengths(
        &self,
        pairs: &[(usize, &str, &str)],
    ) -> LengthValidationResult {
        if !self.config.enabled || !self.config.length_validation {
            return LengthValidationResult {
                passed: true,
                entry_results: vec![],
                total_issues: 0,
                average_ratio: 1.0,
            };
        }

        self.length_validator.validate_collection(pairs)
    }

    /// Should retry based on validation result?
    pub fn should_retry(&self, result: &ValidationResult) -> bool {
        self.config.retry_on_validation_failure
            && result.action == ValidationAction::Retry
    }

    /// Get maximum retries from config
    pub fn max_retries(&self) -> u32 {
        self.config.max_retries
    }
}

impl Default for ValidationService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_entry(seq: usize, start: u64, end: u64, text: &str) -> SubtitleEntry {
        SubtitleEntry::new(seq, start, end, text.to_string())
    }

    #[test]
    fn test_validateEntry_withGoodTranslation_shouldPass() {
        let service = ValidationService::new();

        let source = create_entry(1, 0, 2000, "Hello World");
        let translated = create_entry(1, 0, 2000, "Bonjour le Monde");

        let result = service.validate_entry(&source, &translated);

        assert!(result.passed);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_validateEntry_withEmptyTranslation_shouldFail() {
        let service = ValidationService::new();

        let source = create_entry(1, 0, 2000, "Hello World");
        let translated = create_entry(1, 0, 2000, "");

        let result = service.validate_entry(&source, &translated);

        assert!(!result.passed);
        assert!(result.has_errors);
        assert_eq!(result.action, ValidationAction::Retry);
    }

    #[test]
    fn test_validateEntry_withMissingPositionTag_shouldWarn() {
        let service = ValidationService::new();

        let source = create_entry(1, 0, 2000, r"{\an8}Hello");
        let translated = create_entry(1, 0, 2000, "Bonjour"); // Missing position tag

        let result = service.validate_entry(&source, &translated);

        // Should have a warning about the missing position tag
        assert!(!result.issues.is_empty());
    }

    #[test]
    fn test_validateCollection_shouldValidateAll() {
        let service = ValidationService::new();

        let sources = vec![
            create_entry(1, 0, 2000, "Hello"),
            create_entry(2, 2000, 4000, "World"),
        ];

        let translated = vec![
            create_entry(1, 0, 2000, "Bonjour"),
            create_entry(2, 2000, 4000, "Monde"),
        ];

        let report = service.validate_collection(&sources, &translated);

        assert!(report.all_passed);
        assert_eq!(report.total_entries, 2);
        assert_eq!(report.passed_count, 2);
    }

    #[test]
    fn test_validateCollection_withMissingTranslation_shouldFail() {
        let service = ValidationService::new();

        let sources = vec![
            create_entry(1, 0, 2000, "Hello"),
            create_entry(2, 2000, 4000, "World"),
        ];

        let translated = vec![
            create_entry(1, 0, 2000, "Bonjour"),
            // Missing entry 2
        ];

        let report = service.validate_collection(&sources, &translated);

        assert!(!report.all_passed);
        assert_eq!(report.error_count, 1);
    }

    #[test]
    fn test_validateBatchMarkers_withAllPresent_shouldPass() {
        let service = ValidationService::new();

        let response = r#"<<ENTRY_0>>
First
<<ENTRY_1>>
Second
<<END>>"#;

        let result = service.validate_batch_markers(response, 2);

        assert!(result.passed());
    }

    #[test]
    fn test_validateBatchMarkers_withMissing_shouldFail() {
        let service = ValidationService::new();

        let response = r#"<<ENTRY_0>>
First
<<END>>"#;

        let result = service.validate_batch_markers(response, 2);

        assert!(!result.passed());
        assert_eq!(result.missing_indices, vec![1]);
    }

    #[test]
    fn test_disabledValidation_shouldAlwaysPass() {
        let config = ValidationConfig {
            enabled: false,
            ..Default::default()
        };
        let service = ValidationService::with_config(config);

        let source = create_entry(1, 0, 2000, "Hello");
        let translated = create_entry(1, 0, 2000, ""); // Empty - would normally fail

        let result = service.validate_entry(&source, &translated);

        assert!(result.passed);
    }

    #[test]
    fn test_report_entriesToRetry_shouldReturnFailedEntries() {
        let service = ValidationService::new();

        let sources = vec![
            create_entry(1, 0, 2000, "Hello"),
            create_entry(2, 2000, 4000, "World"),
        ];

        let translated = vec![
            create_entry(1, 0, 2000, "Bonjour"),
            create_entry(2, 2000, 4000, ""), // Empty - will fail
        ];

        let report = service.validate_collection(&sources, &translated);
        let to_retry = report.entries_to_retry();

        assert!(to_retry.contains(&2));
    }
}
