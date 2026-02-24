/*!
 * Quality metrics for translation assessment.
 *
 * Provides quantitative scoring across multiple dimensions:
 * - Completeness: All entries translated
 * - Accuracy: Length ratios within bounds
 * - Consistency: Terminology used correctly
 * - Formatting: Tags and styles preserved
 * - Readability: Characters per second, line lengths
 */

use std::collections::HashMap;

/// Quality score for a single dimension.
#[derive(Debug, Clone, Copy)]
pub struct DimensionScore {
    /// Score value (0.0 - 1.0)
    pub score: f32,
    /// Weight for overall calculation
    pub weight: f32,
    /// Number of issues found
    pub issues: usize,
}

impl DimensionScore {
    /// Create a new dimension score.
    pub fn new(score: f32, weight: f32, issues: usize) -> Self {
        Self {
            score: score.clamp(0.0, 1.0),
            weight,
            issues,
        }
    }

    /// Create a perfect score.
    pub fn perfect(weight: f32) -> Self {
        Self::new(1.0, weight, 0)
    }

    /// Calculate weighted contribution to overall score.
    pub fn weighted(&self) -> f32 {
        self.score * self.weight
    }
}

/// Overall quality score with breakdown by dimension.
#[derive(Debug, Clone)]
pub struct QualityScore {
    /// Overall weighted score (0.0 - 1.0)
    pub overall: f32,

    /// Completeness score (all entries translated)
    pub completeness: DimensionScore,

    /// Accuracy score (length ratios reasonable)
    pub accuracy: DimensionScore,

    /// Consistency score (terminology correct)
    pub consistency: DimensionScore,

    /// Formatting score (tags preserved)
    pub formatting: DimensionScore,

    /// Readability score (CPS, line length)
    pub readability: DimensionScore,

    /// Number of entries evaluated
    pub entries_evaluated: usize,

    /// Number of entries with issues
    pub entries_with_issues: usize,
}

impl QualityScore {
    /// Create a new quality score from dimension scores.
    pub fn from_dimensions(
        completeness: DimensionScore,
        accuracy: DimensionScore,
        consistency: DimensionScore,
        formatting: DimensionScore,
        readability: DimensionScore,
        entries_evaluated: usize,
        entries_with_issues: usize,
    ) -> Self {
        let total_weight =
            completeness.weight + accuracy.weight + consistency.weight + formatting.weight + readability.weight;

        let overall = if total_weight > 0.0 {
            (completeness.weighted()
                + accuracy.weighted()
                + consistency.weighted()
                + formatting.weighted()
                + readability.weighted())
                / total_weight
        } else {
            1.0
        };

        Self {
            overall,
            completeness,
            accuracy,
            consistency,
            formatting,
            readability,
            entries_evaluated,
            entries_with_issues,
        }
    }

    /// Check if score meets minimum threshold.
    pub fn meets_threshold(&self, threshold: f32) -> bool {
        self.overall >= threshold
    }

    /// Get the lowest scoring dimension.
    pub fn weakest_dimension(&self) -> &'static str {
        let scores = [
            (self.completeness.score, "completeness"),
            (self.accuracy.score, "accuracy"),
            (self.consistency.score, "consistency"),
            (self.formatting.score, "formatting"),
            (self.readability.score, "readability"),
        ];

        scores
            .iter()
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, name)| *name)
            .unwrap_or("unknown")
    }

    /// Get a grade letter (A-F) based on overall score.
    pub fn grade(&self) -> char {
        match self.overall {
            s if s >= 0.9 => 'A',
            s if s >= 0.8 => 'B',
            s if s >= 0.7 => 'C',
            s if s >= 0.6 => 'D',
            _ => 'F',
        }
    }

    /// Get a human-readable summary.
    pub fn summary(&self) -> String {
        format!(
            "Quality: {:.1}% (Grade: {}) - {} entries, {} with issues",
            self.overall * 100.0,
            self.grade(),
            self.entries_evaluated,
            self.entries_with_issues
        )
    }
}

