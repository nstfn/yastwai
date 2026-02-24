/*!
 * Validation pass for translation quality assurance.
 *
 * This pass validates translations and attempts auto-repair for common issues:
 * - Completeness: Ensure all entries are translated
 * - Length ratio: Check translated text length is reasonable
 * - Formatting: Ensure formatting tags are preserved
 * - Glossary consistency: Verify terminology is consistent
 * - Timecode integrity: Verify timecodes are unchanged
 */

use crate::translation::context::{ConsistencyIssue, GlossaryEnforcer};
use crate::translation::document::{DocumentEntry, FormattingTag, SubtitleDocument};
use crate::translation::quality::semantic::{SemanticIssue, SemanticValidationResult};

/// Configuration for the validation pass.
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Maximum allowed length ratio (translated / original)
    pub max_length_ratio: f32,

    /// Minimum allowed length ratio
    pub min_length_ratio: f32,

    /// Whether to check formatting preservation
    pub check_formatting: bool,

    /// Whether to check glossary consistency
    pub check_glossary_consistency: bool,

    /// Whether to attempt auto-repair
    pub enable_auto_repair: bool,

    /// Minimum confidence threshold (entries below are flagged)
    pub min_confidence_threshold: f32,

    /// Whether to run semantic validation (requires SemanticValidator)
    pub enable_semantic_validation: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_length_ratio: 1.5,
            min_length_ratio: 0.3,
            check_formatting: true,
            check_glossary_consistency: true,
            enable_auto_repair: true,
            min_confidence_threshold: 0.5,
            enable_semantic_validation: false,
        }
    }
}

impl ValidationConfig {
    /// Create a strict validation config.
    pub fn strict() -> Self {
        Self {
            max_length_ratio: 1.2,
            min_length_ratio: 0.5,
            check_formatting: true,
            check_glossary_consistency: true,
            enable_auto_repair: true,
            min_confidence_threshold: 0.7,
            enable_semantic_validation: true,
        }
    }

    /// Create a lenient validation config.
    pub fn lenient() -> Self {
        Self {
            max_length_ratio: 2.0,
            min_length_ratio: 0.2,
            check_formatting: true,
            check_glossary_consistency: false,
            enable_auto_repair: true,
            min_confidence_threshold: 0.3,
            enable_semantic_validation: false,
        }
    }
}

/// Types of validation issues.
#[derive(Debug, Clone)]
pub enum ValidationIssue {
    /// Entry is missing translation
    MissingTranslation {
        entry_id: usize,
    },

    /// Translation is too long
    LengthTooLong {
        entry_id: usize,
        original_length: usize,
        translated_length: usize,
        ratio: f32,
    },

    /// Translation is too short
    LengthTooShort {
        entry_id: usize,
        original_length: usize,
        translated_length: usize,
        ratio: f32,
    },

    /// Missing formatting tag
    MissingFormatting {
        entry_id: usize,
        tag: FormattingTag,
    },

    /// Glossary term used inconsistently
    GlossaryInconsistency {
        entry_id: usize,
        issue: ConsistencyIssue,
    },

    /// Low confidence translation
    LowConfidence {
        entry_id: usize,
        confidence: f32,
    },

    /// Empty translation for non-empty original
    EmptyTranslation {
        entry_id: usize,
    },

    /// Semantic divergence detected (meaning not preserved)
    SemanticDivergence {
        entry_id: usize,
        confidence: f32,
        issues: Vec<SemanticIssue>,
    },
}

/// Structured failure reason for feedback-informed retry
#[derive(Debug, Clone)]
pub enum FailureReason {
    /// Translation was too long for the subtitle timing
    TooLong {
        entry_id: usize,
        current_ratio: f32,
        max_ratio: f32,
        suggestion: String,
    },
    /// Translation was too short (potential truncation)
    TooShort {
        entry_id: usize,
        current_ratio: f32,
        min_ratio: f32,
        suggestion: String,
    },
    /// Required glossary term was not used
    MissingGlossaryTerm {
        entry_id: usize,
        term: String,
        expected_translation: String,
    },
    /// Character name was altered/translated
    AlteredCharacterName {
        entry_id: usize,
        name: String,
    },
    /// Formatting tag was dropped
    DroppedFormatting {
        entry_id: usize,
        tag: String,
    },
    /// Translation confidence is low
    LowConfidence {
        entry_id: usize,
        confidence: f32,
    },
    /// Semantic meaning was not preserved
    SemanticDivergence {
        entry_id: usize,
        issue_description: String,
    },
}

impl FailureReason {
    /// Get the entry ID for this failure
    pub fn entry_id(&self) -> usize {
        match self {
            FailureReason::TooLong { entry_id, .. } => *entry_id,
            FailureReason::TooShort { entry_id, .. } => *entry_id,
            FailureReason::MissingGlossaryTerm { entry_id, .. } => *entry_id,
            FailureReason::AlteredCharacterName { entry_id, .. } => *entry_id,
            FailureReason::DroppedFormatting { entry_id, .. } => *entry_id,
            FailureReason::LowConfidence { entry_id, .. } => *entry_id,
            FailureReason::SemanticDivergence { entry_id, .. } => *entry_id,
        }
    }

    /// Convert to feedback instruction for retry prompt
    pub fn to_feedback_instruction(&self) -> String {
        match self {
            FailureReason::TooLong { suggestion, .. } => suggestion.clone(),
            FailureReason::TooShort { suggestion, .. } => suggestion.clone(),
            FailureReason::MissingGlossaryTerm { term, expected_translation, .. } => {
                format!("Use '{}' for '{}'", expected_translation, term)
            }
            FailureReason::AlteredCharacterName { name, .. } => {
                format!("Keep character name '{}' unchanged", name)
            }
            FailureReason::DroppedFormatting { tag, .. } => {
                format!("Preserve {} formatting tags", tag)
            }
            FailureReason::LowConfidence { .. } => {
                "Provide a more confident translation".to_string()
            }
            FailureReason::SemanticDivergence { issue_description, .. } => {
                format!("Fix semantic issue: {}", issue_description)
            }
        }
    }
}

impl ValidationIssue {
    /// Get the entry ID associated with this issue.
    pub fn entry_id(&self) -> usize {
        match self {
            ValidationIssue::MissingTranslation { entry_id } => *entry_id,
            ValidationIssue::LengthTooLong { entry_id, .. } => *entry_id,
            ValidationIssue::LengthTooShort { entry_id, .. } => *entry_id,
            ValidationIssue::MissingFormatting { entry_id, .. } => *entry_id,
            ValidationIssue::GlossaryInconsistency { entry_id, .. } => *entry_id,
            ValidationIssue::LowConfidence { entry_id, .. } => *entry_id,
            ValidationIssue::EmptyTranslation { entry_id } => *entry_id,
            ValidationIssue::SemanticDivergence { entry_id, .. } => *entry_id,
        }
    }

    /// Get a human-readable description of the issue.
    pub fn description(&self) -> String {
        match self {
            ValidationIssue::MissingTranslation { entry_id } => {
                format!("Entry {} is missing translation", entry_id)
            }
            ValidationIssue::LengthTooLong { entry_id, ratio, .. } => {
                format!("Entry {} translation too long (ratio: {:.2})", entry_id, ratio)
            }
            ValidationIssue::LengthTooShort { entry_id, ratio, .. } => {
                format!("Entry {} translation too short (ratio: {:.2})", entry_id, ratio)
            }
            ValidationIssue::MissingFormatting { entry_id, tag } => {
                format!("Entry {} missing {:?} formatting", entry_id, tag)
            }
            ValidationIssue::GlossaryInconsistency { entry_id, issue } => {
                format!("Entry {}: {}", entry_id, issue.description())
            }
            ValidationIssue::LowConfidence { entry_id, confidence } => {
                format!("Entry {} has low confidence ({:.2})", entry_id, confidence)
            }
            ValidationIssue::EmptyTranslation { entry_id } => {
                format!("Entry {} has empty translation", entry_id)
            }
            ValidationIssue::SemanticDivergence { entry_id, confidence, issues } => {
                let issue_count = issues.len();
                format!(
                    "Entry {} semantic divergence (confidence: {:.2}, {} issues)",
                    entry_id, confidence, issue_count
                )
            }
        }
    }

    /// Get the severity of the issue (0.0 = minor, 1.0 = critical).
    pub fn severity(&self) -> f32 {
        match self {
            ValidationIssue::MissingTranslation { .. } => 1.0,
            ValidationIssue::EmptyTranslation { .. } => 1.0,
            ValidationIssue::LengthTooLong { ratio, .. } => {
                (*ratio - 1.5).clamp(0.3, 1.0) // Scale based on how extreme
            }
            ValidationIssue::LengthTooShort { ratio, .. } => {
                (0.5 - *ratio).clamp(0.3, 1.0)
            }
            ValidationIssue::MissingFormatting { .. } => 0.5,
            ValidationIssue::GlossaryInconsistency { .. } => 0.4,
            ValidationIssue::LowConfidence { confidence, .. } => 1.0 - confidence,
            ValidationIssue::SemanticDivergence { confidence, .. } => {
                // Semantic divergence is serious - base severity of 0.8 scaled by confidence
                // Lower confidence = higher severity (closer to 1.0)
                0.8 + (1.0 - confidence) * 0.2
            }
        }
    }