impl Default for QualityScore {
    fn default() -> Self {
        Self::from_dimensions(
            DimensionScore::perfect(0.3),
            DimensionScore::perfect(0.25),
            DimensionScore::perfect(0.2),
            DimensionScore::perfect(0.15),
            DimensionScore::perfect(0.1),
            0,
            0,
        )
    }
}

/// Thresholds for quality assessment.
#[derive(Debug, Clone)]
pub struct QualityThresholds {
    /// Minimum overall score to pass (default: 0.7)
    pub min_overall: f32,

    /// Maximum length ratio (translated/source)
    pub max_length_ratio: f32,

    /// Minimum length ratio
    pub min_length_ratio: f32,

    /// Maximum characters per second for readability
    pub max_chars_per_second: f32,

    /// Maximum characters per line
    pub max_chars_per_line: usize,

    /// Minimum confidence score to accept
    pub min_confidence: f32,
}

impl Default for QualityThresholds {
    fn default() -> Self {
        Self {
            min_overall: 0.7,
            max_length_ratio: 1.5,
            min_length_ratio: 0.3,
            max_chars_per_second: 25.0,
            max_chars_per_line: 42,
            min_confidence: 0.5,
        }
    }
}

impl QualityThresholds {
    /// Create strict thresholds for high-quality output.
    pub fn strict() -> Self {
        Self {
            min_overall: 0.85,
            max_length_ratio: 1.3,
            min_length_ratio: 0.5,
            max_chars_per_second: 20.0,
            max_chars_per_line: 37,
            min_confidence: 0.7,
        }
    }

    /// Create lenient thresholds for draft output.
    pub fn lenient() -> Self {
        Self {
            min_overall: 0.5,
            max_length_ratio: 2.0,
            min_length_ratio: 0.2,
            max_chars_per_second: 30.0,
            max_chars_per_line: 50,
            min_confidence: 0.3,
        }
    }
}

/// Quality metrics calculator.
pub struct QualityMetrics {
    thresholds: QualityThresholds,
}

impl QualityMetrics {
    /// Create a new metrics calculator with default thresholds.
    pub fn new() -> Self {
        Self {
            thresholds: QualityThresholds::default(),
        }
    }

    /// Create with custom thresholds.
    pub fn with_thresholds(thresholds: QualityThresholds) -> Self {
        Self { thresholds }
    }

    /// Calculate completeness score.
    pub fn calculate_completeness(&self, total: usize, translated: usize, empty: usize) -> DimensionScore {
        if total == 0 {
            return DimensionScore::perfect(0.3);
        }

        let missing = total.saturating_sub(translated);
        let issues = missing + empty;

        let score = if total > 0 {
            let successful = translated.saturating_sub(empty);
            successful as f32 / total as f32
        } else {
            1.0
        };

        DimensionScore::new(score, 0.3, issues)
    }

    /// Calculate accuracy score based on length ratios.
    pub fn calculate_accuracy(&self, ratios: &[f32]) -> DimensionScore {
        if ratios.is_empty() {
            return DimensionScore::perfect(0.25);
        }

        let mut issues = 0;
        let mut penalty = 0.0;

        for &ratio in ratios {
            if ratio > self.thresholds.max_length_ratio {
                issues += 1;
                let excess = ratio - self.thresholds.max_length_ratio;
                penalty += (excess / self.thresholds.max_length_ratio).min(1.0);
            } else if ratio < self.thresholds.min_length_ratio {
                issues += 1;
                let deficit = self.thresholds.min_length_ratio - ratio;
                penalty += (deficit / self.thresholds.min_length_ratio).min(1.0);
            }
        }

        let score = (1.0 - penalty / ratios.len() as f32).max(0.0);
        DimensionScore::new(score, 0.25, issues)
    }

    /// Calculate consistency score based on terminology issues.
    pub fn calculate_consistency(&self, total_terms: usize, inconsistent_terms: usize) -> DimensionScore {
        if total_terms == 0 {
            return DimensionScore::perfect(0.2);
        }

        let score = (total_terms.saturating_sub(inconsistent_terms)) as f32 / total_terms as f32;
        DimensionScore::new(score, 0.2, inconsistent_terms)
    }