    /// Check if this issue can be auto-repaired.
    pub fn is_repairable(&self) -> bool {
        matches!(
            self,
            ValidationIssue::MissingFormatting { .. } | ValidationIssue::GlossaryInconsistency { .. }
        )
    }

    /// Convert to a structured failure reason for feedback retry
    pub fn to_failure_reason(&self, max_ratio: f32, min_ratio: f32) -> Option<FailureReason> {
        match self {
            ValidationIssue::LengthTooLong { entry_id, ratio, .. } => {
                Some(FailureReason::TooLong {
                    entry_id: *entry_id,
                    current_ratio: *ratio,
                    max_ratio,
                    suggestion: format!(
                        "Translation is {:.0}% longer than original. Shorten to under {:.0}%",
                        ratio * 100.0,
                        max_ratio * 100.0
                    ),
                })
            }
            ValidationIssue::LengthTooShort { entry_id, ratio, .. } => {
                Some(FailureReason::TooShort {
                    entry_id: *entry_id,
                    current_ratio: *ratio,
                    min_ratio,
                    suggestion: format!(
                        "Translation is only {:.0}% of original length. Expand to at least {:.0}%",
                        ratio * 100.0,
                        min_ratio * 100.0
                    ),
                })
            }
            ValidationIssue::GlossaryInconsistency { entry_id, issue } => {
                match issue {
                    ConsistencyIssue::MissingName { name, .. } => {
                        Some(FailureReason::AlteredCharacterName {
                            entry_id: *entry_id,
                            name: name.clone(),
                        })
                    }
                    ConsistencyIssue::InconsistentTerm { source, expected, .. } => {
                        Some(FailureReason::MissingGlossaryTerm {
                            entry_id: *entry_id,
                            term: source.clone(),
                            expected_translation: expected.clone(),
                        })
                    }
                }
            }
            ValidationIssue::MissingFormatting { entry_id, tag } => {
                Some(FailureReason::DroppedFormatting {
                    entry_id: *entry_id,
                    tag: format!("{:?}", tag),
                })
            }
            ValidationIssue::LowConfidence { entry_id, confidence } => {
                Some(FailureReason::LowConfidence {
                    entry_id: *entry_id,
                    confidence: *confidence,
                })
            }
            ValidationIssue::SemanticDivergence { entry_id, issues, .. } => {
                let description = if issues.is_empty() {
                    "Translation may not preserve the original meaning".to_string()
                } else {
                    issues.first().map(|i| i.description()).unwrap_or_default()
                };
                Some(FailureReason::SemanticDivergence {
                    entry_id: *entry_id,
                    issue_description: description,
                })
            }
            // Non-retryable issues
            ValidationIssue::MissingTranslation { .. } | ValidationIssue::EmptyTranslation { .. } => None,
        }
    }
}

/// Repair action taken during auto-repair.
#[derive(Debug, Clone)]
pub enum RepairAction {
    /// Added missing formatting tag
    AddedFormatting {
        entry_id: usize,
        tag: FormattingTag,
    },

    /// Applied glossary correction
    AppliedGlossaryCorrection {
        entry_id: usize,
        before: String,
        after: String,
    },

    /// No repair possible
    NoRepairPossible {
        entry_id: usize,
        reason: String,
    },
}

impl RepairAction {
    /// Get the entry ID associated with this action.
    pub fn entry_id(&self) -> usize {
        match self {
            RepairAction::AddedFormatting { entry_id, .. } => *entry_id,
            RepairAction::AppliedGlossaryCorrection { entry_id, .. } => *entry_id,
            RepairAction::NoRepairPossible { entry_id, .. } => *entry_id,
        }
    }

    /// Get a description of the action.
    pub fn description(&self) -> String {
        match self {
            RepairAction::AddedFormatting { entry_id, tag } => {
                format!("Added {:?} formatting to entry {}", tag, entry_id)
            }
            RepairAction::AppliedGlossaryCorrection { entry_id, before, after } => {
                format!("Entry {}: '{}' -> '{}'", entry_id, before, after)
            }
            RepairAction::NoRepairPossible { entry_id, reason } => {
                format!("Entry {}: {}", entry_id, reason)
            }
        }
    }
}

/// Result of auto-repair attempt.
#[derive(Debug, Clone)]
pub struct RepairResult {
    /// Whether the repair was successful
    pub success: bool,

    /// Actions taken during repair
    pub actions: Vec<RepairAction>,

    /// Issues that could not be repaired
    pub unresolved_issues: Vec<ValidationIssue>,
}

impl RepairResult {
    /// Create a new repair result.
    pub fn new() -> Self {
        Self {
            success: true,
            actions: Vec::new(),
            unresolved_issues: Vec::new(),
        }
    }

    /// Add an action to the result.
    pub fn add_action(&mut self, action: RepairAction) {
        self.actions.push(action);
    }

    /// Add an unresolved issue.
    pub fn add_unresolved(&mut self, issue: ValidationIssue) {
        self.success = false;
        self.unresolved_issues.push(issue);
    }
}

impl Default for RepairResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Validation report containing all issues found.
#[derive(Debug, Clone)]
pub struct ValidationReport {
    /// All issues found during validation
    pub issues: Vec<ValidationIssue>,

    /// Number of entries validated
    pub entries_validated: usize,

    /// Number of entries with issues
    pub entries_with_issues: usize,

    /// Overall quality score (0.0 - 1.0)
    pub quality_score: f32,

    /// Repair result (if auto-repair was attempted)
    pub repair_result: Option<RepairResult>,
}

impl ValidationReport {
    /// Create an empty validation report.
    pub fn new(entries_validated: usize) -> Self {
        Self {
            issues: Vec::new(),
            entries_validated,
            entries_with_issues: 0,
            quality_score: 1.0,
            repair_result: None,
        }
    }

    /// Add an issue to the report.
    pub fn add_issue(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }

    /// Calculate the quality score based on issues.
    pub fn calculate_score(&mut self) {
        if self.entries_validated == 0 {
            self.quality_score = 1.0;
            return;
        }

        let total_severity: f32 = self.issues.iter().map(|i| i.severity()).sum();
        let max_severity = self.entries_validated as f32;

        self.quality_score = (1.0 - total_severity / max_severity).max(0.0);

        // Count unique entries with issues
        let mut entry_ids: Vec<usize> = self.issues.iter().map(|i| i.entry_id()).collect();
        entry_ids.sort_unstable();
        entry_ids.dedup();
        self.entries_with_issues = entry_ids.len();
    }

    /// Check if the document passed validation.
    pub fn passed(&self) -> bool {
        self.critical_issues().is_empty()
    }

    /// Get only critical issues (severity >= 0.8).
    pub fn critical_issues(&self) -> Vec<&ValidationIssue> {
        self.issues.iter().filter(|i| i.severity() >= 0.8).collect()
    }

    /// Get a summary of the report.
    pub fn summary(&self) -> String {
        format!(
            "Validated {} entries: {} issues found, {} entries affected, quality score: {:.2}%",
            self.entries_validated,
            self.issues.len(),
            self.entries_with_issues,
            self.quality_score * 100.0
        )
    }

    /// Export structured failure reasons for feedback-informed retry
    ///
    /// Returns a list of failure reasons that can be used to construct
    /// better retry prompts with specific feedback.
    pub fn export_failure_reasons(&self, max_ratio: f32, min_ratio: f32) -> Vec<FailureReason> {
        self.issues
            .iter()
            .filter_map(|issue| issue.to_failure_reason(max_ratio, min_ratio))
            .collect()
    }

    /// Get failure reasons grouped by entry ID
    pub fn failure_reasons_by_entry(&self, max_ratio: f32, min_ratio: f32) -> std::collections::HashMap<usize, Vec<FailureReason>> {
        let mut by_entry = std::collections::HashMap::new();
        for reason in self.export_failure_reasons(max_ratio, min_ratio) {
            by_entry
                .entry(reason.entry_id())
                .or_insert_with(Vec::new)
                .push(reason);
        }
        by_entry
    }

    /// Add semantic validation result to this report.
    ///
    /// This method allows incorporating pre-computed semantic validation results
    /// (from SemanticValidator) into the validation report.
    pub fn add_semantic_result(&mut self, entry_id: usize, result: SemanticValidationResult) {
        if !result.passed() {
            self.add_issue(ValidationIssue::SemanticDivergence {
                entry_id,
                confidence: result.confidence,
                issues: result.issues,
            });
        }
    }

    /// Check if semantic validation is recommended for this report.
    ///
    /// Returns true if there are entries with low confidence that would
    /// benefit from semantic validation.
    pub fn needs_semantic_validation(&self, threshold: f32) -> bool {
        self.issues.iter().any(|issue| {
            matches!(
                issue,
                ValidationIssue::LowConfidence { confidence, .. } if *confidence < threshold
            )
        })
    }
}

/// Validation pass for checking translation quality.
pub struct ValidationPass {
    config: ValidationConfig,
}

impl ValidationPass {
    /// Create a new validation pass with the given configuration.
    pub fn new(config: ValidationConfig) -> Self {
        Self { config }
    }