    /// Calculate formatting score based on tag preservation.
    pub fn calculate_formatting(&self, total_tags: usize, missing_tags: usize) -> DimensionScore {
        if total_tags == 0 {
            return DimensionScore::perfect(0.15);
        }

        let score = (total_tags.saturating_sub(missing_tags)) as f32 / total_tags as f32;
        DimensionScore::new(score, 0.15, missing_tags)
    }

    /// Calculate readability score based on CPS and line length.
    pub fn calculate_readability(&self, cps_values: &[f32], line_lengths: &[usize]) -> DimensionScore {
        let mut issues = 0;
        let mut penalty = 0.0;

        // Check CPS
        for &cps in cps_values {
            if cps > self.thresholds.max_chars_per_second {
                issues += 1;
                let excess = cps - self.thresholds.max_chars_per_second;
                penalty += (excess / self.thresholds.max_chars_per_second).min(1.0) * 0.5;
            }
        }

        // Check line lengths
        for &len in line_lengths {
            if len > self.thresholds.max_chars_per_line {
                issues += 1;
                let excess = len - self.thresholds.max_chars_per_line;
                penalty += (excess as f32 / self.thresholds.max_chars_per_line as f32).min(1.0) * 0.5;
            }
        }

        let total_checks = cps_values.len() + line_lengths.len();
        let score = if total_checks > 0 {
            (1.0 - penalty / total_checks as f32).max(0.0)
        } else {
            1.0
        };

        DimensionScore::new(score, 0.1, issues)
    }

    /// Calculate overall quality score from entry data.
    pub fn calculate_score(&self, data: &MetricsData) -> QualityScore {
        let completeness = self.calculate_completeness(data.total_entries, data.translated_entries, data.empty_entries);

        let accuracy = self.calculate_accuracy(&data.length_ratios);

        let consistency = self.calculate_consistency(data.total_terms, data.inconsistent_terms);

        let formatting = self.calculate_formatting(data.total_tags, data.missing_tags);

        let readability = self.calculate_readability(&data.cps_values, &data.line_lengths);

        QualityScore::from_dimensions(
            completeness,
            accuracy,
            consistency,
            formatting,
            readability,
            data.total_entries,
            data.entries_with_issues,
        )
    }

    /// Get thresholds.
    pub fn thresholds(&self) -> &QualityThresholds {
        &self.thresholds
    }
}

impl Default for QualityMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Data collected for metrics calculation.
#[derive(Debug, Clone, Default)]
pub struct MetricsData {
    /// Total number of entries
    pub total_entries: usize,

    /// Number of translated entries
    pub translated_entries: usize,

    /// Number of empty translations
    pub empty_entries: usize,

    /// Entries with any issues
    pub entries_with_issues: usize,

    /// Length ratios for each entry
    pub length_ratios: Vec<f32>,

    /// Total glossary terms checked
    pub total_terms: usize,

    /// Inconsistent term usages
    pub inconsistent_terms: usize,

    /// Total formatting tags expected
    pub total_tags: usize,

    /// Missing formatting tags
    pub missing_tags: usize,

    /// Characters per second for each entry
    pub cps_values: Vec<f32>,

    /// Line lengths for each entry
    pub line_lengths: Vec<usize>,

    /// Per-entry details
    pub entry_details: HashMap<usize, EntryMetrics>,
}

impl MetricsData {
    /// Create new empty metrics data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add entry metrics.
    pub fn add_entry(&mut self, entry_id: usize, metrics: EntryMetrics) {
        self.total_entries += 1;

        if metrics.is_translated {
            self.translated_entries += 1;
        }

        if metrics.is_empty {
            self.empty_entries += 1;
        }

        if metrics.has_issues {
            self.entries_with_issues += 1;
        }

        if let Some(ratio) = metrics.length_ratio {
            self.length_ratios.push(ratio);
        }

        if let Some(cps) = metrics.chars_per_second {
            self.cps_values.push(cps);
        }

        self.line_lengths.extend(metrics.line_lengths.iter());
        self.total_tags += metrics.expected_tags;
        self.missing_tags += metrics.missing_tags;

        self.entry_details.insert(entry_id, metrics);
    }
}