    /// Create a validation pass with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ValidationConfig::default())
    }

    /// Get the validation configuration.
    pub fn config(&self) -> &ValidationConfig {
        &self.config
    }

    /// Check if semantic validation is enabled.
    pub fn semantic_validation_enabled(&self) -> bool {
        self.config.enable_semantic_validation
    }

    /// Validate a document and return a report.
    pub fn validate(&self, doc: &SubtitleDocument) -> ValidationReport {
        let mut report = ValidationReport::new(doc.entries.len());

        for entry in &doc.entries {
            self.validate_entry(entry, doc, &mut report);
        }

        report.calculate_score();
        report
    }

    /// Validate a document and attempt auto-repair.
    pub fn validate_and_repair(&self, doc: &mut SubtitleDocument) -> ValidationReport {
        let mut report = self.validate(doc);

        if self.config.enable_auto_repair && !report.issues.is_empty() {
            let repair_result = self.auto_repair(doc, &report.issues);
            report.repair_result = Some(repair_result);

            // Re-validate after repair
            let post_repair_report = self.validate(doc);
            report.issues = post_repair_report.issues;
            report.calculate_score();
        }

        report
    }

    /// Validate a single entry.
    fn validate_entry(
        &self,
        entry: &DocumentEntry,
        doc: &SubtitleDocument,
        report: &mut ValidationReport,
    ) {
        // Check for missing translation
        if entry.translated_text.is_none() {
            report.add_issue(ValidationIssue::MissingTranslation { entry_id: entry.id });
            return; // No point checking other issues for untranslated entry
        }

        let translated = entry.translated_text.as_ref().unwrap();
        let original = &entry.original_text;

        // Check for empty translation
        if translated.trim().is_empty() && !original.trim().is_empty() {
            report.add_issue(ValidationIssue::EmptyTranslation { entry_id: entry.id });
            return;
        }

        // Check length ratio
        if !original.is_empty() {
            let ratio = translated.len() as f32 / original.len() as f32;

            if ratio > self.config.max_length_ratio {
                report.add_issue(ValidationIssue::LengthTooLong {
                    entry_id: entry.id,
                    original_length: original.len(),
                    translated_length: translated.len(),
                    ratio,
                });
            } else if ratio < self.config.min_length_ratio {
                report.add_issue(ValidationIssue::LengthTooShort {
                    entry_id: entry.id,
                    original_length: original.len(),
                    translated_length: translated.len(),
                    ratio,
                });
            }
        }

        // Check formatting preservation
        if self.config.check_formatting {
            self.check_formatting(entry, report);
        }

        // Check glossary consistency
        if self.config.check_glossary_consistency {
            self.check_glossary_consistency(entry, doc, report);
        }

        // Check confidence threshold
        if let Some(confidence) = entry.confidence {
            if confidence < self.config.min_confidence_threshold {
                report.add_issue(ValidationIssue::LowConfidence {
                    entry_id: entry.id,
                    confidence,
                });
            }
        }
    }

    /// Check formatting preservation for an entry.
    fn check_formatting(&self, entry: &DocumentEntry, report: &mut ValidationReport) {
        let translated = match &entry.translated_text {
            Some(t) => t,
            None => return,
        };

        // Check each formatting tag
        for tag in &entry.formatting {
            let tag_present = match tag {
                FormattingTag::Italic => translated.contains("<i>") || translated.contains("</i>"),
                FormattingTag::Bold => translated.contains("<b>") || translated.contains("</b>"),
                FormattingTag::Underline => {
                    translated.contains("<u>") || translated.contains("</u>")
                }
                FormattingTag::Position => translated.contains("{\\an"),
                FormattingTag::Color => {
                    translated.contains("<font") || translated.contains("</font>")
                }
            };

            if !tag_present {
                report.add_issue(ValidationIssue::MissingFormatting {
                    entry_id: entry.id,
                    tag: *tag,
                });
            }
        }
    }

    /// Check glossary consistency for an entry.
    fn check_glossary_consistency(
        &self,
        entry: &DocumentEntry,
        doc: &SubtitleDocument,
        report: &mut ValidationReport,
    ) {
        let translated = match &entry.translated_text {
            Some(t) => t,
            None => return,
        };

        let enforcer = GlossaryEnforcer::new(&doc.glossary);
        let issues = enforcer.check_consistency(&entry.original_text, translated);

        for issue in issues {
            report.add_issue(ValidationIssue::GlossaryInconsistency {
                entry_id: entry.id,
                issue,
            });
        }
    }

    /// Attempt to auto-repair issues.
    fn auto_repair(&self, doc: &mut SubtitleDocument, issues: &[ValidationIssue]) -> RepairResult {
        let mut result = RepairResult::new();

        for issue in issues {
            match issue {
                ValidationIssue::MissingFormatting { entry_id, tag } => {
                    if let Some(entry) = doc.entries.iter_mut().find(|e| e.id == *entry_id) {
                        if let Some(ref mut translated) = entry.translated_text {
                            let repaired = self.repair_formatting(translated, *tag, &entry.original_text);
                            if repaired != *translated {
                                *translated = repaired;
                                result.add_action(RepairAction::AddedFormatting {
                                    entry_id: *entry_id,
                                    tag: *tag,
                                });
                            } else {
                                result.add_action(RepairAction::NoRepairPossible {
                                    entry_id: *entry_id,
                                    reason: "Could not determine formatting placement".to_string(),
                                });
                            }
                        }
                    }
                }
                ValidationIssue::GlossaryInconsistency { entry_id, .. } => {
                    if let Some(entry) = doc.entries.iter_mut().find(|e| e.id == *entry_id) {
                        if let Some(ref mut translated) = entry.translated_text {
                            let enforcer = GlossaryEnforcer::new(&doc.glossary);
                            let before = translated.clone();
                            let repaired = enforcer.enforce(&entry.original_text, translated);
                            if repaired != before {
                                *translated = repaired.clone();
                                result.add_action(RepairAction::AppliedGlossaryCorrection {
                                    entry_id: *entry_id,
                                    before,
                                    after: repaired,
                                });
                            }
                        }
                    }
                }
                _ => {
                    // Non-repairable issues
                    result.add_unresolved(issue.clone());
                }
            }
        }

        result
    }

    /// Attempt to repair missing formatting by adding tags.
    fn repair_formatting(&self, translated: &str, tag: FormattingTag, original: &str) -> String {
        match tag {
            FormattingTag::Italic => {
                // If original is fully italicized, wrap translation
                if original.starts_with("<i>") && original.ends_with("</i>") {
                    format!("<i>{}</i>", translated)
                } else {
                    translated.to_string()
                }
            }
            FormattingTag::Bold => {
                if original.starts_with("<b>") && original.ends_with("</b>") {
                    format!("<b>{}</b>", translated)
                } else {
                    translated.to_string()
                }
            }
            FormattingTag::Underline => {
                if original.starts_with("<u>") && original.ends_with("</u>") {
                    format!("<u>{}</u>", translated)
                } else {
                    translated.to_string()
                }
            }
            FormattingTag::Position => {
                // Extract position tag from original and prepend
                if let Some(start) = original.find("{\\an") {
                    if let Some(end) = original[start..].find('}') {
                        let pos_tag = &original[start..start + end + 1];
                        if !translated.contains(pos_tag) {
                            return format!("{}{}", pos_tag, translated);
                        }
                    }
                }
                translated.to_string()
            }
            FormattingTag::Color => {
                // Color tags are complex, don't auto-repair
                translated.to_string()
            }
        }
    }
}