/// Metrics for a single entry.
#[derive(Debug, Clone, Default)]
pub struct EntryMetrics {
    /// Whether the entry was translated
    pub is_translated: bool,

    /// Whether the translation is empty
    pub is_empty: bool,

    /// Whether there are any issues
    pub has_issues: bool,

    /// Length ratio (translated/source)
    pub length_ratio: Option<f32>,

    /// Characters per second
    pub chars_per_second: Option<f32>,

    /// Line lengths in the translation
    pub line_lengths: Vec<usize>,

    /// Expected formatting tags
    pub expected_tags: usize,

    /// Missing formatting tags
    pub missing_tags: usize,

    /// Confidence score from LLM
    pub confidence: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dimensionScore_weighted_shouldCalculateCorrectly() {
        let score = DimensionScore::new(0.8, 0.5, 2);
        assert!((score.weighted() - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_qualityScore_grade_shouldReturnCorrectGrade() {
        let mut score = QualityScore::default();
        score.overall = 0.95;
        assert_eq!(score.grade(), 'A');

        score.overall = 0.75;
        assert_eq!(score.grade(), 'C');

        score.overall = 0.45;
        assert_eq!(score.grade(), 'F');
    }

    #[test]
    fn test_qualityScore_weakestDimension_shouldFindLowest() {
        let score = QualityScore::from_dimensions(
            DimensionScore::new(0.9, 0.3, 0),
            DimensionScore::new(0.5, 0.25, 3), // Lowest
            DimensionScore::new(0.8, 0.2, 1),
            DimensionScore::new(0.7, 0.15, 2),
            DimensionScore::new(0.85, 0.1, 0),
            10,
            3,
        );

        assert_eq!(score.weakest_dimension(), "accuracy");
    }

    #[test]
    fn test_qualityMetrics_calculateCompleteness_shouldScore() {
        let metrics = QualityMetrics::new();

        let score = metrics.calculate_completeness(10, 8, 1);
        assert!((score.score - 0.7).abs() < 0.01);
        assert_eq!(score.issues, 3); // 2 missing + 1 empty
    }

    #[test]
    fn test_qualityMetrics_calculateAccuracy_shouldPenalizeOutOfBounds() {
        let metrics = QualityMetrics::new();

        // Good ratios
        let good_ratios = vec![1.0, 1.1, 0.9];
        let score = metrics.calculate_accuracy(&good_ratios);
        assert!(score.score > 0.9);

        // Bad ratios
        let bad_ratios = vec![2.0, 0.1, 3.0];
        let score = metrics.calculate_accuracy(&bad_ratios);
        assert!(score.score < 0.5);
    }

    #[test]
    fn test_qualityThresholds_strict_shouldBeHigher() {
        let strict = QualityThresholds::strict();
        let default = QualityThresholds::default();

        assert!(strict.min_overall > default.min_overall);
        assert!(strict.max_length_ratio < default.max_length_ratio);
    }

    #[test]
    fn test_metricsData_addEntry_shouldAccumulate() {
        let mut data = MetricsData::new();

        data.add_entry(1, EntryMetrics {
            is_translated: true,
            is_empty: false,
            has_issues: false,
            length_ratio: Some(1.1),
            ..Default::default()
        });

        data.add_entry(2, EntryMetrics {
            is_translated: true,
            is_empty: true,
            has_issues: true,
            length_ratio: Some(0.5),
            ..Default::default()
        });

        assert_eq!(data.total_entries, 2);
        assert_eq!(data.translated_entries, 2);
        assert_eq!(data.empty_entries, 1);
        assert_eq!(data.entries_with_issues, 1);
        assert_eq!(data.length_ratios.len(), 2);
    }
}