impl Default for ValidationPass {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitle_processor::SubtitleEntry;

    fn create_test_document(entries: Vec<(&str, Option<&str>)>) -> SubtitleDocument {
        let subtitle_entries: Vec<SubtitleEntry> = entries
            .iter()
            .enumerate()
            .map(|(i, (text, _))| {
                SubtitleEntry::new(i + 1, (i as u64) * 1000, (i as u64 + 1) * 1000, text.to_string())
            })
            .collect();

        let mut doc = SubtitleDocument::from_entries(subtitle_entries, "en");

        // Set translations
        for (i, (_, translation)) in entries.iter().enumerate() {
            if let Some(t) = translation {
                doc.entries[i].set_translation(t.to_string(), Some(0.9));
            }
        }

        doc
    }

    #[test]
    fn test_validationPass_validate_shouldDetectMissingTranslations() {
        let doc = create_test_document(vec![
            ("Hello", Some("Bonjour")),
            ("World", None), // Missing translation
            ("Test", Some("Test")),
        ]);

        let pass = ValidationPass::with_defaults();
        let report = pass.validate(&doc);

        assert!(!report.passed());
        assert!(report.issues.iter().any(|i| matches!(
            i,
            ValidationIssue::MissingTranslation { entry_id: 2 }
        )));
    }

    #[test]
    fn test_validationPass_validate_shouldDetectLengthIssues() {
        let doc = create_test_document(vec![
            ("Hi", Some("Bonjour, comment allez-vous aujourd'hui?")), // Too long
            ("Hello world", Some("Hi")), // Too short
        ]);

        let pass = ValidationPass::new(ValidationConfig {
            max_length_ratio: 1.5,
            min_length_ratio: 0.5,
            ..Default::default()
        });

        let report = pass.validate(&doc);

        assert!(report.issues.iter().any(|i| matches!(
            i,
            ValidationIssue::LengthTooLong { entry_id: 1, .. }
        )));
        assert!(report.issues.iter().any(|i| matches!(
            i,
            ValidationIssue::LengthTooShort { entry_id: 2, .. }
        )));
    }

    #[test]
    fn test_validationPass_validate_shouldDetectMissingFormatting() {
        let subtitle_entries = vec![SubtitleEntry::new(
            1,
            0,
            1000,
            "<i>Whispered text</i>".to_string(),
        )];

        let mut doc = SubtitleDocument::from_entries(subtitle_entries, "en");
        doc.entries[0].set_translation("Texte chuchoté".to_string(), Some(0.9)); // Missing <i> tags

        let pass = ValidationPass::with_defaults();
        let report = pass.validate(&doc);

        assert!(report.issues.iter().any(|i| matches!(
            i,
            ValidationIssue::MissingFormatting {
                entry_id: 1,
                tag: FormattingTag::Italic
            }
        )));
    }

    #[test]
    fn test_validationPass_validateAndRepair_shouldFixFormatting() {
        let subtitle_entries = vec![SubtitleEntry::new(
            1,
            0,
            1000,
            "<i>Whispered text</i>".to_string(),
        )];

        let mut doc = SubtitleDocument::from_entries(subtitle_entries, "en");
        doc.entries[0].set_translation("Texte chuchoté".to_string(), Some(0.9));

        let pass = ValidationPass::with_defaults();
        let report = pass.validate_and_repair(&mut doc);

        // Check that formatting was repaired
        let translated = doc.entries[0].translated_text.as_ref().unwrap();
        assert!(translated.contains("<i>"));
        assert!(translated.contains("</i>"));
        assert!(report.repair_result.is_some());
    }

    #[test]
    fn test_validationReport_qualityScore_shouldCalculateCorrectly() {
        let mut report = ValidationReport::new(10);

        // Add some issues
        report.add_issue(ValidationIssue::MissingTranslation { entry_id: 1 });
        report.add_issue(ValidationIssue::LowConfidence {
            entry_id: 2,
            confidence: 0.3,
        });

        report.calculate_score();

        assert!(report.quality_score < 1.0);
        assert!(report.quality_score > 0.0);
        assert_eq!(report.entries_with_issues, 2);
    }

    #[test]
    fn test_validationIssue_severity_shouldRankCorrectly() {
        let critical = ValidationIssue::MissingTranslation { entry_id: 1 };
        let minor = ValidationIssue::LowConfidence {
            entry_id: 2,
            confidence: 0.6,
        };

        assert!(critical.severity() > minor.severity());
    }

    #[test]
    fn test_validationReport_passed_shouldCheckCriticalIssues() {
        let mut report = ValidationReport::new(5);

        // Only add a minor issue
        report.add_issue(ValidationIssue::LowConfidence {
            entry_id: 1,
            confidence: 0.6,
        });
        report.calculate_score();

        assert!(report.passed()); // Should pass with only minor issues

        // Add a critical issue
        report.add_issue(ValidationIssue::MissingTranslation { entry_id: 2 });
        report.calculate_score();

        assert!(!report.passed()); // Should fail with critical issues
    }

    #[test]
    fn test_validationIssue_toFailureReason_shouldConvertLengthTooLong() {
        let issue = ValidationIssue::LengthTooLong {
            entry_id: 1,
            original_length: 10,
            translated_length: 20,
            ratio: 2.0,
        };

        let reason = issue.to_failure_reason(1.5, 0.5).unwrap();
        match reason {
            FailureReason::TooLong { entry_id, current_ratio, max_ratio, .. } => {
                assert_eq!(entry_id, 1);
                assert!((current_ratio - 2.0).abs() < 0.01);
                assert!((max_ratio - 1.5).abs() < 0.01);
            }
            _ => panic!("Expected TooLong failure reason"),
        }
    }

    #[test]
    fn test_validationIssue_toFailureReason_missingTranslation_shouldReturnNone() {
        let issue = ValidationIssue::MissingTranslation { entry_id: 1 };
        assert!(issue.to_failure_reason(1.5, 0.5).is_none());
    }

    #[test]
    fn test_failureReason_toFeedbackInstruction_shouldGenerateUsefulText() {
        let reason = FailureReason::TooLong {
            entry_id: 1,
            current_ratio: 2.0,
            max_ratio: 1.5,
            suggestion: "Translation too long".to_string(),
        };

        let instruction = reason.to_feedback_instruction();
        assert_eq!(instruction, "Translation too long");
    }

    #[test]
    fn test_validationReport_exportFailureReasons_shouldFilterRetryable() {
        let mut report = ValidationReport::new(5);

        // Add retryable issues
        report.add_issue(ValidationIssue::LengthTooLong {
            entry_id: 1,
            original_length: 10,
            translated_length: 20,
            ratio: 2.0,
        });
        report.add_issue(ValidationIssue::LowConfidence {
            entry_id: 2,
            confidence: 0.3,
        });

        // Add non-retryable issues
        report.add_issue(ValidationIssue::MissingTranslation { entry_id: 3 });

        let reasons = report.export_failure_reasons(1.5, 0.5);

        // Should have 2 retryable reasons (LengthTooLong and LowConfidence)
        // MissingTranslation should be filtered out
        assert_eq!(reasons.len(), 2);
    }

    #[test]
    fn test_validationReport_failureReasonsByEntry_shouldGroupCorrectly() {
        let mut report = ValidationReport::new(5);

        report.add_issue(ValidationIssue::LengthTooLong {
            entry_id: 1,
            original_length: 10,
            translated_length: 20,
            ratio: 2.0,
        });
        report.add_issue(ValidationIssue::LowConfidence {
            entry_id: 1,
            confidence: 0.3,
        });
        report.add_issue(ValidationIssue::LengthTooShort {
            entry_id: 2,
            original_length: 20,
            translated_length: 5,
            ratio: 0.25,
        });

        let by_entry = report.failure_reasons_by_entry(1.5, 0.5);

        assert_eq!(by_entry.get(&1).map(|v| v.len()), Some(2));
        assert_eq!(by_entry.get(&2).map(|v| v.len()), Some(1));
    }

    #[test]
    fn test_semanticDivergence_shouldBeReportedAsIssue() {
        let mut report = ValidationReport::new(3);

        let issues = vec![SemanticIssue::MeaningChanged {
            original_meaning: "Hello".to_string(),
            translated_meaning: "Goodbye".to_string(),
        }];

        report.add_issue(ValidationIssue::SemanticDivergence {
            entry_id: 1,
            confidence: 0.4,
            issues,
        });
        report.calculate_score();

        assert!(!report.passed()); // Semantic divergence is critical
        assert_eq!(report.issues.len(), 1);
        assert!(report.issues[0].description().contains("semantic divergence"));
    }

    #[test]
    fn test_validationReport_addSemanticResult_shouldAddDivergentResults() {
        let mut report = ValidationReport::new(3);

        let result = SemanticValidationResult::divergent(
            0.4,
            vec![SemanticIssue::InformationOmitted {
                omitted_content: "important detail".to_string(),
            }],
        );

        report.add_semantic_result(1, result);

        assert_eq!(report.issues.len(), 1);
        match &report.issues[0] {
            ValidationIssue::SemanticDivergence { entry_id, confidence, issues } => {
                assert_eq!(*entry_id, 1);
                assert!(*confidence < 0.5);
                assert_eq!(issues.len(), 1);
            }
            _ => panic!("Expected SemanticDivergence"),
        }
    }

    #[test]
    fn test_validationReport_addSemanticResult_shouldNotAddPassedResults() {
        let mut report = ValidationReport::new(3);

        let result = SemanticValidationResult::equivalent(0.95);

        report.add_semantic_result(1, result);

        assert_eq!(report.issues.len(), 0); // Passed results not added
    }

    #[test]
    fn test_validationReport_needsSemanticValidation_shouldDetectLowConfidence() {
        let mut report = ValidationReport::new(3);

        report.add_issue(ValidationIssue::LowConfidence {
            entry_id: 1,
            confidence: 0.4,
        });

        assert!(report.needs_semantic_validation(0.5));
        assert!(!report.needs_semantic_validation(0.3));
    }

    #[test]
    fn test_semanticDivergence_toFailureReason_shouldConvert() {
        let issue = ValidationIssue::SemanticDivergence {
            entry_id: 1,
            confidence: 0.3,
            issues: vec![SemanticIssue::MeaningChanged {
                original_meaning: "hello".to_string(),
                translated_meaning: "world".to_string(),
            }],
        };

        let reason = issue.to_failure_reason(1.5, 0.5).unwrap();
        match reason {
            FailureReason::SemanticDivergence { entry_id, issue_description } => {
                assert_eq!(entry_id, 1);
                assert!(!issue_description.is_empty());
            }
            _ => panic!("Expected SemanticDivergence failure reason"),
        }
    }

    #[test]
    fn test_validationConfig_strict_shouldEnableSemanticValidation() {
        let config = ValidationConfig::strict();
        assert!(config.enable_semantic_validation);
    }

    #[test]
    fn test_validationConfig_lenient_shouldDisableSemanticValidation() {
        let config = ValidationConfig::lenient();
        assert!(!config.enable_semantic_validation);
    }

    #[test]
    fn test_validationPass_semanticValidationEnabled_shouldReflectConfig() {
        let pass = ValidationPass::new(ValidationConfig::strict());
        assert!(pass.semantic_validation_enabled());

        let pass_lenient = ValidationPass::new(ValidationConfig::lenient());
        assert!(!pass_lenient.semantic_validation_enabled());
    }
}
